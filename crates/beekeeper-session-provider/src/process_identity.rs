//! Exact-generation witness for a Windows ACP child.
//!
//! On Unix a recorded process group is probed with `killpg(group, 0)`; only
//! `ESRCH` proves it gone. Windows has no such group, and a PID alone proves
//! nothing: it is reused, and the job object that held the child dies with the
//! provider that created it, so it is no durable proof either. What names one
//! process for its whole life is the pair (PID, creation `FILETIME`). The
//! provider records both from the spawned child at spawn; a recovering provider
//! opens the PID with the minimum query right and compares.
//!
//! The decision is a pure function of what was recorded and what was observed,
//! so every branch is testable on any host. Anything not positively shown to be
//! a different or missing process stays UNPROVED, which fences the session.

/// What one attempt to look at a recorded PID showed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Observed {
    /// The PID is not in the process table (`ERROR_INVALID_PARAMETER`).
    NotFound,
    /// The PID is open and reports this creation time.
    Live { creation_filetime: u64 },
    /// Access denied, or any other failure to look: nothing was learned.
    Unobservable,
}

/// Whether the recorded child generation is proved absent.
///
/// * No recorded creation time, or a zero one (a witness written before this
///   identity existed, or a failed capture) is never upgraded by assumption:
///   unproved, whatever is observed.
/// * Not found proves the recorded process absent.
/// * A live PID with a different creation time is a reused PID: the recorded
///   generation is absent.
/// * A live PID with the same creation time is still present.
/// * A live PID reporting zero, or any unobservable PID, is unproved.
pub(crate) fn proves_absent(recorded: Option<u64>, observed: Observed) -> bool {
    let Some(recorded) = recorded.filter(|filetime| *filetime != 0) else {
        return false;
    };
    match observed {
        Observed::NotFound => true,
        Observed::Live {
            creation_filetime: 0,
        } => false,
        Observed::Live { creation_filetime } => creation_filetime != recorded,
        Observed::Unobservable => false,
    }
}

/// `ERROR_INVALID_PARAMETER`: what `OpenProcess` reports for a PID that has
/// left the process table.
#[cfg(windows)]
const ERROR_INVALID_PARAMETER: i32 = 87;

/// Look at `pid` with the minimum right that reads identity
/// (`PROCESS_QUERY_LIMITED_INFORMATION`).
#[cfg(windows)]
pub(crate) fn observe(pid: u32) -> Observed {
    use win_custody::{Process, ProcessAccess};
    match Process::open(pid, ProcessAccess::QueryLimited) {
        Ok(process) => match process.creation_time() {
            Ok(creation_filetime) => Observed::Live { creation_filetime },
            Err(_) => Observed::Unobservable,
        },
        Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => Observed::NotFound,
        Err(_) => Observed::Unobservable,
    }
}

/// Whether the recorded Windows child (`pid`, `recorded` creation time) is
/// proved absent right now.
#[cfg(windows)]
pub(crate) fn recorded_child_absent(pid: u32, recorded: Option<u64>) -> bool {
    // A legacy witness is unproved without touching the process table.
    if recorded.is_none_or(|filetime| filetime == 0) || pid == 0 {
        return false;
    }
    proves_absent(recorded, observe(pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 134_000_000_000_000_000;

    #[test]
    fn a_missing_pid_proves_the_recorded_child_absent() {
        assert!(proves_absent(Some(T), Observed::NotFound));
    }

    #[test]
    fn a_live_pid_with_the_same_creation_time_is_still_present() {
        assert!(!proves_absent(
            Some(T),
            Observed::Live {
                creation_filetime: T
            }
        ));
    }

    #[test]
    fn a_live_pid_with_a_different_creation_time_is_a_reused_pid() {
        for other in [T + 1, T - 1, 1, u64::MAX] {
            assert!(proves_absent(
                Some(T),
                Observed::Live {
                    creation_filetime: other
                }
            ));
        }
    }

    #[test]
    fn an_unobservable_pid_stays_unproved() {
        assert!(!proves_absent(Some(T), Observed::Unobservable));
    }

    #[test]
    fn a_live_pid_reporting_zero_stays_unproved() {
        assert!(!proves_absent(
            Some(T),
            Observed::Live {
                creation_filetime: 0
            }
        ));
    }

    #[test]
    fn a_witness_without_creation_identity_is_never_upgraded() {
        for observed in [
            Observed::NotFound,
            Observed::Live {
                creation_filetime: T,
            },
            Observed::Live {
                creation_filetime: 7,
            },
            Observed::Live {
                creation_filetime: 0,
            },
            Observed::Unobservable,
        ] {
            assert!(!proves_absent(None, observed), "{observed:?}");
            assert!(!proves_absent(Some(0), observed), "zero, {observed:?}");
        }
    }

    #[cfg(windows)]
    mod windows {
        use super::*;
        use win_custody::{Process, ProcessAccess};

        fn own_creation() -> u64 {
            Process::open(std::process::id(), ProcessAccess::QueryLimited)
                .expect("open self")
                .creation_time()
                .expect("own creation time")
        }

        #[test]
        fn this_process_is_present_with_its_own_creation_time() {
            let own = own_creation();
            assert_ne!(own, 0);
            assert_eq!(
                observe(std::process::id()),
                Observed::Live {
                    creation_filetime: own
                }
            );
            assert!(!recorded_child_absent(std::process::id(), Some(own)));
        }

        #[test]
        fn a_reused_pid_is_told_from_the_recorded_generation() {
            // A PID reused by a later process shows a later creation time. The
            // one safely constructible form: record a creation time this live
            // PID does not have.
            let own = own_creation();
            assert!(recorded_child_absent(
                std::process::id(),
                Some(own + 10_000_000)
            ));
            assert!(recorded_child_absent(
                std::process::id(),
                Some(own - 10_000_000)
            ));
        }

        #[test]
        fn an_exited_and_released_child_is_absent() {
            let mut child = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "exit", "0"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn");
            let pid = child.id();
            let recorded = Process::open(pid, ProcessAccess::QueryLimited)
                .and_then(|process| process.creation_time())
                .ok();
            child.wait().expect("wait");
            drop(child);
            // The child's own handle is released; the process object is freed
            // once no handle names it. Allow the system a moment.
            let recorded = recorded.expect("creation time read while the child ran or lingered");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !recorded_child_absent(pid, Some(recorded)) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "an exited, released child never read as absent: {:?}",
                    observe(pid)
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        #[test]
        fn a_running_child_is_present_until_it_is_gone() {
            let mut child = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "ping", "-n", "30", "127.0.0.1"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn");
            let pid = child.id();
            let recorded = Process::open(pid, ProcessAccess::QueryLimited)
                .expect("open child")
                .creation_time()
                .expect("child creation time");
            assert!(!recorded_child_absent(pid, Some(recorded)));
            child.kill().expect("kill");
            child.wait().expect("wait");
        }

        #[test]
        fn legacy_and_zero_identity_fail_closed_even_for_a_missing_pid() {
            // PID 0 is never a real child; a legacy witness is not looked up.
            assert!(!recorded_child_absent(u32::MAX - 3, None));
            assert!(!recorded_child_absent(u32::MAX - 3, Some(0)));
            assert!(!recorded_child_absent(0, Some(T)));
        }

        #[test]
        fn a_process_that_cannot_be_inspected_is_never_read_as_absent() {
            // The System process (PID 4) refuses a query-limited open for an
            // ordinary user. Wherever this host allows it instead, the open
            // succeeds and a creation time comes back; either way it is not
            // "not found".
            let observed = observe(4);
            assert_ne!(observed, Observed::NotFound, "{observed:?}");
            if observed == Observed::Unobservable {
                assert!(!recorded_child_absent(4, Some(T)));
            }
        }
    }
}
