//! Usage: cargo run -p companion-pdf --example link_counts -- file.pdf ...
use gpui_pdf::{LinkTarget, page_links};
use hayro::hayro_syntax::object::{Array, Dict, Name};
use std::sync::Arc;

fn main() {
    for path in std::env::args().skip(1) {
        let doc = gpui_pdf::parse(Arc::new(std::fs::read(&path).unwrap())).unwrap();
        let annotations = doc
            .pages()
            .iter()
            .filter_map(|p| p.raw().get::<Array>("Annots"))
            .map(|a| {
                a.iter::<Dict>()
                    .filter(|a| {
                        a.get::<Name>("Subtype")
                            .is_some_and(|n| n.as_str() == "Link")
                    })
                    .count()
            })
            .sum::<usize>();
        let destinations = companion_pdf::destinations::NamedDestinations::new(&doc).unwrap();
        let mut named = 0;
        let mut resolved = 0;
        let mut explicit = 0;
        let mut uri = 0;
        let mut positioned = 0;
        for link in page_links(&doc).into_iter().flatten() {
            match link.target {
                LinkTarget::Named(name) => {
                    named += 1;
                    if let Some(destination) = destinations.get(&name) {
                        resolved += 1;
                        positioned += usize::from(destination.y.is_some());
                    }
                }
                LinkTarget::Destination(destination) => {
                    explicit += 1;
                    positioned += usize::from(destination.y.is_some());
                }
                LinkTarget::Page(_) => explicit += 1,
                LinkTarget::Uri(_) => uri += 1,
            }
        }
        println!(
            "{path}: annotations={annotations}, explicit={explicit}, named={named}, named_resolved={resolved}, uri={uri}, positioned={positioned}, total_resolved={}",
            explicit + resolved + uri
        );
    }
}
