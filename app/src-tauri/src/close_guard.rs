//! Decides whether closing the window needs a "downloads in progress" confirmation.

use engine::{EntryState, QueueEntry};

/// True when any entry is still waiting or running, so closing would interrupt downloads.
pub fn needs_close_confirm(entries: &[QueueEntry]) -> bool {
    entries.iter().any(|e| matches!(e.state, EntryState::Waiting | EntryState::Running))
}

/// What the window's close request should do.
#[derive(Debug, PartialEq, Eq)]
pub enum CloseAction {
    /// Let the window close.
    Close,
    /// Keep the window open and do nothing else.
    Prevent,
    /// Keep the window open and ask the user.
    PreventAndAsk,
}

/// Decides the close request: hold the window during cleanup, show only one dialog at a time,
/// and ask only when downloads are in progress.
pub fn close_action(cleanup_in_progress: bool, confirm_open: bool, needs_confirm: bool) -> CloseAction {
    if cleanup_in_progress || confirm_open {
        CloseAction::Prevent
    } else if needs_confirm {
        CloseAction::PreventAndAsk
    } else {
        CloseAction::Close
    }
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

    #[test] fn idle_closes() { assert_eq!(close_action(false, false, false), CloseAction::Close); }
    #[test] fn busy_asks() { assert_eq!(close_action(false, false, true), CloseAction::PreventAndAsk); }
    #[test] fn open_dialog_blocks_a_second_one() {
        assert_eq!(close_action(false, true, true), CloseAction::Prevent);
        assert_eq!(close_action(false, true, false), CloseAction::Prevent);
    }
    #[test] fn cleanup_holds_the_window() {
        assert_eq!(close_action(true, false, false), CloseAction::Prevent);
        assert_eq!(close_action(true, false, true), CloseAction::Prevent);
        assert_eq!(close_action(true, true, true), CloseAction::Prevent);
    }
}
