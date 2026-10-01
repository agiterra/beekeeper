//! Agents-repository draft operations (kind 44250): one proposed change to
//! one file of a project's agents repository, published so the whole
//! project can read it before anyone commits it.
//!
//! A draft is not a commit. The agents repository's `main` stays the only
//! thing a seat stages from; a draft is what people (and agents, through
//! `bee`) exchange while they agree on what `main` should say next. The
//! fold that turns the op log into "the open draft per path" lives in
//! [`crate::agents_repo_draft_fold`], pinned by
//! `conformance/agents-repo-draft-fold/`. This module owns the **wire
//! contract**: the op vocabulary, the closed content key set, the path
//! grammar, the tag grammar, and the one validator the relay, the SDK
//! builder and `bee agents-repo` all call.
//!
//! Every file op carries the whole new text of one file plus the facts a
//! committer needs to refuse honestly: `base`, the blob the author started
//! from (or `null` for a new file), and `prev`, the draft head the author
//! edited (or `null`). A `commit.record` is the committer's statement that
//! named drafts landed in a commit on `main`; the relay checks that the
//! commit is, or recently was, `main`'s tip, so readers can close those
//! drafts without a cross-author deletion.
//!
//! Why whole files and not field-level ops like the to-do kind: a role or a
//! plan is prose, and the concurrency answer is "the newest save wins, the
//! older one stays visible as superseded, and a save from a stale head is
//! refused by the client before it is signed". Nothing anyone wrote
//! disappears; what disappears is only the *claim* that it is the head.

use std::fmt;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::kind::{
    event_kind_u32, normalize_project_coordinate, project_a_scoped_coordinate,
    KIND_AGENTS_REPO_DRAFT_OP,
};
use crate::project_pack_source::normalize_repository_coordinate;

/// Exact `schema` value carried by kind 44250 content.
pub const AGENTS_REPO_DRAFT_SCHEMA: &str = "buzz-agents-repo-draft/v1";

/// Exact version carried by the `ad-v` tag.
pub const AGENTS_REPO_DRAFT_TAG_VERSION: &str = "ad1-1";

/// Maximum UTF-8 byte length of a complete op payload: the relay's advertised
/// NIP-11 `max_content_len`. JSON escaping can push a text near
/// [`MAX_AGENTS_REPO_DRAFT_TEXT_BYTES`] over this; the refusal names which
/// cap tripped.
pub const MAX_AGENTS_REPO_DRAFT_CONTENT_BYTES: usize = 65_536;

/// Maximum UTF-8 byte length of a `file.put` text. A file on `main` larger
/// than this can be read but not drafted through this kind; it is edited
/// with git.
pub const MAX_AGENTS_REPO_DRAFT_TEXT_BYTES: usize = 60_000;

/// Maximum UTF-8 byte length of the optional one-line `message`.
pub const MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES: usize = 512;

/// Maximum number of `paths` and of `drafts` on one `commit.record`.
pub const MAX_COMMIT_RECORD_ENTRIES: usize = 256;

/// The directory whose contents are retained but never in force. Equal to
/// `buzz_persona::team::ARCHIVE_DIR`; the two crates share no dependency, so
/// a `buzz-cli` test pins them equal.
pub const ARCHIVE_SEGMENT: &str = "archive";

/// The root files a draft may put but never move or delete.
pub const ROOT_FILES: &[&str] = &["README.md", "team.yml", "actions.yml"];

/// The one tree in the layout with folders: a project's **document
/// artifacts**. `plans/`, `roles/` and `skills/` stay flat because a plan's
/// path is cited by every adopted `planRef` and a role's stem is a `team.yml`
/// key. A document is cited by nothing, so it may be organised.
pub const DOCS_ROOT: &str = "docs";

/// How many components a path may have under [`DOCS_ROOT`], the last of them
/// the file. Eight is deep enough for any filing anyone has asked for and
/// shallow enough that a tree walk is bounded.
pub const MAX_DOCUMENT_COMPONENTS: usize = 8;

/// A document's formats, lowercase so one path names one file. Markdown
/// renders in the app; HTML is what an agent's mockup lands as.
pub const DOCUMENT_EXTENSIONS: &[&str] = &[".md", ".html"];

/// An image a document embeds, committed beside it so it is reviewed and
/// versioned with the document it belongs to.
///
/// `.svg` is admitted here although `buzz-media` refuses `image/svg+xml` as an
/// upload MIME: in the tree an asset is only ever rendered through `<img>`,
/// which runs no script, or inside the preview window, which carries its own
/// CSP and no network. Different surfaces, and neither rule is loosened.
pub const DOCUMENT_ASSET_EXTENSIONS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"];

/// The only dotfile the documents tree admits, and the only way a folder
/// someone created and has not filled yet survives a commit — git has no
/// empty directories. Without it, "new folder" and "pin a folder" would be
/// things the product claims and git drops.
pub const GITKEEP: &str = ".gitkeep";

/// What one op does. The wire spelling is the `ad-op` tag and the content
/// `op` field; [`validate_agents_repo_draft_envelope`] requires the two to
/// agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentsRepoDraftOpKind {
    /// Replace (or create) one file's whole text.
    FilePut,
    /// Move a role or plan between its live path and its archive path.
    FileMove,
    /// Delete one file.
    FileDelete,
    /// The committer's record that named drafts landed in a commit on `main`.
    CommitRecord,
}

impl AgentsRepoDraftOpKind {
    /// Every op, in wire order.
    pub const ALL: [Self; 4] = [
        Self::FilePut,
        Self::FileMove,
        Self::FileDelete,
        Self::CommitRecord,
    ];

    /// The wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FilePut => "file.put",
            Self::FileMove => "file.move",
            Self::FileDelete => "file.delete",
            Self::CommitRecord => "commit.record",
        }
    }

    /// The exact content key set for this op, `schema` and `op` included.
    pub const fn content_keys(self) -> &'static [&'static str] {
        match self {
            Self::FilePut => &[
                "schema",
                "op",
                "path",
                "text",
                "base",
                "baseCommit",
                "prev",
                "message",
            ],
            Self::FileMove => &[
                "schema",
                "op",
                "path",
                "to",
                "base",
                "baseCommit",
                "prev",
                "message",
            ],
            Self::FileDelete => &[
                "schema",
                "op",
                "path",
                "base",
                "baseCommit",
                "prev",
                "message",
            ],
            Self::CommitRecord => &["schema", "op", "commit", "paths", "drafts", "message"],
        }
    }

    /// `true` for an op that names one file (everything but a record).
    pub const fn is_file_op(self) -> bool {
        !matches!(self, Self::CommitRecord)
    }
}

impl fmt::Display for AgentsRepoDraftOpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentsRepoDraftOpKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| format!("unsupported agents repo draft op {value:?}"))
    }
}

/// The facts every file op carries about where the author started.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DraftBase {
    /// Blob sha (40 lowercase hex) of the file on `main` the author started
    /// from; `None` for a file that does not exist there.
    pub base: Option<String>,
    /// Advisory: the `main` commit (40 lowercase hex) the author read.
    pub base_commit: Option<String>,
    /// The draft head (64 lowercase hex event id) the author edited from;
    /// `None` when there was none.
    pub prev: Option<String>,
}

/// What one op sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsRepoDraftOpValue {
    /// Whole new text for `path`.
    FilePut {
        /// The file.
        path: String,
        /// Its whole new text.
        text: String,
        /// Where the author started.
        base: DraftBase,
    },
    /// Move `path` to `to`, its archive counterpart.
    FileMove {
        /// The file.
        path: String,
        /// Its archive counterpart.
        to: String,
        /// Where the author started.
        base: DraftBase,
    },
    /// Delete `path`.
    FileDelete {
        /// The file.
        path: String,
        /// Where the author started.
        base: DraftBase,
    },
    /// `commit` on `main` carries the named `drafts`, which touched `paths`.
    CommitRecord {
        /// The commit sha (40 lowercase hex).
        commit: String,
        /// The paths the commit touched.
        paths: Vec<String>,
        /// The draft event ids the commit closes.
        drafts: Vec<String>,
    },
}

/// One decoded op: the repository it belongs to and what it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsRepoDraftOp {
    /// The agents repository, `30617:<lowercase-hex>:<id>`, as the project's
    /// kind:30624 pins it.
    pub repo: String,
    /// Optional one-line reason, surfaced in listings and commit bodies.
    pub message: Option<String>,
    /// The edit or the record.
    pub value: AgentsRepoDraftOpValue,
}

impl AgentsRepoDraftOp {
    /// Which op this is.
    pub const fn kind(&self) -> AgentsRepoDraftOpKind {
        match self.value {
            AgentsRepoDraftOpValue::FilePut { .. } => AgentsRepoDraftOpKind::FilePut,
            AgentsRepoDraftOpValue::FileMove { .. } => AgentsRepoDraftOpKind::FileMove,
            AgentsRepoDraftOpValue::FileDelete { .. } => AgentsRepoDraftOpKind::FileDelete,
            AgentsRepoDraftOpValue::CommitRecord { .. } => AgentsRepoDraftOpKind::CommitRecord,
        }
    }

    /// The file a file op names; `None` for a record.
    pub fn path(&self) -> Option<&str> {
        match &self.value {
            AgentsRepoDraftOpValue::FilePut { path, .. }
            | AgentsRepoDraftOpValue::FileMove { path, .. }
            | AgentsRepoDraftOpValue::FileDelete { path, .. } => Some(path),
            AgentsRepoDraftOpValue::CommitRecord { .. } => None,
        }
    }

    /// Every path this op names: the file op's `path` (and a move's `to`),
    /// or a record's `paths`. This is what rides the `ad-path` tags.
    pub fn paths(&self) -> Vec<&str> {
        match &self.value {
            AgentsRepoDraftOpValue::FilePut { path, .. }
            | AgentsRepoDraftOpValue::FileDelete { path, .. } => vec![path],
            AgentsRepoDraftOpValue::FileMove { path, to, .. } => vec![path, to],
            AgentsRepoDraftOpValue::CommitRecord { paths, .. } => {
                paths.iter().map(String::as_str).collect()
            }
        }
    }

    /// The base facts of a file op; `None` for a record.
    pub fn base(&self) -> Option<&DraftBase> {
        match &self.value {
            AgentsRepoDraftOpValue::FilePut { base, .. }
            | AgentsRepoDraftOpValue::FileMove { base, .. }
            | AgentsRepoDraftOpValue::FileDelete { base, .. } => Some(base),
            AgentsRepoDraftOpValue::CommitRecord { .. } => None,
        }
    }

    /// Encode as canonical content JSON. The inverse of
    /// [`decode_agents_repo_draft_op`].
    pub fn to_content(&self) -> String {
        let mut object = Map::new();
        object.insert("schema".into(), Value::from(AGENTS_REPO_DRAFT_SCHEMA));
        object.insert("op".into(), Value::from(self.kind().as_str()));
        let nullable =
            |value: &Option<String>| value.as_deref().map(Value::from).unwrap_or(Value::Null);
        let put_base = |object: &mut Map<String, Value>, base: &DraftBase| {
            object.insert("base".into(), nullable(&base.base));
            object.insert("baseCommit".into(), nullable(&base.base_commit));
            object.insert("prev".into(), nullable(&base.prev));
        };
        match &self.value {
            AgentsRepoDraftOpValue::FilePut { path, text, base } => {
                object.insert("path".into(), Value::from(path.as_str()));
                object.insert("text".into(), Value::from(text.as_str()));
                put_base(&mut object, base);
            }
            AgentsRepoDraftOpValue::FileMove { path, to, base } => {
                object.insert("path".into(), Value::from(path.as_str()));
                object.insert("to".into(), Value::from(to.as_str()));
                put_base(&mut object, base);
            }
            AgentsRepoDraftOpValue::FileDelete { path, base } => {
                object.insert("path".into(), Value::from(path.as_str()));
                put_base(&mut object, base);
            }
            AgentsRepoDraftOpValue::CommitRecord {
                commit,
                paths,
                drafts,
            } => {
                object.insert("commit".into(), Value::from(commit.as_str()));
                object.insert("paths".into(), Value::from(paths.clone()));
                object.insert("drafts".into(), Value::from(drafts.clone()));
            }
        }
        object.insert("message".into(), nullable(&self.message));
        Value::Object(object).to_string()
    }

    /// The tags this op carries, in canonical order: `a`, `ad-v`, `ad-op`,
    /// `ad-repo`, then one `ad-path` per named path.
    pub fn tags(&self, coordinate: &str) -> Vec<Vec<String>> {
        let mut tags = vec![
            vec!["a".to_owned(), coordinate.to_owned()],
            vec!["ad-v".to_owned(), AGENTS_REPO_DRAFT_TAG_VERSION.to_owned()],
            vec!["ad-op".to_owned(), self.kind().as_str().to_owned()],
            vec!["ad-repo".to_owned(), self.repo.clone()],
        ];
        for path in self.paths() {
            tags.push(vec!["ad-path".to_owned(), path.to_owned()]);
        }
        tags
    }
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// `true` for a 40-character lowercase hex git object id.
pub fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && is_lower_hex(value)
}

/// `true` for a 64-character lowercase hex event id.
pub fn is_event_id(value: &str) -> bool {
    value.len() == 64 && is_lower_hex(value)
}

/// A **role** or **skill** name: the key the team manifest or a role's
/// frontmatter uses, so it is a slug and stays one. `roles/Lead.md` is refused
/// because `Lead` cannot be a `team.yml` key. See [`is_doc_stem`] for why a
/// plan's filename is not held to this.
fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value != ARCHIVE_SEGMENT
}

/// A **plan's** filename stem, which is the name of a document rather than a
/// manifest key. It carries uppercase, `_` and interior dots, because the
/// documents that move into an agents repository are called `CURRENT_STATE.md`,
/// `SESSION_STATE.md` and `README.md`. Requiring [`is_slug`] here was a
/// carry-over from the role rule, and it cost something real: Beekeeper's own
/// map, ledger and in-force plans moved in on 2026-09-22 and every one of them
/// landed at a path the Files tab lists as `other` and refuses to open.
///
/// Still bounded, still never `archive` in any case — that names the sibling
/// directory — and never leading with `.` or `-`, which would make a hidden
/// file or something an argument parser reads as a flag.
/// Pinned by `conformance/agents-repo-draft-path/`.
fn is_doc_stem(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && !value.starts_with(['.', '-'])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !value.eq_ignore_ascii_case(ARCHIVE_SEGMENT)
}

/// A segment of the **documents** tree: a folder name, or a document's or
/// asset's stem. The same shape as [`is_doc_stem`] — these are document names
/// too — except that `archive` is an ordinary name here, because the documents
/// tree has no archive rule. A document is moved or deleted.
fn is_doc_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && !value.starts_with(['.', '-'])
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// The stem of `file` once a known extension is taken off it, or `None` when
/// the name does not end in one of `extensions`. The *trailing* extension is
/// the one read, so `notes.md.txt` is not a document.
fn stem_with_extension<'a>(file: &'a str, extensions: &[&str]) -> Option<&'a str> {
    extensions
        .iter()
        .find_map(|extension| file.strip_suffix(extension))
}

fn is_file_segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Where a path sits in the agents repository's layout (spec § 4.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPathClass {
    /// `README.md`, `team.yml` or `actions.yml`: put-only.
    RootFile,
    /// `roles/<slug>.md`.
    Role,
    /// `roles/archive/<slug>.md`.
    ArchivedRole,
    /// `roles/<slug>/skills/<skill>/<file…>`.
    RoleSkill,
    /// `skills/<skill>/<file…>`.
    SharedSkill,
    /// `plans/<slug>.md`.
    Plan,
    /// `plans/archive/<slug>.md`.
    ArchivedPlan,
    /// `docs/<folder>/…/<stem>.md` or `.html` — a **document artifact**.
    Document,
    /// `docs/<folder>/…/<file>.png` and the other image formats: an image a
    /// document embeds, committed beside it.
    DocumentAsset,
    /// `docs/<folder>/…/.gitkeep` — the keep that is how an empty folder
    /// exists in git at all.
    DocumentFolder,
}

impl DraftPathClass {
    /// Whether this path sits in the documents tree, where folders nest and a
    /// move may name any path of the same class.
    pub const fn is_document(self) -> bool {
        matches!(
            self,
            Self::Document | Self::DocumentAsset | Self::DocumentFolder
        )
    }
}

/// Classify a draft path against the agents repository's layout, refusing
/// anything outside it: absolute paths, `..`, empty or dot segments, a slug
/// of `archive`, or a file nowhere the layout allows.
pub fn validate_draft_path(path: &str) -> Result<DraftPathClass, String> {
    if path.is_empty() || path.len() > 512 {
        return Err("draft path must be 1–512 bytes".to_owned());
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(format!(
            "draft path {path:?} must be relative with no trailing slash"
        ));
    }
    if ROOT_FILES.contains(&path) {
        return Ok(DraftPathClass::RootFile);
    }
    let segments: Vec<&str> = path.split('/').collect();
    if !segments.iter().all(|segment| is_file_segment(segment)) {
        return Err(format!(
            "draft path {path:?} has an empty, dot or non-portable segment"
        ));
    }
    // A role file is named for its manifest key; a plan file is named for the
    // document it holds. Two rules, deliberately.
    let role_file = |segment: &str| -> bool { segment.strip_suffix(".md").is_some_and(is_slug) };
    let plan_file =
        |segment: &str| -> bool { segment.strip_suffix(".md").is_some_and(is_doc_stem) };
    match segments.as_slice() {
        ["roles", ARCHIVE_SEGMENT, file] if role_file(file) => Ok(DraftPathClass::ArchivedRole),
        ["plans", ARCHIVE_SEGMENT, file] if plan_file(file) => Ok(DraftPathClass::ArchivedPlan),
        ["roles", file] if role_file(file) => Ok(DraftPathClass::Role),
        ["plans", file] if plan_file(file) => Ok(DraftPathClass::Plan),
        ["roles", role, "skills", skill, rest @ ..]
            if is_slug(role) && is_slug(skill) && !rest.is_empty() =>
        {
            Ok(DraftPathClass::RoleSkill)
        }
        ["skills", skill, rest @ ..] if is_slug(skill) && !rest.is_empty() => {
            Ok(DraftPathClass::SharedSkill)
        }
        [DOCS_ROOT, components @ ..] if !components.is_empty() => {
            classify_document_path(path, components)
        }
        _ => Err(format!(
            "draft path {path:?} is outside the agents repository layout \
             (README.md, team.yml, actions.yml, roles/<role>.md, \
             roles/archive/<role>.md, roles/<role>/skills/<skill>/…, \
             skills/<skill>/…, plans/<plan>.md, plans/archive/<plan>.md, \
             docs/<folder>/…/<document>.md|.html, docs/<folder>/…/<image>, \
             docs/<folder>/…/.gitkeep)"
        )),
    }
}

/// Classify the components under [`DOCS_ROOT`]. Folders nest, bounded by
/// [`MAX_DOCUMENT_COMPONENTS`]; every folder name is a document name; and the
/// file is a document, an asset, or the keep that holds an empty folder open.
fn classify_document_path(path: &str, components: &[&str]) -> Result<DraftPathClass, String> {
    if components.len() > MAX_DOCUMENT_COMPONENTS {
        return Err(format!(
            "draft path {path:?} is {} components under {DOCS_ROOT}/; the cap is \
             {MAX_DOCUMENT_COMPONENTS}",
            components.len()
        ));
    }
    let (file, folders) = components
        .split_last()
        .expect("the caller refused an empty component list");
    if let Some(folder) = folders.iter().find(|folder| !is_doc_segment(folder)) {
        return Err(format!(
            "draft path {path:?} has a folder {folder:?} that is not a document name \
             (at most 96 bytes of letters, digits, '.', '_' or '-', never leading \
             with '.' or '-')"
        ));
    }
    if *file == GITKEEP {
        return Ok(DraftPathClass::DocumentFolder);
    }
    if stem_with_extension(file, DOCUMENT_EXTENSIONS).is_some_and(is_doc_segment) {
        return Ok(DraftPathClass::Document);
    }
    if stem_with_extension(file, DOCUMENT_ASSET_EXTENSIONS).is_some_and(is_doc_segment) {
        return Ok(DraftPathClass::DocumentAsset);
    }
    Err(format!(
        "draft path {path:?} is not a document ({}), an image ({}) or {GITKEEP}",
        DOCUMENT_EXTENSIONS.join(", "),
        DOCUMENT_ASSET_EXTENSIONS.join(", ")
    ))
}

/// The only legal `to` of a `file.move`: a role's or plan's archive path,
/// or the live path of an archived one. `None` for every other path.
pub fn archive_counterpart(path: &str) -> Option<String> {
    let class = validate_draft_path(path).ok()?;
    let segments: Vec<&str> = path.split('/').collect();
    match (class, segments.as_slice()) {
        (DraftPathClass::Role, [root, file]) | (DraftPathClass::Plan, [root, file]) => {
            Some(format!("{root}/{ARCHIVE_SEGMENT}/{file}"))
        }
        (DraftPathClass::ArchivedRole, [root, _, file])
        | (DraftPathClass::ArchivedPlan, [root, _, file]) => Some(format!("{root}/{file}")),
        _ => None,
    }
}

/// Whether a `file.move` from `path` to `to` names a legal destination, and
/// why not when it does not.
///
/// The rule is not the same for every class, deliberately:
///
/// - a **role** or **plan** has exactly one destination, its
///   [`archive_counterpart`]. A plan is never renamed: every adopted `planRef`
///   names it by path (NIP-PW), and a rename would orphan them.
/// - a **document**, **asset** or **folder keep** may move to any path of its
///   own class — that is what rename and move-between-folders are. Markdown
///   and HTML are one class, so changing a document's format is a move.
/// - a **root file** is put-only, and a skill file has no move at all.
///
/// Renaming a folder is one move per file under it, issued by the client.
/// This admits each one; it does not make a directory move atomic.
/// Pinned by `conformance/agents-repo-draft-path/` (`moves`).
pub fn validate_move_destination(path: &str, to: &str) -> Result<(), String> {
    let from_class = validate_draft_path(path)?;
    if from_class.is_document() {
        if to == path {
            return Err(format!("draft move of {path:?} must name a different path"));
        }
        let to_class = validate_draft_path(to)?;
        if to_class != from_class {
            return Err(format!(
                "draft move of {path:?} may only name another path of its own class, \
                 not {to:?}"
            ));
        }
        return Ok(());
    }
    let counterpart = archive_counterpart(path)
        .ok_or_else(|| format!("draft path {path:?} is not a role or plan that can be archived"))?;
    if to != counterpart {
        return Err(format!(
            "draft move of {path:?} may only go to {counterpart:?}, not {to:?}"
        ));
    }
    Ok(())
}

/// Validate a `file.put` text: within [`MAX_AGENTS_REPO_DRAFT_TEXT_BYTES`],
/// no control characters other than newline, carriage return and tab. An
/// empty text is legal (a `.gitkeep`, an emptied plan).
pub fn validate_draft_text(value: &str) -> Result<(), String> {
    if value.len() > MAX_AGENTS_REPO_DRAFT_TEXT_BYTES {
        return Err(format!(
            "draft text is {} bytes; the cap is {MAX_AGENTS_REPO_DRAFT_TEXT_BYTES} — \
             a file this size is edited with git, not drafted",
            value.len()
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err("draft text must not contain control characters".to_owned());
    }
    Ok(())
}

fn validate_message(value: &str) -> Result<(), String> {
    if value.len() > MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES {
        return Err(format!(
            "draft message exceeds {MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES} bytes"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("draft message must be one line with no control characters".to_owned());
    }
    Ok(())
}

fn take_str<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("draft op field {key:?} must be a string"))
}

fn take_nullable_str<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, String> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(format!("draft op field {key:?} must be a string or null")),
    }
}

fn take_str_list(
    object: &Map<String, Value>,
    key: &str,
    check: fn(&str) -> Result<(), String>,
) -> Result<Vec<String>, String> {
    let list = object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("draft op field {key:?} must be a list"))?;
    if list.is_empty() || list.len() > MAX_COMMIT_RECORD_ENTRIES {
        return Err(format!(
            "draft op field {key:?} must have 1–{MAX_COMMIT_RECORD_ENTRIES} entries"
        ));
    }
    let mut out: Vec<String> = Vec::with_capacity(list.len());
    for value in list {
        let item = value
            .as_str()
            .ok_or_else(|| format!("draft op field {key:?} must list strings"))?;
        check(item)?;
        if out.iter().any(|seen| seen == item) {
            return Err(format!("draft op field {key:?} repeats {item:?}"));
        }
        out.push(item.to_owned());
    }
    Ok(out)
}

fn take_base(object: &Map<String, Value>) -> Result<DraftBase, String> {
    let base = take_nullable_str(object, "base")?.map(str::to_owned);
    if let Some(sha) = &base {
        if !is_git_sha(sha) {
            return Err("draft base must be a 40-character lowercase hex blob sha or null".into());
        }
    }
    let base_commit = take_nullable_str(object, "baseCommit")?.map(str::to_owned);
    if let Some(sha) = &base_commit {
        if !is_git_sha(sha) {
            return Err(
                "draft baseCommit must be a 40-character lowercase hex commit sha or null".into(),
            );
        }
    }
    let prev = take_nullable_str(object, "prev")?.map(str::to_owned);
    if let Some(id) = &prev {
        if !is_event_id(id) {
            return Err("draft prev must be a 64-character lowercase hex event id or null".into());
        }
    }
    Ok(DraftBase {
        base,
        base_commit,
        prev,
    })
}

/// Decode and validate content JSON. The key set is exact per op: every
/// listed key present (nullables as `null`, never absent), no other key.
pub fn decode_agents_repo_draft_op(content: &str, repo: &str) -> Result<AgentsRepoDraftOp, String> {
    if content.len() > MAX_AGENTS_REPO_DRAFT_CONTENT_BYTES {
        return Err(format!(
            "draft op content is {} bytes; the relay's cap is \
             {MAX_AGENTS_REPO_DRAFT_CONTENT_BYTES} — a file this size is edited with git",
            content.len()
        ));
    }
    let value: Value =
        serde_json::from_str(content).map_err(|_| "malformed draft op payload".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "draft op payload must be an object".to_owned())?;
    match object.get("schema").and_then(Value::as_str) {
        Some(AGENTS_REPO_DRAFT_SCHEMA) => {}
        _ => {
            return Err(format!(
                "draft op schema must be {AGENTS_REPO_DRAFT_SCHEMA:?}"
            ))
        }
    }
    let kind = AgentsRepoDraftOpKind::from_str(take_str(object, "op")?)?;
    let expected = kind.content_keys();
    if let Some(unknown) = object.keys().find(|key| !expected.contains(&key.as_str())) {
        return Err(format!(
            "draft op {} has unsupported field {unknown:?}",
            kind.as_str()
        ));
    }
    if let Some(missing) = expected.iter().find(|key| !object.contains_key(**key)) {
        return Err(format!(
            "draft op {} is missing field {missing:?}",
            kind.as_str()
        ));
    }
    let message = take_nullable_str(object, "message")?.map(str::to_owned);
    if let Some(message) = &message {
        validate_message(message)?;
    }
    let value = match kind {
        AgentsRepoDraftOpKind::FilePut => {
            let path = take_str(object, "path")?.to_owned();
            validate_draft_path(&path)?;
            let text = take_str(object, "text")?.to_owned();
            validate_draft_text(&text)?;
            AgentsRepoDraftOpValue::FilePut {
                path,
                text,
                base: take_base(object)?,
            }
        }
        AgentsRepoDraftOpKind::FileMove => {
            let path = take_str(object, "path")?.to_owned();
            let to = take_str(object, "to")?.to_owned();
            validate_move_destination(&path, &to)?;
            AgentsRepoDraftOpValue::FileMove {
                path,
                to,
                base: take_base(object)?,
            }
        }
        AgentsRepoDraftOpKind::FileDelete => {
            let path = take_str(object, "path")?.to_owned();
            if validate_draft_path(&path)? == DraftPathClass::RootFile {
                return Err(format!("draft may not delete the root file {path:?}"));
            }
            AgentsRepoDraftOpValue::FileDelete {
                path,
                base: take_base(object)?,
            }
        }
        AgentsRepoDraftOpKind::CommitRecord => {
            let commit = take_str(object, "commit")?.to_owned();
            if !is_git_sha(&commit) {
                return Err("commit.record commit must be a 40-character lowercase hex sha".into());
            }
            let paths = take_str_list(object, "paths", |path| {
                validate_draft_path(path).map(|_| ())
            })?;
            let drafts = take_str_list(object, "drafts", |id| {
                if is_event_id(id) {
                    Ok(())
                } else {
                    Err("commit.record drafts must be 64-character lowercase hex event ids".into())
                }
            })?;
            AgentsRepoDraftOpValue::CommitRecord {
                commit,
                paths,
                drafts,
            }
        }
    };
    Ok(AgentsRepoDraftOp {
        repo: repo.to_owned(),
        message,
        value,
    })
}

/// Validate a signed op end to end: kind, tag grammar, canonical project
/// and repository coordinates, content envelope, and tag/content agreement.
///
/// This is the single validator. The relay calls it and defines no local
/// copy; the buzz-sdk builder delegates to it rather than repeating the
/// rules. What it does **not** check, because it cannot: that `ad-repo` is
/// the repository the project's kind:30624 pins today (the relay checks at
/// ingest) and that a `commit.record`'s commit is on `main` (likewise).
///
/// **Tag grammar** — position-independent, closed key set: exactly one each
/// of `a`, `ad-v`, `ad-op`, `ad-repo`; `ad-path` once per path the op
/// names (one on `file.put`/`file.delete`, two on `file.move`, one per
/// `paths` entry on `commit.record`), values equal as a set to those paths;
/// every tag exactly two fields; any other key — including `h` — is a
/// rejection.
pub fn validate_agents_repo_draft_envelope(
    event: &nostr::Event,
) -> Result<AgentsRepoDraftOp, String> {
    if event_kind_u32(event) != KIND_AGENTS_REPO_DRAFT_OP {
        return Err("event is not an agents repo draft op (kind 44250)".to_owned());
    }

    let mut coordinate: Option<&str> = None;
    let mut version: Option<&str> = None;
    let mut tag_op: Option<&str> = None;
    let mut tag_repo: Option<&str> = None;
    let mut tag_paths: Vec<&str> = Vec::new();
    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() != 2 {
            return Err("draft op tags must have exactly two fields".to_owned());
        }
        let key = parts[0].as_str();
        let slot = match key {
            "a" => &mut coordinate,
            "ad-v" => &mut version,
            "ad-op" => &mut tag_op,
            "ad-repo" => &mut tag_repo,
            "ad-path" => {
                tag_paths.push(parts[1].as_str());
                continue;
            }
            "h" => return Err("draft op must not carry an h tag; it is project-scoped".to_owned()),
            other => return Err(format!("draft op has unsupported tag key {other:?}")),
        };
        if slot.is_some() {
            return Err(format!("draft op has more than one {key} tag"));
        }
        *slot = Some(parts[1].as_str());
    }

    let coordinate = coordinate.ok_or_else(|| "draft op requires one a tag".to_owned())?;
    if normalize_project_coordinate(coordinate).as_deref() != Some(coordinate) {
        return Err(
            "44250 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> coordinate".to_owned(),
        );
    }
    match version {
        Some(AGENTS_REPO_DRAFT_TAG_VERSION) => {}
        _ => return Err("unsupported draft op tag version".to_owned()),
    }
    let repo = tag_repo.ok_or_else(|| "draft op requires one ad-repo tag".to_owned())?;
    if normalize_repository_coordinate(repo).as_deref() != Some(repo) {
        return Err(
            "draft op ad-repo tag must be a canonical 30617:<lowercase-hex>:<id> coordinate"
                .to_owned(),
        );
    }
    let op = decode_agents_repo_draft_op(&event.content, repo)?;
    let tag_op = tag_op.ok_or_else(|| "draft op requires one ad-op tag".to_owned())?;
    if tag_op != op.kind().as_str() {
        return Err(format!(
            "draft op ad-op tag {tag_op:?} does not match content op {:?}",
            op.kind().as_str()
        ));
    }
    let mut expected: Vec<&str> = op.paths();
    expected.sort_unstable();
    tag_paths.sort_unstable();
    if tag_paths != expected {
        return Err("draft op ad-path tags do not match the paths its content names".to_owned());
    }
    // The extractor and the check above agree by construction; this keeps
    // the validator honest if either changes.
    if project_a_scoped_coordinate(event).as_deref() != Some(coordinate) {
        return Err("draft op a tag did not resolve to its coordinate".to_owned());
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
    const BLOB: &str = "0123456789abcdef0123456789abcdef01234567";
    const COMMIT: &str = "fedcba9876543210fedcba9876543210fedcba98";
    const PREV: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn base() -> DraftBase {
        DraftBase {
            base: Some(BLOB.to_owned()),
            base_commit: Some(COMMIT.to_owned()),
            prev: Some(PREV.to_owned()),
        }
    }

    fn put_op() -> AgentsRepoDraftOp {
        AgentsRepoDraftOp {
            repo: REPO.to_owned(),
            message: Some("tighten the lead's scope".to_owned()),
            value: AgentsRepoDraftOpValue::FilePut {
                path: "roles/lead.md".to_owned(),
                text: "---\ndescription: lead\n---\n![[beekeeper/lead@^1.0.0]]\n".to_owned(),
                base: base(),
            },
        }
    }

    fn move_op() -> AgentsRepoDraftOp {
        AgentsRepoDraftOp {
            repo: REPO.to_owned(),
            message: None,
            value: AgentsRepoDraftOpValue::FileMove {
                path: "plans/rpg.md".to_owned(),
                to: "plans/archive/rpg.md".to_owned(),
                base: DraftBase {
                    base: Some(BLOB.to_owned()),
                    base_commit: None,
                    prev: None,
                },
            },
        }
    }

    fn delete_op() -> AgentsRepoDraftOp {
        AgentsRepoDraftOp {
            repo: REPO.to_owned(),
            message: None,
            value: AgentsRepoDraftOpValue::FileDelete {
                path: "skills/marker/SKILL.md".to_owned(),
                base: base(),
            },
        }
    }

    fn record_op() -> AgentsRepoDraftOp {
        AgentsRepoDraftOp {
            repo: REPO.to_owned(),
            message: Some("landed".to_owned()),
            value: AgentsRepoDraftOpValue::CommitRecord {
                commit: COMMIT.to_owned(),
                paths: vec!["roles/lead.md".to_owned(), "plans/rpg.md".to_owned()],
                drafts: vec![PREV.to_owned()],
            },
        }
    }

    fn event_with(content: &str, tags: &[&[&str]]) -> nostr::Event {
        let keys = Keys::generate();
        let tag_vec: Vec<Tag> = tags
            .iter()
            .map(|parts| Tag::parse(parts.iter().copied()).expect("test tag parses"))
            .collect();
        EventBuilder::new(
            Kind::Custom(KIND_AGENTS_REPO_DRAFT_OP as u16),
            content.to_owned(),
        )
        .tags(tag_vec)
        .sign_with_keys(&keys)
        .expect("test event signs")
    }

    fn signed(op: &AgentsRepoDraftOp) -> nostr::Event {
        let tags = op.tags(COORD);
        let borrowed: Vec<Vec<&str>> = tags
            .iter()
            .map(|t| t.iter().map(String::as_str).collect())
            .collect();
        let slices: Vec<&[&str]> = borrowed.iter().map(Vec::as_slice).collect();
        event_with(&op.to_content(), &slices)
    }

    #[test]
    fn every_op_round_trips_through_content_and_envelope() {
        for op in [put_op(), move_op(), delete_op(), record_op()] {
            let decoded = decode_agents_repo_draft_op(&op.to_content(), REPO).expect("decodes");
            assert_eq!(decoded, op);
            let validated = validate_agents_repo_draft_envelope(&signed(&op)).expect("validates");
            assert_eq!(validated, op);
        }
    }

    #[test]
    fn nullables_are_emitted_as_null_and_required_on_decode() {
        let content = move_op().to_content();
        assert!(content.contains("\"baseCommit\":null"));
        assert!(content.contains("\"prev\":null"));
        assert!(content.contains("\"message\":null"));
        let mut value: Value = serde_json::from_str(&content).expect("json");
        value.as_object_mut().expect("object").remove("prev");
        let err = decode_agents_repo_draft_op(&value.to_string(), REPO).expect_err("refuses");
        assert!(err.contains("missing field \"prev\""), "{err}");
    }

    #[test]
    fn an_unknown_field_refuses() {
        let mut value: Value = serde_json::from_str(&put_op().to_content()).expect("json");
        value
            .as_object_mut()
            .expect("object")
            .insert("mode".into(), Value::from("100755"));
        let err = decode_agents_repo_draft_op(&value.to_string(), REPO).expect_err("refuses");
        assert!(err.contains("unsupported field \"mode\""), "{err}");
    }

    /// The shared corpus at `conformance/agents-repo-draft-path/`, which the
    /// Desktop and Mobile readers bind to as well. The table below is this
    /// reader's own convenience; the corpus is the contract. A rule that
    /// appears in only one of the three is a defect.
    #[test]
    fn conformance_vectors_classify_identically() {
        #[derive(serde::Deserialize)]
        struct Case {
            path: String,
            class: Option<String>,
            #[serde(default)]
            note: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Move {
            from: String,
            to: String,
            ok: bool,
            #[serde(default)]
            note: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct Vectors {
            schema: String,
            cases: Vec<Case>,
            moves: Vec<Move>,
        }
        let raw =
            include_str!("../../../conformance/agents-repo-draft-path/fixtures/path-vectors.json");
        let vectors: Vectors = serde_json::from_str(raw).expect("vectors parse");
        assert_eq!(vectors.schema, "buzz-agents-repo-draft-path-vectors/v2");
        assert!(!vectors.cases.is_empty(), "an empty corpus proves nothing");
        for case in vectors.cases {
            let note = case.note.unwrap_or_default();
            match (validate_draft_path(&case.path), case.class.as_deref()) {
                (Ok(class), Some(expected)) => {
                    assert_eq!(wire_class(class), expected, "case {:?} {note}", case.path)
                }
                (Err(error), Some(expected)) => {
                    panic!("case {:?} should be {expected} {note}: {error}", case.path)
                }
                (Ok(class), None) => panic!(
                    "case {:?} should be refused {note}, got {:?}",
                    case.path, class
                ),
                (Err(_), None) => {}
            }
        }
        assert!(
            !vectors.moves.is_empty(),
            "the destination rule needs vectors too"
        );
        for case in vectors.moves {
            let note = case.note.unwrap_or_default();
            let got = validate_move_destination(&case.from, &case.to);
            assert_eq!(
                got.is_ok(),
                case.ok,
                "move {:?} -> {:?} {note}: {got:?}",
                case.from,
                case.to
            );
        }
    }

    /// The corpus names a class the way the wire and the other two readers do.
    fn wire_class(class: DraftPathClass) -> &'static str {
        match class {
            DraftPathClass::RootFile => "root-file",
            DraftPathClass::Role => "role",
            DraftPathClass::ArchivedRole => "archived-role",
            DraftPathClass::RoleSkill => "role-skill",
            DraftPathClass::SharedSkill => "shared-skill",
            DraftPathClass::Plan => "plan",
            DraftPathClass::ArchivedPlan => "archived-plan",
            DraftPathClass::Document => "document",
            DraftPathClass::DocumentAsset => "document-asset",
            DraftPathClass::DocumentFolder => "document-folder",
        }
    }

    #[test]
    fn path_grammar_admits_the_layout_and_nothing_else() {
        for (path, class) in [
            ("README.md", DraftPathClass::RootFile),
            ("team.yml", DraftPathClass::RootFile),
            ("actions.yml", DraftPathClass::RootFile),
            ("roles/lead.md", DraftPathClass::Role),
            ("roles/archive/old-lead.md", DraftPathClass::ArchivedRole),
            (
                "roles/lead/skills/marker/SKILL.md",
                DraftPathClass::RoleSkill,
            ),
            (
                "roles/lead/skills/marker/notes/a.md",
                DraftPathClass::RoleSkill,
            ),
            ("skills/marker/SKILL.md", DraftPathClass::SharedSkill),
            ("plans/rpg-2.md", DraftPathClass::Plan),
            ("plans/archive/rpg.md", DraftPathClass::ArchivedPlan),
            ("docs/notes.md", DraftPathClass::Document),
            ("docs/mockups/login.html", DraftPathClass::Document),
            ("docs/a/b/c/d/e/f/g/h.md", DraftPathClass::Document),
            ("docs/img/shot.png", DraftPathClass::DocumentAsset),
            ("docs/img/diagram.svg", DraftPathClass::DocumentAsset),
            ("docs/.gitkeep", DraftPathClass::DocumentFolder),
            ("docs/mockups/.gitkeep", DraftPathClass::DocumentFolder),
        ] {
            assert_eq!(validate_draft_path(path).expect(path), class, "{path}");
        }
        for path in [
            "",
            "/roles/lead.md",
            "roles/lead.md/",
            "roles/../team.yml",
            "roles/./lead.md",
            "roles//lead.md",
            "roles/Lead.md",
            "roles/lead.txt",
            "roles/archive.md",
            "roles/archive/archive.md",
            "plans/archive/deeper/x.md",
            "roles/lead/skills/marker",
            "skills/marker",
            "skills/Marker/SKILL.md",
            "docs",
            "docs/x.txt",
            "docs/x.MD",
            "docs/.md",
            "docs/.hidden.md",
            "docs/-leading.md",
            "docs/.hidden/x.md",
            "docs/x/.gitignore",
            "docs/a/b/c/d/e/f/g/h/i.md",
            "plans/mockup.html",
            "beekeeper/actions.yml",
            "roles/lead/notes.md",
            "roles/lead/skills/marker/../x",
            "team.yaml",
        ] {
            assert!(validate_draft_path(path).is_err(), "{path:?} should refuse");
        }
    }

    #[test]
    fn archive_counterparts_are_the_only_move_targets() {
        assert_eq!(
            archive_counterpart("roles/lead.md").as_deref(),
            Some("roles/archive/lead.md")
        );
        assert_eq!(
            archive_counterpart("plans/archive/rpg.md").as_deref(),
            Some("plans/rpg.md")
        );
        assert_eq!(archive_counterpart("team.yml"), None);
        assert_eq!(archive_counterpart("skills/marker/SKILL.md"), None);

        let mut bad = move_op();
        if let AgentsRepoDraftOpValue::FileMove { to, .. } = &mut bad.value {
            *to = "plans/rpg-old.md".to_owned();
        }
        let err = decode_agents_repo_draft_op(&bad.to_content(), REPO).expect_err("refuses");
        assert!(err.contains("may only go to"), "{err}");
    }

    #[test]
    fn root_files_may_not_be_deleted() {
        let mut bad = delete_op();
        if let AgentsRepoDraftOpValue::FileDelete { path, .. } = &mut bad.value {
            *path = "team.yml".to_owned();
        }
        let err = decode_agents_repo_draft_op(&bad.to_content(), REPO).expect_err("refuses");
        assert!(err.contains("root file"), "{err}");
    }

    #[test]
    fn text_and_content_caps_name_which_tripped() {
        let mut big = put_op();
        if let AgentsRepoDraftOpValue::FilePut { text, .. } = &mut big.value {
            *text = "x".repeat(MAX_AGENTS_REPO_DRAFT_TEXT_BYTES + 1);
        }
        let err = decode_agents_repo_draft_op(&big.to_content(), REPO).expect_err("refuses");
        assert!(err.contains("60000"), "{err}");

        // Under the text cap, over the content cap once escaped.
        let mut escaped = put_op();
        if let AgentsRepoDraftOpValue::FilePut { text, .. } = &mut escaped.value {
            *text = "\"\n".repeat(MAX_AGENTS_REPO_DRAFT_TEXT_BYTES / 2);
        }
        let err = decode_agents_repo_draft_op(&escaped.to_content(), REPO).expect_err("refuses");
        assert!(err.contains("relay's cap"), "{err}");

        let mut nul = put_op();
        if let AgentsRepoDraftOpValue::FilePut { text, .. } = &mut nul.value {
            *text = "a\0b".to_owned();
        }
        assert!(decode_agents_repo_draft_op(&nul.to_content(), REPO).is_err());

        let mut empty = put_op();
        if let AgentsRepoDraftOpValue::FilePut { text, .. } = &mut empty.value {
            text.clear();
        }
        assert!(decode_agents_repo_draft_op(&empty.to_content(), REPO).is_ok());
    }

    #[test]
    fn base_prev_and_commit_shapes_are_checked() {
        let mut bad = put_op();
        if let AgentsRepoDraftOpValue::FilePut { base, .. } = &mut bad.value {
            base.base = Some("abc".to_owned());
        }
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
        let mut bad = put_op();
        if let AgentsRepoDraftOpValue::FilePut { base, .. } = &mut bad.value {
            base.prev = Some(BLOB.to_owned());
        }
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
        let mut bad = record_op();
        if let AgentsRepoDraftOpValue::CommitRecord { commit, .. } = &mut bad.value {
            *commit = COMMIT.to_uppercase();
        }
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
        let mut bad = record_op();
        if let AgentsRepoDraftOpValue::CommitRecord { drafts, .. } = &mut bad.value {
            drafts.push(PREV.to_owned());
        }
        let err = decode_agents_repo_draft_op(&bad.to_content(), REPO).expect_err("refuses");
        assert!(err.contains("repeats"), "{err}");
        let mut bad = record_op();
        if let AgentsRepoDraftOpValue::CommitRecord { paths, .. } = &mut bad.value {
            paths.clear();
        }
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
    }

    #[test]
    fn message_is_one_bounded_line() {
        let mut bad = put_op();
        bad.message = Some("two\nlines".to_owned());
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
        bad.message = Some("m".repeat(MAX_AGENTS_REPO_DRAFT_MESSAGE_BYTES + 1));
        assert!(decode_agents_repo_draft_op(&bad.to_content(), REPO).is_err());
    }

    #[test]
    fn envelope_refuses_h_unknown_keys_bad_coordinates_and_tag_mismatches() {
        let op = put_op();
        let content = op.to_content();
        let ok = &[
            &["a", COORD][..],
            &["ad-v", "ad1-1"],
            &["ad-op", "file.put"],
            &["ad-repo", REPO],
            &["ad-path", "roles/lead.md"],
        ];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, ok)).is_ok());

        let with_h = [ok[0], ok[1], ok[2], ok[3], ok[4], &["h", "chan"]];
        let err = validate_agents_repo_draft_envelope(&event_with(&content, &with_h))
            .expect_err("h refuses");
        assert!(err.contains("h tag"), "{err}");

        let unknown = [ok[0], ok[1], ok[2], ok[3], ok[4], &["e", PREV]];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, &unknown)).is_err());

        let no_path = [ok[0], ok[1], ok[2], ok[3]];
        let err = validate_agents_repo_draft_envelope(&event_with(&content, &no_path))
            .expect_err("path mismatch refuses");
        assert!(err.contains("ad-path"), "{err}");

        let wrong_path = [ok[0], ok[1], ok[2], ok[3], &["ad-path", "roles/poker.md"]];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, &wrong_path)).is_err());

        let wrong_op = [ok[0], ok[1], &["ad-op", "file.delete"], ok[3], ok[4]];
        let err = validate_agents_repo_draft_envelope(&event_with(&content, &wrong_op))
            .expect_err("op mismatch refuses");
        assert!(err.contains("does not match content op"), "{err}");

        let upper = COORD.replace("aaaa", "AAAA");
        let variant = [&["a", upper.as_str()][..], ok[1], ok[2], ok[3], ok[4]];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, &variant)).is_err());

        let project_as_repo = [ok[0], ok[1], ok[2], &["ad-repo", COORD], ok[4]];
        let err = validate_agents_repo_draft_envelope(&event_with(&content, &project_as_repo))
            .expect_err("30621 as repo refuses");
        assert!(err.contains("30617"), "{err}");

        let twice = [ok[0], ok[1], ok[2], ok[3], ok[3], ok[4]];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, &twice)).is_err());

        let three_fields = [
            ok[0],
            ok[1],
            ok[2],
            ok[3],
            &["ad-path", "roles/lead.md", "x"],
        ];
        assert!(validate_agents_repo_draft_envelope(&event_with(&content, &three_fields)).is_err());
    }

    #[test]
    fn a_move_and_a_record_carry_one_ad_path_per_named_path() {
        let tags = move_op().tags(COORD);
        let paths: Vec<&str> = tags
            .iter()
            .filter(|t| t[0] == "ad-path")
            .map(|t| t[1].as_str())
            .collect();
        assert_eq!(paths, ["plans/rpg.md", "plans/archive/rpg.md"]);
        let tags = record_op().tags(COORD);
        assert_eq!(tags.iter().filter(|t| t[0] == "ad-path").count(), 2);
        // Order on the wire is free; the set must match.
        let mut reordered = signed(&record_op()).tags.to_vec();
        reordered.reverse();
        let keys = Keys::generate();
        let event = EventBuilder::new(
            Kind::Custom(KIND_AGENTS_REPO_DRAFT_OP as u16),
            record_op().to_content(),
        )
        .tags(reordered)
        .sign_with_keys(&keys)
        .expect("signs");
        assert!(validate_agents_repo_draft_envelope(&event).is_ok());
    }
}
