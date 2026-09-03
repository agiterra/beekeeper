//! Where a project's persona packs live — kind:30624, the pack source record.
//!
//! Brian's direction of 2026-09-03: *"I should be able to log into beekeeper
//! from any machine and have my agents and their packs. If a role evolves it
//! should change on Andy's machine and any other team member's."*
//!
//! Packs are trees of text — seven roles is roughly 200 KB — so they do not
//! belong on the wire. They belong in a **git repository**, versioned and
//! reviewable like the rest of the work. What the wire carries is the
//! **pointer and the proof**: this kind says which repository holds a
//! project's packs, at which commit (or which ref), under which path. A host
//! staging a seat resolves the record, fetches the tree, and — because the
//! pointer can name an exact `sha` — can say afterwards *exactly* which bytes
//! it staged.
//!
//! # The record
//!
//! One addressable event per project:
//!
//! ```json
//! {
//!   "kind": 30624,
//!   "tags": [
//!     ["d", "30621:<owner-hex>:<slug>"],
//!     ["repo", "30617:<owner-hex>:<id>"],
//!     ["sha", "<40-hex>"],
//!     ["path", "personas/roles"]
//!   ],
//!   "content": "{\"schema\":\"buzz-project-pack-source/v1\",\"note\":\"…\"}"
//! }
//! ```
//!
//! `d` is the project's own coordinate, so newest-per-project wins by NIP-33
//! and removal is the kind-5 tombstone the relay already honours for
//! addressables — there is no "none" sentinel to mis-read.
//!
//! Exactly one of `ref` and `sha` is present, and that is enforced rather than
//! preferred: a record carrying both would let two honest hosts stage two
//! different trees from the same signed event, and a record carrying neither
//! points at nothing. `path` is optional and defaults to
//! [`DEFAULT_PACK_PATH`].
//!
//! # Who may write one
//!
//! The relay refuses a 30624 from anyone who is not a founder of one of the
//! project's repositories (L18's [`crate::repository_founders`]) or an Owner
//! of the project itself. This type validates *shape*; the relay owns
//! *authority*, because only the relay can read the roster.
//!
//! # Strict tags, on purpose
//!
//! Unknown tag names are refused rather than ignored. This is a new kind with
//! no signed history to preserve, and a silently-ignored tag on a record that
//! decides which code a seat runs is the wrong kind of tolerance: a typo in
//! `sha` that lands as an unknown tag would stage the ref's tip while its
//! author believed they had pinned a commit. Widening the record means a new
//! schema version, stated in `docs/nips/NIP-PK.md`.

use serde::{Deserialize, Serialize};

use crate::kind::{event_kind_u32, normalize_project_coordinate, KIND_PROJECT_PACK_SOURCE};

/// The content schema string every kind:30624 record carries.
pub const PROJECT_PACK_SOURCE_SCHEMA: &str = "buzz-project-pack-source/v1";

/// The path a record omitting `path` means: `personas/roles`.
pub const DEFAULT_PACK_PATH: &str = "personas/roles";

/// The `packRef.repo` value meaning *the packs the app shipped with*.
///
/// Not a coordinate, and deliberately not shaped like one: a build's bundled
/// packs are not a repository anyone can fetch, and dressing them as
/// `30617:…` would invite a reader to try. When a seat's `packRef` carries
/// this value, `sha` is the **app version** that bundled them.
///
/// This is the third and last rung of the staging ladder — project 30624
/// repository, then the session checkout's own `personas/roles`, then these —
/// and it exists so a team that has published nothing still gets working roles
/// on the first run, **named** rather than silently assumed.
pub const PACK_REF_SHIPPED_REPO: &str = "app:shipped";

/// The human words each `packRef.repo` form is shown as.
///
/// One place, so the CLI, the seat chip and Project settings cannot spell the
/// same source three ways.
pub const PACK_SOURCE_WORD_REPOSITORY: &str = "packs repository";
/// The word for [`PACK_REF_SHIPPED_REPO`].
pub const PACK_SOURCE_WORD_SHIPPED: &str = "shipped defaults";
/// The word for a seat staged from the session checkout alone.
pub const PACK_SOURCE_WORD_CHECKOUT: &str = "session checkout";

/// Tag naming the packs repository, as a `30617:<owner-hex>:<id>` coordinate.
pub const PACK_SOURCE_REPO_TAG: &str = "repo";
/// Tag pinning the packs to a branch tip (`refs/heads/main`).
pub const PACK_SOURCE_REF_TAG: &str = "ref";
/// Tag pinning the packs to one immutable commit (40 hex).
pub const PACK_SOURCE_SHA_TAG: &str = "sha";
/// Tag naming the directory inside the repository that holds the role packs.
pub const PACK_SOURCE_PATH_TAG: &str = "path";

/// Hard ceiling on a kind:30624 `content` string.
pub const MAX_PACK_SOURCE_CONTENT_BYTES: usize = 2048;
/// Hard ceiling on the optional operator note inside that content.
pub const MAX_PACK_SOURCE_NOTE_BYTES: usize = 512;
/// Hard ceiling on the `path` tag value.
pub const MAX_PACK_PATH_BYTES: usize = 200;
/// Hard ceiling on the `ref` tag value.
pub const MAX_PACK_REF_BYTES: usize = 200;

/// Which tree a pack source points at: a moving ref, or one fixed commit.
///
/// Two arms and no third: "the newest thing on this branch" and "exactly this
/// commit" are the only two answers a host can act on without guessing. A
/// record is invalid unless it carries exactly one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackPin {
    /// A fully qualified ref name (`refs/heads/main`). The host records the
    /// sha it actually resolved, so the staging is still nameable afterwards.
    Ref(String),
    /// A 40-hex commit id, lowercase. The bytes are decided by the record.
    Sha(String),
}

impl PackPin {
    /// The tag name this pin is written as: `ref` or `sha`.
    pub fn tag_name(&self) -> &'static str {
        match self {
            Self::Ref(_) => PACK_SOURCE_REF_TAG,
            Self::Sha(_) => PACK_SOURCE_SHA_TAG,
        }
    }

    /// The pin's value as it appears on the wire.
    pub fn value(&self) -> &str {
        match self {
            Self::Ref(value) | Self::Sha(value) => value,
        }
    }

    /// The ref name, when this pin is a ref.
    pub fn as_ref_name(&self) -> Option<&str> {
        match self {
            Self::Ref(value) => Some(value),
            Self::Sha(_) => None,
        }
    }

    /// The commit id, when this pin is a sha.
    pub fn as_sha(&self) -> Option<&str> {
        match self {
            Self::Sha(value) => Some(value),
            Self::Ref(_) => None,
        }
    }
}

/// A decoded kind:30624 record — where one project's packs live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPackSource {
    project: String,
    repo: String,
    pin: PackPin,
    path: String,
    note: Option<String>,
}

impl ProjectPackSource {
    /// The project coordinate this record speaks for (`30621:<hex>:<slug>`).
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The packs repository coordinate (`30617:<hex>:<id>`).
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// Which tree to stage: a ref, or a pinned commit.
    pub fn pin(&self) -> &PackPin {
        &self.pin
    }

    /// The directory inside the repository holding one directory per role.
    /// Never empty — an omitted `path` reads as [`DEFAULT_PACK_PATH`].
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The operator note, when the author wrote one.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// The path inside the repository holding `role`'s pack.
    ///
    /// The one place this join is written, so a host, the CLI's `status`, and
    /// a seat's published `packRef` cannot spell the same directory three
    /// ways. `None` when `role` is not a role slug.
    pub fn role_path(&self, role: &str) -> Option<String> {
        crate::coding_session_lifecycle_command::validate_role_slug(role).ok()?;
        Some(format!("{}/{role}", self.path))
    }

    /// The directory name a host's packs cache uses for this record's
    /// repository: `<first 8 of owner hex>-<repo id>`.
    ///
    /// Shared with the host so the CLI can say what *this machine* would
    /// stage without re-deriving a path the host might spell differently.
    pub fn cache_dir_name(&self) -> Option<String> {
        pack_cache_dir_name(&self.repo)
    }
}

/// The tags and content of a kind:30624 an author is about to sign.
///
/// Returned rather than an `EventBuilder` so the CLI, a test, and any future
/// publisher share one derivation of the shape without this crate taking an
/// opinion on how the event gets signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPackSourceDraft {
    /// The `d` tag value: the project coordinate, normalized.
    pub d_tag: String,
    /// Every tag, in wire order, `d` first.
    pub tags: Vec<Vec<String>>,
    /// The JSON content string.
    pub content: String,
}

/// The JSON content of a kind:30624.
///
/// Exactly one required key and one optional one. `note` is **omitted** when
/// absent rather than written as `null`, matching the `routing`/`beeStamp`
/// discipline on kind 44223: a null in a producer's output means the producer
/// invented a shape, so a reader is right to refuse it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPackSourceContent {
    /// Always [`PROJECT_PACK_SOURCE_SCHEMA`].
    pub schema: String,
    /// An operator note, at most [`MAX_PACK_SOURCE_NOTE_BYTES`] bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Normalize a repository coordinate to `30617:<lowercase-hex>:<dtag>`.
///
/// The 30617 twin of [`normalize_project_coordinate`], written here rather
/// than inferred from a split so a `30621` coordinate can never be accepted
/// where a repository is meant.
pub fn normalize_repository_coordinate(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, ':');
    let kind = parts.next()?;
    let pubkey = parts.next()?;
    let dtag = parts.next()?;
    if kind != "30617" {
        return None;
    }
    if pubkey.len() != 64 || !pubkey.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if dtag.is_empty() || dtag.chars().count() > 64 || dtag.chars().any(char::is_control) {
        return None;
    }
    Some(format!("30617:{}:{dtag}", pubkey.to_ascii_lowercase()))
}

/// The directory name a packs cache uses for a repository coordinate.
///
/// `<first 8 of the owner hex>-<repo id>`; `None` for anything that is not a
/// 30617 coordinate. Eight characters is enough to keep two owners' same-named
/// repositories apart on a developer's disk while staying readable in a path a
/// person has to type.
pub fn pack_cache_dir_name(repo_coordinate: &str) -> Option<String> {
    let normalized = normalize_repository_coordinate(repo_coordinate)?;
    let mut parts = normalized.splitn(3, ':');
    let _kind = parts.next()?;
    let owner = parts.next()?;
    let id = parts.next()?;
    Some(format!("{}-{id}", &owner[..8]))
}

/// Build the tags and content for a kind:30624, validating every part.
///
/// # Errors
/// A sentence naming the part that is wrong: a coordinate that is not a
/// 30621/30617 address, a ref or sha that is not one, a path that escapes the
/// repository, or a note over [`MAX_PACK_SOURCE_NOTE_BYTES`].
pub fn build_project_pack_source(
    project: &str,
    repo: &str,
    pin: &PackPin,
    path: Option<&str>,
    note: Option<&str>,
) -> Result<ProjectPackSourceDraft, String> {
    let project = normalize_project_coordinate(project.trim()).ok_or_else(|| {
        "pack source d must be a project coordinate 30621:<64-hex>:<slug>".to_string()
    })?;
    let repo = normalize_repository_coordinate(repo.trim()).ok_or_else(|| {
        "pack source repo must be a repository coordinate 30617:<64-hex>:<id>".to_string()
    })?;
    let pin = normalize_pin(pin)?;
    let path = match path {
        None => DEFAULT_PACK_PATH.to_string(),
        Some(value) => validate_pack_path(value)?,
    };
    let note = match note {
        None => None,
        Some(value) => Some(validate_note(value)?),
    };

    let mut tags: Vec<Vec<String>> = vec![
        vec!["d".to_string(), project.clone()],
        vec![PACK_SOURCE_REPO_TAG.to_string(), repo.clone()],
        vec![pin.tag_name().to_string(), pin.value().to_string()],
    ];
    if path != DEFAULT_PACK_PATH {
        tags.push(vec![PACK_SOURCE_PATH_TAG.to_string(), path.clone()]);
    }

    let content = serde_json::to_string(&ProjectPackSourceContent {
        schema: PROJECT_PACK_SOURCE_SCHEMA.to_string(),
        note: note.clone(),
    })
    .map_err(|error| format!("pack source content could not be encoded: {error}"))?;
    if content.len() > MAX_PACK_SOURCE_CONTENT_BYTES {
        return Err(format!(
            "pack source content exceeds {MAX_PACK_SOURCE_CONTENT_BYTES} bytes"
        ));
    }

    Ok(ProjectPackSourceDraft {
        d_tag: project,
        tags,
        content,
    })
}

/// Decode and fully validate a signed kind:30624.
///
/// This is the relay's ingest validator and every client's reader — one
/// function, so a record the relay stores is a record a client can read.
///
/// # Errors
/// A sentence naming the first thing wrong with the event: its kind, a
/// missing/repeated/unknown tag, both or neither pin, a malformed coordinate,
/// an escaping path, or content that is not this schema.
pub fn decode_project_pack_source(event: &nostr::Event) -> Result<ProjectPackSource, String> {
    if event_kind_u32(event) != KIND_PROJECT_PACK_SOURCE {
        return Err(format!(
            "pack source must be kind {KIND_PROJECT_PACK_SOURCE}"
        ));
    }

    let mut project: Option<String> = None;
    let mut repo: Option<String> = None;
    let mut pin: Option<PackPin> = None;
    let mut path: Option<String> = None;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        let [name, value] = parts else {
            return Err(format!(
                "pack source tags must be exactly two fields (got {} on {:?})",
                parts.len(),
                parts.first().map(String::as_str).unwrap_or_default()
            ));
        };
        match name.as_str() {
            "d" => {
                if project.is_some() {
                    return Err("pack source has more than one d tag".to_string());
                }
                project = Some(normalize_project_coordinate(value).ok_or_else(|| {
                    "pack source d must be a project coordinate 30621:<64-hex>:<slug>".to_string()
                })?);
            }
            PACK_SOURCE_REPO_TAG => {
                if repo.is_some() {
                    return Err("pack source has more than one repo tag".to_string());
                }
                repo = Some(normalize_repository_coordinate(value).ok_or_else(|| {
                    "pack source repo must be a repository coordinate 30617:<64-hex>:<id>"
                        .to_string()
                })?);
            }
            PACK_SOURCE_REF_TAG | PACK_SOURCE_SHA_TAG => {
                if pin.is_some() {
                    return Err(
                        "pack source must carry exactly one of ref and sha, not both".to_string(),
                    );
                }
                let candidate = if name == PACK_SOURCE_REF_TAG {
                    PackPin::Ref(value.clone())
                } else {
                    PackPin::Sha(value.clone())
                };
                pin = Some(normalize_pin(&candidate)?);
            }
            PACK_SOURCE_PATH_TAG => {
                if path.is_some() {
                    return Err("pack source has more than one path tag".to_string());
                }
                path = Some(validate_pack_path(value)?);
            }
            other => {
                return Err(format!("pack source carries unknown tag {other:?}"));
            }
        }
    }

    let project = project.ok_or_else(|| "pack source requires a d tag".to_string())?;
    let repo = repo.ok_or_else(|| "pack source requires a repo tag".to_string())?;
    let pin = pin.ok_or_else(|| {
        "pack source must carry exactly one of ref and sha, got neither".to_string()
    })?;
    let path = path.unwrap_or_else(|| DEFAULT_PACK_PATH.to_string());

    if event.content.len() > MAX_PACK_SOURCE_CONTENT_BYTES {
        return Err(format!(
            "pack source content exceeds {MAX_PACK_SOURCE_CONTENT_BYTES} bytes"
        ));
    }
    let value: serde_json::Value = serde_json::from_str(&event.content)
        .map_err(|_| "malformed pack source content".to_string())?;
    let object = value
        .as_object()
        .ok_or_else(|| "pack source content must be an object".to_string())?;
    if object.get("note").is_some_and(serde_json::Value::is_null) {
        return Err("pack source content note must not be null".to_string());
    }
    let content: ProjectPackSourceContent = serde_json::from_str(&event.content)
        .map_err(|error| format!("pack source content is not this schema: {error}"))?;
    if content.schema != PROJECT_PACK_SOURCE_SCHEMA {
        return Err(format!(
            "pack source content schema must be {PROJECT_PACK_SOURCE_SCHEMA:?}"
        ));
    }
    let note = match content.note {
        None => None,
        Some(value) => Some(validate_note(&value)?),
    };

    Ok(ProjectPackSource {
        project,
        repo,
        pin,
        path,
        note,
    })
}

/// Validate and normalize a pin.
fn normalize_pin(pin: &PackPin) -> Result<PackPin, String> {
    match pin {
        PackPin::Sha(value) => {
            let candidate = value.trim().to_ascii_lowercase();
            if candidate.len() != 40 || !candidate.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!(
                    "pack source sha must be 40 hex characters (got {value:?})"
                ));
            }
            Ok(PackPin::Sha(candidate))
        }
        PackPin::Ref(value) => Ok(PackPin::Ref(validate_ref_name(value)?)),
    }
}

/// Validate a fully qualified git ref name.
///
/// Deliberately narrower than `git check-ref-format`: a pack source names a
/// branch or a tag to stage, and a value that is not `refs/…` is far more
/// likely to be a mistyped sha than a legitimate ref this feature needs.
fn validate_ref_name(value: &str) -> Result<String, String> {
    let candidate = value.trim();
    if candidate.is_empty() || candidate.len() > MAX_PACK_REF_BYTES {
        return Err(format!(
            "pack source ref must be 1..={MAX_PACK_REF_BYTES} bytes (got {} bytes)",
            candidate.len()
        ));
    }
    if !candidate.starts_with("refs/") {
        return Err(format!(
            "pack source ref must be fully qualified, e.g. refs/heads/main (got {value:?})"
        ));
    }
    if candidate.ends_with('/')
        || candidate.contains("//")
        || candidate.contains("..")
        || candidate.ends_with(".lock")
    {
        return Err(format!(
            "pack source ref is not a valid ref name ({value:?})"
        ));
    }
    if candidate.chars().any(|character| {
        character.is_control()
            || character.is_whitespace()
            || matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\')
    }) {
        return Err(format!(
            "pack source ref is not a valid ref name ({value:?})"
        ));
    }
    Ok(candidate.to_string())
}

/// Validate a repository-relative directory path.
///
/// Refuses anything that could leave the checkout: an absolute path, a `..`
/// segment, a Windows separator, a drive letter's colon. A host takes this
/// value and joins it to a directory it fetched over the network, so a path
/// that escapes is a path that reads the operator's disk.
fn validate_pack_path(value: &str) -> Result<String, String> {
    let candidate = value.trim().trim_end_matches('/');
    if candidate.is_empty() || candidate.len() > MAX_PACK_PATH_BYTES {
        return Err(format!(
            "pack source path must be 1..={MAX_PACK_PATH_BYTES} bytes (got {} bytes)",
            candidate.len()
        ));
    }
    if candidate.starts_with('/') || candidate.starts_with('~') {
        return Err(format!("pack source path must be relative (got {value:?})"));
    }
    if candidate
        .chars()
        .any(|character| character.is_control() || matches!(character, '\\' | ':'))
    {
        return Err(format!(
            "pack source path must not carry control characters, backslashes or colons (got {value:?})"
        ));
    }
    for segment in candidate.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!(
                "pack source path must not carry empty or relative segments (got {value:?})"
            ));
        }
    }
    Ok(candidate.to_string())
}

/// Validate the optional operator note.
fn validate_note(value: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err("pack source note must be nonempty when present".to_string());
    }
    if value.len() > MAX_PACK_SOURCE_NOTE_BYTES {
        return Err(format!(
            "pack source note must be at most {MAX_PACK_SOURCE_NOTE_BYTES} bytes (got {} bytes)",
            value.len()
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err("pack source note must not carry control characters".to_string());
    }
    Ok(value.to_string())
}

#[cfg(test)]
#[path = "project_pack_source_tests.rs"]
mod tests;
