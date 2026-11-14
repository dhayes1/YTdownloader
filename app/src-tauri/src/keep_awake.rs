//! Keeps Windows from sleeping while the queue is running.
//!
//! `SetThreadExecutionState` applies to the calling thread, so one dedicated thread owns it
//! and receives the wanted state over a channel.

use std::sync::{mpsc, Mutex};

use engine::QueueStatus;

/// The system should stay awake only while downloads are actively running.
pub fn wants_awake(status: QueueStatus) -> bool {
    status == QueueStatus::Running
}

pub struct KeepAwake {
    tx: Mutex<mpsc::Sender<bool>>,
}

impl KeepAwake {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<bool>();
        std::thread::spawn(move || {
            while let Ok(awake) = rx.recv() {
                apply(awake);
            }
            apply(false);
        });
        Self { tx: Mutex::new(tx) }
    }

    pub fn set(&self, awake: bool) {
        let _ = self.tx.lock().unwrap().send(awake);
    }
}

#[cfg(windows)]
fn apply(awake: bool) {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
    let flags = if awake { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
    // SAFETY: plain Win32 call with valid flags; affects only this thread's execution state.
    unsafe {
        SetThreadExecutionState(flags);
    }
}

#[cfg(not(windows))]
fn apply(_awake: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::QueueStatus;

    #[test] fn awake_only_while_running() { assert!(wants_awake(QueueStatus::Running)); assert!(!wants_awake(QueueStatus::Idle)); assert!(!wants_awake(QueueStatus::Paused)); }
    #[test] fn keep_awake_set_does_not_panic() { let k = KeepAwake::new(); k.set(true); k.set(false); }
}
