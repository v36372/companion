use gpui_pdf::{NormRect, PageText};

use crate::{page_index::page_0_to_1, snapshot::LiveSelection};

pub fn locate_quote(page: &PageText, quote: &str, occurrence: u32) -> Vec<NormRect> {
    page.locate(quote, occurrence as usize)
}

/// Match the viewer's lowercase/whitespace-insensitive locator while retaining
/// original UTF-8 byte boundaries for nearby context. No byte slicing of chars.
pub fn quote_context(text: &str, quote: &str, occurrence: u32) -> (String, String) {
    let mut normalized = String::new();
    let mut boundaries = Vec::new();
    for (start, ch) in text.char_indices().filter(|(_, ch)| !ch.is_whitespace()) {
        for lower in ch.to_lowercase() {
            let before = normalized.len();
            normalized.push(lower);
            boundaries.extend((before..normalized.len()).map(|_| (start, start + ch.len_utf8())));
        }
    }
    let needle: String = quote
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if needle.is_empty() {
        return (String::new(), String::new());
    }
    let mut matches = normalized
        .char_indices()
        .filter_map(|(start, _)| normalized[start..].starts_with(&needle).then_some(start));
    let Some(start) = matches.nth(occurrence as usize) else {
        return (String::new(), String::new());
    };
    let end = boundaries[start + needle.len() - 1].1;
    let start = boundaries[start].0;
    let prefix = text[..start]
        .chars()
        .rev()
        .take(160)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let suffix = text[end..].chars().take(160).collect();
    (prefix, suffix)
}

pub fn live_selection(page_0: u32, text: &str, quote: String, occurrence: u32) -> LiveSelection {
    let (prefix, suffix) = quote_context(text, &quote, occurrence);
    LiveSelection {
        page: page_0_to_1(page_0),
        quote,
        occurrence,
        prefix,
        suffix,
    }
}

pub fn ask_quote(selection: &LiveSelection) -> String {
    format!(
        "Page {}\n<selected_quote>\n{}\n</selected_quote>",
        selection.page, selection.quote
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_and_repeated_quote_context() {
        assert_eq!(
            quote_context("前 Écho\nword 後 éCHO word fin", "écho word", 1),
            ("前 Écho\nword 後 ".into(), " fin".into())
        );
        assert_eq!(
            quote_context("abc", "missing", 0),
            (String::new(), String::new())
        );
    }
}
