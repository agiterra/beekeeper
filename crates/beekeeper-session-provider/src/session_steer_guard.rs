//! A native queue entry remains revocable until its transport begins writing.

use super::FenceState;
use beekeeper_acp::steer::SteerWriteRefusal;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub(super) struct NativeSteerWriteGuard {
    pub(super) command_id: String,
    pub(super) fenced: Arc<Mutex<FenceState>>,
}

impl beekeeper_acp::steer::SteerWriteGuard for NativeSteerWriteGuard {
    fn begin_write(&self) -> Result<(), SteerWriteRefusal> {
        let Ok(mut state) = self.fenced.lock() else {
            // An unverifiable authority boundary never becomes a runtime write.
            return Err(SteerWriteRefusal::Unavailable);
        };
        if let Some(reason) = state.fenced.remove(&self.command_id) {
            return Err(reason);
        }
        state.dequeued.insert(self.command_id.clone());
        Ok(())
    }
}

#[cfg(test)]
impl super::SessionHandle {
    /// Exercise the actual write guard against a test-controlled mailbox.
    pub(crate) fn begin_native_write_for_test(
        &self,
        command_id: &str,
    ) -> Result<(), SteerWriteRefusal> {
        use beekeeper_acp::steer::SteerWriteGuard;
        NativeSteerWriteGuard {
            command_id: command_id.to_owned(),
            fenced: self.fenced.clone(),
        }
        .begin_write()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_acp::steer::SteerWriteGuard;

    #[test]
    fn native_steer_guard_records_only_admitted_writes() {
        let fenced = Arc::new(Mutex::new(FenceState::default()));
        fenced
            .lock()
            .expect("lock")
            .fenced
            .insert("denied".into(), SteerWriteRefusal::Fenced);
        let denied = NativeSteerWriteGuard {
            command_id: "denied".into(),
            fenced: fenced.clone(),
        };
        assert_eq!(denied.begin_write(), Err(SteerWriteRefusal::Fenced));
        assert!(fenced.lock().expect("lock").dequeued.is_empty());
        let admitted = NativeSteerWriteGuard {
            command_id: "admitted".into(),
            fenced: fenced.clone(),
        };
        assert_eq!(admitted.begin_write(), Ok(()));
        assert!(fenced.lock().expect("lock").dequeued.contains("admitted"));
    }

    #[test]
    fn native_steer_guard_poison_is_unavailable_not_a_claimed_handover() {
        let fenced = Arc::new(Mutex::new(FenceState::default()));
        let poisoned = fenced.clone();
        assert!(std::panic::catch_unwind(move || {
            let _lock = poisoned.lock().expect("lock before poisoning");
            panic!("poison authority guard");
        })
        .is_err());
        let guard = NativeSteerWriteGuard {
            command_id: "unverified".into(),
            fenced: fenced.clone(),
        };
        assert_eq!(guard.begin_write(), Err(SteerWriteRefusal::Unavailable));
        assert!(fenced
            .lock()
            .expect_err("poisoned")
            .into_inner()
            .dequeued
            .is_empty());
    }
}
