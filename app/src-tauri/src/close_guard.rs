//! Decides whether closing the window needs a "downloads in progress" confirmation.

use engine::{EntryState, QueueEntry};

/// True when any entry is still waiting or running, so closing would interrupt downloads.
pub fn needs_close_confirm(entries: &[QueueEntry]) -> bool {
    entries.iter().any(|e| matches!(e.state, EntryState::Waiting | EntryState::Running))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(state: EntryState) -> QueueEntry {
        QueueEntry { id: engine::EntryId(1), group: None, url: "u".into(), title: "t".into(), prefix: None, state }
    }

    #[test] fn empty_needs_no_confirm() { assert!(!needs_close_confirm(&[])); }
    #[test] fn only_done_and_failed_need_no_confirm() {
        let e = [entry(EntryState::Done { path: "a.mp4".into() }), entry(EntryState::Failed { msg: "x".into() })];
        assert!(!needs_close_confirm(&e));
    }
    #[test] fn any_waiting_needs_confirm() {
        assert!(needs_close_confirm(&[entry(EntryState::Done { path: "a.mp4".into() }), entry(EntryState::Waiting)]));
    }
    #[test] fn any_running_needs_confirm() {
        assert!(needs_close_confirm(&[entry(EntryState::Failed { msg: "x".into() }), entry(EntryState::Running)]));
    }
}
