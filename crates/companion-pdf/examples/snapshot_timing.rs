//! Debug-profile phase timings; includes parse/outline, excludes disk read.
use companion_pdf::snapshot::{DocMeta, DocSnapshot};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: snapshot_timing <pdf> [current-page-1]")?;
    let current = std::env::args()
        .nth(2)
        .map(|page| page.parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    let bytes = Arc::new(std::fs::read(&path)?);
    let started = Instant::now();
    let snapshot = DocSnapshot::build(
        bytes,
        DocMeta {
            id: "timing".into(),
            title: String::new(),
            source_path: path.clone(),
            sha256: String::new(),
            page_count: 0,
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let publish = started.elapsed();
    let prefill_started = Instant::now();
    snapshot.prefill(current.saturating_sub(1), &AtomicBool::new(false), |_| {});
    println!(
        "{path}: {} pages; phase-1 publish {:.2} ms; text prefill {:.2} ms; full extraction total {:.2} ms",
        snapshot.meta.page_count,
        publish.as_secs_f64() * 1000.,
        prefill_started.elapsed().as_secs_f64() * 1000.,
        started.elapsed().as_secs_f64() * 1000.
    );
    Ok(())
}
