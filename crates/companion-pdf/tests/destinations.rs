use std::sync::Arc;

use companion_pdf::{
    destinations::resolved_outline,
    snapshot::{DocMeta, DocSnapshot},
};

/// Minimal two-page PDF, with real xref offsets; only hayro parses the fixture.
fn fixture(catalog_dests: &str, extra: &[&str]) -> Arc<Vec<u8>> {
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R {catalog_dests} >>"),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>".into(),
        "<< /Type /Outlines /First 6 0 R >>".into(),
        "<< /Title (Named) /Dest (chapter) /Next 7 0 R /First 8 0 R >>".into(),
        "<< /Title (Explicit) /Dest [3 0 R /Fit] /Next 9 0 R >>".into(),
        "<< /Title (Action) /A << /S /GoTo /D /chapter >> >>".into(),
        "<< /Title (Missing) /Dest (missing) /Next 10 0 R >>".into(),
        "<< /Title (Remote) /A << /S /GoToR /F (other.pdf) /D (chapter) >> /Next 11 0 R >>".into(),
        "<< /Title (Byte key) /Dest /#E9 >>".into(),
    ];
    objects.extend(extra.iter().map(|s| s.to_string()));
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in offsets.iter().skip(1) {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    Arc::new(bytes)
}

fn assert_pages(bytes: Arc<Vec<u8>>) {
    let doc = gpui_pdf::parse(bytes.clone()).unwrap();
    let before = gpui_pdf::outline(&doc);
    let after = resolved_outline(&doc);
    assert_eq!(
        after
            .iter()
            .map(|i| (&i.title, i.level))
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|i| (&i.title, i.level))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        before.iter().map(|i| i.page).collect::<Vec<_>>(),
        [None, None, Some(0), None, None, None]
    );
    assert_eq!(
        after.iter().map(|i| i.page).collect::<Vec<_>>(),
        [Some(1), Some(1), Some(0), None, None, Some(0)]
    );
    let snapshot = DocSnapshot::build(
        bytes,
        DocMeta {
            id: "fixture".into(),
            title: "Named".into(),
            source_path: "fixture.pdf".into(),
            sha256: "fixture".into(),
            page_count: 0,
        },
    )
    .unwrap();
    assert_eq!(snapshot.outline, after);
    assert!(snapshot.outline_md.contains("Named (page 2)"));
    assert!(snapshot.outline_md.contains("  - Action (page 2)"));
    snapshot.record_navigation(snapshot.outline[0].page.unwrap() as u32, 0.0);
    assert_eq!(snapshot.viewport.read().unwrap().page, 2);
}

#[test]
fn nested_name_tree_with_indirect_arrays_and_dictionary_destinations() {
    assert_pages(fixture(
        "/Names << /Dests 12 0 R >>",
        &[
            "<< /Kids [13 0 R 14 0 R] >>",
            "<< /Names [(chapter) 15 0 R] >>",
            "<< /Names [<E9> << /D [3 0 R /XYZ 0 0 0] >>] >>",
            "[4 0 R /Fit]",
        ],
    ));
}

#[test]
fn inline_name_tree_nodes_and_exact_untrimmed_keys() {
    assert_pages(fixture(
        "/Names << /Dests << /Kids [<< /Names [(chapter) [4 0 R /Fit] ( chapter ) [3 0 R /Fit]] >> << /Names [<E9> [3 0 R /Fit]] >>] >> >>",
        &[],
    ));
}

#[test]
fn legacy_dests_dictionary_with_array_and_indirect_dictionary() {
    assert_pages(fixture(
        "/Dests << /chapter 12 0 R /#E9 [3 0 R /Fit] >>",
        &["<< /D [4 0 R /XYZ 0 0 0] >>"],
    ));
}

#[test]
fn malformed_name_tree_cycles_and_odd_leaves_are_bounded() {
    let doc = gpui_pdf::parse(fixture(
        "/Names << /Dests 12 0 R >>",
        &[
            "<< /Kids [12 0 R 13 0 R] >>",
            "<< /Names [(chapter) [999 0 R /Fit] (odd)] >>",
        ],
    ))
    .unwrap();
    let items = resolved_outline(&doc);
    assert_eq!(items[0].page, None);
    assert_eq!(items[2].page, Some(0));
}

#[test]
fn name_tree_work_limit_stops_wide_malformed_trees() {
    let root = format!("<< /Kids [{}13 0 R] >>", "12 0 R ".repeat(100_001));
    let doc = gpui_pdf::parse(fixture(
        "/Names << /Dests 12 0 R >>",
        &[&root, "<< /Names [(chapter) [4 0 R /Fit]] >>"],
    ))
    .unwrap();
    assert_eq!(resolved_outline(&doc)[0].page, None);
}

#[test]
fn name_tree_takes_precedence_over_legacy_dictionary() {
    let doc = gpui_pdf::parse(fixture("/Dests << /chapter [3 0 R /Fit] >> /Names << /Dests << /Names [(chapter) [4 0 R /Fit]] >> >>", &[])).unwrap();
    assert_eq!(resolved_outline(&doc)[0].page, Some(1));
}

#[test]
fn link_names_preserve_bytes_and_resolve_y() {
    let doc = gpui_pdf::parse(fixture(
        "/Dests << /chapter [4 0 R /XYZ 0 75 null] /#E9 [3 0 R /FitH 20] >>",
        &[],
    ))
    .unwrap();
    let index = companion_pdf::destinations::NamedDestinations::new(&doc).unwrap();
    assert_eq!(
        index.get("chapter"),
        Some(gpui_pdf::PageDestination {
            page: 1,
            y: Some(0.25)
        })
    );
    assert_eq!(
        index.get("é"),
        Some(gpui_pdf::PageDestination {
            page: 0,
            y: Some(0.8)
        })
    );
    assert_eq!(index.get(" chapter "), None);
    assert_eq!(index.get("missing"), None);
    assert_eq!(index.get("❌"), None);
}

#[test]
fn depth_limit_and_cyclic_destination_dictionary() {
    let mut objects = (13..=50)
        .map(|next| format!("<< /Kids [{next} 0 R] >>"))
        .collect::<Vec<_>>();
    objects.push("<< /Names [(chapter) [4 0 R /Fit]] >>".into());
    let refs = objects.iter().map(String::as_str).collect::<Vec<_>>();
    let doc = gpui_pdf::parse(fixture("/Names << /Dests 12 0 R >>", &refs)).unwrap();
    assert_eq!(resolved_outline(&doc)[0].page, None);
    let doc = gpui_pdf::parse(fixture(
        "/Dests << /chapter 12 0 R >>",
        &["<< /D 12 0 R >>"],
    ))
    .unwrap();
    assert_eq!(resolved_outline(&doc)[0].page, None);
}
