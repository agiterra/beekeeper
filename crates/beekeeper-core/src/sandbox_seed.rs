//! Carrying out a [`crate::sandbox_manifest`] declaration, and the inverse
//! verb that frees what it seeded.
//!
//! The decisions live in [`preflight`], which touches nothing and answers what
//! every entry would do. [`apply`] then does the filesystem work through an
//! injected [`SeedOps`], so the whole engine is testable without a disk —
//! `buzz-core` carries no dev-dependencies and there is no `tempfile` here.
//!
//! # `run` entries are not executed here
//!
//! [`apply`] returns them in [`SeedReceipt::pending_runs`] for the caller to
//! execute with whatever boundary it holds. A seat's host runs them inside the
//! new tree's prepared sandbox; a person running `bee` on their own machine
//! runs them unconfined and the receipt says so. One engine, and the
//! disclosure tells the two apart instead of the engine guessing.
//!
//! # Four invariants
//!
//! **S1 — never a symlink at a root git would not ignore.** A `.gitignore`
//! entry written `node_modules/` is a *directory* pattern, and matching is
//! stat-dependent: it hides a directory and not a symlink. A tree seeded with
//! a symlink there has a `git status --porcelain` that is never empty, which
//! is what a seat's push gate reads. So the seeder creates every intermediate
//! parent as a real directory, and refuses an entry whose produced shape git
//! would not ignore, naming `link: entries` as the expression that works. It
//! does not quietly write to `info/exclude`: that file is shared by the main
//! checkout and every linked worktree, so one sandbox's convenience would
//! become everyone's diff.
//!
//! **S2 — delete nothing this engine did not write.** A destination that is
//! already a non-empty real directory is a [`SeedDisposition::Conflict`] and is
//! left exactly as it is. The only thing ever removed is a link this engine's
//! own receipt records placing.
//!
//! **S3 — never read a directory a build is writing.** A copy-on-write clone
//! is atomic per file and not per tree, and cargo's fingerprints are precisely
//! what a torn copy corrupts. Where a build tool's lock is held, the entry is
//! skipped and says why.
//!
//! **S4 — the source is the host's to name.** [`SeedInputs::source`] comes
//! from the caller, never from the manifest, and every resolved path is
//! checked to be inside the checkout it belongs to. A `sandbox.yml` is a
//! tracked file an agent may edit.
//!
//! # Nothing here returns `Err`
//!
//! [`apply`] returns a receipt. A refused, skipped, partial or failed entry is
//! disclosed by name and [`SeedReceipt::complete`] goes false; it never fails
//! the caller. An unseeded sandbox is *cold*, which is slow; a sandbox that is
//! not the commit it claims is *wrong*. Letting a manifest fail a hire would
//! also hand whoever can edit it a lever to fail every hire.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::sandbox_manifest::{
    LinkShape, ReclaimAction, ReclaimPlan, SandboxEntry, SandboxPlan, SeedKind, SharedLock,
    UnsupportedFallback,
};

/// No source checkout was resolved, so there is nothing to seed from.
pub const SANDBOX_SEED_SOURCE_NOT_RECORDED: &str = "SANDBOX_SEED_SOURCE_NOT_RECORDED";
/// A destination that resolves outside the new tree.
pub const SANDBOX_SEED_DEST_OUTSIDE: &str = "SANDBOX_SEED_DEST_OUTSIDE";
/// A source that resolves outside the source checkout.
pub const SANDBOX_SEED_SOURCE_OUTSIDE: &str = "SANDBOX_SEED_SOURCE_OUTSIDE";
/// Seeding over a path git tracks.
pub const SANDBOX_SEED_PATH_TRACKED: &str = "SANDBOX_SEED_PATH_TRACKED";
/// A destination git would not ignore in the shape this entry produces.
pub const SANDBOX_SEED_NOT_IGNORED: &str = "SANDBOX_SEED_NOT_IGNORED";
/// A clone on a filesystem that cannot clone, where the entry refuses a copy.
pub const SANDBOX_SEED_CLONE_UNSUPPORTED: &str = "SANDBOX_SEED_CLONE_UNSUPPORTED";
/// A `share` pool that resolves outside the host's pool root.
pub const SANDBOX_SEED_SHARE_POOL_OUTSIDE: &str = "SANDBOX_SEED_SHARE_POOL_OUTSIDE";
/// A `share` entry with no project scope to key its pool on.
pub const SANDBOX_SEED_SHARE_NO_SCOPE: &str = "SANDBOX_SEED_SHARE_NO_SCOPE";
/// The source path is not there.
pub const SANDBOX_SEED_SOURCE_ABSENT: &str = "SANDBOX_SEED_SOURCE_ABSENT";
/// A destination that is already something this engine did not write.
pub const SANDBOX_SEED_DEST_OCCUPIED: &str = "SANDBOX_SEED_DEST_OCCUPIED";
/// A build tool holds the source directory's lock.
pub const SANDBOX_SEED_DONOR_BUSY: &str = "SANDBOX_SEED_DONOR_BUSY";
/// A rewrite that matched nothing, so the source's absolute paths would stay.
pub const SANDBOX_SEED_REWRITE_NO_MATCH: &str = "SANDBOX_SEED_REWRITE_NO_MATCH";
/// A rewrite naming a file the source does not have.
pub const SANDBOX_SEED_REWRITE_ABSENT: &str = "SANDBOX_SEED_REWRITE_ABSENT";
/// The filesystem work failed.
pub const SANDBOX_SEED_FAILED: &str = "SANDBOX_SEED_FAILED";
/// No manifest: nothing was seeded, and reclaim uses the fallback list.
pub const SANDBOX_SEED_MANIFEST_ABSENT: &str = "SANDBOX_SEED_MANIFEST_ABSENT";

/// Build-tool locks held while a directory of a given shape is being written.
///
/// A closed list, keyed on the destination's trailing components, with paths
/// relative to the source directory. Measured on this repository: cargo takes
/// a build lock per profile under a target directory, and a `CARGO_HOME` has
/// the registry's package-cache lock at its root.
const DONOR_LOCKS: &[(&[&str], &[&str])] = &[
    (&["target"], &["debug/.cargo-lock", "release/.cargo-lock"]),
    (&[".hermit", "rust"], &[".package-cache"]),
];

/// What a path is, without following links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A regular file.
    File,
    /// A real directory.
    Directory,
    /// A symbolic link, whatever it points at.
    Symlink,
}

/// Everything the seeder needs from the world, injected so the decisions can
/// be tested without one.
///
/// Every method takes absolute paths. Nothing here follows a symlink unless
/// its name says it does: [`SeedOps::node_kind`] is an `lstat`, because an
/// engine that confused a link for the directory it points at would delete
/// the source's copy.
pub trait SeedOps {
    /// What is at `path`, without following a link. `None` when nothing is.
    fn node_kind(&self, path: &Path) -> Option<NodeKind>;
    /// Where a link points, verbatim.
    fn link_target(&self, path: &Path) -> Option<PathBuf>;
    /// The fully resolved path, following links. `None` when it does not exist.
    fn real_path(&self, path: &Path) -> Option<PathBuf>;
    /// The names directly inside a directory.
    fn children(&self, path: &Path) -> std::io::Result<Vec<String>>;
    /// Allocated size of a tree, or `None` when it could not be measured.
    /// Unknown is not zero.
    fn tree_bytes(&self, path: &Path) -> Option<u64>;
    /// Whether a copy-on-write clone is possible from `from` to `to`.
    fn clone_supported(&self, from: &Path, to: &Path) -> bool;
    /// Whether git tracks anything at or under a checkout-relative path.
    fn tracked(&self, root: &Path, relative: &str) -> bool;
    /// Whether git would ignore a checkout-relative path, were it `kind`.
    fn ignored(&self, root: &Path, relative: &str, kind: NodeKind) -> bool;
    /// Whether a lock file is held right now. A lock that cannot be read is
    /// reported as held: refusing to read a directory that might be mid-build
    /// costs a cold build, and guessing wrong corrupts one.
    fn lock_held(&self, path: &Path) -> bool;

    /// Create a directory and every missing parent, as real directories.
    fn make_dir(&self, path: &Path) -> std::io::Result<()>;
    /// Copy-on-write clone a tree. `to` must not exist.
    fn clone_tree(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// Copy a tree byte for byte. `to` must not exist.
    fn copy_tree(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// Create a symlink at `at` pointing to `target`.
    fn link(&self, target: &Path, at: &Path) -> std::io::Result<()>;
    /// Remove a symlink or a file. Never follows the link.
    fn unlink(&self, path: &Path) -> std::io::Result<()>;
    /// Remove a real directory and its contents.
    fn remove_tree(&self, path: &Path) -> std::io::Result<()>;
    /// Move a path within one filesystem.
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// Read a file as text.
    fn read_text(&self, path: &Path) -> std::io::Result<String>;
    /// Write a file as text, replacing it.
    fn write_text(&self, path: &Path, text: &str) -> std::io::Result<()>;
}

/// The two checkouts and the pool root, as the host resolved them.
pub struct SeedInputs<'a> {
    /// The source checkout every entry reads from. The host's to name: a
    /// manifest cannot say where it is.
    pub source: Option<&'a Path>,
    /// The new tree. Every destination resolves inside it.
    pub dest: &'a Path,
    /// Root for this project's `share` pools, inside a directory the project's
    /// executions already have. `None` when the caller has no project scope,
    /// which downgrades every `share` to a clone rather than quietly making a
    /// per-tree directory and calling it shared.
    pub pool_root: Option<&'a Path>,
}

/// What was actually used, as opposed to what was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SeedMechanism {
    /// A copy-on-write clone.
    Clonefile,
    /// A real copy.
    Copy,
    /// A link into the source checkout.
    Symlink,
    /// A link into a shared pool.
    Share,
    /// The project's own recipe or script.
    Recipe,
}

impl SeedMechanism {
    /// How this reads in a receipt.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clonefile => "clone",
            Self::Copy => "copy",
            Self::Symlink => "symlink",
            Self::Share => "share",
            Self::Recipe => "recipe",
        }
    }
}

/// What an entry's destination costs.
///
/// `logical` is what a size measurement reports. `exclusive` is what removing
/// it would actually release, which for a clone is **unknown**: the blocks are
/// shared with the source until one side writes. Unknown is not zero, and it
/// is not the logical size either — reporting a cloned 36 GB directory as 36 GB
/// of reclaimable disk would be a number nobody can act on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedBytes {
    /// Allocated size, as measured.
    pub logical: Option<u64>,
    /// What freeing it would release, when that is knowable.
    pub exclusive: Option<u64>,
}

/// How one entry ended up.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SeedDisposition {
    /// It landed.
    Seeded,
    /// It was already there, in the shape this entry asks for.
    AlreadyPresent,
    /// It landed by a weaker mechanism than it declared, which is named.
    Downgraded {
        /// Why.
        code: &'static str,
        /// What was used instead.
        to: SeedKind,
    },
    /// Not done because nothing needed doing: the entry declares
    /// `seed: never`, or an optional source is simply not there. By design,
    /// so it does not make a seed incomplete.
    Skipped {
        /// Why.
        code: &'static str,
    },
    /// Wanted, and could not be done *now* — a build is writing the source
    /// directory, say. Nothing is wrong, but the sandbox is colder than it
    /// asked to be, so the seed is not complete and the reason is named.
    Unavailable {
        /// Why.
        code: &'static str,
    },
    /// Not done because doing it would be wrong. Nothing was touched.
    Refused {
        /// Why.
        code: &'static str,
    },
    /// Attempted and did not work. Anything partly written was removed.
    Failed {
        /// Why.
        code: &'static str,
    },
}

impl SeedDisposition {
    /// Whether this entry leaves the sandbox as warm as it asked to be.
    #[must_use]
    pub const fn is_satisfied(&self) -> bool {
        matches!(
            self,
            Self::Seeded | Self::AlreadyPresent | Self::Downgraded { .. } | Self::Skipped { .. }
        )
    }

    /// The code, when there is one.
    #[must_use]
    pub const fn code(&self) -> Option<&'static str> {
        match self {
            Self::Seeded | Self::AlreadyPresent => None,
            Self::Downgraded { code, .. }
            | Self::Skipped { code }
            | Self::Unavailable { code }
            | Self::Refused { code }
            | Self::Failed { code } => Some(code),
        }
    }
}

/// One entry's line in the receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedOutcome {
    /// The entry's own name: its destination, or the recipe a `run` names.
    pub id: String,
    /// What the manifest declared.
    pub declared: SeedKind,
    /// What was used. `None` when nothing was done.
    pub used: Option<SeedMechanism>,
    /// Source, relative to the source checkout.
    pub source: Option<String>,
    /// Destination, relative to the new tree.
    pub destination: Option<String>,
    /// How it ended up.
    pub disposition: SeedDisposition,
    /// What it costs.
    pub bytes: SeedBytes,
    /// A sentence a person can act on.
    pub detail: String,
    /// Non-fatal notes: a clone that fell back to a copy, a rewritten state
    /// file, a shared pool's declared lock.
    pub notes: Vec<String>,
}

/// A `run` entry the caller must execute, with the boundary it holds.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRun {
    /// The entry's name.
    pub id: String,
    /// A `just` recipe in the project's own `Justfile`.
    pub recipe: Option<String>,
    /// A script under the project's scripts directory.
    pub script: Option<String>,
    /// What it writes.
    pub produces: Vec<String>,
    /// Literal environment.
    pub env: BTreeMap<String, String>,
    /// Names the host must supply.
    pub env_from_host: Vec<String>,
    /// Wall-clock ceiling.
    pub timeout_secs: Option<u64>,
}

/// How a receipt's `run` entries were confined.
///
/// Two arms that are never collapsed, so a not-enforced seed cannot read as an
/// enforced one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum SeedBoundary {
    /// Confined, by the named backend.
    Enforced {
        /// What enforced it.
        backend: String,
    },
    /// Not confined, for the named reason.
    NotEnforced {
        /// Why not.
        reason: String,
    },
}

/// What a seed did, start to finish.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeedReceipt {
    /// Which manifest ran. `None` when the project ships none.
    pub manifest_sha256: Option<String>,
    /// The source checkout that was read.
    pub source: Option<PathBuf>,
    /// The tree that was seeded.
    pub tree: PathBuf,
    /// The pool namespace, when the manifest declares one.
    pub pool: Option<String>,
    /// One line per entry, in the order they were carried out.
    pub outcomes: Vec<SeedOutcome>,
    /// `run` entries for the caller to execute.
    pub pending_runs: Vec<PendingRun>,
    /// How those runs were confined. `None` until the caller has run them and
    /// said: an empty value is not a claim that they were bounded.
    pub boundary: Option<SeedBoundary>,
    /// The union of the environment the seeded entries declare, for the
    /// caller to put on the executions that follow.
    pub env: BTreeMap<String, String>,
    /// How many of [`SeedReceipt::pending_runs`] the caller has reported back.
    /// A receipt with runs still outstanding is not complete, because nobody
    /// has said whether they worked.
    #[serde(skip)]
    pending_runs_recorded: usize,
    /// False when any entry did not leave the sandbox as warm as it asked, or
    /// a `run` entry has not been reported back. Computed, never asserted.
    pub complete: bool,
}

impl SeedReceipt {
    /// Recompute [`SeedReceipt::complete`] from the outcomes.
    fn settle(mut self) -> Self {
        self.complete = self
            .outcomes
            .iter()
            .all(|outcome| outcome.disposition.is_satisfied())
            && self.pending_runs_recorded >= self.pending_runs.len();
        self
    }

    /// Report back what happened to one `run` entry the caller executed.
    ///
    /// Until every pending run is reported, [`SeedReceipt::complete`] stays
    /// false: a receipt must not read as finished while nobody has said
    /// whether the project's own setup recipes worked.
    pub fn record_run(
        &mut self,
        run: &PendingRun,
        disposition: SeedDisposition,
        detail: impl Into<String>,
    ) {
        self.outcomes.push(SeedOutcome {
            id: run.id.clone(),
            declared: SeedKind::Run,
            used: Some(SeedMechanism::Recipe),
            source: None,
            destination: run.produces.first().cloned(),
            disposition,
            bytes: SeedBytes::default(),
            detail: detail.into(),
            notes: Vec::new(),
        });
        self.pending_runs_recorded = self.pending_runs_recorded.saturating_add(1);
        let settled = std::mem::replace(self, Self::empty()).settle();
        *self = settled;
    }

    /// Say how the `run` entries were confined.
    ///
    /// There is no default: a receipt that was never told reads `None`, which
    /// is not a claim that anything was bounded.
    pub fn set_boundary(&mut self, boundary: SeedBoundary) {
        self.boundary = Some(boundary);
    }

    /// A receipt that describes nothing, for moving one out of a `&mut`.
    fn empty() -> Self {
        Self {
            manifest_sha256: None,
            source: None,
            tree: PathBuf::new(),
            pool: None,
            outcomes: Vec::new(),
            pending_runs: Vec::new(),
            boundary: None,
            env: BTreeMap::new(),
            pending_runs_recorded: 0,
            complete: false,
        }
    }

    /// The entries that did not land, for a caller that wants to say so.
    #[must_use]
    pub fn unsatisfied(&self) -> Vec<&SeedOutcome> {
        self.outcomes
            .iter()
            .filter(|outcome| !outcome.disposition.is_satisfied())
            .collect()
    }
}

/// One resolved filesystem step, or an outcome already decided.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Carry this out.
    Act(Box<Action>),
    /// Nothing to carry out; report this.
    Settled(SeedOutcome),
}

/// A resolved filesystem action: absolute paths, mechanism already chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Action {
    id: String,
    declared: SeedKind,
    mechanism: SeedMechanism,
    /// Absolute source to read: a directory in the source checkout, or the
    /// pool for a `share` that is already populated.
    from: PathBuf,
    /// Absolute destination inside the new tree.
    to: PathBuf,
    /// Relative spellings, for the receipt.
    relative_from: String,
    relative_to: String,
    /// For a `share`: the pool to fill, and the source to fill it from.
    pool: Option<PathBuf>,
    link: LinkShape,
    rewrite: Vec<String>,
    materialize: Vec<String>,
    /// Lock files that mean the source is mid-write.
    locks: Vec<PathBuf>,
    notes: Vec<String>,
}

/// Everything [`apply`] will do, decided and touching nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedProgram {
    steps: Vec<Step>,
    pending_runs: Vec<PendingRun>,
    source: Option<PathBuf>,
    tree: PathBuf,
    pool: Option<String>,
    manifest_sha256: Option<String>,
    env: BTreeMap<String, String>,
}

impl SeedProgram {
    /// What each entry would do, without doing any of it.
    ///
    /// The same lines [`apply`] would produce, with every step that is already
    /// decided — a refusal, a skip, an already-present destination — carrying
    /// its final outcome. This is what `--dry-run` prints.
    #[must_use]
    pub fn preview(&self) -> Vec<SeedOutcome> {
        self.steps
            .iter()
            .map(|step| match step {
                Step::Settled(outcome) => outcome.clone(),
                Step::Act(action) => SeedOutcome {
                    id: action.id.clone(),
                    declared: action.declared,
                    used: Some(action.mechanism),
                    source: Some(action.relative_from.clone()),
                    destination: Some(action.relative_to.clone()),
                    disposition: SeedDisposition::Seeded,
                    bytes: SeedBytes::default(),
                    detail: format!(
                        "would {} {} into {}",
                        action.mechanism.as_str(),
                        action.relative_from,
                        action.relative_to
                    ),
                    notes: action.notes.clone(),
                },
            })
            .collect()
    }

    /// The `run` entries this program would hand back.
    #[must_use]
    pub fn pending_runs(&self) -> &[PendingRun] {
        &self.pending_runs
    }
}

/// Resolve every entry against the two checkouts, without touching anything.
///
/// Each entry comes back either as work to carry out or as an outcome already
/// decided — refused, skipped, or already present. Nothing here writes, so
/// this is also what `--dry-run` reports.
#[must_use]
pub fn preflight(
    plan: Option<&SandboxPlan>,
    inputs: &SeedInputs<'_>,
    ops: &dyn SeedOps,
) -> SeedProgram {
    let tree = inputs.dest.to_path_buf();
    let Some(plan) = plan else {
        return SeedProgram {
            steps: vec![Step::Settled(SeedOutcome {
                id: "sandbox.yml".to_owned(),
                declared: SeedKind::Copy,
                used: None,
                source: None,
                destination: None,
                disposition: SeedDisposition::Skipped {
                    code: SANDBOX_SEED_MANIFEST_ABSENT,
                },
                bytes: SeedBytes::default(),
                detail: "this project declares no sandbox.yml, so nothing was seeded; reclaim \
                         uses the built-in fallback list"
                    .to_owned(),
                notes: Vec::new(),
            })],
            pending_runs: Vec::new(),
            source: inputs.source.map(Path::to_path_buf),
            tree,
            pool: None,
            manifest_sha256: None,
            env: BTreeMap::new(),
        };
    };
    let tree_real = ops.real_path(&tree);
    let source_real = inputs.source.and_then(|source| ops.real_path(source));
    let mut shares = Vec::new();
    let mut copies = Vec::new();
    let mut links = Vec::new();
    let mut pending_runs = Vec::new();
    let mut env = BTreeMap::new();

    for entry in &plan.entries {
        if let SandboxEntry::Run(run) = entry {
            for (name, value) in &run.env {
                env.insert(name.clone(), value.clone());
            }
            pending_runs.push(PendingRun {
                id: entry.id(),
                recipe: run.recipe.as_ref().map(|name| name.trim().to_owned()),
                script: run.script.as_ref().map(|name| name.trim().to_owned()),
                produces: run.produces.iter().map(|p| p.trim().to_owned()).collect(),
                env: run.env.clone(),
                env_from_host: run.env_from_host.clone(),
                timeout_secs: run.timeout_secs,
            });
            continue;
        }
        let step = resolve_entry(
            entry,
            inputs,
            ops,
            tree_real.as_deref(),
            source_real.as_deref(),
        );
        if matches!(step, Step::Act(_)) || seeded_entry_contributes_env(entry) {
            for (name, value) in entry.env() {
                env.insert(name.clone(), value.clone());
            }
        }
        match entry.kind() {
            SeedKind::Share => shares.push(step),
            SeedKind::Clone | SeedKind::Copy => copies.push(step),
            SeedKind::Symlink => links.push(step),
            SeedKind::Run => unreachable!("handled above"),
        }
    }

    // Order is load-bearing. Pools are filled and linked first, then the
    // copies, then the links into the source, and `run` entries last of all —
    // a recipe writes into directories a later copy would overwrite.
    let mut steps = shares;
    steps.extend(copies);
    steps.extend(links);

    SeedProgram {
        steps,
        pending_runs,
        source: inputs.source.map(Path::to_path_buf),
        tree,
        pool: plan.pool.clone(),
        manifest_sha256: Some(plan.manifest_sha256.clone()),
        env,
    }
}

/// An entry whose environment the sandbox needs even where the entry itself
/// was already satisfied: the declared variable describes how the directory
/// must be *used*, not how it was put there.
fn seeded_entry_contributes_env(entry: &SandboxEntry) -> bool {
    entry.seeds()
}

fn resolve_entry(
    entry: &SandboxEntry,
    inputs: &SeedInputs<'_>,
    ops: &dyn SeedOps,
    tree_real: Option<&Path>,
    source_real: Option<&Path>,
) -> Step {
    let id = entry.id();
    let declared = entry.kind();
    let relative_to = entry
        .destination()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let settled = |disposition: SeedDisposition, detail: String| {
        Step::Settled(SeedOutcome {
            id: id.clone(),
            declared,
            used: None,
            source: None,
            destination: Some(relative_to.clone()),
            disposition,
            bytes: SeedBytes::default(),
            detail,
            notes: Vec::new(),
        })
    };

    if !entry.seeds() {
        return settled(
            SeedDisposition::Skipped { code: "seed-never" },
            format!("{relative_to} is declared build state for reclaim, and is not seeded"),
        );
    }

    let to = inputs.dest.join(&relative_to);

    // The destination's *parent* is what must be inside the tree. The
    // destination itself may legitimately be a link that points out of it —
    // that is what `symlink` and `share` are — so resolving it here would
    // refuse every re-seed of a tree this engine had already linked.
    let parent_confined = match (tree_real, to.parent()) {
        (Some(root), Some(parent)) => confined(ops, root, parent),
        _ => false,
    };
    if !parent_confined {
        return settled(
            SeedDisposition::Refused {
                code: SANDBOX_SEED_DEST_OUTSIDE,
            },
            format!(
                "{relative_to} resolves outside the sandbox; nothing was written. A destination \
                 is always inside the tree being seeded."
            ),
        );
    }

    if ops.tracked(inputs.dest, &relative_to) {
        return settled(
            SeedDisposition::Refused {
                code: SANDBOX_SEED_PATH_TRACKED,
            },
            format!(
                "git tracks files at or under {relative_to}, so it is source, not build state; \
                 nothing was written"
            ),
        );
    }

    let link_shape = match entry {
        SandboxEntry::Symlink(link) => link.link,
        SandboxEntry::Share(share) => share.link,
        _ => LinkShape::Entries,
    };
    // A `share` with no pool root to key on is carried out as a clone, so it
    // produces a directory and must be judged as one. Refusing it for an
    // ignore rule that would not apply to what it actually does would be a
    // refusal about something that was never going to happen.
    let Some(source) = inputs.source else {
        return settled(
            SeedDisposition::Refused {
                code: SANDBOX_SEED_SOURCE_NOT_RECORDED,
            },
            format!("no source checkout was recorded, so {relative_to} has nothing to seed from"),
        );
    };

    let from_source = source.join(&relative_to);
    let source_exists = ops.node_kind(&from_source).is_some();
    if !source_exists {
        let detail = format!(
            "{relative_to} is not in the source checkout, so there was nothing to seed from"
        );
        return if entry.required() {
            settled(
                SeedDisposition::Refused {
                    code: SANDBOX_SEED_SOURCE_ABSENT,
                },
                detail,
            )
        } else {
            settled(
                SeedDisposition::Skipped {
                    code: SANDBOX_SEED_SOURCE_ABSENT,
                },
                detail,
            )
        };
    }
    if source_real.is_none_or(|root| !confined(ops, root, &from_source)) {
        return settled(
            SeedDisposition::Refused {
                code: SANDBOX_SEED_SOURCE_OUTSIDE,
            },
            format!(
                "{relative_to} in the source checkout resolves outside it, so reading it would \
                 leave the project's own files"
            ),
        );
    }

    // A `share` with no pool root to key on is carried out as a clone, so it
    // produces a directory and must be judged as one. Refusing it for an
    // ignore rule that would not apply to what it actually does would be a
    // refusal about something that was never going to happen.
    let will_downgrade_to_clone = declared == SeedKind::Share && inputs.pool_root.is_none();
    let places_a_link = matches!(declared, SeedKind::Symlink | SeedKind::Share)
        && matches!(link_shape, LinkShape::Zelf)
        && !will_downgrade_to_clone;
    // For anything that is not a link, the shape produced is whatever the
    // source is: a `.env` entry makes a file and a `target` entry makes a
    // directory, and git answers differently for the two, because an ignore
    // rule written with a trailing slash hides only a directory. This is why
    // the source has to be resolved before the question can be asked.
    let produced_kind = if places_a_link {
        NodeKind::Symlink
    } else {
        ops.node_kind(&from_source).unwrap_or(NodeKind::Directory)
    };
    if !ops.ignored(inputs.dest, &relative_to, produced_kind) {
        let shape = match produced_kind {
            NodeKind::Symlink => "a symlink",
            NodeKind::Directory => "a directory",
            NodeKind::File => "a file",
        };
        let remedy = if places_a_link {
            " An ignore rule written with a trailing slash hides a directory and not a link, so \
             express this entry as `link: entries`, whose destination is a real directory."
        } else {
            ""
        };
        return settled(
            SeedDisposition::Refused {
                code: SANDBOX_SEED_NOT_IGNORED,
            },
            format!(
                "git would not ignore {relative_to} as {shape}, so the sandbox's \
                 `git status --porcelain` would never be empty and its push gate would read the \
                 tree as dirty.{remedy}"
            ),
        );
    }

    let mut notes = Vec::new();
    let (mechanism, from, pool) = match entry {
        SandboxEntry::Clone(clone) => {
            if ops.clone_supported(&from_source, &to) {
                (SeedMechanism::Clonefile, from_source.clone(), None)
            } else if matches!(clone.on_unsupported, UnsupportedFallback::Copy) {
                notes.push(
                    "this filesystem cannot clone, so the entry fell back to a real copy"
                        .to_owned(),
                );
                (SeedMechanism::Copy, from_source.clone(), None)
            } else {
                return settled(
                    SeedDisposition::Refused {
                        code: SANDBOX_SEED_CLONE_UNSUPPORTED,
                    },
                    format!(
                        "{relative_to} is declared a clone and this filesystem cannot clone. The \
                         entry refuses a real copy rather than silently making one, which for a \
                         directory this size is the difference between seconds and a disk."
                    ),
                );
            }
        }
        SandboxEntry::Copy(_) => (SeedMechanism::Copy, from_source.clone(), None),
        SandboxEntry::Symlink(_) => (SeedMechanism::Symlink, from_source.clone(), None),
        SandboxEntry::Share(share) => {
            let Some(pool_root) = inputs.pool_root else {
                notes.push(
                    "no project scope was resolved, so a shared pool would have been this one \
                     tree's own directory; the entry was carried out as a clone instead"
                        .to_owned(),
                );
                if !ops.clone_supported(&from_source, &to) {
                    return settled(
                        SeedDisposition::Refused {
                            code: SANDBOX_SEED_SHARE_NO_SCOPE,
                        },
                        format!(
                            "{relative_to} is shared, no project scope was resolved to key the \
                             pool on, and this filesystem cannot clone either"
                        ),
                    );
                }
                let mut step = Action {
                    id: id.clone(),
                    declared,
                    mechanism: SeedMechanism::Clonefile,
                    from: from_source,
                    to,
                    relative_from: relative_to.clone(),
                    relative_to,
                    pool: None,
                    link: link_shape,
                    rewrite: Vec::new(),
                    materialize: Vec::new(),
                    locks: Vec::new(),
                    notes,
                };
                step.locks = lock_candidates(&step.relative_to, &step.from);
                return Step::Act(Box::new(step));
            };
            let pool_path = pool_root.join(share.id.trim());
            if !pool_path.starts_with(pool_root) {
                return settled(
                    SeedDisposition::Refused {
                        code: SANDBOX_SEED_SHARE_POOL_OUTSIDE,
                    },
                    format!("the pool for {relative_to} resolves outside this project's pool root"),
                );
            }
            notes.push(format!(
                "pool {:?}, declared lock: {}",
                share.id.trim(),
                match share.lock {
                    SharedLock::Shared => "shared — sandboxes block each other here",
                    SharedLock::None => "none — declared lock-free by the project, not measured",
                }
            ));
            // `from` stays the source directory, because that is what fills
            // the pool the first time. `pool` is what the link points at.
            (SeedMechanism::Share, from_source.clone(), Some(pool_path))
        }
        SandboxEntry::Run(_) => unreachable!("run entries never reach here"),
    };

    // Every rewrite must have something to rewrite. A rewrite that silently
    // matched nothing is how a seeded `node_modules` keeps the source's
    // absolute paths and purges the store every sandbox resolves through.
    for name in entry.rewrite() {
        if ops.node_kind(&from_source.join(name.trim())).is_none() {
            return settled(
                SeedDisposition::Refused {
                    code: SANDBOX_SEED_REWRITE_ABSENT,
                },
                format!(
                    "{relative_to} declares a rewrite of {name:?}, which the source checkout does \
                     not have"
                ),
            );
        }
    }

    let (rewrite, materialize) = match entry {
        SandboxEntry::Clone(clone) => (clone.rewrite.clone(), Vec::new()),
        SandboxEntry::Copy(copy) => (copy.rewrite.clone(), Vec::new()),
        SandboxEntry::Symlink(link) => (link.rewrite.clone(), link.materialize.clone()),
        SandboxEntry::Share(_) | SandboxEntry::Run(_) => (Vec::new(), Vec::new()),
    };

    let mut action = Action {
        id,
        declared,
        mechanism,
        from,
        to,
        relative_from: relative_to.clone(),
        relative_to,
        pool,
        link: link_shape,
        rewrite,
        materialize,
        locks: Vec::new(),
        notes,
    };
    action.locks = lock_candidates(&action.relative_to, &from_source);
    Step::Act(Box::new(action))
}

/// Whether `candidate` is inside `root`, resolving as much of it as exists.
///
/// A path that does not exist yet is judged by its deepest existing ancestor,
/// which is the only part a symlink could have redirected.
fn confined(ops: &dyn SeedOps, root: &Path, candidate: &Path) -> bool {
    let mut probe = candidate.to_path_buf();
    loop {
        if let Some(real) = ops.real_path(&probe) {
            return real == root || real.starts_with(root);
        }
        if !probe.pop() {
            return false;
        }
    }
}

/// The build-tool locks that mean this source directory is mid-write.
fn lock_candidates(relative: &str, source_dir: &Path) -> Vec<PathBuf> {
    let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
    for (suffix, locks) in DONOR_LOCKS {
        if parts.len() >= suffix.len() && parts[parts.len() - suffix.len()..] == **suffix {
            return locks.iter().map(|lock| source_dir.join(lock)).collect();
        }
    }
    Vec::new()
}

/// Carry out a [`SeedProgram`].
///
/// Returns a receipt and never an `Err`: a refused, skipped or failed entry is
/// a line in the receipt, and [`SeedReceipt::complete`] goes false. See this
/// module's header for why an unseeded sandbox must not fail a hire.
#[must_use]
pub fn apply(program: &SeedProgram, ops: &dyn SeedOps) -> SeedReceipt {
    let mut outcomes = Vec::with_capacity(program.steps.len());
    for step in &program.steps {
        match step {
            Step::Settled(outcome) => outcomes.push(outcome.clone()),
            Step::Act(action) => outcomes.push(carry_out(action, program, ops)),
        }
    }
    SeedReceipt {
        manifest_sha256: program.manifest_sha256.clone(),
        source: program.source.clone(),
        tree: program.tree.clone(),
        pool: program.pool.clone(),
        outcomes,
        pending_runs: program.pending_runs.clone(),
        boundary: None,
        env: program.env.clone(),
        pending_runs_recorded: 0,
        complete: false,
    }
    .settle()
}

fn carry_out(action: &Action, program: &SeedProgram, ops: &dyn SeedOps) -> SeedOutcome {
    let mut notes = action.notes.clone();
    let report = |disposition: SeedDisposition,
                  used: Option<SeedMechanism>,
                  bytes: SeedBytes,
                  detail: String,
                  notes: Vec<String>| SeedOutcome {
        id: action.id.clone(),
        declared: action.declared,
        used,
        source: Some(action.relative_from.clone()),
        destination: Some(action.relative_to.clone()),
        disposition,
        bytes,
        detail,
        notes,
    };

    // S3: never read a directory a build tool is writing.
    for lock in &action.locks {
        if ops.lock_held(lock) {
            return report(
                SeedDisposition::Unavailable {
                    code: SANDBOX_SEED_DONOR_BUSY,
                },
                None,
                SeedBytes::default(),
                format!(
                    "a build is writing {} in the source checkout, so it was left alone. A \
                     copy-on-write clone is atomic per file and not per tree, and a build \
                     directory's fingerprints are exactly what a torn copy corrupts.",
                    action.relative_from
                ),
                notes,
            );
        }
    }

    // What the link, if any, will point at.
    let link_target = action.pool.clone().unwrap_or_else(|| action.from.clone());

    // S2: delete nothing this engine did not write.
    match ops.node_kind(&action.to) {
        None => {}
        Some(NodeKind::Symlink) => {
            let existing = ops.link_target(&action.to);
            if existing.as_deref() == Some(link_target.as_path()) {
                return report(
                    SeedDisposition::AlreadyPresent,
                    Some(action.mechanism),
                    link_bytes(),
                    format!(
                        "{} already links to {}",
                        action.relative_to,
                        link_target.display()
                    ),
                    notes,
                );
            }
            // A link this engine would plausibly have placed — one pointing
            // into the source checkout or into the pool root — is re-pointed.
            // Anything else is somebody's, and is left where it is.
            let ours = existing.as_deref().is_some_and(|target| {
                program
                    .source
                    .as_deref()
                    .is_some_and(|source| target.starts_with(source))
                    || action
                        .pool
                        .as_deref()
                        .and_then(Path::parent)
                        .is_some_and(|root| target.starts_with(root))
            });
            if !ours {
                return report(
                    SeedDisposition::Refused {
                        code: SANDBOX_SEED_DEST_OCCUPIED,
                    },
                    None,
                    SeedBytes::default(),
                    format!(
                        "{} is already a link to {}, which this seeder did not place; it was left \
                         exactly as it is",
                        action.relative_to,
                        existing.unwrap_or_default().display()
                    ),
                    notes,
                );
            }
            if let Err(error) = ops.unlink(&action.to) {
                return failed(&report, &action.relative_to, &error, notes);
            }
            notes.push("re-pointed a link this seeder had placed earlier".to_owned());
        }
        Some(NodeKind::Directory) => {
            let empty = ops
                .children(&action.to)
                .map(|children| children.is_empty())
                .unwrap_or(false);
            if empty {
                if let Err(error) = ops.remove_tree(&action.to) {
                    return failed(&report, &action.relative_to, &error, notes);
                }
            } else {
                let bytes = measured(ops, &action.to, action.mechanism);
                return report(
                    SeedDisposition::AlreadyPresent,
                    Some(action.mechanism),
                    bytes,
                    format!(
                        "{} already holds build state, and was left as it is; re-seeding never \
                         removes a directory this seeder did not write",
                        action.relative_to
                    ),
                    notes,
                );
            }
        }
        Some(NodeKind::File) => {
            return report(
                SeedDisposition::Refused {
                    code: SANDBOX_SEED_DEST_OCCUPIED,
                },
                None,
                SeedBytes::default(),
                format!(
                    "{} is a file, and this entry would put a directory there; nothing was touched",
                    action.relative_to
                ),
                notes,
            );
        }
    }

    // S1: every parent is a real directory, so a trailing-slash ignore rule
    // still hides what lands here.
    if let Some(parent) = action.to.parent() {
        if let Err(error) = ops.make_dir(parent) {
            return failed(&report, &action.relative_to, &error, notes);
        }
    }

    // A `share` fills its pool once, from the source, before anything links
    // into it.
    if let Some(pool) = action.pool.as_deref() {
        if ops.node_kind(pool).is_none() {
            if let Some(pool_parent) = pool.parent() {
                if let Err(error) = ops.make_dir(pool_parent) {
                    return failed(&report, &action.relative_to, &error, notes);
                }
            }
            // The pool is filled by a clone where that is possible, and by a
            // real copy where it is not — and says which, rather than
            // reporting a clone it did not do.
            let how = if ops.clone_supported(&action.from, pool) {
                SeedMechanism::Clonefile
            } else {
                SeedMechanism::Copy
            };
            if let Err(error) = stage_tree(ops, &action.from, pool, how) {
                return failed(&report, &action.relative_to, &error, notes);
            }
            notes.push(format!(
                "filled the shared pool at {} from the source checkout by {}",
                pool.display(),
                how.as_str()
            ));
        } else {
            notes.push("the shared pool was already populated".to_owned());
        }
    }

    let outcome = match action.mechanism {
        SeedMechanism::Clonefile | SeedMechanism::Copy => {
            if let Err(error) = stage_tree(ops, &action.from, &action.to, action.mechanism) {
                return failed(&report, &action.relative_to, &error, notes);
            }
            Ok(())
        }
        SeedMechanism::Symlink | SeedMechanism::Share => {
            place_link(action, &link_target, ops, &mut notes)
        }
        SeedMechanism::Recipe => unreachable!("a recipe is never carried out here"),
    };
    if let Err(error) = outcome {
        return failed(&report, &action.relative_to, &error, notes);
    }

    // Re-root the source's absolute paths, or undo the whole entry. A state
    // file still naming the source checkout is worse than no seeding at all.
    if let Err(refusal) = rewrite_state_files(action, program, ops, &mut notes) {
        let _ = undo(ops, action);
        return report(
            SeedDisposition::Failed { code: refusal.0 },
            None,
            SeedBytes::default(),
            refusal.1,
            notes,
        );
    }

    let bytes = measured(ops, &action.to, action.mechanism);
    let detail = match action.mechanism {
        SeedMechanism::Clonefile => format!(
            "cloned {} from the source checkout; the blocks are shared until one side writes",
            action.relative_to
        ),
        SeedMechanism::Copy => format!("copied {} from the source checkout", action.relative_to),
        SeedMechanism::Symlink => format!("linked {} into the source checkout", action.relative_to),
        SeedMechanism::Share => format!(
            "linked {} into this project's shared pool",
            action.relative_to
        ),
        SeedMechanism::Recipe => unreachable!(),
    };
    let disposition =
        if action.declared == SeedKind::Clone && action.mechanism == SeedMechanism::Copy {
            SeedDisposition::Downgraded {
                code: SANDBOX_SEED_CLONE_UNSUPPORTED,
                to: SeedKind::Copy,
            }
        } else if action.declared == SeedKind::Share && action.pool.is_none() {
            SeedDisposition::Downgraded {
                code: SANDBOX_SEED_SHARE_NO_SCOPE,
                to: SeedKind::Clone,
            }
        } else {
            SeedDisposition::Seeded
        };
    report(disposition, Some(action.mechanism), bytes, detail, notes)
}

fn failed(
    report: &impl Fn(
        SeedDisposition,
        Option<SeedMechanism>,
        SeedBytes,
        String,
        Vec<String>,
    ) -> SeedOutcome,
    relative: &str,
    error: &std::io::Error,
    notes: Vec<String>,
) -> SeedOutcome {
    report(
        SeedDisposition::Failed {
            code: SANDBOX_SEED_FAILED,
        },
        None,
        SeedBytes::default(),
        format!("{relative} could not be seeded: {error}"),
        notes,
    )
}

/// Build a tree beside its destination and move it into place, so a crash
/// never leaves half a build directory looking like a whole one.
fn stage_tree(
    ops: &dyn SeedOps,
    from: &Path,
    to: &Path,
    mechanism: SeedMechanism,
) -> std::io::Result<()> {
    let staging = staging_path(to);
    if ops.node_kind(&staging).is_some() {
        ops.remove_tree(&staging)?;
    }
    match mechanism {
        SeedMechanism::Copy => ops.copy_tree(from, &staging)?,
        _ => ops.clone_tree(from, &staging)?,
    }
    ops.rename(&staging, to)
}

fn staging_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".seeding-{}", std::process::id()));
    to.with_file_name(name)
}

/// Place the link, or the directory of links a `link: entries` entry asks for.
fn place_link(
    action: &Action,
    target: &Path,
    ops: &dyn SeedOps,
    notes: &mut Vec<String>,
) -> std::io::Result<()> {
    match action.link {
        LinkShape::Zelf => ops.link(target, &action.to),
        LinkShape::Entries => {
            ops.make_dir(&action.to)?;
            let mut linked = 0_usize;
            for child in ops.children(target)? {
                let destination = action.to.join(&child);
                if action.materialize.iter().any(|name| name.trim() == child)
                    || action.rewrite.iter().any(|name| name.trim() == child)
                {
                    // This child carries the source's absolute paths, so the
                    // sandbox gets its own copy to re-root rather than a link
                    // that would write back into the source checkout.
                    ops.copy_tree(&target.join(&child), &destination)?;
                    continue;
                }
                ops.link(&target.join(&child), &destination)?;
                linked += 1;
            }
            notes.push(format!(
                "{} is a real directory of {linked} links, so a trailing-slash ignore rule still \
                 hides it",
                action.relative_to
            ));
            Ok(())
        }
    }
}

/// Re-root every absolute path that names the source checkout.
fn rewrite_state_files(
    action: &Action,
    program: &SeedProgram,
    ops: &dyn SeedOps,
    notes: &mut Vec<String>,
) -> Result<(), (&'static str, String)> {
    if action.rewrite.is_empty() {
        return Ok(());
    }
    let Some(source) = program.source.as_deref() else {
        return Ok(());
    };
    let source_text = source.to_string_lossy().into_owned();
    let tree_text = program.tree.to_string_lossy().into_owned();
    for name in &action.rewrite {
        let path = action.to.join(name.trim());
        let text = ops.read_text(&path).map_err(|error| {
            (
                SANDBOX_SEED_FAILED,
                format!("{} could not be read to re-root it: {error}", name.trim()),
            )
        })?;
        if !text.contains(&source_text) {
            return Err((
                SANDBOX_SEED_REWRITE_NO_MATCH,
                format!(
                    "{} names no path inside the source checkout, so re-rooting it would be a \
                     silent no-op. That is exactly how a seeded {} keeps the source's absolute \
                     paths and makes every package-manager run purge the store this project's \
                     sandboxes share, so the entry was undone instead.",
                    name.trim(),
                    action.relative_to
                ),
            ));
        }
        let rerooted = text.replace(&source_text, &tree_text);
        ops.write_text(&path, &rerooted).map_err(|error| {
            (
                SANDBOX_SEED_FAILED,
                format!("{} could not be re-rooted: {error}", name.trim()),
            )
        })?;
        notes.push(format!(
            "re-rooted {} onto this sandbox, so the package manager does not read the install as \
             stale",
            name.trim()
        ));
    }
    Ok(())
}

/// Undo one entry's own writes, and nothing else.
fn undo(ops: &dyn SeedOps, action: &Action) -> std::io::Result<()> {
    match ops.node_kind(&action.to) {
        Some(NodeKind::Symlink) => ops.unlink(&action.to),
        Some(NodeKind::Directory) => ops.remove_tree(&action.to),
        Some(NodeKind::File) => ops.unlink(&action.to),
        None => Ok(()),
    }
}

/// A link costs nothing and frees nothing, and says so rather than reporting a
/// size it does not have.
const fn link_bytes() -> SeedBytes {
    SeedBytes {
        logical: None,
        exclusive: Some(0),
    }
}

fn measured(ops: &dyn SeedOps, path: &Path, mechanism: SeedMechanism) -> SeedBytes {
    match mechanism {
        SeedMechanism::Symlink | SeedMechanism::Share => link_bytes(),
        SeedMechanism::Clonefile => SeedBytes {
            logical: ops.tree_bytes(path),
            // Unknown, and not zero: the blocks are shared with the source
            // until one side writes, so what freeing this would release is not
            // something a size measurement can answer.
            exclusive: None,
        },
        SeedMechanism::Copy => {
            let logical = ops.tree_bytes(path);
            SeedBytes {
                logical,
                exclusive: logical,
            }
        }
        SeedMechanism::Recipe => SeedBytes::default(),
    }
}

/// Render a seeded or reclaimable size the way a person should read it.
///
/// An unknown size reads `unknown`, never `0`. A size whose blocks are shared
/// with the source reads as an upper bound, because a clone's directory
/// measures its full logical size while freeing it releases only what this
/// sandbox itself wrote.
#[must_use]
pub fn render_seed_bytes(bytes: SeedBytes) -> String {
    match (bytes.logical, bytes.exclusive) {
        (None, Some(0)) => "frees nothing".to_owned(),
        (None, _) => "unknown".to_owned(),
        (Some(logical), None) => format!(
            "up to {} — shared with the source, so the exclusive share is unknown",
            crate::worktree_lifecycle::render_reclaimable_bytes(Some(logical))
        ),
        (Some(_), Some(exclusive)) => {
            crate::worktree_lifecycle::render_reclaimable_bytes(Some(exclusive))
        }
    }
}

/// A path reclaim could not act on as declared.
pub const SANDBOX_RECLAIM_LINK_NOT_DIRECTORY: &str = "SANDBOX_RECLAIM_LINK_NOT_DIRECTORY";

/// What reclaim did to one path.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ReclaimDisposition {
    /// The directory was removed.
    Removed,
    /// A link was unlinked. What it pointed at was not touched.
    Unlinked,
    /// There was nothing there.
    Absent,
    /// Not reclaim's business.
    Kept,
    /// The tree is not the shape the declaration describes, so nothing was
    /// done.
    Refused {
        /// Why.
        code: &'static str,
    },
}

/// One path's line in a reclaim receipt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimOutcome {
    /// Checkout-relative path.
    pub path: String,
    /// What the plan said may be done.
    pub action: ReclaimAction,
    /// What was done.
    pub disposition: ReclaimDisposition,
    /// What it freed.
    pub bytes: SeedBytes,
    /// A sentence a person can act on.
    pub detail: String,
}

/// What a reclaim did, start to finish.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimReceipt {
    /// Which declaration drove it.
    pub source: crate::sandbox_manifest::ReclaimSource,
    /// One line per path.
    pub outcomes: Vec<ReclaimOutcome>,
}

impl ReclaimReceipt {
    /// Everything that was actually freed, where that is knowable.
    ///
    /// A clone's blocks are shared with the source, so its exclusive share is
    /// unknown and is left out rather than counted — which is why this returns
    /// a total *and* whether anything was unmeasurable, instead of a number
    /// that reads as the whole truth.
    #[must_use]
    pub fn freed(&self) -> (u64, bool) {
        let mut total = 0_u64;
        let mut unknown = false;
        for outcome in &self.outcomes {
            match outcome.bytes.exclusive {
                Some(bytes) => total = total.saturating_add(bytes),
                None => {
                    if matches!(outcome.disposition, ReclaimDisposition::Removed) {
                        unknown = true;
                    }
                }
            }
        }
        (total, unknown)
    }
}

/// Free what a project calls build state, and nothing else.
///
/// This is the one implementation. It replaced four copies of the same loop,
/// each of which gated on a path being a directory *with the link followed*
/// (`Path::is_dir`) and then called `remove_dir_all`.
///
/// Measured rather than assumed, because the obvious guess is wrong: on macOS
/// and this toolchain `remove_dir_all` on a symlink-to-directory returns `Ok`,
/// removes the link alone, and leaves the tree on the other side untouched. So
/// those loops were not a data-loss path. What they did get wrong is the
/// *accounting* — `du` on a link reports the link, and on a clone reports a
/// logical size whose blocks are shared — and, far more expensively, their
/// coverage: the list named `target` while a second cargo workspace grew under
/// `desktop/src-tauri/target`.
///
/// This reads the shape without following anything and branches on it
/// explicitly anyway. Not because the alternative loses data, but because
/// relying on `remove_dir_all`'s treatment of a link is relying on a detail
/// nothing in this repository pins, and because an `UnlinkOnly` that frees
/// nothing should say so instead of reporting a directory's size.
#[must_use]
pub fn reclaim(plan: &ReclaimPlan, tree: &Path, ops: &dyn SeedOps) -> ReclaimReceipt {
    let mut outcomes = Vec::with_capacity(plan.targets.len());
    for target in &plan.targets {
        let path = tree.join(&target.path);
        let kind = ops.node_kind(&path);
        let (disposition, bytes, detail) = match (target.action, kind) {
            (ReclaimAction::Keep, _) => (
                ReclaimDisposition::Kept,
                SeedBytes::default(),
                format!("{} is not reclaimed", target.path),
            ),
            (_, None) => (
                ReclaimDisposition::Absent,
                SeedBytes::default(),
                format!("{} is not there", target.path),
            ),
            // The bug this unification exists to kill: a declaration that says
            // "delete the directory" never deletes through a link.
            (ReclaimAction::DeleteDirectory, Some(NodeKind::Symlink))
            | (ReclaimAction::UnlinkOnly, Some(NodeKind::Symlink)) => match ops.unlink(&path) {
                Ok(()) => (
                    ReclaimDisposition::Unlinked,
                    SeedBytes {
                        logical: None,
                        exclusive: Some(0),
                    },
                    format!(
                        "{} was a link, so the link was removed and what it pointed at was \
                             left alone; this frees nothing",
                        target.path
                    ),
                ),
                Err(error) => (
                    ReclaimDisposition::Refused {
                        code: SANDBOX_RECLAIM_LINK_NOT_DIRECTORY,
                    },
                    SeedBytes::default(),
                    format!("{} could not be unlinked: {error}", target.path),
                ),
            },
            (ReclaimAction::UnlinkOnly, Some(_)) => (
                ReclaimDisposition::Refused {
                    code: SANDBOX_RECLAIM_LINK_NOT_DIRECTORY,
                },
                SeedBytes::default(),
                format!(
                    "{} is declared a link but is not one, so it was left alone rather than \
                     removed on a guess",
                    target.path
                ),
            ),
            (ReclaimAction::DeleteDirectory, Some(NodeKind::Directory)) => {
                let logical = ops.tree_bytes(&path);
                let exclusive = if target.bytes_are_shared {
                    None
                } else {
                    logical
                };
                match ops.remove_tree(&path) {
                    Ok(()) => (
                        ReclaimDisposition::Removed,
                        SeedBytes { logical, exclusive },
                        format!(
                            "{} was removed, freeing {}",
                            target.path,
                            render_seed_bytes(SeedBytes { logical, exclusive })
                        ),
                    ),
                    Err(error) => (
                        ReclaimDisposition::Refused {
                            code: SANDBOX_SEED_FAILED,
                        },
                        SeedBytes::default(),
                        format!("{} could not be removed: {error}", target.path),
                    ),
                }
            }
            (ReclaimAction::DeleteDirectory, Some(NodeKind::File)) => match ops.unlink(&path) {
                Ok(()) => (
                    ReclaimDisposition::Removed,
                    SeedBytes::default(),
                    format!("{} was a file, and was removed", target.path),
                ),
                Err(error) => (
                    ReclaimDisposition::Refused {
                        code: SANDBOX_SEED_FAILED,
                    },
                    SeedBytes::default(),
                    format!("{} could not be removed: {error}", target.path),
                ),
            },
        };
        outcomes.push(ReclaimOutcome {
            path: target.path.clone(),
            action: target.action,
            disposition,
            bytes,
            detail,
        });
    }
    ReclaimReceipt {
        source: plan.source.clone(),
        outcomes,
    }
}

/// How much a reclaim would free, and whether that number is the whole truth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclaimEstimate {
    /// Measured size of everything reclaim would remove, or `None` when it
    /// could not be measured.
    pub bytes: Option<u64>,
    /// True when some of that was seeded by a copy-on-write clone, so its
    /// blocks are shared with the source checkout and the measured size is an
    /// upper bound on what removing it would actually release.
    pub any_shared: bool,
}

/// Measure what a reclaim would free, without removing anything.
///
/// Counts only what the plan would actually delete: a `Keep` is not counted,
/// and neither is an `UnlinkOnly`, which frees nothing by definition.
#[must_use]
pub fn estimate_reclaimable(plan: &ReclaimPlan, tree: &Path, ops: &dyn SeedOps) -> ReclaimEstimate {
    let mut total: Option<u64> = Some(0);
    let mut any_shared = false;
    for target in &plan.targets {
        if !matches!(target.action, ReclaimAction::DeleteDirectory) {
            continue;
        }
        let path = tree.join(&target.path);
        match ops.node_kind(&path) {
            // A link frees nothing, whatever the declaration says.
            None | Some(NodeKind::Symlink) => {}
            Some(_) => {
                if target.bytes_are_shared {
                    any_shared = true;
                }
                match (total, ops.tree_bytes(&path)) {
                    (Some(running), Some(bytes)) => total = running.checked_add(bytes),
                    _ => total = None,
                }
            }
        }
    }
    ReclaimEstimate {
        bytes: total,
        any_shared,
    }
}

/// Render an estimate the way a person should read it.
///
/// An unmeasured size reads `unknown`, never `0`. A size that includes a
/// cloned directory reads as an upper bound, because its blocks are shared
/// with the source checkout until one side writes — so removing it releases
/// only what this sandbox itself wrote, which a size measurement cannot say.
#[must_use]
pub fn render_reclaim_estimate(estimate: ReclaimEstimate) -> String {
    let rendered = crate::worktree_lifecycle::render_reclaimable_bytes(estimate.bytes);
    if estimate.any_shared && estimate.bytes.is_some() {
        format!("up to {rendered} — some of it cloned, so the exclusive share is unknown")
    } else {
        rendered
    }
}

#[cfg(test)]
#[path = "sandbox_seed_tests.rs"]
mod tests;
