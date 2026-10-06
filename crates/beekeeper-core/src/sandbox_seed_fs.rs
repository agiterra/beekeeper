//! [`SeedOps`] over a real filesystem.
//!
//! One implementation, reachable by all three callers, because the four copies
//! of the reclaim loop this change replaces are what a second copy becomes.
//!
//! Two deliberate seams:
//!
//! * **git answers are injected.** `tracked` and `ignored` need `git`, and
//!   *how* git may be run is the caller's business, not this crate's: a seat's
//!   host must run it inside the tree's prepared boundary, while a person
//!   running `bee` on their own machine just runs it. So this takes a
//!   [`SeedGit`] rather than spawning git itself, and `buzz-core` keeps its
//!   habit of not reaching for the network or a subprocess.
//! * **sizes are opt-in.** Measuring a 36 GB build directory means walking it,
//!   which is seconds of pure overhead on a path whose whole point is to be
//!   fast. A caller that wants sizes asks for them with
//!   [`StdSeedOps::measuring`]; one that does not gets `unknown`, which is the
//!   honest answer for a size nobody paid to find out.
//!
//! The one thing here that is not plain `std` is the copy-on-write clone,
//! which is `cp -Rc`: there is no `std` call for `clonefile`, and shelling out
//! to the one tool that has it beats adding a libc dependency to the crate
//! everything else depends on. `cp -Rc` **fails** rather than silently making
//! a real copy when the clone cannot be done, which is exactly the behaviour
//! `on_unsupported: refuse` needs.

use std::path::{Path, PathBuf};

use crate::sandbox_seed::{NodeKind, SeedOps};

#[cfg(test)]
#[path = "sandbox_seed_fs_tests.rs"]
mod tests;

/// The git questions the seeder needs answered, by whoever may run git.
pub trait SeedGit {
    /// Whether git tracks anything at or under a checkout-relative path.
    fn tracked(&self, root: &Path, relative: &str) -> bool;
    /// Whether git would ignore a checkout-relative path, were it `kind`.
    ///
    /// The `kind` is load-bearing and not a formality: an ignore rule written
    /// with a trailing slash hides a directory and not a symlink, so the same
    /// path can be ignored in one shape and staged in the other.
    fn ignored(&self, root: &Path, relative: &str, kind: NodeKind) -> bool;
}

/// Whether git would ignore a path, from the patterns git reports.
///
/// The rule, and the one place it lives. Both the CLI and the desktop supply
/// `pattern_for`, which answers `git check-ignore -v` for one query — the
/// matched pattern, or `None` — through whatever git each of them is allowed to
/// run. The logic must not be written twice, because it is not obvious and two
/// copies would drift:
///
/// * **A sandbox is seeded before these paths exist.** Without a trailing slash
///   git evaluates the query as a non-directory, so a directory-only pattern
///   like `/target/` does not match and every entry on a fresh tree reads as
///   unignored. Asking about a directory therefore asks about `"<path>/"`.
/// * **A link and a file are not directories.** A pattern written with a
///   trailing slash hides neither, which is exactly why a seeded `node_modules`
///   may not be a symlink: `.gitignore` says `node_modules/`, so the link would
///   be staged and the sandbox would never look clean.
/// * **An ignored parent hides everything under it, in any shape.** `.hermit/`
///   hides the directory above `.hermit/rust`, so a *link* at `.hermit/rust` is
///   ignored even though the matching pattern ends in a slash. Asking about the
///   parent first is what tells the two cases apart, because the pathname git
///   echoes back is the query either way.
pub fn ignored_from_patterns(
    relative: &str,
    kind: NodeKind,
    mut pattern_for: impl FnMut(&str) -> Option<String>,
) -> bool {
    if let Some(parent) = Path::new(relative).parent() {
        let parent = parent.to_string_lossy();
        if !parent.is_empty() && pattern_for(&format!("{parent}/")).is_some() {
            return true;
        }
    }
    match kind {
        NodeKind::Directory => pattern_for(&format!("{relative}/")).is_some(),
        NodeKind::Symlink | NodeKind::File => {
            pattern_for(relative).is_some_and(|pattern| !pattern.ends_with('/'))
        }
    }
}

/// Git answers for a caller that never asks any: reclaim reads shapes off the
/// disk and consults git about nothing.
///
/// Both answers fail closed, so this cannot be used to *seed* and quietly
/// work: every entry would be refused as not-ignored or as tracked.
pub struct NoGitAnswers;

impl SeedGit for NoGitAnswers {
    fn tracked(&self, _root: &Path, _relative: &str) -> bool {
        true
    }

    fn ignored(&self, _root: &Path, _relative: &str, _kind: NodeKind) -> bool {
        false
    }
}

/// Where `cp` lives. Absolute, so nothing on `PATH` can stand in for it.
const CP: &str = "/bin/cp";

/// [`SeedOps`] over `std::fs`, with git delegated and sizes opt-in.
pub struct StdSeedOps<G: SeedGit> {
    git: G,
    measure: bool,
}

impl<G: SeedGit> StdSeedOps<G> {
    /// Operate on the real filesystem, answering git questions through `git`.
    pub const fn new(git: G) -> Self {
        Self {
            git,
            measure: false,
        }
    }

    /// Measure the directories that are seeded.
    ///
    /// Off by default: it walks every file, which on a build directory is
    /// seconds. Worth it for `bee sandbox seed`, where a person is reading the
    /// output; not worth it on the path that creates a seat.
    #[must_use]
    pub const fn measuring(mut self, measure: bool) -> Self {
        self.measure = measure;
        self
    }

    /// Whether a copy-on-write clone could work between two paths.
    ///
    /// Same volume, on a platform that has `clonefile`. A same-volume path on
    /// a filesystem that cannot clone — an exFAT disk, say — passes this and
    /// then fails in [`SeedOps::clone_tree`], which is reported as a failure
    /// with the error `cp` gave rather than as a silent real copy.
    fn same_volume(from: &Path, to: &Path) -> bool {
        use std::os::unix::fs::MetadataExt as _;
        let Some(from) = std::fs::metadata(from).ok().map(|meta| meta.dev()) else {
            return false;
        };
        // The destination does not exist yet, so its volume is its nearest
        // ancestor's.
        let mut probe = to.to_path_buf();
        loop {
            if let Ok(meta) = std::fs::metadata(&probe) {
                return meta.dev() == from;
            }
            if !probe.pop() {
                return false;
            }
        }
    }

    fn run_cp(args: &[&std::ffi::OsStr]) -> std::io::Result<()> {
        let output = std::process::Command::new(CP).args(args).output()?;
        if output.status.success() {
            return Ok(());
        }
        let tail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(std::io::Error::other(format!(
            "{CP} {}: {tail}",
            args.iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        )))
    }

    /// Allocated size of a tree, the way `du` counts it: blocks, not lengths,
    /// and never following a link out of the tree.
    fn allocated(path: &Path) -> Option<u64> {
        use std::os::unix::fs::MetadataExt as _;
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            return Some(0);
        }
        let mut total = meta.blocks().saturating_mul(512);
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).ok()? {
                let entry = entry.ok()?;
                total = total.saturating_add(Self::allocated(&entry.path())?);
            }
        }
        Some(total)
    }
}

impl<G: SeedGit> SeedOps for StdSeedOps<G> {
    fn node_kind(&self, path: &Path) -> Option<NodeKind> {
        let meta = std::fs::symlink_metadata(path).ok()?;
        let file_type = meta.file_type();
        Some(if file_type.is_symlink() {
            NodeKind::Symlink
        } else if file_type.is_dir() {
            NodeKind::Directory
        } else {
            NodeKind::File
        })
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        std::fs::read_link(path).ok()
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    fn children(&self, path: &Path) -> std::io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        Ok(names)
    }

    fn tree_bytes(&self, path: &Path) -> Option<u64> {
        // Unknown, not zero, when nobody asked to pay for the walk.
        if !self.measure {
            return None;
        }
        Self::allocated(path)
    }

    fn clone_supported(&self, from: &Path, to: &Path) -> bool {
        cfg!(target_os = "macos") && Self::same_volume(from, to)
    }

    fn tracked(&self, root: &Path, relative: &str) -> bool {
        self.git.tracked(root, relative)
    }

    fn ignored(&self, root: &Path, relative: &str, kind: NodeKind) -> bool {
        self.git.ignored(root, relative, kind)
    }

    fn lock_held(&self, path: &Path) -> bool {
        let Ok(file) = std::fs::File::open(path) else {
            // No lock file means no build holding it. A lock file that cannot
            // be opened is treated as held below, via the error arm.
            return false;
        };
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            // Held, or unknowable. Both answer "do not read this directory":
            // being wrong the other way corrupts a copy.
            Err(_) => true,
        }
    }

    fn make_dir(&self, path: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn clone_tree(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if !cfg!(target_os = "macos") {
            return Err(std::io::Error::other(
                "copy-on-write clone is only wired up for macOS",
            ));
        }
        Self::run_cp(&[
            std::ffi::OsStr::new("-Rc"),
            from.as_os_str(),
            to.as_os_str(),
        ])
    }

    fn copy_tree(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        Self::run_cp(&[
            std::ffi::OsStr::new("-Rp"),
            from.as_os_str(),
            to.as_os_str(),
        ])
    }

    fn link(&self, target: &Path, at: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, at)
    }

    fn unlink(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_tree(&self, path: &Path) -> std::io::Result<()> {
        // Never follows a link: the caller has already established that this
        // is a real directory, and `remove_dir_all` on a link would take the
        // tree on the other side of it.
        std::fs::remove_dir_all(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }

    fn read_text(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_text(&self, path: &Path, text: &str) -> std::io::Result<()> {
        std::fs::write(path, text)
    }
}
