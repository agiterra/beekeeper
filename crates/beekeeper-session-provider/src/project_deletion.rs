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
//! **Turn checkpoint refs (SV-55).** The checkpoints brief (Host/provider
//! step 6) says a project's deletion also deletes its sessions'
//! `refs/beekeeper/checkpoints/<session>/…`. Once a deletion is applied,
//! [`checkpoint_retirements`] names every session this host recorded under
//! that project — open or closed — grouped by the working directory it ran
//! in, and [`retire_checkpoint_refs`] deletes exactly those sessions' refs
//! from each repository, as the host with hooks off (no project code runs).
//! A session of any other project, and any ref outside one of those
//! sessions' own prefixes, is never listed. What is still not retired here:
//!
//! * refs of a session whose working directory is gone (a seat worktree
//!   already pruned): there is no repository left to reach from it, and
//!   `worktree_prune.rs` retired them with the tree when the record named the
//!   session — it will not guess when the record did not;
//! * refs of a session this host has no record of (another machine's, or a
//!   record already dropped);
//! * a deletion observed while the project has no open execution is never
//!   seen at all, since the subscription above names only those projects.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use beekeeper_acp::relay::RestClient;
use beekeeper_ws_client::{NostrWsConnection, RelayMessage, WsClientError};
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

/// Ceiling on retiring one repository's checkpoint refs.
const RETIRE_TIMEOUT: Duration = Duration::from_secs(20);

/// The sessions of `project` whose checkpoint refs go with it, grouped by the
/// working directory each ran in: `(session_id, project_ref, cwd)` for every
/// record this host holds, open or closed.
///
/// Only sessions recorded under exactly `project` are named, and only ids
/// that could have named a checkpoint ref at all.
// Called by `stop_project_executions` in `lib.rs` once a deletion applies.
pub(crate) fn checkpoint_retirements<'a>(
    sessions: impl IntoIterator<Item = (&'a str, Option<&'a str>, &'a Path)>,
    project: &str,
) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut by_cwd: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for (session_id, project_ref, cwd) in sessions {
        if project_ref != Some(project)
            || crate::turn_checkpoint_git::checkpoint_session_prefix(session_id).is_none()
        {
            continue;
        }
        by_cwd
            .entry(cwd.to_path_buf())
            .or_default()
            .insert(session_id.to_owned());
    }
    by_cwd
}

/// Delete each named session's `refs/beekeeper/checkpoints/<session>/…` from
/// the repository its working directory is in, and answer how many refs went.
///
/// Never fails the deletion that already happened: a directory that is gone
/// or is no longer a repository, or a git that refuses, is logged and the
/// rest are still retired.
pub(crate) async fn retire_checkpoint_refs(
    retirements: BTreeMap<PathBuf, BTreeSet<String>>,
) -> usize {
    let mut retired = 0;
    for (cwd, sessions) in retirements {
        if !cwd.is_dir() {
            tracing::info!(target: "csp::projects", sessions = sessions.len(),
                "checkpoint refs not retired: the sessions' working directory is gone");
            continue;
        }
        match tokio::time::timeout(RETIRE_TIMEOUT, delete_checkpoint_refs(&cwd, &sessions)).await {
            Ok(Ok(count)) => retired += count,
            Ok(Err(reason)) => tracing::warn!(target: "csp::projects",
                "a deleted project's checkpoint refs could not be retired: {reason}"),
            Err(_) => tracing::warn!(target: "csp::projects",
                "retiring a deleted project's checkpoint refs timed out"),
        }
    }
    retired
}

/// List every ref under the sessions' prefixes in `cwd`'s repository and
/// delete them in one `update-ref --stdin` transaction.
async fn delete_checkpoint_refs(cwd: &Path, sessions: &BTreeSet<String>) -> Result<usize, String> {
    let prefixes: Vec<String> = sessions
        .iter()
        .filter_map(|session| crate::turn_checkpoint_git::checkpoint_session_prefix(session))
        .collect();
    if prefixes.is_empty() {
        return Ok(0);
    }
    let mut list = tokio::process::Command::from(crate::host_command::metadata_git_command(cwd));
    list.args(["for-each-ref", "--format=%(refname)"])
        .args(&prefixes)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let listed = list
        .output()
        .await
        .map_err(|_| "git could not be started to list them".to_owned())?;
    if !listed.status.success() {
        return Err(format!("git for-each-ref failed ({})", listed.status));
    }
    let names: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| {
            prefixes
                .iter()
                .any(|prefix| name.starts_with(prefix.as_str()))
        })
        .map(str::to_owned)
        .collect();
    if names.is_empty() {
        return Ok(0);
    }
    let mut script = String::new();
    for name in &names {
        script.push_str("delete ");
        script.push_str(name);
        script.push('\n');
    }
    let mut delete = tokio::process::Command::from(crate::host_command::metadata_git_command(cwd));
    delete
        .args(["update-ref", "--no-deref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = delete
        .spawn()
        .map_err(|_| "git could not be started to delete them".to_owned())?;
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        // A failed write surfaces as git's own refusal below.
        let _ = stdin.write_all(script.as_bytes()).await;
    }
    let status = child
        .wait()
        .await
        .map_err(|_| "git did not finish deleting them".to_owned())?;
    if !status.success() {
        return Err(format!("git update-ref failed ({status})"));
    }
    Ok(names.len())
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

    const DELETED: &str = "30621:aa:deleted";
    const OTHER: &str = "30621:aa:other";

    #[test]
    fn only_the_deleted_projects_sessions_are_named_grouped_by_cwd() {
        let checkout = Path::new("/checkout");
        let seat = Path::new("/checkout-seat");
        let sessions = [
            ("a", Some(DELETED), checkout),
            ("b", Some(OTHER), checkout),
            ("c", Some(DELETED), seat),
            ("d", None, checkout),
            ("../e", Some(DELETED), checkout),
        ];
        let named = checkpoint_retirements(sessions, DELETED);
        let expected: BTreeMap<PathBuf, BTreeSet<String>> = [
            (checkout.to_path_buf(), BTreeSet::from(["a".to_owned()])),
            (seat.to_path_buf(), BTreeSet::from(["c".to_owned()])),
        ]
        .into();
        assert_eq!(named, expected);
    }

    /// `git` in `cwd`, hermetic; asserts success and returns stdout.
    fn git(cwd: &Path, args: &[&str]) -> String {
        let mut command = std::process::Command::new("git");
        for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let output = command
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Every repository here is a throwaway in the test's own temporary
    /// directory, never this checkout.
    #[tokio::test]
    async fn a_deleted_projects_session_refs_go_and_no_other_ref_does() {
        let dir = tempfile::tempdir().expect("tempdir");
        let checkout = dir.path().join("checkout");
        let seat = dir.path().join("seat");
        std::fs::create_dir_all(&checkout).expect("dir");
        git(&checkout, &["init", "-q", "-b", "main", "."]);
        std::fs::write(checkout.join("a.txt"), "a\n").expect("write");
        git(&checkout, &["add", "."]);
        git(&checkout, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
        git(
            &checkout,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "seat",
                seat.to_str().expect("utf8"),
            ],
        );
        let head = git(&checkout, &["rev-parse", "HEAD"]);
        let refs = [
            "refs/beekeeper/checkpoints/sess-a/1/base-1",
            "refs/beekeeper/checkpoints/sess-a/1/2",
            "refs/beekeeper/checkpoints/sess-a/2/5",
            "refs/beekeeper/checkpoints/sess-c/1/3",
            "refs/beekeeper/checkpoints/sess-a1/1/2",
            "refs/beekeeper/checkpoints/sess-b/1/2",
            "refs/heads/sess-a",
        ];
        for name in refs {
            git(&checkout, &["update-ref", name, &head]);
        }
        let gone = dir.path().join("pruned");
        let sessions = [
            ("sess-a", Some(DELETED), checkout.as_path()),
            ("sess-c", Some(DELETED), seat.as_path()),
            ("sess-b", Some(OTHER), checkout.as_path()),
            ("sess-gone", Some(DELETED), gone.as_path()),
        ];

        let retired = retire_checkpoint_refs(checkpoint_retirements(sessions, DELETED)).await;

        assert_eq!(
            retired, 4,
            "sess-a's three refs and sess-c's one, from a linked worktree"
        );
        let left = git(&checkout, &["for-each-ref", "--format=%(refname)"]);
        let left: Vec<&str> = left.lines().collect();
        assert_eq!(
            left,
            vec![
                "refs/beekeeper/checkpoints/sess-a1/1/2",
                "refs/beekeeper/checkpoints/sess-b/1/2",
                "refs/heads/main",
                "refs/heads/seat",
                "refs/heads/sess-a",
            ]
        );
    }
}
