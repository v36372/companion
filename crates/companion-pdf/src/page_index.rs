/// Convert a viewer/SQL index to the agent/chat index. Real PDF page counts
/// cannot approach u32::MAX; reject overflow rather than silently wrapping.
pub fn page_0_to_1(page_0: u32) -> u32 {
    page_0.checked_add(1).expect("PDF page index overflow")
}

/// Tool input must reject page zero before calling the viewer.
pub fn page_1_to_0(page_1: u32) -> Option<u32> {
    page_1.checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_boundaries() {
        assert_eq!(page_0_to_1(0), 1);
        assert_eq!(page_1_to_0(1), Some(0));
        assert_eq!(page_1_to_0(0), None);
        assert_eq!(page_1_to_0(page_0_to_1(42)), Some(42));
    }
}
