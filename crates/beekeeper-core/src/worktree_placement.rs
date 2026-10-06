//! Where a coding session's git worktree may live, decided from paths alone.
//!
//! One rule, four callers. Before this module the rule — "worktrees live in
//! `<repo>.worktrees/`" — was written out four separate times: the desktop
//! host's placement, its record guard, its prune guard, and the CLI's own
//! copy. The three guards decide whether a directory may be *recorded*,
//! *listed* and *removed*; placement decides where one is *cut*. When those
//! disagree the failure is silent and total: a tree lands somewhere the guards
//! refuse, so the host can never name it and will never remove it, and the
//! only signal is a line on stderr.
//!
//! So the rule is pure and lives here, where one test can pin it and all four
//! callers can share it. Nothing in this module touches the filesystem, runs
//! `git`, or reads the network — the caller gathers the facts, this decides
//! what they mean.
//!
//! # What is admissible, and why it is a closed set
//!
//! A recorded worktree hands the prune path a licence over a directory. That
//! licence must be narrow and legible, so exactly four shapes are admitted:
//!
//! - `<repo>/.worktrees/<child>` — the in-repo holder, used only where git is
//!   already ignoring it, so live trees never appear in `git status`.
//! - `<repo>.worktrees/<child>` — the shape cut before this module existed.
//!   Admitted forever and never chosen again: trees already on disk must stay
//!   recordable and removable, or the work in them becomes unmanageable.
//! - `<parent>/<stem>-wt-<tail>` — one sibling directory per worktree.
//! - anything under a folder a person named for this repository.
//!
//! Anything else is refused rather than trusted.

use std::path::{Path, PathBuf};

/// The infix marking a sibling directory as a machine-cut worktree.
const SIBLING_INFIX: &str = "-wt-";

/// The directory name of the in-repo holder.
const HOLDER_NAME: &str = ".worktrees";

/// The suffix of the pre-fix holder, kept admissible forever.
const LEGACY_HOLDER_SUFFIX: &str = ".worktrees";

/// The canonical repository folder implied by a common git dir.
///
/// Git's own derivation (`worktree.c: get_main_worktree`): strip a trailing
/// `/.git`. So `/x/proj/.git` names the repository `/x/proj`, while a
/// conventional bare clone at `/x/proj.git` is its own root — `proj.git` is a
/// directory name, not the `.git` component.
///
/// The caller must pass an **absolute** path: git answers relatively unless
/// asked with `--path-format=absolute`, and a relative answer here would
/// silently produce a root that is wrong rather than one that is missing.
pub fn repo_root_from_common_dir(common_dir: &Path) -> PathBuf {
    if common_dir.file_name().and_then(|name| name.to_str()) == Some(".git") {
        if let Some(parent) = common_dir.parent() {
            return parent.to_path_buf();
        }
    }
    common_dir.to_path_buf()
}

/// The name a sibling worktree is built from.
///
/// The repository folder's own name with a trailing `.git` removed, so a bare
/// `/x/proj.git` yields siblings called `proj-wt-…` rather than the unreadable
/// `proj.git-wt-…`.
pub fn repo_stem(repo_root: &Path) -> Option<&str> {
    let name = repo_root.file_name()?.to_str()?;
    Some(name.strip_suffix(".git").unwrap_or(name))
}

/// Where a new worktree for a repository goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeParent {
    /// `<repo>/.worktrees` — chosen only when the caller has established the
    /// directory exists *and* git ignores it.
    InRepoHolder(PathBuf),
    /// One sibling directory per worktree: `<parent>/<stem>-wt-<slug>`. Not a
    /// container — each worktree is its own folder beside the repository.
    Sibling {
        /// The directory holding the repository.
        parent: PathBuf,
        /// The repository folder's name, minus any `.git`.
        stem: String,
    },
    /// A folder a person named for this repository's worktrees.
    Chosen(PathBuf),
}

impl WorktreeParent {
    /// The directory a worktree named `slug` would occupy.
    pub fn path_for(&self, slug: &str) -> PathBuf {
        match self {
            Self::InRepoHolder(holder) => holder.join(slug),
            Self::Sibling { parent, stem } => parent.join(format!("{stem}{SIBLING_INFIX}{slug}")),
            Self::Chosen(chosen) => chosen.join(slug),
        }
    }

    /// A stable name for this rule, for the plan payload and for tests.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InRepoHolder(_) => "in-repo-holder",
            Self::Sibling { .. } => "sibling",
            Self::Chosen(_) => "chosen",
        }
    }
}

/// Where a worktree goes when nobody named a folder.
///
/// `holder_exists_and_is_ignored` is the one fact this module cannot
/// establish: it needs a stat and a `git check-ignore`. Both conditions
/// matter. Unignored, live worktrees would show up in every `git status` and
/// could be committed; absent, the holder is not a convention this repository
/// has opted into, so a sibling is the safer default.
///
/// `None` only when the repository has no parent directory (it is `/`) and no
/// usable name — there is nowhere to put anything.
pub fn default_worktree_parent(
    repo_root: &Path,
    holder_exists_and_is_ignored: bool,
) -> Option<WorktreeParent> {
    if holder_exists_and_is_ignored {
        return Some(WorktreeParent::InRepoHolder(repo_root.join(HOLDER_NAME)));
    }
    let parent = repo_root.parent()?;
    let stem = repo_stem(repo_root)?;
    Some(WorktreeParent::Sibling {
        parent: parent.to_path_buf(),
        stem: stem.to_string(),
    })
}

/// Whether `path` is somewhere this host may record, list and later remove a
/// worktree of `repo_root`.
///
/// The single predicate behind placement and all three guards. `chosen` is the
/// folders a person named for this repository; an empty slice is the ordinary
/// case.
pub fn is_managed_worktree_path(repo_root: &Path, path: &Path, chosen: &[PathBuf]) -> bool {
    if !path.is_absolute() || !repo_root.is_absolute() {
        return false;
    }
    if is_under(&repo_root.join(HOLDER_NAME), path) {
        return true;
    }
    if let Some(legacy) = legacy_holder(repo_root) {
        if is_under(&legacy, path) {
            return true;
        }
    }
    if is_sibling_worktree(repo_root, path) {
        return true;
    }
    chosen.iter().any(|folder| is_under(folder, path))
}

/// The pre-fix holder: `<repo>.worktrees`, a sibling *container*.
fn legacy_holder(repo_root: &Path) -> Option<PathBuf> {
    let name = repo_root.file_name()?.to_str()?;
    let parent = repo_root.parent()?;
    Some(parent.join(format!("{name}{LEGACY_HOLDER_SUFFIX}")))
}

/// Whether `path` lies under `holder`, and is not the holder itself.
///
/// Descendant, not direct child. Worktree names legitimately contain slashes
/// in this repository — `lane/batch3-l24-relay-build` and friends — so a tree
/// may sit two levels below the holder. Restricting this to direct children
/// silently makes every such tree unrecordable and unprunable, which is the
/// exact failure this shared predicate exists to prevent. Pinned by
/// `worktree_prune_tests.rs`'s
/// `the_worktrees_folder_test_accepts_only_children_of_the_holder`.
fn is_under(holder: &Path, path: &Path) -> bool {
    path.starts_with(holder) && path != holder
}

/// Whether `path` is a per-worktree sibling of `repo_root`.
///
/// Deliberately not `Path::starts_with`, which matches whole components: the
/// prefix here is part of a *file name*, so `<parent>/<stem>-wt-` is never a
/// component prefix and `starts_with` would silently answer `false` for every
/// sibling. The tail must be non-empty, so a bare `proj-wt-` is not a
/// worktree, and `proj-wtf` does not match the infix at all.
fn is_sibling_worktree(repo_root: &Path, path: &Path) -> bool {
    let (Some(parent), Some(stem)) = (repo_root.parent(), repo_stem(repo_root)) else {
        return false;
    };
    if path.parent() != Some(parent) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    name.strip_prefix(&format!("{stem}{SIBLING_INFIX}"))
        .is_some_and(|tail| !tail.is_empty())
}

/// Why a folder a person named may not hold this repository's worktrees, or
/// `None` when it may.
///
/// `inside_repo_and_ignored` is `None` when the folder is not inside the
/// repository at all, and otherwise says whether git ignores it — the caller
/// establishes that, since it needs `git check-ignore`.
///
/// Every refusal is a sentence a person can act on. Raw git stderr is never a
/// substitute: the failure here is a choice being refused, not a command
/// failing.
pub fn chosen_parent_refusal(
    repo_root: &Path,
    chosen: &Path,
    inside_repo_and_ignored: Option<bool>,
    home: Option<&Path>,
    process_cwd: Option<&Path>,
) -> Option<&'static str> {
    if !chosen.is_absolute() {
        return Some("Use an absolute path for the worktree folder.");
    }
    if chosen.parent().is_none() {
        return Some("The filesystem root cannot hold worktrees.");
    }
    if chosen == repo_root || repo_root.starts_with(chosen) {
        return Some(
            "That folder contains the repository itself, so a worktree inside it would \
             swallow the checkout. Choose a different folder.",
        );
    }
    if home.is_some_and(|home| chosen == home) {
        return Some(
            "Choose a folder inside your home directory rather than the home directory itself.",
        );
    }
    if process_cwd.is_some_and(|cwd| chosen == cwd || cwd.starts_with(chosen)) {
        return Some("That folder contains this app's own working directory. Choose another.");
    }
    if inside_repo_and_ignored == Some(false) {
        return Some(
            "That folder is inside the repository and git does not ignore it, so every \
             session would show up in `git status`. Add it to .gitignore, or choose a \
             folder outside the repository.",
        );
    }
    None
}

#[cfg(test)]
#[path = "worktree_placement_tests.rs"]
mod tests;
