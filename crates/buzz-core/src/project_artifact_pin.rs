//! Project artifact pins (kind 44251): which documents, plans and folders of
//! a project's agents repository show in every member's sidebar, and in what
//! order.
//!
//! A pin is not a draft. It says nothing about a file's contents, and a
//! `commit.record` must never close one — which is exactly why this is its own
//! kind rather than another op on 44250, whose fold is a per-path draft chain
//! that a commit closes. What it shares with 44250 is the gate: the same
//! canonical project `a` coordinate, the same membership admission
//! ([`crate::kind::is_project_a_scoped_kind`]), the same repository check, so
//! none of that is written twice.
//!
//! A pin is **shared**. `pin.set` is a fact about the project that every
//! member sees, the same promise NIP-TD's `list.pinned` makes. Hiding the
//! pinned rows is a per-device viewing preference and never travels here.
//!
//! Every op sets one thing, so two people pinning and reordering never
//! overwrite each other: the latest `(created_at, id)` write per field wins,
//! and `pin.set`'s own `rank` takes part in the rank field under the set's
//! key — the rule NIP-TD's `item.add` already follows. The fold is
//! [`crate::project_artifact_pin_fold`], pinned by
//! `conformance/project-artifact-pin-fold/`.

use std::fmt;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::agents_repo_draft::{
    validate_draft_path, DraftPathClass, DOCS_ROOT, MAX_DOCUMENT_COMPONENTS,
};
use crate::fractional_rank::validate_rank;
use crate::kind::{
    event_kind_u32, normalize_project_coordinate, project_a_scoped_coordinate,
    KIND_PROJECT_ARTIFACT_PIN_OP,
};
use crate::project_pack_source::normalize_repository_coordinate;

/// Exact `schema` value carried by kind 44251 content.
pub const PROJECT_ARTIFACT_PIN_SCHEMA: &str = "buzz-project-artifact-pin/v1";

/// Exact version carried by the `ar-v` tag.
pub const PROJECT_ARTIFACT_PIN_TAG_VERSION: &str = "ar1-1";

/// Maximum UTF-8 byte length of a complete op payload.
pub const MAX_PROJECT_ARTIFACT_PIN_CONTENT_BYTES: usize = 2 * 1024;

/// What one op does. The wire spelling is the `ar-op` tag and the content `op`
/// field; [`validate_project_artifact_pin_envelope`] requires the two to
/// agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectArtifactPinOpKind {
    /// Pin or unpin one target, declaring what kind of thing it is and where
    /// it sits in the order.
    PinSet,
    /// Move one target in the order.
    PinRank,
}

impl ProjectArtifactPinOpKind {
    /// Every op, in wire order.
    pub const ALL: [Self; 2] = [Self::PinSet, Self::PinRank];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PinSet => "pin.set",
            Self::PinRank => "pin.rank",
        }
    }

    /// The exact content key set for this op, `schema` and `op` included.
    pub const fn content_keys(self) -> &'static [&'static str] {
        match self {
            Self::PinSet => &["schema", "op", "target", "targetKind", "pinned", "rank"],
            Self::PinRank => &["schema", "op", "target", "rank"],
        }
    }
}

impl fmt::Display for ProjectArtifactPinOpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectArtifactPinOpKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unsupported project artifact pin op {value:?}"))
    }
}

/// What a pin points at.
///
/// A **file** is a path the agents repository's grammar admits — a plan, a
/// document, a role, the manifest. A **folder** is a directory of the
/// documents tree, which is not a path the grammar admits at all: git has no
/// directory objects, so a folder is named here by the prefix its files share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinTargetKind {
    /// One file in the repository.
    File,
    /// One directory of the documents tree.
    Folder,
}

impl PinTargetKind {
    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
        }
    }
}

impl fmt::Display for PinTargetKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PinTargetKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "file" => Ok(Self::File),
            "folder" => Ok(Self::Folder),
            other => Err(format!(
                "pin targetKind must be \"file\" or \"folder\", not {other:?}"
            )),
        }
    }
}

/// What one op sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectArtifactPinOpValue {
    /// Pin or unpin `target`, at `rank` in the order.
    PinSet {
        /// What the target is.
        target_kind: PinTargetKind,
        /// `true` to show it in every member's project sidebar.
        pinned: bool,
        /// Its order key (see [`crate::fractional_rank`]).
        rank: String,
    },
    /// Move `target` to `rank` in the order.
    PinRank {
        /// The new order key.
        rank: String,
    },
}

/// One decoded op: the repository it is about, the target, and what it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectArtifactPinOp {
    /// The agents repository, `30617:<lowercase-hex>:<id>`, as the project's
    /// kind:30624 pins it. A pin names a path *in a repository*, so a
    /// re-pointed project's old pins stay readable and are reported as
    /// another repository's rather than silently re-aimed.
    pub repo: String,
    /// The file path or folder prefix.
    pub target: String,
    /// The edit.
    pub value: ProjectArtifactPinOpValue,
}

impl ProjectArtifactPinOp {
    /// Which op this is.
    pub const fn kind(&self) -> ProjectArtifactPinOpKind {
        match self.value {
            ProjectArtifactPinOpValue::PinSet { .. } => ProjectArtifactPinOpKind::PinSet,
            ProjectArtifactPinOpValue::PinRank { .. } => ProjectArtifactPinOpKind::PinRank,
        }
    }

    /// The order key this op carries. Both ops carry one: a `pin.set`
    /// establishes the order a target enters at, and takes part in the rank
    /// field under its own key.
    pub fn rank(&self) -> &str {
        match &self.value {
            ProjectArtifactPinOpValue::PinSet { rank, .. }
            | ProjectArtifactPinOpValue::PinRank { rank } => rank,
        }
    }

    /// Encode as canonical content JSON. The inverse of
    /// [`decode_project_artifact_pin_op`].
    pub fn to_content(&self) -> String {
        let mut object = Map::new();
        object.insert("schema".into(), Value::from(PROJECT_ARTIFACT_PIN_SCHEMA));
        object.insert("op".into(), Value::from(self.kind().as_str()));
        object.insert("target".into(), Value::from(self.target.as_str()));
        match &self.value {
            ProjectArtifactPinOpValue::PinSet {
                target_kind,
                pinned,
                rank,
            } => {
                object.insert("targetKind".into(), Value::from(target_kind.as_str()));
                object.insert("pinned".into(), Value::from(*pinned));
                object.insert("rank".into(), Value::from(rank.as_str()));
            }
            ProjectArtifactPinOpValue::PinRank { rank } => {
                object.insert("rank".into(), Value::from(rank.as_str()));
            }
        }
        Value::Object(object).to_string()
    }

    /// The tags this op carries, in canonical order: `a`, `ar-v`, `ar-op`,
    /// `ar-repo`, `ar-target`.
    pub fn tags(&self, coordinate: &str) -> Vec<Vec<String>> {
        vec![
            vec!["a".to_owned(), coordinate.to_owned()],
            vec![
                "ar-v".to_owned(),
                PROJECT_ARTIFACT_PIN_TAG_VERSION.to_owned(),
            ],
            vec!["ar-op".to_owned(), self.kind().as_str().to_owned()],
            vec!["ar-repo".to_owned(), self.repo.clone()],
            vec!["ar-target".to_owned(), self.target.clone()],
        ]
    }
}

/// Validate a pin target against its kind, returning why not.
///
/// A **file** is any path the agents repository's grammar admits except a
/// folder keep: the keep is how an empty directory exists in git, not a thing
/// anyone means to pin, and pinning it instead of its folder would put a row
/// called `.gitkeep` in the sidebar.
///
/// A **folder** is `docs/<segment>/…` with one to
/// [`MAX_DOCUMENT_COMPONENTS`] − 1 segments, each a document name — one fewer
/// than a path, because a file under the deepest pinnable folder still has to
/// fit. It is not a path [`validate_draft_path`] admits, and that is the
/// point: there is no such object in git, only a prefix its files share.
pub fn validate_pin_target(target: &str, kind: PinTargetKind) -> Result<(), String> {
    match kind {
        PinTargetKind::File => match validate_draft_path(target)? {
            DraftPathClass::DocumentFolder => Err(format!(
                "pin target {target:?} is a folder's keep; pin the folder it holds open, \
                 not the file"
            )),
            _ => Ok(()),
        },
        PinTargetKind::Folder => validate_pin_folder(target),
    }
}

/// What `target` is, inferred from its own shape, or why that cannot be
/// decided from the shape alone.
///
/// A caller usually need not label a target: a file is a path the layout
/// admits and a folder is a prefix of the documents tree. But the two
/// grammars overlap on one shape — a folder name may contain a dot, so
/// `docs/notes.txt` is a legal *folder* as well as a refused *file* — and
/// there the inference **refuses rather than guesses**. Reading a mistyped
/// `.txt` as a folder would pin a row that names nothing, and the person who
/// typed a filename would never learn why.
///
/// A caller that knows which it has says so with [`validate_pin_target`]; the
/// CLI's `--folder` exists for exactly the dotted-folder case this refuses.
pub fn pin_target_kind_of(target: &str) -> Result<PinTargetKind, String> {
    let as_file = validate_pin_target(target, PinTargetKind::File);
    if as_file.is_ok() {
        return Ok(PinTargetKind::File);
    }
    if validate_pin_target(target, PinTargetKind::Folder).is_ok() {
        let last = target.rsplit('/').next().unwrap_or(target);
        if last.contains('.') {
            return Err(format!(
                "{target:?} is file-shaped but is not a file the layout admits, and it is \
                 also a legal folder name — say which you mean (a folder is pinned with \
                 --folder). As a file: {}",
                as_file.unwrap_err()
            ));
        }
        return Ok(PinTargetKind::Folder);
    }
    // The file reading is the more specific refusal of the two — it names the
    // keep case and the grammar — so it is what the caller is told.
    Err(as_file.unwrap_err())
}

/// Validate a folder prefix of the documents tree.
fn validate_pin_folder(target: &str) -> Result<(), String> {
    if target.is_empty() || target.len() > 512 {
        return Err("pin folder must be 1–512 bytes".to_owned());
    }
    if target.starts_with('/') || target.ends_with('/') {
        return Err(format!(
            "pin folder {target:?} must be relative with no trailing slash"
        ));
    }
    let segments: Vec<&str> = target.split('/').collect();
    let Some((root, folders)) = segments.split_first() else {
        return Err(format!("pin folder {target:?} is empty"));
    };
    if *root != DOCS_ROOT {
        return Err(format!(
            "pin folder {target:?} is outside the documents tree, the only part of the \
             layout with folders"
        ));
    }
    if folders.is_empty() {
        return Err(format!(
            "pin folder {target:?} is the documents tree itself, which is not pinnable"
        ));
    }
    if folders.len() >= MAX_DOCUMENT_COMPONENTS {
        return Err(format!(
            "pin folder {target:?} is {} segments under {DOCS_ROOT}/; the deepest pinnable \
             folder is {} so a file under it still fits",
            folders.len(),
            MAX_DOCUMENT_COMPONENTS - 1
        ));
    }
    // One rule for both: a folder name is a document name, so a path built
    // under a pinnable folder is a path the grammar admits.
    if let Some(bad) = folders
        .iter()
        .find(|segment| validate_draft_path(&format!("{DOCS_ROOT}/{segment}/x.md")).is_err())
    {
        return Err(format!(
            "pin folder {target:?} has a segment {bad:?} that is not a document name"
        ));
    }
    Ok(())
}

fn take_str<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("pin op field {key:?} must be a string"))
}

fn take_bool(object: &Map<String, Value>, key: &str) -> Result<bool, String> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("pin op field {key:?} must be a boolean"))
}

/// Decode and validate content JSON. The key set is exact per op: every listed
/// key present, no other key.
pub fn decode_project_artifact_pin_op(
    content: &str,
    repo: &str,
) -> Result<ProjectArtifactPinOp, String> {
    if content.len() > MAX_PROJECT_ARTIFACT_PIN_CONTENT_BYTES {
        return Err(format!(
            "pin op content is {} bytes; the cap is {MAX_PROJECT_ARTIFACT_PIN_CONTENT_BYTES}",
            content.len()
        ));
    }
    let value: Value =
        serde_json::from_str(content).map_err(|_| "malformed pin op payload".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "pin op payload must be an object".to_owned())?;
    match object.get("schema").and_then(Value::as_str) {
        Some(PROJECT_ARTIFACT_PIN_SCHEMA) => {}
        _ => {
            return Err(format!(
                "pin op schema must be {PROJECT_ARTIFACT_PIN_SCHEMA:?}"
            ))
        }
    }
    let kind = ProjectArtifactPinOpKind::from_str(take_str(object, "op")?)?;
    let expected = kind.content_keys();
    if let Some(unknown) = object.keys().find(|key| !expected.contains(&key.as_str())) {
        return Err(format!(
            "pin op {} has unsupported field {unknown:?}",
            kind.as_str()
        ));
    }
    if let Some(missing) = expected.iter().find(|key| !object.contains_key(**key)) {
        return Err(format!(
            "pin op {} is missing field {missing:?}",
            kind.as_str()
        ));
    }
    let target = take_str(object, "target")?.to_owned();
    let rank = take_str(object, "rank")?.to_owned();
    validate_rank(&rank).map_err(|error| format!("pin op rank is invalid: {error}"))?;
    let value = match kind {
        ProjectArtifactPinOpKind::PinSet => {
            let target_kind = PinTargetKind::from_str(take_str(object, "targetKind")?)?;
            validate_pin_target(&target, target_kind)?;
            ProjectArtifactPinOpValue::PinSet {
                target_kind,
                pinned: take_bool(object, "pinned")?,
                rank,
            }
        }
        // A `pin.rank` does not repeat what the target is, so it cannot be
        // checked against a kind. Either shape is legal here and the fold
        // keeps it only when a `pin.set` established the target.
        ProjectArtifactPinOpKind::PinRank => {
            if validate_pin_target(&target, PinTargetKind::File).is_err()
                && validate_pin_target(&target, PinTargetKind::Folder).is_err()
            {
                return Err(format!(
                    "pin op target {target:?} is neither a file the layout admits nor a \
                     folder of the documents tree"
                ));
            }
            ProjectArtifactPinOpValue::PinRank { rank }
        }
    };
    Ok(ProjectArtifactPinOp {
        repo: repo.to_owned(),
        target,
        value,
    })
}

/// Validate a signed op end to end: kind, tag grammar, canonical project and
/// repository coordinates, content envelope, and tag/content agreement.
///
/// This is the single validator. The relay calls it and defines no local copy;
/// the SDK builder delegates to it. What it cannot check, and the relay does:
/// that `ar-repo` is the repository the project's kind:30624 pins today.
///
/// **Tag grammar** — position-independent, closed key set: exactly one each of
/// `a`, `ar-v`, `ar-op`, `ar-repo`, `ar-target`; every tag exactly two fields;
/// any other key — including `h` — is a rejection. A pin belongs to a project,
/// never to a room.
pub fn validate_project_artifact_pin_envelope(
    event: &nostr::Event,
) -> Result<ProjectArtifactPinOp, String> {
    if event_kind_u32(event) != KIND_PROJECT_ARTIFACT_PIN_OP {
        return Err("event is not a project artifact pin op (kind 44251)".to_owned());
    }

    let mut coordinate: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut tag_op: Option<&str> = None;
    let mut tag_repo: Option<&str> = None;
    let mut tag_target: Option<&str> = None;
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() != 2 {
            return Err("pin op tags must have exactly two fields".to_owned());
        }
        let key = parts[0].as_str();
        let slot = match key {
            "a" => &mut coordinate,
            "ar-v" => &mut version,
            "ar-op" => &mut tag_op,
            "ar-repo" => &mut tag_repo,
            "ar-target" => &mut tag_target,
            "h" => return Err("pin op must not carry an h tag; it is project-scoped".to_owned()),
            other => return Err(format!("pin op has unsupported tag key {other:?}")),
        };
        if slot.is_some() {
            return Err(format!("pin op has more than one {key} tag"));
        }
        *slot = Some(parts[1].as_str());
    }

    let coordinate = coordinate.ok_or_else(|| "pin op requires one a tag".to_owned())?;
    if normalize_project_coordinate(coordinate).as_deref() != Some(coordinate) {
        return Err(
            "44251 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> coordinate".to_owned(),
        );
    }
    match version {
        Some(PROJECT_ARTIFACT_PIN_TAG_VERSION) => {}
        _ => return Err("unsupported pin op tag version".to_owned()),
    }
    let repo = tag_repo.ok_or_else(|| "pin op requires one ar-repo tag".to_owned())?;
    if normalize_repository_coordinate(repo).as_deref() != Some(repo) {
        return Err(
            "44251 `ar-repo` tag must be a canonical 30617:<lowercase-hex>:<id> coordinate"
                .to_owned(),
        );
    }
    let op = decode_project_artifact_pin_op(&event.content, repo)?;
    let tag_op = tag_op.ok_or_else(|| "pin op requires one ar-op tag".to_owned())?;
    if tag_op != op.kind().as_str() {
        return Err(format!(
            "pin op ar-op tag {tag_op:?} does not match content op {:?}",
            op.kind().as_str()
        ));
    }
    let tag_target = tag_target.ok_or_else(|| "pin op requires one ar-target tag".to_owned())?;
    if tag_target != op.target {
        return Err("pin op ar-target tag does not match content target".to_owned());
    }
    // The extractor and the check above agree by construction; this keeps the
    // validator honest if either changes.
    if project_a_scoped_coordinate(event).as_deref() != Some(coordinate) {
        return Err("pin op a tag did not resolve to its coordinate".to_owned());
    }
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    const COORD: &str =
        "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop";
    const REPO: &str = "30617:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:tank-loop-beekeeper-agents";

    fn set_op() -> ProjectArtifactPinOp {
        ProjectArtifactPinOp {
            repo: REPO.to_owned(),
            target: "docs/mockups/login.html".to_owned(),
            value: ProjectArtifactPinOpValue::PinSet {
                target_kind: PinTargetKind::File,
                pinned: true,
                rank: "a0".to_owned(),
            },
        }
    }

    fn rank_op() -> ProjectArtifactPinOp {
        ProjectArtifactPinOp {
            repo: REPO.to_owned(),
            target: "docs/mockups".to_owned(),
            value: ProjectArtifactPinOpValue::PinRank {
                rank: "a1".to_owned(),
            },
        }
    }

    fn signed(op: &ProjectArtifactPinOp) -> nostr::Event {
        let keys = Keys::generate();
        let tags: Vec<Tag> = op
            .tags(COORD)
            .iter()
            .map(|parts| Tag::parse(parts.iter().map(String::as_str)).expect("tag parses"))
            .collect();
        EventBuilder::new(
            Kind::Custom(KIND_PROJECT_ARTIFACT_PIN_OP as u16),
            op.to_content(),
        )
        .tags(tags)
        .sign_with_keys(&keys)
        .expect("signs")
    }

    #[test]
    fn every_op_round_trips_through_content_and_envelope() {
        for op in [set_op(), rank_op()] {
            let decoded = decode_project_artifact_pin_op(&op.to_content(), REPO).expect("decodes");
            assert_eq!(decoded, op);
            let validated =
                validate_project_artifact_pin_envelope(&signed(&op)).expect("validates");
            assert_eq!(validated, op);
        }
    }

    #[test]
    fn a_file_target_is_a_path_the_layout_admits_and_never_a_keep() {
        for target in [
            "docs/mockups/login.html",
            "docs/notes/a.md",
            "plans/CURRENT_STATE.md",
            "roles/lead.md",
            "team.yml",
            "docs/img/shot.png",
        ] {
            validate_pin_target(target, PinTargetKind::File)
                .unwrap_or_else(|error| panic!("{target}: {error}"));
        }
        // A keep is how an empty folder exists, not a row anyone means to pin.
        let error = validate_pin_target("docs/mockups/.gitkeep", PinTargetKind::File)
            .expect_err("refuses a keep");
        assert!(error.contains("pin the folder"), "{error}");
        assert!(validate_pin_target("docs/x.txt", PinTargetKind::File).is_err());
        assert!(validate_pin_target("docs/mockups", PinTargetKind::File).is_err());
    }

    #[test]
    fn a_targets_kind_is_inferred_from_its_shape() {
        assert_eq!(
            pin_target_kind_of("docs/notes/a.md").expect("a document is a file"),
            PinTargetKind::File
        );
        assert_eq!(
            pin_target_kind_of("plans/CURRENT_STATE.md").expect("a plan is a file"),
            PinTargetKind::File
        );
        assert_eq!(
            pin_target_kind_of("docs/mockups").expect("a prefix is a folder"),
            PinTargetKind::Folder
        );
        // Nothing is both, so a mislabelled pin is impossible rather than
        // merely refused.
        let error = pin_target_kind_of("docs/mockups/.gitkeep").expect_err("a keep is neither");
        assert!(error.contains("pin the folder"), "{error}");
        assert!(pin_target_kind_of("plans").is_err());
        // The one overlap: a folder name may contain a dot, so this is both a
        // refused file and a legal folder. Inferring "folder" would pin a row
        // naming nothing and never tell the person who typed a filename why,
        // so the shape alone does not decide it.
        let error = pin_target_kind_of("docs/x.txt").expect_err("ambiguous");
        assert!(error.contains("--folder"), "{error}");
        assert!(error.contains("As a file:"), "{error}");
        // Said explicitly, both readings still work.
        assert!(validate_pin_target("docs/x.txt", PinTargetKind::Folder).is_ok());
        assert!(validate_pin_target("docs/x.txt", PinTargetKind::File).is_err());
        // A dotless folder is unambiguous and needs no flag.
        assert_eq!(
            pin_target_kind_of("docs/v2").expect("dotless"),
            PinTargetKind::Folder
        );
    }

    #[test]
    fn a_folder_target_is_a_prefix_of_the_documents_tree() {
        for target in ["docs/mockups", "docs/a/b/c", "docs/a/b/c/d/e/f/g"] {
            validate_pin_target(target, PinTargetKind::Folder)
                .unwrap_or_else(|error| panic!("{target}: {error}"));
        }
        // The deepest pinnable folder leaves room for a file under it.
        let error = validate_pin_target("docs/a/b/c/d/e/f/g/h", PinTargetKind::Folder)
            .expect_err("refuses the deepest+1");
        assert!(error.contains("so a file under it still fits"), "{error}");
        for target in [
            "docs",
            "plans",
            "plans/archive",
            "roles/lead",
            "docs/mockups/",
            "/docs/mockups",
            "docs/.hidden",
            "docs/-x",
        ] {
            assert!(
                validate_pin_target(target, PinTargetKind::Folder).is_err(),
                "{target} should refuse"
            );
        }
    }

    #[test]
    fn the_key_set_is_exact_and_the_rank_is_a_rank() {
        let content = set_op().to_content();
        let swap = |key: &str, value: Option<Value>| {
            let mut object: Value = serde_json::from_str(&content).expect("json");
            let map = object.as_object_mut().expect("object");
            match value {
                Some(value) => map.insert(key.into(), value),
                None => map.remove(key),
            };
            decode_project_artifact_pin_op(&object.to_string(), REPO).expect_err("refuses")
        };
        let error = swap("mode", Some(Value::from("100755")));
        assert!(error.contains("unsupported field \"mode\""), "{error}");
        let error = swap("pinned", None);
        assert!(error.contains("missing field \"pinned\""), "{error}");
        let error = swap("pinned", Some(Value::from("yes")));
        assert!(error.contains("must be a boolean"), "{error}");
        let error = swap("rank", Some(Value::from("")));
        assert!(error.contains("rank is invalid"), "{error}");
        let error = swap("targetKind", Some(Value::from("directory")));
        assert!(error.contains("\"file\" or \"folder\""), "{error}");
        // A `pin.rank` carries no kind, so either shape of target is legal.
        assert!(decode_project_artifact_pin_op(&rank_op().to_content(), REPO).is_ok());
    }

    #[test]
    fn the_envelope_refuses_h_unknown_keys_and_tag_mismatches() {
        let op = set_op();
        let base = op.tags(COORD);
        let event_with = |tags: Vec<Vec<String>>, content: String| -> nostr::Event {
            let keys = Keys::generate();
            let tag_vec: Vec<Tag> = tags
                .iter()
                .map(|parts| Tag::parse(parts.iter().map(String::as_str)).expect("tag parses"))
                .collect();
            EventBuilder::new(Kind::Custom(KIND_PROJECT_ARTIFACT_PIN_OP as u16), content)
                .tags(tag_vec)
                .sign_with_keys(&keys)
                .expect("signs")
        };
        let mut with_h = base.clone();
        with_h.push(vec!["h".to_owned(), "room".to_owned()]);
        let error = validate_project_artifact_pin_envelope(&event_with(with_h, op.to_content()))
            .expect_err("refuses h");
        assert!(error.contains("must not carry an h tag"), "{error}");

        let mut twice = base.clone();
        twice.push(vec!["ar-target".to_owned(), "docs/x.md".to_owned()]);
        let error = validate_project_artifact_pin_envelope(&event_with(twice, op.to_content()))
            .expect_err("refuses a repeat");
        assert!(error.contains("more than one ar-target"), "{error}");

        let mut wrong_target = base.clone();
        for tag in &mut wrong_target {
            if tag[0] == "ar-target" {
                tag[1] = "docs/other.md".to_owned();
            }
        }
        let error =
            validate_project_artifact_pin_envelope(&event_with(wrong_target, op.to_content()))
                .expect_err("refuses a mismatch");
        assert!(error.contains("ar-target tag does not match"), "{error}");

        let mut wrong_op = base.clone();
        for tag in &mut wrong_op {
            if tag[0] == "ar-op" {
                tag[1] = "pin.rank".to_owned();
            }
        }
        let error = validate_project_artifact_pin_envelope(&event_with(wrong_op, op.to_content()))
            .expect_err("refuses a mismatch");
        assert!(error.contains("ar-op tag"), "{error}");

        let mut bad_repo = base.clone();
        for tag in &mut bad_repo {
            if tag[0] == "ar-repo" {
                tag[1] = "30617:NOTHEX:x".to_owned();
            }
        }
        let error = validate_project_artifact_pin_envelope(&event_with(bad_repo, op.to_content()))
            .expect_err("refuses a non-canonical repo");
        assert!(error.contains("canonical 30617"), "{error}");
    }
}
