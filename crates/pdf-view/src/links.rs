//! Cached, bounded PDF link extraction. Coordinates match the rendered crop box.
use std::collections::HashMap;

use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, Rect, String as PdfString},
    page::Rotation,
};

const MAX_ANNOTS: usize = 100_000;
const MAX_STRING: usize = 64 * 1024;
const MAX_STRING_BYTES: usize = 8 * 1024 * 1024;

/// A resolved local destination. `y` is a normalized top-origin page offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageDestination {
    pub page: usize,
    pub y: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    /// Legacy page-only target.
    Page(usize),
    Destination(PageDestination),
    /// PDF key bytes mapped one-to-one to Unicode U+0000..U+00FF (not text decoded).
    Named(String),
    Uri(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfLink {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub target: LinkTarget,
}

impl PdfLink {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x.is_finite()
            && y.is_finite()
            && x >= self.x
            && y >= self.y
            && x <= self.x + self.w
            && y <= self.y + self.h
    }
}

fn key(bytes: &[u8]) -> Option<String> {
    (bytes.len() <= MAX_STRING).then(|| bytes.iter().map(|&b| char::from(b)).collect())
}

/// Only these schemes may be dispatched after an actual user click.
pub fn is_safe_external_uri(uri: &str) -> bool {
    uri.len() <= MAX_STRING
        && !uri.chars().any(|c| c.is_whitespace() || c.is_control())
        && ["http://", "https://", "mailto:"].iter().any(|scheme| {
            uri.get(..scheme.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
                && uri.len() > scheme.len()
        })
}

/// Resolve position once a destination's page reference has been looked up.
pub fn destination_on_page(doc: &Pdf, array: &Array<'_>, page: usize) -> PageDestination {
    let mut values = array.iter::<Object>();
    let _ = values.next();
    let kind = values.next().and_then(Object::into_name);
    let left = values.next().and_then(Object::into_f32);
    let top = match kind.as_ref().map(Name::as_str) {
        Some("XYZ" | "FitR") => values.next().and_then(Object::into_f32),
        Some("FitH" | "FitBH") => left,
        _ => None,
    };
    let y = doc.pages().get(page).and_then(|p| {
        let cb = p.crop_box();
        // /FitR is [left bottom right top], unlike /XYZ.
        let top = if kind.as_ref().is_some_and(|n| n.as_str() == "FitR") {
            let _ = values.next();
            values.next().and_then(Object::into_f32)
        } else {
            top
        }?;
        let left = left.unwrap_or(cb.x0 as f32) as f64;
        let (_, y) = rotate(
            ((left - cb.x0) / cb.width()) as f32,
            ((cb.y1 - top as f64) / cb.height()) as f32,
            p.rotation(),
        );
        y.is_finite().then(|| y.clamp(0.0, 1.0))
    });
    PageDestination { page, y }
}

fn target(
    doc: &Pdf,
    value: Object<'_>,
    pages: &HashMap<ObjectIdentifier, usize>,
) -> Option<LinkTarget> {
    match value {
        Object::Name(n) => Some(LinkTarget::Named(key(n.as_ref())?)),
        Object::String(s) => Some(LinkTarget::Named(key(s.as_bytes())?)),
        Object::Array(a) => {
            let MaybeRef::Ref(reference) = a.raw_iter().next()? else {
                return None;
            };
            let page = *pages.get(&reference.into())?;
            Some(LinkTarget::Destination(destination_on_page(doc, &a, page)))
        }
        _ => None,
    }
}

fn rotate(x: f32, y: f32, rotation: Rotation) -> (f32, f32) {
    match rotation {
        Rotation::None => (x, y),
        Rotation::Horizontal => (1.0 - y, x),
        Rotation::Flipped => (1.0 - x, 1.0 - y),
        Rotation::FlippedHorizontal => (y, 1.0 - x),
    }
}

/// Extract off the UI thread during document preparation; cache for its lifetime.
/// A document-wide annotation budget bounds malformed or unusually wide arrays.
pub fn page_links(doc: &Pdf) -> Vec<Vec<PdfLink>> {
    let pages = doc
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.raw().obj_id().map(|id| (id, i)))
        .collect();
    let mut budget = MAX_ANNOTS;
    let mut string_bytes_left = MAX_STRING_BYTES;
    doc.pages()
        .iter()
        .map(|page| {
            let mut links = Vec::new();
            let cb = page.crop_box();
            if cb.width() <= 0.0 || cb.height() <= 0.0 {
                return links;
            }
            if let Some(annots) = page.raw().get::<Array>("Annots") {
                for value in annots.iter::<Object>().take(budget) {
                    budget -= 1;
                    let Some(annot) = value.into_dict() else {
                        continue;
                    };
                    if annot
                        .get::<Name>("Subtype")
                        .is_none_or(|n| n.as_str() != "Link")
                    {
                        continue;
                    }
                    let destination = if let Some(dest) = annot.get::<Object>("Dest") {
                        target(doc, dest, &pages)
                    } else if let Some(action) = annot.get::<Dict>("A") {
                        match action.get::<Name>("S").as_ref().map(Name::as_str) {
                            Some("GoTo") => action
                                .get::<Object>("D")
                                .and_then(|d| target(doc, d, &pages)),
                            Some("URI") => action.get::<PdfString>("URI").and_then(|s| {
                                let uri = std::str::from_utf8(s.as_bytes()).ok()?;
                                is_safe_external_uri(uri).then(|| LinkTarget::Uri(uri.to_owned()))
                            }),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    let (Some(target), Some(r)) = (destination, annot.get::<Rect>("Rect")) else {
                        continue;
                    };
                    let (x0, x1) = (r.x0.min(r.x1).max(cb.x0), r.x0.max(r.x1).min(cb.x1));
                    let (y0, y1) = (r.y0.min(r.y1).max(cb.y0), r.y0.max(r.y1).min(cb.y1));
                    if x1 <= x0 || y1 <= y0 {
                        continue;
                    }
                    let a = rotate(
                        ((x0 - cb.x0) / cb.width()) as f32,
                        ((cb.y1 - y1) / cb.height()) as f32,
                        page.rotation(),
                    );
                    let b = rotate(
                        ((x1 - cb.x0) / cb.width()) as f32,
                        ((cb.y1 - y0) / cb.height()) as f32,
                        page.rotation(),
                    );
                    if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
                        continue;
                    }
                    if let LinkTarget::Named(name) | LinkTarget::Uri(name) = &target {
                        if name.len() > string_bytes_left {
                            continue;
                        }
                        string_bytes_left -= name.len();
                    }
                    links.push(PdfLink {
                        x: a.0.min(b.0),
                        y: a.1.min(b.1),
                        w: (a.0 - b.0).abs(),
                        h: (a.1 - b.1).abs(),
                        target,
                    });
                }
            }
            links
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn fixture(annots: &str, rotation: u16) -> Arc<Pdf> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_owned(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [10 20 210 420] /CropBox [10 20 210 420] /Rotate {rotation} /Annots [{annots}] >>"
            ),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 200] >>".to_owned(),
        ];
        let mut bytes = b"%PDF-1.7\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        crate::parse(Arc::new(bytes)).unwrap()
    }

    fn annotation(destination: &str) -> String {
        format!("<< /Subtype /Link /Rect [30 300 110 400] {destination} >>")
    }

    #[test]
    fn names_strings_explicit_and_goto() {
        let doc = fixture(
            &[
                annotation("/Dest /chapter"),
                annotation("/Dest ( chapter )"),
                annotation("/Dest <E9>"),
                annotation("/Dest [4 0 R /XYZ 0 150 null]"),
                annotation("/A << /S /GoTo /D [4 0 R /FitH 100] >>"),
                annotation("/A << /S /GoTo /D (section) >>"),
            ]
            .join(" "),
            0,
        );
        let links = page_links(&doc);
        assert_eq!(links[0].len(), 6);
        assert_eq!(links[0][0].target, LinkTarget::Named("chapter".into()));
        assert_eq!(links[0][1].target, LinkTarget::Named(" chapter ".into()));
        assert_eq!(links[0][2].target, LinkTarget::Named("é".into()));
        assert_eq!(
            links[0][3].target,
            LinkTarget::Destination(PageDestination {
                page: 1,
                y: Some(0.25)
            })
        );
        assert_eq!(
            links[0][4].target,
            LinkTarget::Destination(PageDestination {
                page: 1,
                y: Some(0.5)
            })
        );
        assert_eq!(links[0][5].target, LinkTarget::Named("section".into()));
    }

    #[test]
    fn uri_allowlist_and_remote_actions() {
        let doc = fixture(
            &[
                annotation("/A << /S /URI /URI (https://example.org) >>"),
                annotation("/A << /S /URI /URI (mailto:a@b.org) >>"),
                annotation("/A << /S /URI /URI (file:///etc/passwd) >>"),
                annotation("/A << /S /Launch /D [4 0 R /Fit] >>"),
                annotation("/A << /S /GoToR /D [4 0 R /Fit] >>"),
            ]
            .join(" "),
            0,
        );
        let links = page_links(&doc);
        assert_eq!(links[0].len(), 2);
        assert_eq!(
            links[0][0].target,
            LinkTarget::Uri("https://example.org".into())
        );
        assert_eq!(links[0][1].target, LinkTarget::Uri("mailto:a@b.org".into()));
        for good in [
            "https://example.org",
            "HTTP://example.org",
            "mailto:a@b.org",
        ] {
            assert!(is_safe_external_uri(good));
        }
        for bad in [
            "https://",
            "file:///etc/passwd",
            "smb://evil",
            "javascript:x",
            "data:x",
            " https://x",
            "https://x\ny",
            "mailto:",
        ] {
            assert!(!is_safe_external_uri(bad), "{bad}");
        }
    }

    #[test]
    fn explicit_position_kinds_and_clamping() {
        for (array, y) in [
            ("[4 0 R /XYZ null 150 null]", Some(0.25)),
            ("[4 0 R /FitR 0 20 80 180]", Some(0.1)),
            ("[4 0 R /FitBH 100]", Some(0.5)),
            ("[4 0 R /Fit]", None),
            ("[4 0 R /FitV 20]", None),
            ("[4 0 R /XYZ 0 250 null]", Some(0.0)),
            ("[4 0 R /XYZ 0 -50 null]", Some(1.0)),
        ] {
            let doc = fixture(&annotation(&format!("/Dest {array}")), 0);
            let links = page_links(&doc);
            let LinkTarget::Destination(destination) = &links[0][0].target else {
                panic!("missing destination")
            };
            assert_eq!(destination.page, 1);
            match (destination.y, y) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 0.0001, "{array}"),
                (None, None) => (),
                _ => panic!("wrong position for {array}"),
            }
        }
    }

    #[test]
    fn crop_rotation_and_normalized_hit_testing() {
        for rotation in [0, 90, 180, 270] {
            let doc = fixture(&annotation("/Dest [4 0 R /Fit]"), rotation);
            let link = &page_links(&doc)[0][0];
            assert!(link.contains(link.x + link.w / 2.0, link.y + link.h / 2.0));
            assert!(!link.contains(link.x - 0.001, link.y));
            assert!(!link.contains(f32::NAN, link.y));
            if rotation == 0 {
                assert!((link.x - 0.1).abs() < 0.0001);
                assert!((link.y - 0.05).abs() < 0.0001);
                assert!((link.w - 0.4).abs() < 0.0001);
                assert!((link.h - 0.25).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn null_positions_bad_refs_and_clipped_rects() {
        let doc = fixture(
            &[
                annotation("/Dest [4 0 R /XYZ null null null]"),
                annotation("/Dest [999 0 R /Fit]"),
                "<< /Subtype /Link /Rect [-100 -100 50 100] /Dest /clipped >>".into(),
                "<< /Subtype /Link /Rect [-100 -100 -50 -50] /Dest /outside >>".into(),
            ]
            .join(" "),
            0,
        );
        let links = page_links(&doc);
        assert_eq!(links[0].len(), 2);
        assert_eq!(
            links[0][0].target,
            LinkTarget::Destination(PageDestination { page: 1, y: None })
        );
        assert_eq!(links[0][1].x, 0.0);
        assert!((links[0][1].w - 0.2).abs() < 0.0001);
    }
}
