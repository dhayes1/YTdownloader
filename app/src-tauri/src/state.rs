//! V1 runs one download at a time. The app (not the engine) enforces that here.

use std::sync::Mutex;

use engine::JobId;

/// The running job's ID, if any.
#[derive(Default)]
pub struct ActiveJob(Mutex<Option<JobId>>);

impl ActiveJob {
    /// Calls `start` and records its job, unless a job is already running (then returns `None`
    /// without calling `start`). The lock is held while `start` runs, so two quick clicks can't
    /// both start a job.
    pub fn begin(&self, start: impl FnOnce() -> JobId) -> Option<JobId> {
        let mut slot = self.0.lock().unwrap();
        if slot.is_some() {
            return None;
        }
        let id = start();
        *slot = Some(id);
        Some(id)
    }

    /// Frees the slot if `id` is the running job.
    pub fn finish(&self, id: JobId) {
        let mut slot = self.0.lock().unwrap();
        if *slot == Some(id) {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::JobId;

    #[test]
    fn second_begin_is_refused() {
        let a = ActiveJob::default();
        assert_eq!(a.begin(|| JobId(1)), Some(JobId(1)));
        assert_eq!(a.begin(|| JobId(2)), None);
    }

    #[test]
    fn refused_begin_does_not_start() {
        let a = ActiveJob::default();
        a.begin(|| JobId(1));
        let mut started = false;
        a.begin(|| {
            started = true;
            JobId(2)
        });
        assert!(!started);
    }

    #[test]
    fn finish_frees_slot() {
        let a = ActiveJob::default();
        a.begin(|| JobId(1));
        a.finish(JobId(1));
        assert_eq!(a.begin(|| JobId(2)), Some(JobId(2)));
    }

    #[test]
    fn finish_other_id_keeps_slot() {
        let a = ActiveJob::default();
        a.begin(|| JobId(1));
        a.finish(JobId(9));
        assert_eq!(a.begin(|| JobId(2)), None);
    }
}
