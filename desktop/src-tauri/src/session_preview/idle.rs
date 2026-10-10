//! The idle close for hidden agent pages (ledger 371(d)).
//!
//! An agent may open a page while its session's Browser tab is not on screen;
//! the page then lives hidden (`placement: "hidden"`) and the person is told
//! it exists. If nobody ever shows it and no agent op touches it for
//! [`super::AGENT_HIDDEN_IDLE_SECS`], it is closed — not as the person (agents may
//! open again) — and the state carries [`ClosedReason::IDLE_HIDDEN`] so
//! neither the person nor the agent is left guessing where it went.

use std::time::Duration;

use tauri::AppHandle;

use super::{emit_state, unix_now, view, with_record, ClosedReason};

/// How often a watcher re-checks a hidden agent page.
pub const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// What one check of the watcher decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleVerdict {
    /// Keep watching.
    Wait,
    /// Close it now.
    Close,
    /// Stop watching: it closed, or someone has seen it.
    Stop,
}

/// Decide one check at unix time `now`. Pure over the record.
pub fn idle_verdict(record: &super::PreviewRecord, now: u64) -> IdleVerdict {
    if !record.has_view || record.ever_shown {
        IdleVerdict::Stop
    } else if record.idle_close_due(now) {
        IdleVerdict::Close
    } else {
        IdleVerdict::Wait
    }
}

/// Note an agent's open or op (resets the idle clock) and make sure one
/// watcher runs for the channel while its page is hidden and unseen.
pub fn note_agent_activity(app: &AppHandle, channel_id: &str) {
    let start = with_record(channel_id, |record| {
        record.last_agent_activity = unix_now();
        if record.idle_watch || record.ever_shown {
            return false;
        }
        record.idle_watch = true;
        true
    });
    if !start {
        return;
    }
    let app = app.clone();
    let channel = channel_id.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(IDLE_CHECK_INTERVAL).await;
            let verdict = with_record(&channel, |record| {
                let verdict = idle_verdict(record, unix_now());
                if verdict != IdleVerdict::Wait {
                    record.idle_watch = false;
                }
                verdict
            });
            match verdict {
                IdleVerdict::Wait => continue,
                IdleVerdict::Stop => return,
                IdleVerdict::Close => {
                    if let Err(error) = view::close(&app, &channel, false).await {
                        eprintln!("session-preview: idle close failed: {}", error.message);
                    }
                    with_record(&channel, |record| {
                        record.closed_reason = Some(ClosedReason::IDLE_HIDDEN);
                    });
                    emit_state(&app, &channel);
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_preview::{Binding, PreviewRecord, AGENT_HIDDEN_IDLE_SECS};
    use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;

    fn hidden_agent_page(last: u64) -> PreviewRecord {
        let mut record = PreviewRecord::new("chan");
        record.has_view = true;
        record.slot_window = Some("main".into());
        record.last_agent_activity = last;
        record.binding = Binding::Agent {
            target: CodingSessionTarget {
                driver: "claude".into(),
                instance_id: "i".into(),
                session_id: "S".into(),
                generation: 1,
            },
            execution_id: "e".into(),
        };
        record
    }

    #[test]
    fn an_unseen_hidden_agent_page_closes_after_the_idle_window() {
        let record = hidden_agent_page(1_000);
        assert_eq!(
            idle_verdict(&record, 1_000 + AGENT_HIDDEN_IDLE_SECS - 1),
            IdleVerdict::Wait
        );
        assert_eq!(
            idle_verdict(&record, 1_000 + AGENT_HIDDEN_IDLE_SECS),
            IdleVerdict::Close
        );
    }

    #[test]
    fn a_page_someone_has_seen_or_that_is_shown_is_never_closed_for_idleness() {
        let mut seen = hidden_agent_page(0);
        seen.ever_shown = true;
        assert_eq!(idle_verdict(&seen, u64::MAX), IdleVerdict::Stop);

        let mut docked = hidden_agent_page(0);
        docked.slot = Some(crate::session_preview::geometry::SlotRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        });
        assert_eq!(idle_verdict(&docked, u64::MAX), IdleVerdict::Wait);

        let mut person = hidden_agent_page(0);
        person.binding = Binding::None;
        assert_eq!(idle_verdict(&person, u64::MAX), IdleVerdict::Wait);

        let mut gone = hidden_agent_page(0);
        gone.has_view = false;
        assert_eq!(idle_verdict(&gone, u64::MAX), IdleVerdict::Stop);
    }
}
