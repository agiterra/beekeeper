//! Durable owner-notice recovery drained by the existing conversation backfill slot.
use super::*;

const CAPACITY: usize = 64;
const MAX_ATTEMPTS: u32 = 32;
#[derive(Debug, thiserror::Error)]
#[error("HOST_NOTICE_CAPACITY: {0}")]
pub(super) struct NoticeCapacity(&'static str);
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct PendingNotice {
    pub(super) event: Event,
    pub(super) channel: Uuid,
    actor: String,
    pub(super) hire_id: String,
    pub(super) hire: Option<Event>,
    attempts: u32,
    next_attempt: u64,
    error: Option<String>,
}
impl Provider {
    pub(super) fn retain_host_notice(
        &self,
        channel: Uuid,
        event: &Event,
        actor: &str,
        hire_id: &str,
    ) -> anyhow::Result<()> {
        let mut origins = self.conversation_origins()?;
        if origins.pending_notices.contains_key(&event.id.to_hex()) {
            return Ok(());
        }
        if serde_json::to_vec(event)?.len() > 16 * 1024 {
            return Err(NoticeCapacity("notice exceeds 16 KiB").into());
        }
        origins.pending_notices.retain(|id, _| {
            !self.state.is_command_consumed(id) && !self.state.is_command_refused(id)
        });
        if origins.pending_notices.len() >= CAPACITY {
            return Err(NoticeCapacity("64 retained notices; no obligation evicted").into());
        }
        origins.pending_notices.insert(
            event.id.to_hex(),
            PendingNotice {
                event: event.clone(),
                channel,
                actor: actor.into(),
                hire_id: hire_id.into(),
                hire: None,
                attempts: 0,
                next_attempt: crate::now_secs().saturating_add(2),
                error: None,
            },
        );
        self.save_conversation_origins(&origins)
    }

    pub(super) fn due_host_notice(&mut self, now: u64) -> anyhow::Result<Option<PendingNotice>> {
        let mut origins = self.conversation_origins()?;
        let before = origins.pending_notices.len();
        origins.pending_notices.retain(|id, _| {
            !self.state.is_command_consumed(id) && !self.state.is_command_refused(id)
        });
        let id = origins
            .pending_notices
            .iter()
            .filter(|(_, pending)| pending.attempts < MAX_ATTEMPTS && pending.next_attempt <= now)
            .min_by_key(|(id, pending)| (pending.next_attempt, *id))
            .map(|(id, _)| id.clone());
        let result = if let Some(id) = id {
            let pending = origins
                .pending_notices
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("pending notice disappeared"))?;
            let trusted = match verified::verify_host_notice_origin(
                self,
                pending.channel,
                &pending.event,
                &pending.actor,
            ) {
                Ok(hire) => {
                    pending.event.id.to_hex() == id
                        && hire.as_deref() == Some(pending.hire_id.as_str())
                }
                Err(error) if error.is::<verified::NoticeLookupPending>() => return Err(error),
                Err(_) => false,
            };
            if !trusted {
                origins.pending_notices.remove(&id);
                self.save_conversation_origins(&origins)?;
                tracing::warn!(notice = %id, "HOST_NOTICE_PENDING_UNVERIFIED: discarded without execution");
                return Ok(None);
            }
            pending.attempts += 1;
            pending.next_attempt = now.saturating_add((1u64 << pending.attempts.min(9)).min(300));
            if pending.attempts == MAX_ATTEMPTS {
                pending.error =
                    Some("HOST_NOTICE_RETRY_EXHAUSTED: parked; exact input retained".into());
            }
            // Already verified hire/transcript survives an unavailable lookup on retry.
            if let Some(notice) = origins
                .requests
                .get(&id)
                .and_then(|origin| origin.host_notice.as_ref())
            {
                pending.hire = Some(notice.hire_event.clone());
            }
            Some(pending.clone())
        } else {
            None
        };
        if result.is_some() || before != origins.pending_notices.len() {
            self.save_conversation_origins(&origins)?;
        }
        Ok(result)
    }

    pub(super) async fn finish_host_notice_retry(
        &mut self,
        pending: PendingNotice,
        result: Result<Vec<Event>, String>,
    ) -> anyhow::Result<()> {
        let id = pending.event.id.to_hex();
        let mut origins = self.conversation_origins()?;
        let Some(saved) = origins.pending_notices.get_mut(&id) else {
            return Ok(());
        };
        match result {
            Ok(events) => saved.hire = events.into_iter().next(),
            Err(error) => {
                if saved.attempts < MAX_ATTEMPTS {
                    saved.error = Some(error.chars().take(512).collect());
                }
                self.save_conversation_origins(&origins)?;
                return Ok(());
            }
        }
        self.save_conversation_origins(&origins)?;
        // Same admission, signature, owner and historical-source checks as live input.
        // The exact hire is cached only as data and is verified there again.
        if let Err(error) =
            Box::pin(self.handle_command_event(pending.channel, &pending.event)).await
        {
            let mut origins = self.conversation_origins()?;
            if let Some(saved) = origins.pending_notices.get_mut(&id) {
                if saved.attempts < MAX_ATTEMPTS {
                    saved.error = Some(error.to_string().chars().take(512).collect());
                }
            }
            self.save_conversation_origins(&origins)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn notice_write_fault(&mut self, phase: u8) -> anyhow::Result<()> {
        if self.conversation_backfill.notice_write_fault == Some(phase) {
            self.conversation_backfill.notice_write_fault = None;
            anyhow::bail!("injected Host notice write boundary {phase}");
        }
        Ok(())
    }
}
