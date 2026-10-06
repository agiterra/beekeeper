//! What the host can see of the provider's live sessions, read-only.
//!
//! The provider persists its session map in `state.json` inside its state
//! directory, and that file is **replaced atomically** — written to a temp
//! file, fsynced, renamed — so "a torn write can therefore never produce a
//! half-updated" record (`beekeeper-session-provider/src/state.rs`). That property
//! is what makes this safe: the host opens the file read-only, with no lock, no
//! new channel into the provider, and no race.
//!
//! # Why a narrow deserializer rather than the provider's own type
//!
//! `Snapshot` is private to the provider and carries a great deal the host has
//! no business knowing — execution bindings, pack refs, retirement records.
//! Declaring only the handful of fields a status display needs, with `default`
//! on every one, means the provider can add, reshape or remove anything else
//! without this reader breaking. A shared struct would couple a menu bar to
//! the provider's internal state shape, which is the kind of coupling that
//! stops a refactor for no good reason.
//!
//! # The snapshot is stamped, on purpose
//!
//! A read of a file is a fact about a moment. [`SessionSnapshot::read_at`]
//! travels with the rows so a stale snapshot renders as stale rather than as
//! truth — a provider that died holding a session would otherwise leave that
//! session on display forever.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The provider's state file inside its state directory.
const STATE_FILE: &str = "state.json";

/// One session the provider is holding, as much of it as a status display
/// needs.
///
/// **Not necessarily doing anything.** The provider keeps a record per session
/// it has attached to, so most rows here are sessions it *remembers*; the ones
/// working are those with `turn_started_at_ms` set. This type was called
/// `LiveSession`, and the name cost something immediately: reading
/// `bee host status` on a real machine, 33 rows with no turn among them got
/// read as 33 live sessions. Hence the rename, and
/// [`SessionSnapshot::turns_in_flight`] so a caller does not have to know the
/// rule to get the number right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    /// Producer-minted session id.
    pub session_id: String,
    /// Generation number; advances on each provider reattach.
    #[serde(default)]
    pub generation: u64,
    /// Channel every event for this session is published into.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Runtime slug behind the driver ("claude", "codex", …).
    #[serde(default)]
    pub runtime: Option<String>,
    /// The agent seat this execution runs as, lowercase 64-hex, or `None` for
    /// a human-created execution.
    #[serde(default)]
    pub actor: Option<String>,
    /// The role slug the seat holds within its umbrella.
    #[serde(default)]
    pub role: Option<String>,
    /// When the turn in flight started, milliseconds since the Unix epoch, or
    /// `None` when no turn is open.
    ///
    /// An absolute instant, never a formatted duration: the client relabels
    /// from its own clock once a second, which is the whole reason it does not
    /// need to poll once a second.
    #[serde(default)]
    pub turn_started_at_ms: Option<i64>,
}

/// The provider's session map at one moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    /// RFC 3339 stamp of when this was read off disk.
    pub read_at: String,
    /// Why there are no rows, when that is not simply "none are running".
    ///
    /// Present so "the provider has no sessions" and "the host could not read
    /// the provider's state" are different answers. They need different words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// Every session the provider is holding, ordered by session id.
    ///
    /// Most of these are not working — see [`SessionRow`]. Count
    /// [`turns_in_flight`](Self::turns_in_flight) rather than `sessions.len()`
    /// for "how many agents are busy".
    pub sessions: Vec<SessionRow>,
}

impl SessionSnapshot {
    /// How many of these sessions have a turn open.
    ///
    /// The number a person means by "how many agents are running". Provided
    /// here so every consumer does not re-derive it — and the one that got it
    /// wrong was a human reading the JSON, which is exactly who a status
    /// command is for.
    pub fn turns_in_flight(&self) -> usize {
        self.sessions
            .iter()
            .filter(|session| session.turn_started_at_ms.is_some())
            .count()
    }
}

/// Only the fields this reader needs, so the provider stays free to change the
/// rest.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSnapshot {
    #[serde(default)]
    sessions: BTreeMap<String, WireSession>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSession {
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    runtime: Option<String>,
    #[serde(default)]
    actor: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    open_turn: Option<WireOpenTurn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireOpenTurn {
    #[serde(default)]
    started_at_ms: i64,
}

/// Read the provider's session map, or say why it could not be read.
///
/// Never an `Err`: a status call must always answer, and "unavailable, because
/// …" is an answer. Returning an error here would make the whole `status`
/// response fail over a detail, which is how a client ends up showing nothing
/// when it could have shown almost everything.
pub fn read_sessions(state_dir: &Path) -> SessionSnapshot {
    let read_at = beekeeper_host_core::logs::now_iso();
    let path = state_dir.join(STATE_FILE);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The provider has not written a snapshot yet. That is not a
            // failure and it is not "no sessions" either — say which.
            return SessionSnapshot {
                read_at,
                unavailable: Some(format!(
                    "the provider has not written {} yet",
                    path.display()
                )),
                sessions: Vec::new(),
            };
        }
        Err(error) => {
            return SessionSnapshot {
                read_at,
                unavailable: Some(format!("could not read {}: {error}", path.display())),
                sessions: Vec::new(),
            }
        }
    };
    let wire: WireSnapshot = match serde_json::from_str(&content) {
        Ok(wire) => wire,
        Err(error) => {
            return SessionSnapshot {
                read_at,
                unavailable: Some(format!("could not parse {}: {error}", path.display())),
                sessions: Vec::new(),
            }
        }
    };
    let mut sessions: Vec<SessionRow> = wire
        .sessions
        .into_iter()
        .map(|(key, session)| SessionRow {
            session_id: if session.session_id.is_empty() {
                key
            } else {
                session.session_id
            },
            generation: session.generation,
            channel_id: session.channel_id,
            runtime: session.runtime,
            actor: session.actor,
            role: session.role,
            turn_started_at_ms: session.open_turn.map(|turn| turn.started_at_ms),
        })
        .collect();
    sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    SessionSnapshot {
        read_at,
        unavailable: None,
        sessions,
    }
}

#[cfg(test)]
mod tests {
    /// "How many sessions" and "how many agents are working" are different
    /// numbers, and the row count is the wrong one.
    #[test]
    fn turns_in_flight_counts_only_the_sessions_with_a_turn_open() {
        let row = |id: &str, started: Option<i64>| SessionRow {
            session_id: id.to_string(),
            generation: 1,
            channel_id: None,
            runtime: None,
            actor: None,
            role: None,
            turn_started_at_ms: started,
        };
        let snapshot = SessionSnapshot {
            read_at: "2026-09-30T00:00:00Z".to_string(),
            unavailable: None,
            sessions: vec![
                row("a", None),
                row("b", Some(1_759_000_000_000)),
                row("c", None),
            ],
        };
        assert_eq!(snapshot.sessions.len(), 3, "three sessions are remembered");
        assert_eq!(snapshot.turns_in_flight(), 1, "one agent is working");
    }

    use super::*;

    #[test]
    fn an_unwritten_snapshot_is_not_the_same_answer_as_no_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let snapshot = read_sessions(dir.path());
        assert!(snapshot.sessions.is_empty());
        let unavailable = snapshot.unavailable.expect("must say why");
        assert!(unavailable.contains("has not written"), "{unavailable}");

        // An actually-empty snapshot says nothing is wrong.
        std::fs::write(
            dir.path().join(STATE_FILE),
            r#"{"version":1,"sessions":{}}"#,
        )
        .expect("write");
        let snapshot = read_sessions(dir.path());
        assert!(snapshot.sessions.is_empty());
        assert_eq!(snapshot.unavailable, None);
    }

    #[test]
    fn unreadable_and_unparseable_state_files_say_which() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(STATE_FILE), "{ not json").expect("write");
        let unavailable = read_sessions(dir.path()).unavailable.expect("must say why");
        assert!(unavailable.contains("could not parse"), "{unavailable}");
    }

    /// Everything else in the provider's snapshot must be ignorable: the
    /// provider adds fields to `state.json` regularly, and a reader that
    /// refused an unknown one would break the menu bar on every release.
    #[test]
    fn fields_this_reader_does_not_know_are_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join(STATE_FILE),
            r#"{
                "version": 7,
                "somethingNew": {"nested": [1, 2, 3]},
                "watermarks": {"a": 1},
                "sessions": {
                    "s-2": {
                        "sessionId": "s-2",
                        "generation": 3,
                        "channelId": "11111111-1111-1111-1111-111111111111",
                        "runtime": "claude",
                        "actor": "aa",
                        "role": "lead",
                        "executionBinding": {"whatever": true},
                        "openTurn": {"turnId": "t", "startedAtMs": 1750000000000,
                                     "commandId": null, "teamWakeEligible": true}
                    },
                    "s-1": {"sessionId": "s-1", "cwd": "/tmp"}
                }
            }"#,
        )
        .expect("write");
        let snapshot = read_sessions(dir.path());
        assert_eq!(snapshot.unavailable, None);
        let ids: Vec<&str> = snapshot
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["s-1", "s-2"], "rows are ordered by session id");
        let second = &snapshot.sessions[1];
        assert_eq!(second.generation, 3);
        assert_eq!(second.role.as_deref(), Some("lead"));
        assert_eq!(second.turn_started_at_ms, Some(1_750_000_000_000));
        // A session with no open turn carries no start time rather than a zero.
        assert_eq!(snapshot.sessions[0].turn_started_at_ms, None);
    }

    /// The wire must carry the turn's absolute start, and no formatted
    /// duration: a formatted duration is only true when it was written.
    #[test]
    fn the_wire_carries_milliseconds_and_no_elapsed_string() {
        let json = serde_json::to_string(&SessionSnapshot {
            read_at: "2026-09-30T00:00:00Z".to_string(),
            unavailable: None,
            sessions: vec![SessionRow {
                session_id: "s-1".to_string(),
                generation: 1,
                channel_id: None,
                runtime: None,
                actor: None,
                role: None,
                turn_started_at_ms: Some(1_750_000_000_000),
            }],
        })
        .expect("encode");
        assert!(json.contains("\"turnStartedAtMs\":1750000000000"), "{json}");
        assert!(json.contains("\"readAt\""), "{json}");
        assert!(!json.contains("elapsed"), "{json}");
    }
}
