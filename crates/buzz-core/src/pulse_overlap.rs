//! Two umbrellas touching the same file, computed rather than reported.
//!
//! # The rule, and the line nothing crosses
//!
//! **There is no cross-umbrella wake.** This module emits rows. It never
//! produces an event, a target, a wake, or anything else that places work on
//! another umbrella's seat — one team's lead never puts a task on another
//! team's builder. A lead who wants to act publishes a `note` citing the other
//! umbrella's commit or checkpoint (note refs are pointers, and cross-umbrella
//! pointers are allowed) and messages that lead's pubkey over the relay. Both
//! of those are a person's choice, made with their own key, on their own
//! surface. Contact stays judgment; only the *observation* is mechanical.
//!
//! # Mechanical means mechanical
//!
//! A row exists when two **different** umbrellas' newest checkpoints name the
//! same path by **exact string equality**. Not a directory-prefix guess, not a
//! fuzzy match, not "these look related": a guess here reads as a collision two
//! teams do not have, and one false overlap costs more attention than the ten
//! true ones save.
//!
//! # Composed only from what the reader can already query
//!
//! Every side of a row comes from events the reader supplied — the relay's
//! 30618 ref state and the seat's own kind 44246 checkpoint. If either side is
//! unreadable there is **no row**: never a leak, never a guess. A reader who
//! cannot read one umbrella's records learns nothing about it here.

use std::collections::{BTreeMap, BTreeSet};

use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pulse_mission::{short_hex, PulseMissionLine, PulseMissionNames};

/// How many shared paths one overlap row names before the rest are counted.
pub const MAX_PULSE_OVERLAP_PATHS: usize = 6;

/// How many overlap rows one digest carries.
pub const MAX_PULSE_OVERLAP_ROWS: usize = 12;

/// The stated reason this module produces no event of any kind.
pub const PULSE_OVERLAP_NO_WAKE: &str =
    "An overlap row is shown to both sides and wakes nobody: a lead may note or \
     message the other lead, and no record ever places work on another umbrella's seat";

/// One umbrella's newest wip commit, paired with the checkpoint naming that SHA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseOverlapSide {
    /// Umbrella key.
    pub session_key: String,
    /// The seat that made the commit.
    pub author_pubkey: String,
    /// The commit the wip ref stands at.
    pub sha: String,
    /// The checkpoint's own `created_at`, Unix seconds — display only.
    pub as_of: Option<i64>,
    /// Paths the checkpoint named, verbatim.
    pub files: Vec<String>,
}

/// One computed overlap, before it is rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseOverlapFacts {
    /// Shared paths, sorted, bounded by [`MAX_PULSE_OVERLAP_PATHS`].
    pub paths: Vec<String>,
    /// Shared paths beyond the bound.
    pub paths_truncated: usize,
    /// Exactly the two sides, in umbrella-key order.
    pub sides: Vec<PulseOverlapSide>,
}

/// One overlap row as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseOverlapRow {
    /// Shared paths, bounded.
    pub paths: Vec<String>,
    /// How many more paths are shared but not named.
    pub paths_truncated: usize,
    /// The two seats.
    pub seats: Vec<PulseOverlapSeatRow>,
    /// The sentences, composed in Rust.
    pub lines: Vec<PulseMissionLine>,
}

/// One side of an overlap row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulseOverlapSeatRow {
    /// Umbrella key.
    pub session_key: String,
    /// The seat that made the commit.
    pub author_pubkey: String,
    /// The commit.
    pub sha: String,
    /// Age of the checkpoint, or `null` when nothing readable dated it.
    pub age_seconds: Option<i64>,
}

/// The `files` a kind 44246 checkpoint named, when it named any.
///
/// Reads the event's own content JSON because `checkpoint.files` is Lane L5's
/// field to land and the decoded struct does not carry it yet. Bounded at
/// 64 paths of 256 bytes exactly as §1e specifies, so a malformed or hostile
/// record cannot make a row unbounded. Until L5 lands the field this returns
/// `None` for every checkpoint and no overlap row is ever computed — the honest
/// failure, because a row asserted from paths nobody published would be a guess.
pub fn pulse_checkpoint_files(event: &Event) -> Option<Vec<String>> {
    let content: Value = serde_json::from_str(&event.content).ok()?;
    let files = content.get("body")?.get("files")?.as_array()?;
    let mut out: Vec<String> = Vec::new();
    for entry in files.iter().take(64) {
        let path = entry.as_str()?;
        if path.is_empty() || path.len() > 256 {
            return None;
        }
        out.push(path.to_owned());
    }
    Some(out)
}

/// Pair every two **different** umbrellas whose newest checkpoints share a path.
///
/// One row per pair, not one per path: two teams editing four of the same files
/// have one collision to talk about, not four.
pub fn fold_pulse_overlaps(sides: &[PulseOverlapSide]) -> Vec<PulseOverlapFacts> {
    let mut newest: BTreeMap<&str, &PulseOverlapSide> = BTreeMap::new();
    for side in sides {
        let entry = newest.entry(side.session_key.as_str()).or_insert(side);
        if (side.as_of, &side.sha) > (entry.as_of, &entry.sha) {
            *entry = side;
        }
    }
    let ordered: Vec<&PulseOverlapSide> = newest.into_values().collect();

    let mut rows: Vec<PulseOverlapFacts> = Vec::new();
    for (index, left) in ordered.iter().enumerate() {
        for right in ordered.iter().skip(index + 1) {
            // Different umbrellas only: the same path inside one umbrella is
            // one team's own business and is not a collision.
            if left.session_key == right.session_key {
                continue;
            }
            let left_paths: BTreeSet<&str> = left.files.iter().map(String::as_str).collect();
            let shared: Vec<String> = right
                .files
                .iter()
                .filter(|path| left_paths.contains(path.as_str()))
                .cloned()
                .collect::<BTreeSet<String>>()
                .into_iter()
                .collect();
            if shared.is_empty() {
                continue;
            }
            let paths_truncated = shared.len().saturating_sub(MAX_PULSE_OVERLAP_PATHS);
            let mut paths = shared;
            paths.truncate(MAX_PULSE_OVERLAP_PATHS);
            rows.push(PulseOverlapFacts {
                paths,
                paths_truncated,
                sides: vec![(*left).clone(), (*right).clone()],
            });
        }
    }
    rows.truncate(MAX_PULSE_OVERLAP_ROWS);
    rows
}

/// Compose the overlap sentences — the only place they exist.
pub fn render_pulse_overlap_rows(
    facts: &[PulseOverlapFacts],
    names: &PulseMissionNames,
    now_unix: i64,
) -> Vec<PulseOverlapRow> {
    facts
        .iter()
        .map(|row| {
            let seats: Vec<PulseOverlapSeatRow> = row
                .sides
                .iter()
                .map(|side| PulseOverlapSeatRow {
                    session_key: side.session_key.clone(),
                    author_pubkey: side.author_pubkey.clone(),
                    sha: side.sha.clone(),
                    age_seconds: side
                        .as_of
                        .and_then(|as_of| now_unix.checked_sub(as_of))
                        .filter(|seconds| *seconds >= 0),
                })
                .collect();
            let mut lines = Vec::new();
            let described = row
                .sides
                .iter()
                .zip(seats.iter())
                .map(|(side, seat)| {
                    let who = names.who(&side.author_pubkey);
                    let mut text = format!(
                        "{who} ({}) {}",
                        short_hex(&side.session_key),
                        short_hex(&side.sha)
                    );
                    if let Some(seconds) = seat.age_seconds {
                        text.push_str(&format!(" {}", relative(seconds)));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join(", and ");
            lines.push(PulseMissionLine {
                id: "overlap".to_owned(),
                text: format!("Overlap · {}: {described}", row.paths.join(", ")),
            });
            if row.paths_truncated > 0 {
                let noun = if row.paths_truncated == 1 {
                    "path"
                } else {
                    "paths"
                };
                lines.push(PulseMissionLine {
                    id: "overlap-truncated".to_owned(),
                    text: format!("{} more shared {noun} not shown", row.paths_truncated),
                });
            }
            PulseOverlapRow {
                paths: row.paths.clone(),
                paths_truncated: row.paths_truncated,
                seats,
                lines,
            }
        })
        .collect()
}

fn relative(seconds: i64) -> String {
    if seconds < 60 {
        return "just now".to_owned();
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

#[cfg(test)]
#[path = "pulse_overlap_tests.rs"]
mod tests;
