//! `sandbox.yml` at the root of the code repository: how a fresh sandbox of
//! this project is seeded with build state, and what counts as build state
//! when that sandbox is reclaimed.
//!
//! A sandbox — a seat's worktree, an action step's detached tree, or a tree a
//! person cut by hand — arrives with its tracked files and nothing else. The
//! code takes seconds; the build state does not. On this repository that is a
//! 36 GB `target/`, a second 19 GB `target/` under `desktop/src-tauri`, a
//! 1.3 GB `CARGO_HOME` at `.hermit/rust`, and `node_modules` in four places.
//! Nothing seeded any of it, so every sandbox re-downloaded the registry and
//! rebuilt from cold, and 14 trees accumulated 331 GB of duplicates.
//!
//! The project declares what to seed and how, once, here. The mechanism is
//! per entry on purpose: `target/` wants a copy-on-write clone, `.hermit/rust`
//! wants one shared pool, generated files want the recipe that generates them,
//! and `.env` wants a real copy. There is no global default, because no single
//! mechanism is right for all four.
//!
//! This module is pure: it parses, validates, and decides. Nothing here
//! touches the filesystem, runs `git`, or executes a `run` entry.
//! [`crate::sandbox_seed`] carries the decision out.
//!
//! # The donor is never in the file
//!
//! Every path is relative to a checkout, and the *source* checkout is resolved
//! by the host, never named by an entry. `sandbox.yml` is a tracked file an
//! agent may edit, and the execution boundary "cannot be widened by ACP
//! permission requests, runtime 'unsandboxed' flags, project config or shell
//! wrappers". A key that tries to name a donor is refused by name
//! ([`RESERVED_DONOR_KEYS`]) rather than ignored, so the attempt is legible.
//!
//! # Two safety rules are structural, not checked
//!
//! * `symlink` and `share` have no `reclaim` field at all, so "never reclaim
//!   through a link" cannot be expressed. Deleting through a link would delete
//!   the donor's copy, or empty a pool every other lane resolves through.
//! * `rewrite` is absent from `share`, and from `symlink` with `link: self`,
//!   so "never write through a link into the donor's own file" cannot be
//!   expressed either.
//!
//! Naming either key anyway is a refusal with its own code, because serde's
//! "unknown field" would not say *why*.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The manifest file name, at the root of the code repository.
pub const SANDBOX_YML: &str = "sandbox.yml";

/// The one schema this parser reads.
pub const SANDBOX_SCHEMA: &str = "beekeeper-sandbox/v1";

/// Largest manifest this parser reads, in bytes.
pub const MAX_SANDBOX_YML_BYTES: u64 = 64 * 1024;

/// Ceiling on entries in one file.
pub const MAX_SANDBOX_ENTRIES: usize = 64;

/// Directories a link into a donor can never safely be, because a package
/// manager keeps absolute-path state inside them. A closed list.
///
/// `node_modules` is the measured case: pnpm 11 purges the shared store
/// whenever workspace state looks stale, and a donor's
/// `.pnpm-workspace-state-v1.json` keys its projects by absolute path, so
/// every `pnpm <script>` in a tree linked to a donor decides the install is
/// stale and purges the store every concurrent lane resolves through.
pub const LINK_UNSAFE_BASENAMES: &[&str] = &["node_modules"];

/// The state files that make a `link: entries` layout safe for a
/// [`LINK_UNSAFE_BASENAMES`] directory: rewriting one of these re-roots the
/// donor's absolute paths onto the new tree.
pub const LINK_STATE_FILES: &[&str] = &[".pnpm-workspace-state-v1.json", ".modules.yaml"];

/// Keys recognized only so the refusal can say *why*, instead of serde's
/// generic "unknown field".
pub const RESERVED_DONOR_KEYS: &[&str] = &[
    "from",
    "donor",
    "source",
    "checkout",
    "host_path",
    "absolute",
];

/// Directories a `run` entry's `script` may live under.
pub const RUN_SCRIPT_ROOTS: &[&str] = &["scripts"];

/// Top-level keys this parser reads.
const TOP_LEVEL_KEYS: &[&str] = &["schema", "pool", "entries"];

/// How an entry's destination is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedKind {
    /// Copy-on-write clone (`clonefile` on APFS): the bytes are shared with
    /// the donor until one side writes, so a 36 GB `target/` costs seconds and
    /// almost no disk.
    Clone,
    /// A real, independent copy.
    Copy,
    /// A link into the donor checkout.
    Symlink,
    /// A link into a project-scoped pool shared by every sandbox of the
    /// project, rather than into one donor.
    Share,
    /// Run the project's own recipe or script, and let it produce the files.
    Run,
}

impl SeedKind {
    /// The spelling this kind has in the file.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clone => "clone",
            Self::Copy => "copy",
            Self::Symlink => "symlink",
            Self::Share => "share",
            Self::Run => "run",
        }
    }
}

/// Where the link goes when an entry links rather than copies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkShape {
    /// One symlink at the destination path.
    ///
    /// Note for the caller: [`crate::sandbox_seed`] still never places a
    /// symlink *at* a seeded root whose ignore rule is a directory pattern —
    /// see that module's invariant. This shape says what the project asked
    /// for; the seeder says whether git would stay silent about it.
    #[default]
    #[serde(rename = "self")]
    Zelf,
    /// The destination is a real directory and each top-level child of the
    /// source directory is linked into it. The form that works for pnpm, and
    /// the form a directory ignore pattern still hides.
    Entries,
}

/// Whether writers in a shared pool take a lock, as the project declares it.
///
/// Declared, never verified. Every receipt prints it as a declared value,
/// because a host that claimed to have checked would be lying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedLock {
    /// Writers take a lock; lanes sharing this pool block each other.
    Shared,
    /// Writers are lock-free, or lock per file, so lanes coexist.
    None,
}

/// What to do when the filesystem cannot clone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedFallback {
    /// Refuse the entry. The default: a declared clone must never silently
    /// become a real 36 GB copy.
    #[default]
    Refuse,
    /// Fall back to a real copy, and disclose that it did.
    Copy,
}

/// Whether an entry seeds at all, or only declares reclaim.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedWhen {
    /// Seed it.
    #[default]
    Always,
    /// Never seed it. The entry exists to tell reclaim this path is build
    /// state — cheap to rebuild, and a stale copy would mislead.
    Never,
}

/// What reclaim may do to an entry's destination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReclaimRule {
    /// Remove the directory when build output is reclaimable.
    Delete,
    /// Never removed. The default, for the reason an omitted setting is always
    /// given the restrictive answer: a typo must not authorize a deletion.
    #[default]
    Never,
}

/// A copy-on-write clone of a directory the donor already built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CloneEntry {
    /// Checkout-relative path, in both checkouts.
    pub path: String,
    /// What to do where the filesystem cannot clone.
    #[serde(default)]
    pub on_unsupported: UnsupportedFallback,
    /// Files inside the destination whose donor absolute paths must be
    /// re-rooted onto the new tree after the copy.
    #[serde(default)]
    pub rewrite: Vec<String>,
    /// Whether this entry seeds, or only declares reclaim.
    #[serde(default)]
    pub seed: SeedWhen,
    /// What reclaim may do to it.
    #[serde(default)]
    pub reclaim: ReclaimRule,
    /// Whether a missing source is a refusal. Defaults to true.
    #[serde(default)]
    pub required: Option<bool>,
    /// Literal environment this entry needs. Literals only.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Why this entry reads the way it does, for a person and for a receipt.
    #[serde(default)]
    pub note: Option<String>,
}

/// A real, independent copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CopyEntry {
    /// Checkout-relative path, in both checkouts.
    pub path: String,
    /// Files inside the destination whose donor absolute paths must be
    /// re-rooted onto the new tree after the copy.
    #[serde(default)]
    pub rewrite: Vec<String>,
    /// Whether this entry seeds, or only declares reclaim.
    #[serde(default)]
    pub seed: SeedWhen,
    /// What reclaim may do to it.
    #[serde(default)]
    pub reclaim: ReclaimRule,
    /// Whether a missing source is a refusal. Defaults to true.
    #[serde(default)]
    pub required: Option<bool>,
    /// Literal environment this entry needs. Literals only.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Why this entry reads the way it does.
    #[serde(default)]
    pub note: Option<String>,
}

/// A link into the donor checkout.
///
/// There is deliberately no `reclaim` field: deleting through this link would
/// delete the donor's own copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LinkEntry {
    /// Checkout-relative path, in both checkouts.
    pub path: String,
    /// Whether the path itself is the link, or the path is a directory of
    /// links to the source's children.
    #[serde(default)]
    pub link: LinkShape,
    /// With `link: entries` only: children copied and re-rooted rather than
    /// linked, because they carry the donor's absolute paths.
    #[serde(default)]
    pub materialize: Vec<String>,
    /// With `link: entries` only: materialized files to re-root.
    #[serde(default)]
    pub rewrite: Vec<String>,
    /// Whether a missing source is a refusal. Defaults to true.
    #[serde(default)]
    pub required: Option<bool>,
    /// Literal environment this entry needs.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Why this entry reads the way it does.
    #[serde(default)]
    pub note: Option<String>,
}

/// A link into a pool shared by every sandbox of this project.
///
/// There is deliberately no `reclaim` and no `rewrite`: the pool is nobody's
/// lane. Deleting it would empty it for every other sandbox, and rewriting a
/// file in it would edit shared state from inside one tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ShareEntry {
    /// Checkout-relative path the link is placed at.
    pub path: String,
    /// The pool's name inside this project's pool root. A slug, never a path:
    /// the host computes the root, the project names only the pool in it.
    pub id: String,
    /// Whether writers here block each other, as the project declares it.
    pub lock: SharedLock,
    /// Whether the path itself is the link, or a directory of links.
    #[serde(default)]
    pub link: LinkShape,
    /// Literal environment this entry needs.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Why this entry reads the way it does.
    #[serde(default)]
    pub note: Option<String>,
}

/// Run the project's own recipe or script and let it produce the files.
///
/// There is no free command: a recipe name or a script path under
/// [`RUN_SCRIPT_ROOTS`], and nothing else. Both measured needs — the sidecar
/// stubs and the per-worktree mobile identity files — are recipes the
/// repository already ships, and copying their output from a live checkout is
/// explicitly the wrong answer.
///
/// Stated plainly, because the honesty rule applies to documentation too: this
/// shape is legibility and defence in depth, **not** the security boundary.
/// The `Justfile` is tracked and editable as well. What enforces is that the
/// caller runs these inside the new tree's prepared boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RunEntry {
    /// A `just` recipe in the project's own `Justfile`.
    #[serde(default)]
    pub recipe: Option<String>,
    /// A script under one of [`RUN_SCRIPT_ROOTS`].
    #[serde(default)]
    pub script: Option<String>,
    /// What this run writes. Reclaim acts on these; seeding does not read them.
    #[serde(default)]
    pub produces: Vec<String>,
    /// What reclaim may do to `produces`.
    #[serde(default)]
    pub reclaim: ReclaimRule,
    /// Literal environment for the run. Literals only.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Names only: the host supplies the values and scrubs them from tails,
    /// and refuses a name the adapter fence covers.
    #[serde(default)]
    pub env_from_host: Vec<String>,
    /// Wall-clock ceiling for the run.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Why this entry reads the way it does.
    #[serde(default)]
    pub note: Option<String>,
}

/// One entry, by kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SandboxEntry {
    /// See [`CloneEntry`].
    Clone(CloneEntry),
    /// See [`CopyEntry`].
    Copy(CopyEntry),
    /// See [`LinkEntry`].
    Symlink(LinkEntry),
    /// See [`ShareEntry`].
    Share(ShareEntry),
    /// See [`RunEntry`].
    Run(RunEntry),
}

impl SandboxEntry {
    /// This entry's kind.
    #[must_use]
    pub const fn kind(&self) -> SeedKind {
        match self {
            Self::Clone(_) => SeedKind::Clone,
            Self::Copy(_) => SeedKind::Copy,
            Self::Symlink(_) => SeedKind::Symlink,
            Self::Share(_) => SeedKind::Share,
            Self::Run(_) => SeedKind::Run,
        }
    }

    /// The destination this entry owns, when it owns one.
    ///
    /// A `run` entry owns none: it names what it writes in `produces`, and
    /// several files at once.
    #[must_use]
    pub fn destination(&self) -> Option<&str> {
        match self {
            Self::Clone(entry) => Some(entry.path.as_str()),
            Self::Copy(entry) => Some(entry.path.as_str()),
            Self::Symlink(entry) => Some(entry.path.as_str()),
            Self::Share(entry) => Some(entry.path.as_str()),
            Self::Run(_) => None,
        }
    }

    /// Whether a missing source refuses the entry rather than skipping it.
    #[must_use]
    pub fn required(&self) -> bool {
        match self {
            Self::Clone(entry) => entry.required.unwrap_or(true),
            Self::Copy(entry) => entry.required.unwrap_or(true),
            Self::Symlink(entry) => entry.required.unwrap_or(true),
            Self::Share(_) | Self::Run(_) => true,
        }
    }

    /// Whether this entry seeds, as opposed to only declaring reclaim.
    #[must_use]
    pub fn seeds(&self) -> bool {
        match self {
            Self::Clone(entry) => matches!(entry.seed, SeedWhen::Always),
            Self::Copy(entry) => matches!(entry.seed, SeedWhen::Always),
            Self::Symlink(_) | Self::Share(_) | Self::Run(_) => true,
        }
    }

    /// The literal environment this entry declares.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<String, String> {
        match self {
            Self::Clone(entry) => &entry.env,
            Self::Copy(entry) => &entry.env,
            Self::Symlink(entry) => &entry.env,
            Self::Share(entry) => &entry.env,
            Self::Run(entry) => &entry.env,
        }
    }

    /// The files this entry re-roots after copying, if any.
    #[must_use]
    pub fn rewrite(&self) -> &[String] {
        match self {
            Self::Clone(entry) => &entry.rewrite,
            Self::Copy(entry) => &entry.rewrite,
            Self::Symlink(entry) => &entry.rewrite,
            Self::Share(_) | Self::Run(_) => &[],
        }
    }

    /// A short name for this entry in a receipt: its destination, or the
    /// recipe or script a `run` names.
    #[must_use]
    pub fn id(&self) -> String {
        match self {
            Self::Run(entry) => entry
                .recipe
                .clone()
                .or_else(|| entry.script.clone())
                .unwrap_or_else(|| "run".to_owned()),
            other => other.destination().unwrap_or_default().to_owned(),
        }
    }
}

/// The file as authored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxManifest {
    /// Must equal [`SANDBOX_SCHEMA`].
    pub schema: String,
    /// Namespace for this project's `share` pools. A slug, never a path.
    #[serde(default)]
    pub pool: Option<String>,
    /// The entries, in the order they are authored.
    #[serde(default)]
    pub entries: Vec<SandboxEntry>,
}

/// What the parser hands back: the manifest, what it warned about, and the
/// digest a receipt names so a reader knows which manifest ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPlan {
    /// The pool namespace, when the file declares one.
    pub pool: Option<String>,
    /// The validated entries.
    pub entries: Vec<SandboxEntry>,
    /// Non-fatal notes a person must see.
    pub warnings: Vec<SandboxWarning>,
    /// Lowercase hex SHA-256 of the manifest text, as authored.
    pub manifest_sha256: String,
}

/// A refusal, with a code callers and tests can match on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxRefusal {
    /// Stable identifier for this refusal.
    pub code: &'static str,
    /// What was wrong, in a sentence a person can act on.
    pub message: String,
}

impl std::fmt::Display for SandboxRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SandboxRefusal {}

/// A warning: surfaced, never fatal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxWarning {
    /// Stable identifier for this warning.
    pub code: &'static str,
    /// What a person should know.
    pub message: String,
}

fn refuse(code: &'static str, message: impl Into<String>) -> SandboxRefusal {
    SandboxRefusal {
        code,
        message: message.into(),
    }
}

fn warn(code: &'static str, message: impl Into<String>) -> SandboxWarning {
    SandboxWarning {
        code,
        message: message.into(),
    }
}

/// Not a YAML mapping, or a required field is missing.
pub const SANDBOX_MANIFEST_SHAPE: &str = "SANDBOX_MANIFEST_SHAPE";
/// `schema` is not [`SANDBOX_SCHEMA`].
pub const SANDBOX_MANIFEST_SCHEMA: &str = "SANDBOX_MANIFEST_SCHEMA";
/// A key no kind reads.
pub const SANDBOX_MANIFEST_UNKNOWN_KEY: &str = "SANDBOX_MANIFEST_UNKNOWN_KEY";
/// A key that tries to name a donor.
pub const SANDBOX_MANIFEST_DONOR_NAMED: &str = "SANDBOX_MANIFEST_DONOR_NAMED";
/// More entries than [`MAX_SANDBOX_ENTRIES`].
pub const SANDBOX_MANIFEST_TOO_MANY_ENTRIES: &str = "SANDBOX_MANIFEST_TOO_MANY_ENTRIES";
/// `kind` absent, or not one of the five.
pub const SANDBOX_MANIFEST_KIND_UNKNOWN: &str = "SANDBOX_MANIFEST_KIND_UNKNOWN";
/// A path that is empty or whitespace.
pub const SANDBOX_MANIFEST_PATH_EMPTY: &str = "SANDBOX_MANIFEST_PATH_EMPTY";
/// A path that is absolute.
pub const SANDBOX_MANIFEST_PATH_ABSOLUTE: &str = "SANDBOX_MANIFEST_PATH_ABSOLUTE";
/// A path with a component that climbs out of the checkout.
pub const SANDBOX_MANIFEST_PATH_ESCAPES: &str = "SANDBOX_MANIFEST_PATH_ESCAPES";
/// A path that normalizes to the checkout itself.
pub const SANDBOX_MANIFEST_PATH_IS_ROOT: &str = "SANDBOX_MANIFEST_PATH_IS_ROOT";
/// A path under `.git`.
pub const SANDBOX_MANIFEST_PATH_IS_GIT: &str = "SANDBOX_MANIFEST_PATH_IS_GIT";
/// The same destination claimed twice.
pub const SANDBOX_MANIFEST_PATH_DUPLICATE: &str = "SANDBOX_MANIFEST_PATH_DUPLICATE";
/// One entry's destination inside another's.
pub const SANDBOX_MANIFEST_PATH_NESTED: &str = "SANDBOX_MANIFEST_PATH_NESTED";
/// An option on a kind that does not take it.
pub const SANDBOX_MANIFEST_OPTION_NOT_LEGAL: &str = "SANDBOX_MANIFEST_OPTION_NOT_LEGAL";
/// `reclaim` on a `symlink` or `share` entry.
pub const SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK: &str = "SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK";
/// `rewrite` on `share`, or on `symlink` with `link: self`.
pub const SANDBOX_MANIFEST_REWRITE_THROUGH_LINK: &str = "SANDBOX_MANIFEST_REWRITE_THROUGH_LINK";
/// `materialize` without `link: entries`.
pub const SANDBOX_MANIFEST_MATERIALIZE_WITHOUT_ENTRIES: &str =
    "SANDBOX_MANIFEST_MATERIALIZE_WITHOUT_ENTRIES";
/// A link at a directory a package manager keeps absolute-path state in.
pub const SANDBOX_MANIFEST_LINK_UNSAFE: &str = "SANDBOX_MANIFEST_LINK_UNSAFE";
/// A `share` pool id that is missing or is not a slug.
pub const SANDBOX_MANIFEST_SHARE_ID: &str = "SANDBOX_MANIFEST_SHARE_ID";
/// A `share` entry that does not declare its concurrency cost.
pub const SANDBOX_MANIFEST_SHARE_LOCK_MISSING: &str = "SANDBOX_MANIFEST_SHARE_LOCK_MISSING";
/// `pool` is not a slug, or a `share` entry exists with no pool declared.
pub const SANDBOX_MANIFEST_POOL_SLUG: &str = "SANDBOX_MANIFEST_POOL_SLUG";
/// A `run` entry with neither or both of `recipe` and `script`.
pub const SANDBOX_MANIFEST_RUN_SHAPE: &str = "SANDBOX_MANIFEST_RUN_SHAPE";
/// A recipe name that could read as an option.
pub const SANDBOX_MANIFEST_RUN_RECIPE_NAME: &str = "SANDBOX_MANIFEST_RUN_RECIPE_NAME";
/// A script outside [`RUN_SCRIPT_ROOTS`].
pub const SANDBOX_MANIFEST_RUN_SCRIPT_PATH: &str = "SANDBOX_MANIFEST_RUN_SCRIPT_PATH";
/// A `run` entry claiming a destination.
pub const SANDBOX_MANIFEST_RUN_HAS_PATH: &str = "SANDBOX_MANIFEST_RUN_HAS_PATH";
/// An environment name that is not a variable name.
pub const SANDBOX_MANIFEST_ENV_NAME: &str = "SANDBOX_MANIFEST_ENV_NAME";
/// A name in both `env` and `env_from_host`.
pub const SANDBOX_MANIFEST_ENV_DUPLICATE: &str = "SANDBOX_MANIFEST_ENV_DUPLICATE";
/// An entry that neither seeds nor reclaims.
pub const SANDBOX_MANIFEST_ENTRY_INERT: &str = "SANDBOX_MANIFEST_ENTRY_INERT";
/// A manifest over [`MAX_SANDBOX_YML_BYTES`].
pub const SANDBOX_MANIFEST_TOO_LARGE: &str = "SANDBOX_MANIFEST_TOO_LARGE";
/// Present, but unreadable or not UTF-8.
pub const SANDBOX_MANIFEST_UNREADABLE: &str = "SANDBOX_MANIFEST_UNREADABLE";

/// An `env` literal that looks like a credential.
pub const WARN_SANDBOX_ENV_SECRET_SHAPED: &str = "WARN_SANDBOX_ENV_SECRET_SHAPED";
/// A lock-free `share` of a cargo target directory.
pub const WARN_SANDBOX_SHARE_CARGO_TARGET: &str = "WARN_SANDBOX_SHARE_CARGO_TARGET";
/// An entry that seeds an env file.
pub const WARN_SANDBOX_SEEDS_ENV: &str = "WARN_SANDBOX_SEEDS_ENV";
/// A clone that may become a real copy.
pub const WARN_SANDBOX_CLONE_FALLBACK_COPY: &str = "WARN_SANDBOX_CLONE_FALLBACK_COPY";

/// Keys each kind reads, `kind` included.
fn allowed_keys(kind: SeedKind) -> &'static [&'static str] {
    match kind {
        SeedKind::Clone => &[
            "kind",
            "path",
            "on_unsupported",
            "rewrite",
            "seed",
            "reclaim",
            "required",
            "env",
            "note",
        ],
        SeedKind::Copy => &[
            "kind", "path", "rewrite", "seed", "reclaim", "required", "env", "note",
        ],
        SeedKind::Symlink => &[
            "kind",
            "path",
            "link",
            "materialize",
            "rewrite",
            "required",
            "env",
            "note",
        ],
        SeedKind::Share => &["kind", "path", "id", "lock", "link", "env", "note"],
        SeedKind::Run => &[
            "kind",
            "recipe",
            "script",
            "produces",
            "reclaim",
            "env",
            "env_from_host",
            "timeout_secs",
            "note",
        ],
    }
}

/// Every key some kind reads, so a key that is merely on the wrong kind can
/// say so instead of reading as a typo.
const EVERY_KNOWN_KEY: &[&str] = &[
    "kind",
    "path",
    "on_unsupported",
    "rewrite",
    "seed",
    "reclaim",
    "required",
    "env",
    "note",
    "link",
    "materialize",
    "id",
    "lock",
    "recipe",
    "script",
    "produces",
    "env_from_host",
    "timeout_secs",
];

fn kind_from_str(raw: &str) -> Option<SeedKind> {
    match raw {
        "clone" => Some(SeedKind::Clone),
        "copy" => Some(SeedKind::Copy),
        "symlink" => Some(SeedKind::Symlink),
        "share" => Some(SeedKind::Share),
        "run" => Some(SeedKind::Run),
        _ => None,
    }
}

/// Which kinds read a key, for the "not legal on this kind" message.
fn kinds_reading(key: &str) -> Vec<&'static str> {
    [
        SeedKind::Clone,
        SeedKind::Copy,
        SeedKind::Symlink,
        SeedKind::Share,
        SeedKind::Run,
    ]
    .into_iter()
    .filter(|kind| allowed_keys(*kind).contains(&key))
    .map(SeedKind::as_str)
    .collect()
}

/// Split a checkout-relative path into its normalized components.
///
/// The predicate is the one `team.yml` uses for a role file: every component
/// must be `Normal` or `CurDir`, so `..`, a root, and a Windows prefix are all
/// one rule rather than three checks that can disagree.
fn normalize_relative(context: &str, raw: &str) -> Result<Vec<String>, SandboxRefusal> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(refuse(
            SANDBOX_MANIFEST_PATH_EMPTY,
            format!("{context} is empty"),
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(refuse(
            SANDBOX_MANIFEST_PATH_ABSOLUTE,
            format!(
                "{context} must be relative to the checkout, got {trimmed:?}; \
                 an entry cannot name a path outside it"
            ),
        ));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let Some(text) = part.to_str() else {
                    return Err(refuse(
                        SANDBOX_MANIFEST_PATH_ESCAPES,
                        format!("{context} is not valid UTF-8: {trimmed:?}"),
                    ));
                };
                parts.push(text.to_owned());
            }
            _ => {
                return Err(refuse(
                    SANDBOX_MANIFEST_PATH_ESCAPES,
                    format!(
                        "{context} must stay inside the checkout, got {trimmed:?}; \
                         no component may climb out of it"
                    ),
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(refuse(
            SANDBOX_MANIFEST_PATH_IS_ROOT,
            format!("{context} normalizes to the checkout itself, which is not an entry"),
        ));
    }
    if parts[0] == ".git" {
        return Err(refuse(
            SANDBOX_MANIFEST_PATH_IS_GIT,
            format!("{context} names {trimmed:?} under .git, which is never build state"),
        ));
    }
    Ok(parts)
}

/// A slug: 1-64 bytes of `[a-z0-9-]`.
fn is_slug(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 64
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// An environment variable name: `[A-Z_][A-Z0-9_]*`.
fn is_env_name(raw: &str) -> bool {
    let mut bytes = raw.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase() || first == b'_') {
        return false;
    }
    bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

/// A `just` recipe name that can never read as an option.
fn is_recipe_name(raw: &str) -> bool {
    !raw.is_empty()
        && raw.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_' || byte == b'-'
        })
        && !raw.starts_with('-')
}

/// Whether an `env` literal looks like a credential.
///
/// Heuristic and deliberately loud rather than precise: it warns, never
/// refuses, and the actions.yml precedent is the same ("literals only; warn
/// on secret-shaped values").
fn looks_secret(name: &str, value: &str) -> bool {
    const SUSPICIOUS: &[&str] = &[
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "APIKEY",
        "API_KEY",
        "PRIVATE_KEY",
        "NSEC",
        "CREDENTIAL",
    ];
    if SUSPICIOUS.iter().any(|needle| name.contains(needle)) {
        return true;
    }
    value.starts_with("nsec1")
        || (value.len() >= 32
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' || byte == b'='
            })
            && value.bytes().any(|byte| byte.is_ascii_digit())
            && value.bytes().any(|byte| byte.is_ascii_lowercase())
            && value.bytes().any(|byte| byte.is_ascii_uppercase()))
}

/// Look a key up by name, without depending on a `serde_yaml` version's
/// `Mapping::get` signature.
fn field<'a>(mapping: &'a serde_yaml::Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    mapping
        .iter()
        .find(|(name, _)| name.as_str() == Some(key))
        .map(|(_, value)| value)
}

/// Parse and validate manifest text.
///
/// Two passes on purpose. The first walks the raw mapping against a per-kind
/// key allowlist, so a key on the wrong kind, or one trying to name a donor,
/// is refused with its own code; serde's internally tagged enums would
/// otherwise collapse all of it into one "unknown field". The second is the
/// typed parse, then the semantic rules.
///
/// # Errors
///
/// A [`SandboxRefusal`] whose `code` names what was wrong. Every refusal in
/// this module's `SANDBOX_MANIFEST_*` constants is reachable from here.
pub fn parse_sandbox_yml(text: &str) -> Result<SandboxPlan, SandboxRefusal> {
    let manifest_sha256 = hex::encode(Sha256::digest(text.as_bytes()));
    let document: serde_yaml::Value = serde_yaml::from_str(text).map_err(|error| {
        refuse(
            SANDBOX_MANIFEST_SHAPE,
            format!("{SANDBOX_YML} is not valid YAML: {error}"),
        )
    })?;
    {
        let mapping = document.as_mapping().ok_or_else(|| {
            refuse(
                SANDBOX_MANIFEST_SHAPE,
                format!("{SANDBOX_YML} must be a mapping with a schema and entries"),
            )
        })?;
        for (key, _) in mapping {
            let Some(name) = key.as_str() else {
                return Err(refuse(
                    SANDBOX_MANIFEST_SHAPE,
                    format!("{SANDBOX_YML} has a non-string top-level key"),
                ));
            };
            if TOP_LEVEL_KEYS.contains(&name) {
                continue;
            }
            if RESERVED_DONOR_KEYS.contains(&name) {
                return Err(donor_named(name, "the file"));
            }
            return Err(refuse(
                SANDBOX_MANIFEST_UNKNOWN_KEY,
                format!(
                    "{SANDBOX_YML} has no top-level {name:?}; it reads {}",
                    TOP_LEVEL_KEYS.join(", ")
                ),
            ));
        }
        if let Some(entries) = field(mapping, "entries") {
            let sequence = entries.as_sequence().ok_or_else(|| {
                refuse(
                    SANDBOX_MANIFEST_SHAPE,
                    format!("{SANDBOX_YML} entries must be a list"),
                )
            })?;
            if sequence.len() > MAX_SANDBOX_ENTRIES {
                return Err(refuse(
                    SANDBOX_MANIFEST_TOO_MANY_ENTRIES,
                    format!(
                        "{SANDBOX_YML} lists {} entries; at most {MAX_SANDBOX_ENTRIES} are allowed",
                        sequence.len()
                    ),
                ));
            }
            for (index, item) in sequence.iter().enumerate() {
                check_entry_keys(index, item)?;
            }
        }
    }
    let manifest: SandboxManifest = serde_yaml::from_value(document).map_err(|error| {
        refuse(
            SANDBOX_MANIFEST_SHAPE,
            format!("{SANDBOX_YML} does not read as a manifest: {error}"),
        )
    })?;
    if manifest.schema != SANDBOX_SCHEMA {
        return Err(refuse(
            SANDBOX_MANIFEST_SCHEMA,
            format!(
                "{SANDBOX_YML} schema must be {SANDBOX_SCHEMA:?} (got {:?})",
                manifest.schema
            ),
        ));
    }
    let warnings = validate(&manifest)?;
    Ok(SandboxPlan {
        pool: manifest.pool.map(|pool| pool.trim().to_owned()),
        entries: manifest.entries,
        warnings,
        manifest_sha256,
    })
}

fn donor_named(name: &str, where_at: &str) -> SandboxRefusal {
    refuse(
        SANDBOX_MANIFEST_DONOR_NAMED,
        format!(
            "{where_at} names {name:?}, but an entry cannot name a donor: the source checkout is \
             resolved by the host, and every path here is relative to a checkout"
        ),
    )
}

/// First pass: the raw keys of one entry, against the allowlist for its kind.
fn check_entry_keys(index: usize, item: &serde_yaml::Value) -> Result<(), SandboxRefusal> {
    let at = format!("{SANDBOX_YML} entries[{index}]");
    let mapping = item
        .as_mapping()
        .ok_or_else(|| refuse(SANDBOX_MANIFEST_SHAPE, format!("{at} must be a mapping")))?;
    let kind_raw = field(mapping, "kind")
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| {
            refuse(
                SANDBOX_MANIFEST_KIND_UNKNOWN,
                format!("{at} must name a kind: clone, copy, symlink, share or run"),
            )
        })?;
    let kind = kind_from_str(kind_raw).ok_or_else(|| {
        refuse(
            SANDBOX_MANIFEST_KIND_UNKNOWN,
            format!("{at} kind {kind_raw:?} is not one of clone, copy, symlink, share or run"),
        )
    })?;
    let allowed = allowed_keys(kind);
    let kind_name = kind.as_str();
    for (key, _) in mapping {
        let Some(name) = key.as_str() else {
            return Err(refuse(
                SANDBOX_MANIFEST_SHAPE,
                format!("{at} has a non-string key"),
            ));
        };
        if allowed.contains(&name) {
            continue;
        }
        if RESERVED_DONOR_KEYS.contains(&name) {
            return Err(donor_named(name, &at));
        }
        if name == "reclaim" && matches!(kind, SeedKind::Symlink | SeedKind::Share) {
            return Err(refuse(
                SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK,
                format!(
                    "{at} is a {kind_name} entry and has no reclaim: deleting through a link would \
                     delete what it points at. A link is unlinked, never reclaimed."
                ),
            ));
        }
        if name == "rewrite" && kind == SeedKind::Share {
            return Err(refuse(
                SANDBOX_MANIFEST_REWRITE_THROUGH_LINK,
                format!(
                    "{at} is a share entry and has no rewrite: the file belongs to a pool every \
                     sandbox of this project resolves through, and rewriting it from inside one \
                     tree would edit shared state"
                ),
            ));
        }
        if name == "path" && kind == SeedKind::Run {
            return Err(refuse(
                SANDBOX_MANIFEST_RUN_HAS_PATH,
                format!(
                    "{at} is a run entry and owns no destination; name what it writes in produces"
                ),
            ));
        }
        if EVERY_KNOWN_KEY.contains(&name) {
            let readers = kinds_reading(name);
            return Err(refuse(
                SANDBOX_MANIFEST_OPTION_NOT_LEGAL,
                format!(
                    "{at}: {name:?} is not legal on a {kind_name} entry; it is read by {}",
                    readers.join(", ")
                ),
            ));
        }
        return Err(refuse(
            SANDBOX_MANIFEST_UNKNOWN_KEY,
            format!("{at}: no kind reads {name:?}"),
        ));
    }
    if kind == SeedKind::Share {
        if field(mapping, "id").is_none() {
            return Err(refuse(
                SANDBOX_MANIFEST_SHARE_ID,
                format!("{at} is a share entry and must name its pool with id"),
            ));
        }
        if field(mapping, "lock").is_none() {
            return Err(refuse(
                SANDBOX_MANIFEST_SHARE_LOCK_MISSING,
                format!(
                    "{at} is a share entry and must declare lock: shared or lock: none. An \
                     unanswered concurrency cost is not a default."
                ),
            ));
        }
    }
    Ok(())
}

/// Second pass: the semantic rules, over the typed manifest.
fn validate(manifest: &SandboxManifest) -> Result<Vec<SandboxWarning>, SandboxRefusal> {
    let mut warnings = Vec::new();
    if let Some(pool) = manifest.pool.as_deref() {
        let pool = pool.trim();
        if !is_slug(pool) {
            return Err(refuse(
                SANDBOX_MANIFEST_POOL_SLUG,
                format!(
                    "{SANDBOX_YML} pool must be a slug of [a-z0-9-], at most 64 bytes, never a \
                     path (got {pool:?}); the host computes the pool root, the project names only \
                     the pool inside it"
                ),
            ));
        }
    }
    // Every destination any entry claims, so two entries cannot claim one path
    // and no entry can sit inside another's directory.
    let mut claimed: Vec<(Vec<String>, String)> = Vec::new();
    for (index, entry) in manifest.entries.iter().enumerate() {
        let at = format!("{SANDBOX_YML} entries[{index}]");
        check_env(&at, entry, &mut warnings)?;
        if let Some(path) = entry.destination() {
            let parts = normalize_relative(&format!("{at}.path"), path)?;
            let basename = parts.last().map(String::as_str).unwrap_or_default();
            if entry.seeds() && basename.starts_with(".env") {
                warnings.push(warn(
                    WARN_SANDBOX_SEEDS_ENV,
                    format!(
                        "{at} seeds {path:?} from the source checkout. Nothing sweeps sibling env \
                         files; this one is seeded because the project declared it, and every \
                         receipt says so."
                    ),
                ));
            }
            claimed.push((parts.clone(), format!("{at}.path ({path})")));
            match entry {
                SandboxEntry::Clone(clone) => {
                    if matches!(clone.on_unsupported, UnsupportedFallback::Copy) {
                        warnings.push(warn(
                            WARN_SANDBOX_CLONE_FALLBACK_COPY,
                            format!(
                                "{at} declares on_unsupported: copy, so where the filesystem \
                                 cannot clone, {path:?} becomes a real copy. The receipt names the \
                                 mechanism it actually used."
                            ),
                        ));
                    }
                    if matches!(clone.seed, SeedWhen::Never)
                        && matches!(clone.reclaim, ReclaimRule::Never)
                    {
                        return Err(inert(&at));
                    }
                    check_inner_paths(&at, "rewrite", &clone.rewrite)?;
                }
                SandboxEntry::Copy(copy) => {
                    if matches!(copy.seed, SeedWhen::Never)
                        && matches!(copy.reclaim, ReclaimRule::Never)
                    {
                        return Err(inert(&at));
                    }
                    check_inner_paths(&at, "rewrite", &copy.rewrite)?;
                }
                SandboxEntry::Symlink(link) => {
                    check_inner_paths(&at, "rewrite", &link.rewrite)?;
                    check_inner_paths(&at, "materialize", &link.materialize)?;
                    if matches!(link.link, LinkShape::Zelf) {
                        if !link.rewrite.is_empty() {
                            return Err(refuse(
                                SANDBOX_MANIFEST_REWRITE_THROUGH_LINK,
                                format!(
                                    "{at} links {path:?} itself, so a rewrite would edit the \
                                     source checkout's own file. Use link: entries, where the \
                                     destination is a real directory and the rewritten file is \
                                     this tree's own."
                                ),
                            ));
                        }
                        if !link.materialize.is_empty() {
                            return Err(refuse(
                                SANDBOX_MANIFEST_MATERIALIZE_WITHOUT_ENTRIES,
                                format!("{at} names materialize, which needs link: entries"),
                            ));
                        }
                    }
                    check_link_safety(&at, basename, link.link, &link.rewrite, &link.materialize)?;
                }
                SandboxEntry::Share(share) => {
                    if manifest.pool.is_none() {
                        return Err(refuse(
                            SANDBOX_MANIFEST_POOL_SLUG,
                            format!(
                                "{at} shares a pool, but {SANDBOX_YML} declares no top-level pool \
                                 to put it in"
                            ),
                        ));
                    }
                    if !is_slug(share.id.trim()) {
                        return Err(refuse(
                            SANDBOX_MANIFEST_SHARE_ID,
                            format!(
                                "{at} id must be a slug of [a-z0-9-], at most 64 bytes, never a \
                                 path (got {:?})",
                                share.id
                            ),
                        ));
                    }
                    if matches!(share.lock, SharedLock::None) && basename == "target" {
                        warnings.push(warn(
                            WARN_SANDBOX_SHARE_CARGO_TARGET,
                            format!(
                                "{at} shares {path:?} with lock: none. CI shares one target \
                                 directory only because it runs one workflow at a time; local \
                                 sandboxes build in parallel and will block each other on cargo's \
                                 build lock. clone is the mechanism for a target directory."
                            ),
                        ));
                    }
                    check_link_safety(&at, basename, share.link, &[], &[])?;
                }
                SandboxEntry::Run(_) => unreachable!("a run entry owns no destination"),
            }
        }
        if let SandboxEntry::Run(run) = entry {
            match (run.recipe.as_deref(), run.script.as_deref()) {
                (Some(recipe), None) => {
                    if !is_recipe_name(recipe.trim()) {
                        return Err(refuse(
                            SANDBOX_MANIFEST_RUN_RECIPE_NAME,
                            format!(
                                "{at} recipe must be [A-Za-z0-9._-] and may not start with a dash, \
                                 so it can never read as an option (got {recipe:?})"
                            ),
                        ));
                    }
                }
                (None, Some(script)) => {
                    let parts = normalize_relative(&format!("{at}.script"), script)?;
                    if !RUN_SCRIPT_ROOTS.contains(&parts[0].as_str()) || parts.len() < 2 {
                        return Err(refuse(
                            SANDBOX_MANIFEST_RUN_SCRIPT_PATH,
                            format!(
                                "{at} script must live under {} (got {script:?})",
                                RUN_SCRIPT_ROOTS.join(", ")
                            ),
                        ));
                    }
                }
                _ => {
                    return Err(refuse(
                        SANDBOX_MANIFEST_RUN_SHAPE,
                        format!(
                            "{at} must name exactly one of recipe or script: a run entry has no \
                             free command"
                        ),
                    ));
                }
            }
            for (slot, produced) in run.produces.iter().enumerate() {
                let parts = normalize_relative(&format!("{at}.produces[{slot}]"), produced)?;
                claimed.push((parts, format!("{at}.produces[{slot}] ({produced})")));
            }
            let mut seen = BTreeSet::new();
            for name in &run.env_from_host {
                if !is_env_name(name) {
                    return Err(refuse(
                        SANDBOX_MANIFEST_ENV_NAME,
                        format!(
                            "{at} env_from_host must be variable names of [A-Z_][A-Z0-9_]* \
                             (got {name:?})"
                        ),
                    ));
                }
                if run.env.contains_key(name) || !seen.insert(name.clone()) {
                    return Err(refuse(
                        SANDBOX_MANIFEST_ENV_DUPLICATE,
                        format!(
                            "{at} names {name:?} twice: a variable comes either from a literal or \
                             from the host, never both"
                        ),
                    ));
                }
            }
        }
    }
    for (outer, (parts, context)) in claimed.iter().enumerate() {
        for (other_parts, other_context) in claimed.iter().skip(outer + 1) {
            if parts == other_parts {
                return Err(refuse(
                    SANDBOX_MANIFEST_PATH_DUPLICATE,
                    format!("{context} and {other_context} claim the same destination"),
                ));
            }
            let (shorter, longer, short_context, long_context) = if parts.len() < other_parts.len()
            {
                (parts, other_parts, context, other_context)
            } else {
                (other_parts, parts, other_context, context)
            };
            if longer[..shorter.len()] == shorter[..] {
                return Err(refuse(
                    SANDBOX_MANIFEST_PATH_NESTED,
                    format!(
                        "{long_context} is inside {short_context}; the order the two would be \
                         carried out in is undefined, so one of them has to go"
                    ),
                ));
            }
        }
    }
    Ok(warnings)
}

fn inert(at: &str) -> SandboxRefusal {
    refuse(
        SANDBOX_MANIFEST_ENTRY_INERT,
        format!("{at} neither seeds nor reclaims, so it declares nothing"),
    )
}

/// Paths inside an entry's own destination: the same rules, minus the
/// duplicate and nesting checks, which only apply between entries.
fn check_inner_paths(at: &str, field_name: &str, paths: &[String]) -> Result<(), SandboxRefusal> {
    for (slot, path) in paths.iter().enumerate() {
        normalize_relative(&format!("{at}.{field_name}[{slot}]"), path)?;
    }
    Ok(())
}

/// Refuse a link at a directory a package manager keeps absolute-path state
/// in, unless the entry is expressed in the one shape that survives it.
fn check_link_safety(
    at: &str,
    basename: &str,
    link: LinkShape,
    rewrite: &[String],
    materialize: &[String],
) -> Result<(), SandboxRefusal> {
    if !LINK_UNSAFE_BASENAMES.contains(&basename) {
        return Ok(());
    }
    let re_roots_state = rewrite
        .iter()
        .chain(materialize.iter())
        .any(|name| LINK_STATE_FILES.contains(&name.trim()));
    if matches!(link, LinkShape::Entries) && re_roots_state {
        return Ok(());
    }
    Err(refuse(
        SANDBOX_MANIFEST_LINK_UNSAFE,
        format!(
            "{at} links {basename:?}, which keeps absolute paths to the checkout it was installed \
             in. pnpm reads that state, decides the install is stale, and purges the store every \
             sandbox of this project resolves through. Two expressions survive it: \
             `kind: clone` with `rewrite: [\".pnpm-workspace-state-v1.json\"]`, or `kind: run` \
             with the project's own install recipe."
        ),
    ))
}

fn check_env(
    at: &str,
    entry: &SandboxEntry,
    warnings: &mut Vec<SandboxWarning>,
) -> Result<(), SandboxRefusal> {
    for (name, value) in entry.env() {
        if !is_env_name(name) {
            return Err(refuse(
                SANDBOX_MANIFEST_ENV_NAME,
                format!("{at} env names must be variable names of [A-Z_][A-Z0-9_]* (got {name:?})"),
            ));
        }
        if looks_secret(name, value) {
            warnings.push(warn(
                WARN_SANDBOX_ENV_SECRET_SHAPED,
                format!(
                    "{at} env {name} looks like a credential. This file is tracked in git; env \
                     here is for literals, and a value the host owns belongs in env_from_host."
                ),
            ));
        }
    }
    Ok(())
}

/// Where a [`ReclaimPlan`] came from, named so a reader can tell the two apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReclaimSource {
    /// Derived from the project's own `sandbox.yml`.
    Manifest {
        /// The digest of the manifest that produced it.
        sha256: String,
    },
    /// No manifest: [`crate::worktree_lifecycle::RECLAIMABLE_BUILD_DIRS`].
    ClosedListFallback,
}

/// What reclaim may do to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReclaimAction {
    /// Remove the directory.
    DeleteDirectory,
    /// Remove the link only, never what it points at. Frees nothing, and is
    /// reported as freeing nothing.
    UnlinkOnly,
    /// Not reclaim's business.
    Keep,
}

/// One path reclaim knows about, and what it may do to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimTarget {
    /// Checkout-relative path.
    pub path: String,
    /// What reclaim may do.
    pub action: ReclaimAction,
    /// The kind that declared it, when a manifest did.
    pub declared: Option<SeedKind>,
    /// True for a clone: the blocks are shared with the source, so a size
    /// measured here is an upper bound on what removing it would release.
    pub bytes_are_shared: bool,
}

/// Every path reclaim may touch for one project, and what it may do to each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimPlan {
    /// Where this plan came from.
    pub source: ReclaimSource,
    /// The paths, in manifest order.
    pub targets: Vec<ReclaimTarget>,
}

impl ReclaimPlan {
    /// The targets reclaim would act on, skipping the ones it must keep.
    #[must_use]
    pub fn actionable(&self) -> Vec<&ReclaimTarget> {
        self.targets
            .iter()
            .filter(|target| !matches!(target.action, ReclaimAction::Keep))
            .collect()
    }

    /// What reclaim may do to one checkout-relative path.
    ///
    /// A path no plan names is [`ReclaimAction::Keep`]: anything not declared
    /// is source until someone proves otherwise.
    #[must_use]
    pub fn action_for(&self, path: &str) -> ReclaimAction {
        self.targets
            .iter()
            .find(|target| target.path == path)
            .map_or(ReclaimAction::Keep, |target| target.action)
    }
}

/// Every path reclaim may touch, and what it may do to it.
///
/// With no manifest this is [`crate::worktree_lifecycle::RECLAIMABLE_BUILD_DIRS`],
/// named as the fallback so nobody reads it as the project's own answer. With
/// one, it is exactly what the project declared — and a `symlink` is unlinked
/// while a `share` is left alone, because deleting through either would take
/// something that is not this sandbox's to take.
#[must_use]
pub fn reclaim_plan(plan: Option<&SandboxPlan>) -> ReclaimPlan {
    let Some(plan) = plan else {
        return ReclaimPlan {
            source: ReclaimSource::ClosedListFallback,
            targets: crate::worktree_lifecycle::RECLAIMABLE_BUILD_DIRS
                .iter()
                .map(|path| ReclaimTarget {
                    path: (*path).to_owned(),
                    action: ReclaimAction::DeleteDirectory,
                    declared: None,
                    bytes_are_shared: false,
                })
                .collect(),
        };
    };
    let mut targets = Vec::new();
    for entry in &plan.entries {
        match entry {
            SandboxEntry::Clone(clone) => targets.push(ReclaimTarget {
                path: clone.path.trim().to_owned(),
                action: rule_action(clone.reclaim),
                declared: Some(SeedKind::Clone),
                bytes_are_shared: true,
            }),
            SandboxEntry::Copy(copy) => targets.push(ReclaimTarget {
                path: copy.path.trim().to_owned(),
                action: rule_action(copy.reclaim),
                declared: Some(SeedKind::Copy),
                bytes_are_shared: false,
            }),
            SandboxEntry::Symlink(link) => targets.push(ReclaimTarget {
                path: link.path.trim().to_owned(),
                action: ReclaimAction::UnlinkOnly,
                declared: Some(SeedKind::Symlink),
                bytes_are_shared: false,
            }),
            SandboxEntry::Share(share) => targets.push(ReclaimTarget {
                path: share.path.trim().to_owned(),
                action: ReclaimAction::Keep,
                declared: Some(SeedKind::Share),
                bytes_are_shared: false,
            }),
            SandboxEntry::Run(run) => {
                for produced in &run.produces {
                    targets.push(ReclaimTarget {
                        path: produced.trim().to_owned(),
                        action: rule_action(run.reclaim),
                        declared: Some(SeedKind::Run),
                        bytes_are_shared: false,
                    });
                }
            }
        }
    }
    ReclaimPlan {
        source: ReclaimSource::Manifest {
            sha256: plan.manifest_sha256.clone(),
        },
        targets,
    }
}

const fn rule_action(rule: ReclaimRule) -> ReclaimAction {
    match rule {
        ReclaimRule::Delete => ReclaimAction::DeleteDirectory,
        ReclaimRule::Never => ReclaimAction::Keep,
    }
}

/// Where the manifest sits in a checkout.
#[must_use]
pub fn sandbox_manifest_path(root: &Path) -> PathBuf {
    root.join(SANDBOX_YML)
}

/// Read `<root>/sandbox.yml` when it exists, with the read injected.
///
/// `read` answers `Ok(None)` for a checkout that has no manifest, and
/// `Ok(Some((len, text)))` for one that does. `Ok(None)` is not an error: a
/// project that declares nothing is seeded with nothing and reclaimed by the
/// fallback list. A manifest that is *present and wrong*, though, is refused
/// rather than skipped — the same rule `team.yml` follows, because silently
/// ignoring a file someone wrote is how a control comes to do nothing.
///
/// # Errors
///
/// [`SANDBOX_MANIFEST_TOO_LARGE`] above [`MAX_SANDBOX_YML_BYTES`],
/// [`SANDBOX_MANIFEST_UNREADABLE`] when `read` fails, or any refusal from
/// [`parse_sandbox_yml`].
pub fn load_sandbox_manifest_with(
    root: &Path,
    read: impl FnOnce(&Path) -> std::io::Result<Option<(u64, String)>>,
) -> Result<Option<SandboxPlan>, SandboxRefusal> {
    let path = sandbox_manifest_path(root);
    match read(&path) {
        Err(error) => Err(refuse(
            SANDBOX_MANIFEST_UNREADABLE,
            format!("{} could not be read: {error}", path.display()),
        )),
        Ok(None) => Ok(None),
        Ok(Some((len, _))) if len > MAX_SANDBOX_YML_BYTES => Err(refuse(
            SANDBOX_MANIFEST_TOO_LARGE,
            format!(
                "{} is {len} bytes; at most {MAX_SANDBOX_YML_BYTES} are read",
                path.display()
            ),
        )),
        Ok(Some((_, text))) => parse_sandbox_yml(&text).map(Some),
    }
}

/// [`load_sandbox_manifest_with`] over `std::fs`.
///
/// # Errors
///
/// As [`load_sandbox_manifest_with`].
pub fn load_sandbox_manifest(root: &Path) -> Result<Option<SandboxPlan>, SandboxRefusal> {
    load_sandbox_manifest_with(root, |path| match std::fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(metadata) => {
            // The size is checked before the read, which is the point of a
            // ceiling. A directory or a broken link falls out of the read as
            // unreadable rather than as absent.
            let text = std::fs::read_to_string(path)?;
            Ok(Some((metadata.len(), text)))
        }
    })
}

/// The reclaim plan for a checkout: its own `sandbox.yml` when it has one,
/// and the built-in fallback when it does not.
///
/// A manifest that is present and *wrong* falls back too, and hands back the
/// refusal alongside, because the two failures are not the same: refusing to
/// free anything until a YAML typo is fixed would be a worse answer than
/// freeing what the old closed list always freed and saying loudly why the
/// declaration was not used.
///
/// # Panics
///
/// Never.
#[must_use]
pub fn reclaim_plan_for(checkout: &Path) -> (ReclaimPlan, Option<SandboxRefusal>) {
    match load_sandbox_manifest(checkout) {
        Ok(Some(plan)) => (reclaim_plan(Some(&plan)), None),
        Ok(None) => (reclaim_plan(None), None),
        Err(refusal) => (reclaim_plan(None), Some(refusal)),
    }
}

#[cfg(test)]
#[path = "sandbox_manifest_tests.rs"]
mod tests;
