use std::{cell::RefCell, sync::OnceLock, time::Instant};

pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("GPUI_PDF_PROFILE").as_deref() == Ok("1"))
}

#[derive(Default)]
struct Samples {
    build: Vec<f64>,
    frame: Vec<f64>,
    interval: Vec<f64>,
    last: Option<Instant>,
}

thread_local! { static SAMPLES: RefCell<Samples> = RefCell::new(Samples::default()); }

fn percentile(values: &mut [f64], p: usize) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[(values.len() - 1) * p / 100]
}

pub fn paint(start: Instant, build_ms: f64, slots: usize, links: usize, pages: usize) {
    let now = Instant::now();
    SAMPLES.with_borrow_mut(|s| {
        s.build.push(build_ms);
        s.frame.push(now.duration_since(start).as_secs_f64() * 1000.0);
        if let Some(last) = s.last { s.interval.push(now.duration_since(last).as_secs_f64() * 1000.0); }
        s.last = Some(now);
        if s.frame.len() == 60 {
            eprintln!("gpui-pdf profile: frames=60 build_ms p50={:.2} p95={:.2} frame_ms p50={:.2} p95={:.2} interval_ms p50={:.2} p95={:.2} slots={slots} links={links} pages={pages}", percentile(&mut s.build, 50), percentile(&mut s.build, 95), percentile(&mut s.frame, 50), percentile(&mut s.frame, 95), percentile(&mut s.interval, 50), percentile(&mut s.interval, 95));
            s.build.clear(); s.frame.clear(); s.interval.clear();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::percentile;
    #[test]
    fn summary_percentiles() {
        let mut values = [5.0, 1.0, 3.0, 4.0, 2.0];
        assert_eq!(percentile(&mut values, 50), 3.0);
        assert_eq!(percentile(&mut values, 95), 4.0);
        assert_eq!(percentile(&mut [], 50), 0.0);
    }
}
