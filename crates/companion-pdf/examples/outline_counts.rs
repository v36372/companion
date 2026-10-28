//! Usage: lc-cargo run -p companion-pdf --example outline_counts -- book.pdf [...]
use companion_pdf::destinations::resolved_outline;

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::sync::Arc::new(std::fs::read(&path).expect("read PDF"));
        let doc = gpui_pdf::parse(bytes).expect("parse PDF");
        let before = gpui_pdf::outline(&doc);
        let after = resolved_outline(&doc);
        println!(
            "{path}: before {}/{}, after {}/{}",
            before.iter().filter(|i| i.page.is_some()).count(),
            before.len(),
            after.iter().filter(|i| i.page.is_some()).count(),
            after.len()
        );
        println!(
            "  in-page links extracted upstream: {} (named links remain unsupported by PdfView)",
            gpui_pdf::page_links(&doc)
                .iter()
                .map(Vec::len)
                .sum::<usize>()
        );
    }
}
