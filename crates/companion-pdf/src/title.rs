//! Human-readable display titles, never the managed UUID copy's basename.
use std::path::Path;

/// Prefer a sensible PDF /Title, then the original source stem, then catalog title.
pub fn display_title(metadata: Option<&[u8]>, source_path: &str, catalog_title: &str) -> String {
    let decoded = metadata
        .filter(|bytes| bytes.len() <= 4096)
        .and_then(|bytes| {
            if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
                if !bytes.len().is_multiple_of(2) {
                    return None;
                }
                String::from_utf16(
                    &bytes
                        .chunks_exact(2)
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>(),
                )
                .ok()
            } else {
                let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
                String::from_utf8(bytes.to_vec())
                    .ok()
                    .or_else(|| Some(bytes.iter().map(|&b| char::from(b)).collect()))
            }
        });
    decoded
        .as_deref()
        .and_then(sensible)
        .or_else(|| {
            Path::new(source_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(plain_title)
        })
        .or_else(|| plain_title(catalog_title))
        .unwrap_or_else(|| "Untitled PDF".into())
}

/// gpui-pdf derives its label from the path. Give it a disposable title-named
/// hard link; the catalog and all writes still refer to the managed UUID copy.
/// Call off-thread. Each document must have its own display directory.
pub fn display_path(
    stored: &Path,
    directory: &Path,
    title: &str,
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join(display_filename(title));
    if !path.is_file() {
        match std::fs::hard_link(stored, &path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() => {}
            Err(_) => {
                std::fs::copy(stored, &path)?;
            }
        }
    }
    Ok(path)
}

fn display_filename(title: &str) -> String {
    let mut name = String::new();
    for ch in title.trim().chars() {
        let ch = if ch.is_control()
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            '-'
        } else {
            ch
        };
        if name.len() + ch.len_utf8() > 160 {
            break;
        }
        name.push(ch);
    }
    let name = name.trim_matches([' ', '.']);
    format!(
        "{}.pdf",
        if name.is_empty() {
            "Untitled PDF"
        } else {
            name
        }
    )
}

fn sensible(value: &str) -> Option<String> {
    if value.trim().chars().count() > 240
        || matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "untitled" | "untitled document" | "document" | "microsoft word"
        )
    {
        None
    } else {
        plain_title(value)
    }
}

fn plain_title(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{display_filename, display_path, display_title};

    #[test]
    fn safe_display_names_and_uuid_identity() {
        assert_eq!(display_filename("a/b\\c:d\ne"), "a-b-c-d-e.pdf");
        assert_eq!(display_filename("..."), "Untitled PDF.pdf");
        assert!(display_filename(&"界".repeat(200)).len() <= 164);
        let root = tempfile::tempdir().unwrap();
        let stored = root.path().join("uuid.pdf");
        std::fs::write(&stored, b"managed bytes").unwrap();
        let directory = root.path().join("display/uuid");
        let path = display_path(&stored, &directory, "A title").unwrap();
        assert_eq!(path.file_name().unwrap(), "A title.pdf");
        assert_eq!(std::fs::read(&path).unwrap(), b"managed bytes");
        assert_eq!(display_path(&stored, &directory, "A title").unwrap(), path);
        assert_eq!(std::fs::read(&stored).unwrap(), b"managed bytes");
    }

    #[test]
    fn title_fallback_order() {
        assert_eq!(
            display_title(Some(b"  PDF title  "), "/books/Original.pdf", "catalog"),
            "PDF title"
        );
        for title in [b"".as_slice(), b"Untitled", b"bad\0title"] {
            assert_eq!(
                display_title(Some(title), "/books/Original.pdf", "catalog"),
                "Original"
            );
        }
        assert_eq!(display_title(None, "Document.pdf", "catalog"), "Document");
        assert_eq!(display_title(None, "", "catalog"), "catalog");
        assert_eq!(display_title(None, "", ""), "Untitled PDF");
        assert_eq!(
            display_title(Some(&[0xfe, 0xff, 0]), "Original.pdf", ""),
            "Original"
        );
        assert_eq!(
            display_title(Some(&[b'a'; 4097]), "Original.pdf", ""),
            "Original"
        );
    }
    #[test]
    fn unicode_metadata() {
        assert_eq!(
            display_title(
                Some(&[0xfe, 0xff, 0, 0x43, 0, 0x61, 0, 0x66, 0, 0xe9]),
                "file.pdf",
                ""
            ),
            "Café"
        );
        assert_eq!(
            display_title(Some("日本語".as_bytes()), "file.pdf", ""),
            "日本語"
        );
    }
}
