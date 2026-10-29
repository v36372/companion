use crate::text::{NormPoint, NormRect};

pub(crate) struct CommittedSelection {
    pub page: usize,
    pub rects: Vec<NormRect>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum ReleaseAction {
    Commit,
    Clear,
    FollowLink,
}

pub(crate) fn release_action(start: NormPoint, end: NormPoint, on_link: bool) -> ReleaseAction {
    if on_link {
        ReleaseAction::FollowLink
    } else if (start.x - end.x).abs() < 0.005 && (start.y - end.y).abs() < 0.005 {
        ReleaseAction::Clear
    } else {
        ReleaseAction::Commit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_distinguishes_click_drag_and_link() {
        let a = NormPoint { x: 0.1, y: 0.1 };
        let b = NormPoint { x: 0.2, y: 0.1 };
        assert_eq!(release_action(a, a, false), ReleaseAction::Clear);
        assert_eq!(release_action(a, b, false), ReleaseAction::Commit);
        assert_eq!(release_action(a, a, true), ReleaseAction::FollowLink);
    }
}
