//! The project artifact pin fold: from a bag of kind 44251 ops to the pinned
//! rows a member's sidebar shows, in order.
//!
//! Pure and total: any set of events in, one digest out, the same digest from
//! every client. The rules are the contract in
//! `conformance/project-artifact-pin-fold/CONTRACT.md` and are pinned by the
//! vectors beside it, which the TypeScript fold in Desktop and the Dart fold
//! in Mobile bind to as well. A rule implemented in only one fold is a defect.
//!
//! Rules, in the order the fold applies them:
//!
//! 1. **Decode.** An event that is not a 44251, whose tags do not carry
//!    exactly one `a` naming this project (compared after normalizing hex
//!    case), whose tags do not carry exactly one canonical `ar-repo`, or whose
//!    content fails [`crate::project_artifact_pin`] is counted in `ignored`
//!    and dropped. A well-formed op for a repository this project no longer
//!    pins is counted in `otherRepo` and dropped — saying so beats losing it.
//!    Duplicate ids keep the first.
//! 2. **Order.** Ops sort by `(created_at, id)` ascending. That pair is the
//!    only clock.
//! 3. **Exist.** A target exists in the digest when at least one `pin.set`
//!    names it. A `pin.rank` alone cannot conjure a row: it says where
//!    something sits, not that it is pinned, and a client that reordered a
//!    target someone else then un-pinned should not resurrect it. Those ops
//!    are counted in `ranksWithoutPin` rather than dropped in silence.
//! 4. **Fields.** Per target, per field, the write with the greatest
//!    `(created_at, id)` wins. `pinned` and `targetKind` come from `pin.set`
//!    only. `rank` is contested by both ops, with a `pin.set`'s own rank
//!    taking part under the set's key — so a `pin.rank` stamped *before* the
//!    set that introduced the target (a skewed clock) loses to it, exactly as
//!    NIP-TD resolves `item.add` against a later field op.
//! 5. **Order out.** Rows sort by `(rank, target)`, both bytewise. Equal ranks
//!    are legal and break on the target. `updatedAt` is the greatest applied
//!    op key for that target, and `by` the author of the winning `pin.set` —
//!    who pinned it, not who last nudged its order.
//!
//! Unpinned rows stay in the digest with `pinned: false`. A caller showing a
//! sidebar filters them; a caller drawing the pin control needs to know the
//! target was pinned once and is not now, and a row that vanished would make
//! the control guess.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::kind::{normalize_project_coordinate, KIND_PROJECT_ARTIFACT_PIN_OP};
use crate::project_artifact_pin::{
    decode_project_artifact_pin_op, PinTargetKind, ProjectArtifactPinOp, ProjectArtifactPinOpValue,
};
use crate::project_pack_source::normalize_repository_coordinate;

/// Exact `schema` value carried by a digest.
pub const PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA: &str = "buzz-project-artifact-pin-digest/v1";

/// One stored event as the fold sees it — the subset of a Nostr event the
/// rules read. A relay-stored event converts losslessly; the conformance
/// vectors are written in this shape directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinFoldEvent {
    /// Event id (64 hex).
    pub id: String,
    /// Author (64 hex).
    pub pubkey: String,
    /// Seconds since the epoch.
    pub created_at: u64,
    /// Event kind; anything but 44251 is ignored.
    pub kind: u32,
    /// Tags; the single `a` and single `ar-repo` tags are read.
    pub tags: Vec<Vec<String>>,
    /// Op content JSON.
    pub content: String,
}

impl From<&nostr::Event> for PinFoldEvent {
    fn from(event: &nostr::Event) -> Self {
        Self {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: crate::kind::event_kind_u32(event),
            tags: event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
        }
    }
}

/// One target the digest reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PinRow {
    /// The file path or folder prefix.
    pub target: String,
    /// `file` or `folder`.
    pub target_kind: String,
    /// Whether it shows in every member's sidebar now.
    pub pinned: bool,
    /// Its order key.
    pub rank: String,
    /// Who pinned it: the author of the winning `pin.set`.
    pub by: String,
    /// The greatest applied op key for this target.
    pub updated_at: u64,
}

/// What every reader gets from one bag of ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectArtifactPinDigest {
    /// Always [`PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA`].
    pub schema: String,
    /// The project coordinate folded.
    pub project: String,
    /// The agents repository folded.
    pub repo: String,
    /// Events dropped as malformed or mis-scoped.
    pub ignored: usize,
    /// Well-formed ops for another repository.
    pub other_repo: usize,
    /// `pin.rank` ops naming a target no `pin.set` ever introduced.
    pub ranks_without_pin: usize,
    /// Every target, ordered by `(rank, target)`.
    pub pins: Vec<PinRow>,
}

/// The pinned rows of a digest, in order — what a sidebar draws.
pub fn pinned_only(digest: &ProjectArtifactPinDigest) -> Vec<&PinRow> {
    digest.pins.iter().filter(|row| row.pinned).collect()
}

/// `(created_at, id)`: the only clock.
type Key = (u64, String);

struct Decoded {
    key: Key,
    pubkey: String,
    op: ProjectArtifactPinOp,
}

enum Decode {
    Op(ProjectArtifactPinOp),
    OtherRepo,
    Ignored,
}

fn single_tag<'a>(event: &'a PinFoldEvent, key: &str) -> Option<&'a str> {
    let mut found: Option<&str> = None;
    for tag in &event.tags {
        if tag.len() == 2 && tag[0] == key {
            if found.is_some() {
                return None;
            }
            found = Some(tag[1].as_str());
        }
    }
    found
}

fn decode(project: &str, repo: &str, event: &PinFoldEvent) -> Decode {
    if event.kind != KIND_PROJECT_ARTIFACT_PIN_OP {
        return Decode::Ignored;
    }
    let Some(coordinate) = single_tag(event, "a") else {
        return Decode::Ignored;
    };
    if normalize_project_coordinate(coordinate).as_deref() != Some(project) {
        return Decode::Ignored;
    }
    let Some(tag_repo) = single_tag(event, "ar-repo") else {
        return Decode::Ignored;
    };
    let Some(canonical_repo) = normalize_repository_coordinate(tag_repo) else {
        return Decode::Ignored;
    };
    if canonical_repo != tag_repo {
        return Decode::Ignored;
    }
    match decode_project_artifact_pin_op(&event.content, &canonical_repo) {
        Ok(_) if canonical_repo != repo => Decode::OtherRepo,
        Ok(op) => Decode::Op(op),
        Err(_) => Decode::Ignored,
    }
}

/// One field's winner: the value and the key that set it.
struct Slot<T> {
    value: Option<T>,
    key: Option<Key>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: None,
            key: None,
        }
    }
}

impl<T> Slot<T> {
    /// Take `value` when `key` beats what is held. Equal keys cannot happen:
    /// the id breaks every tie.
    fn offer(&mut self, key: &Key, value: T) {
        if self.key.as_ref().is_none_or(|held| *key > *held) {
            self.key = Some(key.clone());
            self.value = Some(value);
        }
    }
}

#[derive(Default)]
struct TargetState {
    pinned: Slot<bool>,
    target_kind: Slot<PinTargetKind>,
    rank: Slot<String>,
    /// The author of the winning `pin.set` — who pinned it.
    by: Slot<String>,
    updated_at: u64,
    /// At least one `pin.set` named this target.
    introduced: bool,
}

/// Fold `events` for `project` and its agents repository `repo` into a digest.
/// See the module doc for the rules. Both coordinates must already be
/// canonical; events naming any other project are ignored, and well-formed ops
/// for another repository are counted, not folded.
pub fn fold_project_artifact_pins(
    project: &str,
    repo: &str,
    events: &[PinFoldEvent],
) -> ProjectArtifactPinDigest {
    let mut ignored = 0usize;
    let mut other_repo = 0usize;
    let mut seen = HashSet::new();
    let mut ops: Vec<Decoded> = Vec::new();
    for event in events {
        if !seen.insert(event.id.clone()) {
            continue;
        }
        match decode(project, repo, event) {
            Decode::Op(op) => ops.push(Decoded {
                key: (event.created_at, event.id.clone()),
                pubkey: event.pubkey.clone(),
                op,
            }),
            Decode::OtherRepo => other_repo += 1,
            Decode::Ignored => ignored += 1,
        }
    }
    ops.sort_by(|a, b| a.key.cmp(&b.key));

    let mut targets: HashMap<String, TargetState> = HashMap::new();
    let mut ranks_without_pin = 0usize;
    // Introduce every target first, so a `pin.rank` that arrived before its
    // `pin.set` in the log still counts — the order of the bag is not the
    // order of the clock, and a reader must not depend on it.
    for decoded in &ops {
        if matches!(decoded.op.value, ProjectArtifactPinOpValue::PinSet { .. }) {
            targets
                .entry(decoded.op.target.clone())
                .or_default()
                .introduced = true;
        }
    }
    for decoded in &ops {
        let Some(state) = targets.get_mut(&decoded.op.target) else {
            ranks_without_pin += 1;
            continue;
        };
        if !state.introduced {
            ranks_without_pin += 1;
            continue;
        }
        match &decoded.op.value {
            ProjectArtifactPinOpValue::PinSet {
                target_kind,
                pinned,
                rank,
            } => {
                state.pinned.offer(&decoded.key, *pinned);
                state.target_kind.offer(&decoded.key, *target_kind);
                state.rank.offer(&decoded.key, rank.clone());
                state.by.offer(&decoded.key, decoded.pubkey.clone());
            }
            ProjectArtifactPinOpValue::PinRank { rank } => {
                state.rank.offer(&decoded.key, rank.clone());
            }
        }
        state.updated_at = state.updated_at.max(decoded.key.0);
    }

    let mut pins: Vec<PinRow> = targets
        .into_iter()
        .filter_map(|(target, state)| {
            Some(PinRow {
                target,
                target_kind: state.target_kind.value?.as_str().to_owned(),
                pinned: state.pinned.value?,
                rank: state.rank.value?,
                by: state.by.value?,
                updated_at: state.updated_at,
            })
        })
        .collect();
    pins.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.target.cmp(&b.target)));

    ProjectArtifactPinDigest {
        schema: PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA.to_owned(),
        project: project.to_owned(),
        repo: repo.to_owned(),
        ignored,
        other_repo,
        ranks_without_pin,
        pins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[derive(serde::Deserialize)]
    struct Vectors {
        schema: String,
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        project: String,
        repo: String,
        events: Vec<PinFoldEvent>,
        expected: Value,
    }

    #[test]
    fn conformance_vectors_fold_identically() {
        let raw = include_str!(
            "../../../conformance/project-artifact-pin-fold/fixtures/fold-vectors.json"
        );
        let vectors: Vectors = serde_json::from_str(raw).expect("vectors parse");
        assert_eq!(vectors.schema, "buzz-project-artifact-pin-fold-vectors/v1");
        assert!(!vectors.cases.is_empty(), "an empty corpus proves nothing");
        for case in vectors.cases {
            let digest = fold_project_artifact_pins(&case.project, &case.repo, &case.events);
            let actual = serde_json::to_value(&digest).expect("digest serializes");
            assert_eq!(actual, case.expected, "case {:?}", case.name);
        }
    }

    #[test]
    fn pinned_only_keeps_the_order_and_drops_the_unpinned() {
        let digest = ProjectArtifactPinDigest {
            schema: PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA.to_owned(),
            project: "30621:a:x".to_owned(),
            repo: "30617:a:y".to_owned(),
            ignored: 0,
            other_repo: 0,
            ranks_without_pin: 0,
            pins: vec![
                PinRow {
                    target: "docs/a.md".to_owned(),
                    target_kind: "file".to_owned(),
                    pinned: true,
                    rank: "a0".to_owned(),
                    by: "1".repeat(64),
                    updated_at: 1,
                },
                PinRow {
                    target: "docs/b.md".to_owned(),
                    target_kind: "file".to_owned(),
                    pinned: false,
                    rank: "a1".to_owned(),
                    by: "1".repeat(64),
                    updated_at: 2,
                },
                PinRow {
                    target: "docs/c".to_owned(),
                    target_kind: "folder".to_owned(),
                    pinned: true,
                    rank: "a2".to_owned(),
                    by: "1".repeat(64),
                    updated_at: 3,
                },
            ],
        };
        let shown: Vec<&str> = pinned_only(&digest)
            .iter()
            .map(|row| row.target.as_str())
            .collect();
        assert_eq!(shown, vec!["docs/a.md", "docs/c"]);
    }
}
