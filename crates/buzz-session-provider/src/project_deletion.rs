//! Whether a project an execution runs under has been deleted, from the
//! project's own deletion event.
//!
//! A project is deleted by the existing NIP-09 route: a kind 5 naming the
//! project coordinate in an `a` tag (`30621:<creator>:<dtag>`). The relay
//! stores such a deletion only after admitting its signer — the creator, the
//! creator's owner, or a roster Owner of the project
//! (`buzz-relay` `validate_standard_deletion_event`) — and then stops serving
//! the head it deleted. So a tombstone the relay serves carries the relay's
//! own authority decision; what this module adds is whether that deletion is
//! *applied to the head this host knows*:
//!
//! * **Applied** — the tombstone is at least as new as the head this host last
//!   saw, and a successful read shows the head gone.
//! * **Re-created** — the relay serves a head newer than the tombstone.
//! * **Not applied** — the relay still serves a head the tombstone is newer
//!   than.
//! * **Unknown** — a failed or malformed read, or a head this host never saw
//!   (it cannot tell "deleted" from "not visible to it"). Never a deletion.
//!
//! Delivery is one authenticated subscription (the
//! [`crate::action_step_listener`] pattern): `{"kinds":[5],"#a":[<projects
//! with open executions>]}`, re-issued when that set changes, replaying stored
//! tombstones on every connect and liveness probe, and idle while the set is
//! empty. A candidate is checked off the provider's loop; the provider stops
//! the affected executions. Starts and resumes re-check the same facts.
//!
//! **Not done here yet: turn checkpoint refs.** The checkpoints brief
//! (Host/provider step 6) says a project's deletion also deletes its sessions'
//! `refs/beekeeper/checkpoints/<session>/…`. This module only stops
//! executions; it deletes no ref. Three consequences, until the provider's
//! checkpoint writer (`turn_checkpoint.rs`, its first caller) also retires
//! what it wrote:
//!
//! * refs of a session that ran in the project's own checkout are never
//!   retired — no seat worktree prune ever names them;
//! * refs of a seat worktree whose record carries no `session_id` stay after
//!   that tree is pruned (`worktree_prune.rs` will not guess whose they were);
//! * a deletion observed while the project has no open execution is never
//!   seen at all, since the subscription above names only those projects.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use buzz_acp::relay::RestClient;
use buzz_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
use nostr::{Alphabet, Event, Keys, Kind, SingleLetterTag, Tag};
use serde_json::json;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;

use crate::action_step_listener::{
    LISTENER_EVENT_CAPACITY, LISTENER_FIRST_BACKOFF, LISTENER_IDLE, LISTENER_MAX_BACKOFF,
    LISTENER_PROBE_INTERVAL, LISTENER_PROBE_TIMEOUT,
};

/// Kind of a NIP-MP project head.
const KIND_PROJECT: u16 = 30621;
/// Prefix of the listener's subscription ids.
const SUBSCRIPTION_PREFIX: &str = "csp-project-deletions";

/// The creator and `d` tag of a project coordinate, or `None` when it is not
/// one.
fn coordinate_parts(project_ref: &str) -> Option<(nostr::PublicKey, &str)> {
    let mut parts = project_ref.splitn(3, ':');
    let kind = parts.next()?;
    let owner = parts.next()?;
    let dtag = parts.next()?;
    (kind == KIND_PROJECT.to_string() && !dtag.is_empty())
        .then(|| {
            nostr::PublicKey::from_hex(owner)
                .ok()
                .map(|owner| (owner, dtag))
        })
        .flatten()
}

/// What the relay shows about one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectFact {
    /// The head is served; no deletion applies to it.
    Present {
        /// The served head's `created_at`.
        head_at: u64,
    },
    /// The deletion is applied to the head this host knew.
    Deleted,
    /// A tombstone exists, but the relay still serves a head it did not
    /// remove, or a newer one.
    NotApplied,
    /// Nothing can be concluded.
    Unknown(String),
}

/// Judge a tombstone (`tombstone_at`) against the relay's current head and
/// the head this host last saw.
pub(crate) async fn check(
    rest: &RestClient,
    project: &str,
    tombstone_at: u64,
    seen_head_at: Option<u64>,
) -> ProjectFact {
    match head(rest, project).await {
        Err(reason) => ProjectFact::Unknown(reason),
        Ok(Some(head_at)) if head_at > tombstone_at => ProjectFact::Present { head_at },
        Ok(Some(_)) => ProjectFact::NotApplied,
        Ok(None) => match seen_head_at {
            Some(seen) if tombstone_at >= seen => ProjectFact::Deleted,
            Some(_) => ProjectFact::Unknown(
                "the tombstone is older than the head this host saw, and the head is not served"
                    .to_owned(),
            ),
            None => ProjectFact::Unknown(
                "this host never saw the project's head, so a missing head is not proof of \
                 deletion"
                    .to_owned(),
            ),
        },
    }
}

/// The start/resume check: the newest served tombstone, judged by [`check`];
/// without one, whatever the head read says.
pub(crate) async fn revalidate(
    rest: &RestClient,
    project: &str,
    seen_head_at: Option<u64>,
) -> ProjectFact {
    if coordinate_parts(project).is_none() {
        return ProjectFact::Unknown("not a project coordinate".to_owned());
    }
    let filter = nostr::Filter::new()
        .kind(Kind::EventDeletion)
        .custom_tags(SingleLetterTag::lowercase(Alphabet::A), [project])
        .limit(20);
    let rows = match rest.query(&[filter]).await {
        Ok(rows) => rows,
        Err(error) => return ProjectFact::Unknown(format!("the deletion query failed: {error}")),
    };
    let Some(rows) = rows.as_array() else {
        return ProjectFact::Unknown("the deletion query returned a non-array response".to_owned());
    };
    let newest = rows
        .iter()
        .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
        .filter(|event| names_project(event, project))
        .map(|event| event.created_at.as_secs())
        .max();
    match newest {
        Some(tombstone_at) => check(rest, project, tombstone_at, seen_head_at).await,
        None => match head(rest, project).await {
            Ok(Some(head_at)) => ProjectFact::Present { head_at },
            Ok(None) => ProjectFact::Unknown("no project head is served to this host".to_owned()),
            Err(reason) => ProjectFact::Unknown(reason),
        },
    }
}

/// A signature-verified kind 5 naming `project` in an `a` tag.
fn names_project(event: &Event, project: &str) -> bool {
    event.kind == Kind::EventDeletion
        && event.verify().is_ok()
        && event.tags.iter().any(|tag| {
            let values = tag.as_slice();
            values.first().map(String::as_str) == Some("a")
                && values.get(1).map(String::as_str) == Some(project)
        })
}

/// The `created_at` of the head the relay serves, `None` when a successful
/// read returned none.
async fn head(rest: &RestClient, project: &str) -> Result<Option<u64>, String> {
    let Some((owner, dtag)) = coordinate_parts(project) else {
        return Err("not a project coordinate".to_owned());
    };
    let filter = nostr::Filter::new()
        .kind(Kind::Custom(KIND_PROJECT))
        .author(owner)
        .identifier(dtag)
        .limit(1);
    let rows = rest
        .query(&[filter])
        .await
        .map_err(|error| format!("the project head query failed: {error}"))?;
    let rows = rows
        .as_array()
        .ok_or("the project head query returned a non-array response")?;
    Ok(rows
        .iter()
        .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
        .filter(|event| event.verify().is_ok())
        .map(|event| event.created_at.as_secs())
        .max())
}

/// What reaches the provider's main loop.
#[derive(Debug)]
pub enum ProjectDeletionEvent {
    /// A verified tombstone naming a watched project, not yet judged.
    Candidate {
        /// The project coordinate.
        project: String,
        /// The tombstone's `created_at`.
        tombstone_at: u64,
    },
    /// The judgement of a candidate, made off the provider's loop.
    Checked {
        /// The project coordinate.
        project: String,
        /// The judged tombstone's `created_at`: a head witnessed after it
        /// while the check ran makes a `Deleted` judgement stale.
        tombstone_at: u64,
        /// What the relay showed.
        fact: ProjectFact,
    },
}

/// The provider's handle on the running listener task.
#[derive(Debug)]
pub struct ProjectDeletionListener {
    watched: watch::Sender<Arc<BTreeSet<String>>>,
    events: mpsc::Sender<ProjectDeletionEvent>,
    task: tokio::task::JoinHandle<()>,
}

/// What the listener connects with.
#[derive(Clone)]
pub struct DeletionListenerConfig {
    /// Relay WebSocket URL.
    pub relay_url: String,
    /// This provider's signing keys.
    pub keys: Keys,
    /// NIP-OA authorization tag, when the deployment requires one.
    pub auth_tag: Option<Tag>,
}

impl ProjectDeletionListener {
    /// Start the listener, returning the handle and the receiver the main loop
    /// selects on.
    pub fn spawn(config: DeletionListenerConfig) -> (Self, mpsc::Receiver<ProjectDeletionEvent>) {
        let (events_tx, events_rx) = mpsc::channel(LISTENER_EVENT_CAPACITY);
        let (watched, watched_rx) = watch::channel(Arc::new(BTreeSet::new()));
        let task = tokio::spawn(run(config, watched_rx, events_tx.clone()));
        (
            Self {
                watched,
                events: events_tx,
                task,
            },
            events_rx,
        )
    }

    /// Replace the set of watched projects (those with open executions). A
    /// no-op when unchanged.
    pub fn watch(&self, projects: BTreeSet<String>) {
        if **self.watched.borrow() == projects {
            return;
        }
        let _ = self.watched.send(Arc::new(projects));
    }

    /// A sender the off-loop checks report through.
    pub fn reporter(&self) -> mpsc::Sender<ProjectDeletionEvent> {
        self.events.clone()
    }

    /// Stop the listener task.
    pub fn shutdown(self) {
        self.task.abort();
    }
}

/// Wait for the next event, or park forever once there is no queue.
pub async fn next_event(
    events: &mut Option<mpsc::Receiver<ProjectDeletionEvent>>,
) -> ProjectDeletionEvent {
    match events {
        Some(events) => match events.recv().await {
            Some(event) => event,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

async fn run(
    config: DeletionListenerConfig,
    mut watched: watch::Receiver<Arc<BTreeSet<String>>>,
    events: mpsc::Sender<ProjectDeletionEvent>,
) {
    let mut backoff = LISTENER_FIRST_BACKOFF;
    loop {
        let wanted = watched.borrow_and_update().clone();
        if wanted.is_empty() {
            if watched.changed().await.is_err() {
                return;
            }
            backoff = LISTENER_FIRST_BACKOFF;
            continue;
        }
        match serve(&config, &wanted, &mut watched, &events).await {
            Ok(true) => backoff = LISTENER_FIRST_BACKOFF,
            Ok(false) => return,
            Err(reason) => {
                tracing::warn!(
                    target: "csp::projects",
                    backoff_secs = backoff.as_secs(),
                    "project deletion listener connection ended: {reason}"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(LISTENER_MAX_BACKOFF);
            }
        }
    }
}

async fn subscribe(
    conn: &mut NostrWsConnection,
    wanted: &BTreeSet<String>,
    previous: Option<&str>,
    sequence: u64,
) -> Result<String, String> {
    if let Some(previous) = previous {
        conn.send_raw(&json!(["CLOSE", previous]))
            .await
            .map_err(|error| format!("close previous subscription: {error}"))?;
    }
    let id = format!("{SUBSCRIPTION_PREFIX}-{sequence}");
    let projects: Vec<&String> = wanted.iter().collect();
    let filter = json!({"kinds": [Kind::EventDeletion.as_u16()], "#a": projects});
    conn.send_raw(&json!(["REQ", &id, filter]))
        .await
        .map_err(|error| format!("subscribe: {error}"))?;
    Ok(id)
}

/// Serve one connection. `Ok(true)` when the watched set changed (reconnect
/// with the new filter), `Ok(false)` when the provider is gone.
async fn serve(
    config: &DeletionListenerConfig,
    wanted: &BTreeSet<String>,
    watched: &mut watch::Receiver<Arc<BTreeSet<String>>>,
    events: &mpsc::Sender<ProjectDeletionEvent>,
) -> Result<bool, String> {
    let mut conn = NostrWsConnection::connect_authenticated(
        &config.relay_url,
        &config.keys,
        config.auth_tag.as_ref(),
    )
    .await
    .map_err(|error| format!("connect: {error}"))?;
    let mut sequence = 0;
    let mut current = subscribe(&mut conn, wanted, None, sequence).await?;
    let mut last_message = Instant::now();
    let mut probe_at = last_message + LISTENER_PROBE_INTERVAL;
    let mut eose_by = Some(last_message + LISTENER_PROBE_TIMEOUT);
    loop {
        let now = Instant::now();
        if eose_by.is_some_and(|deadline| now >= deadline) {
            return Err(format!("no EOSE for {current}"));
        }
        if now.saturating_duration_since(last_message) >= LISTENER_IDLE {
            return Err("no relay message within the idle window".into());
        }
        if now >= probe_at {
            sequence += 1;
            current = subscribe(&mut conn, wanted, Some(&current), sequence).await?;
            probe_at = now + LISTENER_PROBE_INTERVAL;
            eose_by = Some(now + LISTENER_PROBE_TIMEOUT);
            continue;
        }
        let mut wait = LISTENER_IDLE
            .saturating_sub(now.saturating_duration_since(last_message))
            .min(probe_at.saturating_duration_since(now));
        if let Some(deadline) = eose_by {
            wait = wait.min(deadline.saturating_duration_since(now));
        }
        let wait = wait.max(Duration::from_millis(1));
        let message = tokio::select! {
            changed = watched.changed() => return Ok(changed.is_ok()),
            message = conn.next_event(wait) => message,
        };
        if message.is_ok() {
            last_message = Instant::now();
        }
        match message {
            Ok(RelayMessage::Event {
                subscription_id,
                event,
            }) if subscription_id.starts_with(SUBSCRIPTION_PREFIX) => {
                for project in wanted
                    .iter()
                    .filter(|project| names_project(&event, project))
                {
                    let candidate = ProjectDeletionEvent::Candidate {
                        project: project.clone(),
                        tombstone_at: event.created_at.as_secs(),
                    };
                    match events.try_send(candidate) {
                        Ok(()) => {}
                        // The next probe replays it.
                        Err(mpsc::error::TrySendError::Full(_)) => {}
                        Err(mpsc::error::TrySendError::Closed(_)) => return Ok(false),
                    }
                }
            }
            Ok(RelayMessage::Eose { subscription_id }) if subscription_id == current => {
                eose_by = None
            }
            Ok(RelayMessage::Closed {
                subscription_id,
                message,
            }) if subscription_id == current => {
                return Err(format!("relay closed the deletion subscription: {message}"));
            }
            Ok(_) | Err(WsClientError::Timeout) => {}
            Err(error) => return Err(format!("receive: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_project_coordinates_are_read() {
        let owner = "ab".repeat(32);
        assert!(coordinate_parts(&format!("30621:{owner}:demo")).is_some());
        assert!(coordinate_parts(&format!("30620:{owner}:demo")).is_none());
        assert!(coordinate_parts(&format!("30621:{owner}:")).is_none());
        assert!(coordinate_parts("30621:not-hex:demo").is_none());
    }
}
