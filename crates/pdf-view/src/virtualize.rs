#[cfg(feature = "markup")]
use std::ops::RangeInclusive;

/// Group indices only for pages in the content window, preserving input order.
#[cfg(feature = "markup")]
pub fn group_window<T>(
    items: &[T],
    window: RangeInclusive<usize>,
    page: impl Fn(&T) -> usize,
) -> Vec<Vec<usize>> {
    let start = *window.start();
    let mut groups = vec![Vec::new(); window.end() - start + 1];
    for (index, item) in items.iter().enumerate() {
        let p = page(item);
        if window.contains(&p) {
            groups[p - start].push(index);
        }
    }
    groups
}

#[cfg(feature = "markup")]
pub fn nearby_first(count: usize, window: RangeInclusive<usize>) -> impl Iterator<Item = usize> {
    let start = (*window.start()).min(count);
    let end = window.end().saturating_add(1).min(count);
    (start..end).chain(0..start).chain(end..count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clamps_window_and_empty_document() {
        let dims = vec![(1.0, 1.0); 20];
        assert_eq!(crate::keep_window(&[], 100.0, 0.0, 100.0), (0, 0));
        assert_eq!(crate::keep_window(&dims, 100.0, 0.0, 100.0).0, 0);
        assert_eq!(crate::keep_window(&dims, 100.0, 2200.0, 100.0).1, 19);
    }
    #[cfg(feature = "markup")]
    #[test]
    fn headless_window_grouping_benchmark() {
        let dims = vec![(8.5, 11.0); 2000];
        let links: Vec<_> = (0..10000).map(|i| i % 2000).collect();
        let mut samples = Vec::new();
        for _ in 0..100 {
            let now = std::time::Instant::now();
            let (start, end) = crate::keep_window(&dims, 820.0, 900_000.0, 900.0);
            let grouped = group_window(&links, start..=end, |p| *p);
            std::hint::black_box(grouped);
            samples.push(now.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        eprintln!(
            "headless 2000 pages / 10000 links: window+group p50={:.3}ms p95={:.3}ms",
            samples[50], samples[95]
        );
    }

    #[cfg(feature = "markup")]
    #[test]
    fn groups_and_search_order() {
        assert_eq!(
            group_window(&[2, 4, 2, 8], 2..=4, |p| *p),
            vec![vec![0, 2], vec![], vec![1]]
        );
        assert_eq!(
            nearby_first(6, 2..=4).collect::<Vec<_>>(),
            vec![2, 3, 4, 0, 1, 5]
        );
        assert_eq!(nearby_first(0, 0..=0).count(), 0);
    }
}
