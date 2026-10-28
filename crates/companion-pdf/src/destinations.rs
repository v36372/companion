//! Resolve the named destinations gpui-pdf 0.6.1 leaves out of its outline.
//! Keys are PDF bytes, not display strings: names and strings must compare
//! without lossy Unicode conversion, trimming, or PDF text-string decoding.
use std::collections::{HashMap, HashSet};

use gpui_pdf::{Document, OutlineItem};
use hayro::hayro_syntax::object::{
    Array, Dict, MaybeRef, Object, ObjectIdentifier, String as PdfString,
};

const MAX_DEPTH: usize = 32;
const MAX_ITEMS: usize = 10_000;
const MAX_WORK: usize = 100_000;
const MAX_KEY_BYTES: usize = 8 * 1024 * 1024;

struct Destinations {
    pages: HashMap<ObjectIdentifier, usize>,
    named: HashMap<Vec<u8>, usize>,
    work_left: usize,
    key_bytes_left: usize,
}

impl Destinations {
    fn new(doc: &Document, catalog: &Dict<'_>) -> Self {
        let mut this = Self {
            pages: doc
                .pages()
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.raw().obj_id().map(|id| (id, i)))
                .collect(),
            named: HashMap::new(),
            work_left: MAX_WORK,
            key_bytes_left: MAX_KEY_BYTES,
        };
        // The legacy dictionary is keyed by PDF names, while name-tree leaves
        // use PDF strings. Prefer the name tree if both define the same key.
        if let Some(dests) = catalog.get::<Dict>("Dests") {
            for key in dests.keys().take(MAX_WORK) {
                if !this.spend() {
                    break;
                }
                if let Some(value) = dests.get::<Object>(&key) {
                    this.insert(key.as_ref(), value);
                }
            }
        }
        if let Some(tree) = catalog
            .get::<Dict>("Names")
            .and_then(|n| n.get::<Dict>("Dests"))
        {
            this.walk_names(doc, tree, 0, &mut HashSet::new());
        }
        this
    }

    fn spend(&mut self) -> bool {
        if self.work_left == 0 {
            return false;
        }
        self.work_left -= 1;
        true
    }

    fn insert(&mut self, key: &[u8], value: Object<'_>) {
        if key.len() > self.key_bytes_left {
            return;
        }
        if let Some(page) = self.explicit_page(value, 0) {
            self.key_bytes_left -= key.len();
            self.named.insert(key.to_vec(), page);
        }
    }

    fn walk_names(
        &mut self,
        doc: &Document,
        node: Dict<'_>,
        depth: usize,
        visited: &mut HashSet<ObjectIdentifier>,
    ) {
        if depth > MAX_DEPTH || !self.spend() {
            return;
        }
        if let Some(names) = node.get::<Array>("Names") {
            let mut entries = names.iter::<Object>();
            while self.spend() {
                let (Some(key), Some(value)) = (entries.next(), entries.next()) else {
                    break;
                };
                if let Some(key) = key.into_string() {
                    self.insert(key.as_bytes(), value);
                }
            }
        }
        if let Some(kids) = node.get::<Array>("Kids") {
            for kid in kids.raw_iter().take(MAX_WORK) {
                if !self.spend() {
                    break;
                }
                let dict = match kid {
                    MaybeRef::Ref(reference) => {
                        let id = reference.into();
                        if !visited.insert(id) {
                            continue;
                        }
                        doc.xref().get::<Dict>(id)
                    }
                    MaybeRef::NotRef(object) => object.into_dict(),
                };
                if let Some(dict) = dict {
                    self.walk_names(doc, dict, depth + 1, visited);
                }
            }
        }
    }

    fn explicit_page(&self, value: Object<'_>, depth: usize) -> Option<usize> {
        if depth > MAX_DEPTH {
            return None;
        }
        match value {
            Object::Array(array) => match array.raw_iter().next()? {
                MaybeRef::Ref(page) => self.pages.get(&page.into()).copied(),
                _ => None,
            },
            Object::Dict(dict) => self.explicit_page(dict.get::<Object>("D")?, depth + 1),
            _ => None,
        }
    }

    fn page(&self, value: Object<'_>) -> Option<usize> {
        match value {
            Object::String(key) => self.named.get(key.as_bytes()).copied(),
            Object::Name(key) => self.named.get(key.as_ref()).copied(),
            other => self.explicit_page(other, 0),
        }
    }

    fn item_page(&self, item: &Dict<'_>) -> Option<usize> {
        if let Some(dest) = item.get::<Object>("Dest") {
            return self.page(dest);
        }
        let action = item.get::<Dict>("A")?;
        // Do not accidentally turn remote GoToR / Launch actions into local jumps.
        if action
            .get::<hayro::hayro_syntax::object::Name>("S")?
            .as_str()
            != "GoTo"
        {
            return None;
        }
        self.page(action.get::<Object>("D")?)
    }
}

/// Keep upstream's labels, ordering, and explicit resolutions; fill missing
/// pages by walking the same bounded, depth-first outline object sequence.
/// Both the rail and the agent-facing Markdown consume this result.
pub fn resolved_outline(doc: &Document) -> Vec<OutlineItem> {
    let mut items = gpui_pdf::outline(doc);
    if items.iter().all(|item| item.page.is_some()) {
        return items;
    }
    let Some(catalog) = doc.xref().get::<Dict>(doc.xref().root_id()) else {
        return items;
    };
    let destinations = Destinations::new(doc, &catalog);
    let first = catalog
        .get::<Dict>("Outlines")
        .and_then(|o| o.get_ref("First"));
    let mut stack = first.into_iter().map(|r| (r, 0)).collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut index = 0;
    while let Some((reference, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        if index >= items.len() || index >= MAX_ITEMS {
            break;
        }
        if !visited.insert(reference) {
            continue;
        }
        let Some(item) = doc.xref().get::<Dict>(reference.into()) else {
            continue;
        };
        if item.get::<PdfString>("Title").is_some() {
            items[index].page = items[index].page.or_else(|| destinations.item_page(&item));
            index += 1;
        }
        if let Some(next) = item.get_ref("Next") {
            stack.push((next, depth));
        }
        if let Some(child) = item.get_ref("First") {
            stack.push((child, depth + 1));
        }
        if visited.len() >= MAX_WORK {
            break;
        }
    }
    items
}
