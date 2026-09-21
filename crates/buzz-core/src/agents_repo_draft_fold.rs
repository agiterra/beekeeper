//! The agents-repository draft fold: from a bag of kind 44250 ops to the
//! open draft per path a reader sees.
//!
//! The fold is pure and total: any set of events in, one digest out, the
//! same digest from every client. Its rules are the contract in
//! `conformance/agents-repo-draft-fold/CONTRACT.md` and are pinned by the
//! vectors beside it, which the TypeScript fold in Desktop and the Dart fold
//! in Mobile bind to as well. A rule implemented in only one fold is a
//! defect.
//!
//! Rules, in the order the fold applies them:
//!
//! 1. **Decode.** An event that is not a 44250, whose single `a` tag is not
//!    this project, whose single `ad-repo` tag is not a canonical repository
//!    coordinate, or whose content fails [`crate::agents_repo_draft`] is
//!    counted in `ignored` and dropped. A well-formed op whose `ad-repo` is
//!    not the repository the fold was asked about is counted in `otherRepo`
//!    and dropped — it belongs to a repository the project no longer pins,
//!    and saying so beats losing it. Duplicate ids keep the first.
//! 2. **Order.** Ops sort by `(created_at, id)` ascending. That pair is the
//!    only clock; there is no per-author sequence.
//! 3. **Close.** `closed` is the union of every `commit.record`'s `drafts`.
//!    Records themselves are listed in `commits`, newest first.
//! 4. **Heads.** A file op *names* a path `P` when its `path` is `P`, or it
//!    is a `file.move` whose `to` is `P`. Per path, the ops naming it that
//!    are not closed are its *open* ops; the greatest key is the `head`, the
//!    rest are `superseded`, oldest first. A path with no open op is absent.
//! 5. **Divergence.** `diverged` is true when the head did not build on the
//!    newest superseded op: `superseded` is non-empty and its last id is not
//!    the head's `prev`. It reports that some open text is not in the head;
//!    it never resolves anything.
//! 6. **Order out.** Paths sort bytewise. `updatedAt` on a path is the
//!    head's `created_at`.
//!
//! Nothing is ever dropped for being stale against `main`: that is a fact
//! only a reader with the repository can establish, and it is reported by
//! that reader, not by this fold.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::agents_repo_draft::{
    decode_agents_repo_draft_op, AgentsRepoDraftOp, AgentsRepoDraftOpValue, DraftBase,
};
use crate::kind::{normalize_project_coordinate, KIND_AGENTS_REPO_DRAFT_OP};
use crate::project_pack_source::normalize_repository_coordinate;

/// Exact `schema` value carried by a digest.
pub const AGENTS_REPO_DRAFT_DIGEST_SCHEMA: &str = "buzz-agents-repo-draft-digest/v1";

/// One stored event as the fold sees it — the subset of a Nostr event the
/// rules read. A relay-stored event converts losslessly; the conformance
/// vectors are written in this shape directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftFoldEvent {
    /// Event id (64 hex).
    pub id: String,
    /// Author (64 hex).
    pub pubkey: String,
    /// Seconds since the epoch.
    pub created_at: u64,
    /// Event kind; anything but 44250 is ignored.
    pub kind: u32,
    /// Tags; the single `a` and single `ad-repo` tags are read.
    pub tags: Vec<Vec<String>>,
    /// Op content JSON.
    pub content: String,
}

impl From<&nostr::Event> for DraftFoldEvent {
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

/// One file op as the digest reports it, head or superseded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRow {
    /// Event id.
    pub id: String,
    /// Author.
    pub author: String,
    /// The op's `created_at`.
    pub created_at: u64,
    /// `file.put`, `file.move` or `file.delete`.
    pub op: String,
    /// The op's `path`.
    pub path: String,
    /// A move's destination; null otherwise.
    pub to: Option<String>,
    /// A put's whole text; null otherwise.
    pub text: Option<String>,
    /// The blob the author started from, or null for a new file.
    pub base: Option<String>,
    /// The `main` commit the author read, or null.
    pub base_commit: Option<String>,
    /// The draft head the author edited from, or null.
    pub prev: Option<String>,
    /// The author's one-line reason, or null.
    pub message: Option<String>,
}

/// One path with an open draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftPath {
    /// The path.
    pub path: String,
    /// The newest open op naming it.
    pub head: DraftRow,
    /// Every other open op naming it, oldest first.
    pub superseded: Vec<DraftRow>,
    /// The head did not build on the newest superseded op.
    pub diverged: bool,
    /// The head's `created_at`.
    pub updated_at: u64,
}

/// One `commit.record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRecordRow {
    /// Event id.
    pub id: String,
    /// The commit the record names.
    pub commit: String,
    /// Who recorded it.
    pub by: String,
    /// The record's `created_at`.
    pub created_at: u64,
    /// The paths the record names.
    pub paths: Vec<String>,
    /// The draft ids the record closes.
    pub drafts: Vec<String>,
    /// The committer's one-line message, or null.
    pub message: Option<String>,
}

/// The fold's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoDraftDigest {
    /// Always [`AGENTS_REPO_DRAFT_DIGEST_SCHEMA`].
    pub schema: String,
    /// The project coordinate the ops were folded for.
    pub project: String,
    /// The repository coordinate the ops were folded for.
    pub repo: String,
    /// Events that did not decode.
    pub ignored: usize,
    /// Well-formed ops for a repository other than `repo`.
    pub other_repo: usize,
    /// Paths with an open draft, bytewise.
    pub paths: Vec<DraftPath>,
    /// Commit records, newest first.
    pub commits: Vec<CommitRecordRow>,
}

/// `(created_at, id)` — the fold's only clock.
type OpKey = (u64, String);

struct Decoded<'a> {
    key: OpKey,
    pubkey: &'a str,
    op: AgentsRepoDraftOp,
}

enum Decode {
    Ignored,
    OtherRepo,
    Op(AgentsRepoDraftOp),
}

fn single_tag<'a>(event: &'a DraftFoldEvent, key: &str) -> Option<&'a str> {
    let mut values = event
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .filter_map(|t| t.get(1));
    let first = values.next()?;
    if values.next().is_some() {
        return None;
    }
    Some(first)
}

fn decode(project: &str, repo: &str, event: &DraftFoldEvent) -> Decode {
    if event.kind != KIND_AGENTS_REPO_DRAFT_OP {
        return Decode::Ignored;
    }
    let Some(coordinate) = single_tag(event, "a") else {
        return Decode::Ignored;
    };
    if normalize_project_coordinate(coordinate).as_deref() != Some(project) {
        return Decode::Ignored;
    }
    let Some(tag_repo) = single_tag(event, "ad-repo") else {
        return Decode::Ignored;
    };
    let Some(canonical_repo) = normalize_repository_coordinate(tag_repo) else {
        return Decode::Ignored;
    };
    if canonical_repo != tag_repo {
        return Decode::Ignored;
    }
    match decode_agents_repo_draft_op(&event.content, &canonical_repo) {
        Ok(_) if canonical_repo != repo => Decode::OtherRepo,
        Ok(op) => Decode::Op(op),
        Err(_) => Decode::Ignored,
    }
}

fn row(d: &Decoded<'_>) -> Option<DraftRow> {
    let (op, path, to, text, base) = match &d.op.value {
        AgentsRepoDraftOpValue::FilePut { path, text, base } => {
            ("file.put", path, None, Some(text.clone()), base)
        }
        AgentsRepoDraftOpValue::FileMove { path, to, base } => {
            ("file.move", path, Some(to.clone()), None, base)
        }
        AgentsRepoDraftOpValue::FileDelete { path, base } => {
            ("file.delete", path, None, None, base)
        }
        AgentsRepoDraftOpValue::CommitRecord { .. } => return None,
    };
    let DraftBase {
        base,
        base_commit,
        prev,
    } = base.clone();
    Some(DraftRow {
        id: d.key.1.clone(),
        author: d.pubkey.to_owned(),
        created_at: d.key.0,
        op: op.to_owned(),
        path: path.clone(),
        to,
        text,
        base,
        base_commit,
        prev,
        message: d.op.message.clone(),
    })
}

/// Fold `events` for `project` and its agents repository `repo` into a
/// digest. See the module doc for the rules. Both coordinates must already
/// be canonical; events naming any other project are ignored, and
/// well-formed ops for another repository are counted, not folded.
pub fn fold_agents_repo_drafts(
    project: &str,
    repo: &str,
    events: &[DraftFoldEvent],
) -> AgentsRepoDraftDigest {
    let mut ignored = 0usize;
    let mut other_repo = 0usize;
    let mut seen = HashSet::new();
    let mut ops: Vec<Decoded<'_>> = Vec::new();
    for event in events {
        if !seen.insert(event.id.as_str()) {
            continue;
        }
        match decode(project, repo, event) {
            Decode::Op(op) => ops.push(Decoded {
                key: (event.created_at, event.id.clone()),
                pubkey: &event.pubkey,
                op,
            }),
            Decode::OtherRepo => other_repo += 1,
            Decode::Ignored => ignored += 1,
        }
    }
    ops.sort_by(|a, b| a.key.cmp(&b.key));

    let mut closed: HashSet<&str> = HashSet::new();
    let mut commits: Vec<CommitRecordRow> = Vec::new();
    for d in &ops {
        if let AgentsRepoDraftOpValue::CommitRecord {
            commit,
            paths,
            drafts,
        } = &d.op.value
        {
            closed.extend(drafts.iter().map(String::as_str));
            commits.push(CommitRecordRow {
                id: d.key.1.clone(),
                commit: commit.clone(),
                by: d.pubkey.to_owned(),
                created_at: d.key.0,
                paths: paths.clone(),
                drafts: drafts.clone(),
                message: d.op.message.clone(),
            });
        }
    }
    commits.reverse();

    let mut by_path: BTreeMap<String, Vec<DraftRow>> = BTreeMap::new();
    for d in &ops {
        if closed.contains(d.key.1.as_str()) {
            continue;
        }
        let Some(row) = row(d) else {
            continue;
        };
        for path in d.op.paths() {
            by_path
                .entry(path.to_owned())
                .or_default()
                .push(row.clone());
        }
    }

    let paths = by_path
        .into_iter()
        .filter_map(|(path, mut open)| {
            let head = open.pop()?;
            let diverged = open
                .last()
                .is_some_and(|last| Some(last.id.as_str()) != head.prev.as_deref());
            Some(DraftPath {
                path,
                updated_at: head.created_at,
                head,
                superseded: open,
                diverged,
            })
        })
        .collect();

    AgentsRepoDraftDigest {
        schema: AGENTS_REPO_DRAFT_DIGEST_SCHEMA.to_owned(),
        project: project.to_owned(),
        repo: repo.to_owned(),
        ignored,
        other_repo,
        paths,
        commits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[derive(Deserialize)]
    struct Vectors {
        schema: String,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        project: String,
        repo: String,
        events: Vec<DraftFoldEvent>,
        expected: Value,
    }

    #[test]
    fn conformance_vectors_fold_identically() {
        let raw =
            include_str!("../../../conformance/agents-repo-draft-fold/fixtures/fold-vectors.json");
        let vectors: Vectors = serde_json::from_str(raw).expect("vectors parse");
        assert_eq!(vectors.schema, "buzz-agents-repo-draft-fold-vectors/v1");
        assert!(!vectors.cases.is_empty());
        for case in vectors.cases {
            let digest = fold_agents_repo_drafts(&case.project, &case.repo, &case.events);
            let actual = serde_json::to_value(&digest).expect("digest serializes");
            assert_eq!(
                actual,
                case.expected,
                "case {:?}\nactual: {}",
                case.name,
                serde_json::to_string_pretty(&actual).unwrap()
            );
        }
    }
}
