// Where `bee --version`'s commit stamp comes from, and which files make it
// stale.
//
// This file is the build script's own resolution, `include!`d by `build.rs`
// and compiled into the crate under `cfg(test)` so it can be tested: `cargo
// test` never runs a build script, and the 2026-09-01 review found two
// defects living in exactly this code — a stamp that named a commit the
// binary did not contain, and a stamp that never went stale because the only
// watched file was a symref that a commit does not rewrite (REVIEW-A1 F1).
//
// Plain comments, not `//!`: the file is included into `build.rs` mid-stream,
// where an inner doc comment is not allowed.

use std::path::{Path, PathBuf};

/// What the build stamps when nothing can name a commit.
pub const UNKNOWN_STAMP: &str = "unknown";

/// The suffix a build from a tree with uncommitted tracked changes carries.
pub const DIRTY_SUFFIX: &str = "-dirty";

/// Shortest commit abbreviation this accepts from the environment.
const MIN_STAMP_HEX: usize = 7;

/// Longest one: a full SHA-1 object name.
const MAX_STAMP_HEX: usize = 40;

/// The stamp `bee --version` prints for one build.
///
/// `dirty` is the checkout's state when the stamp was taken: a commit name
/// alone would claim the binary contains exactly that commit, and a build from
/// a tree with uncommitted changes to tracked files does not. `unknown` is
/// already the absence of a commit and is never decorated — there is nothing
/// for the suffix to qualify.
pub fn stamp(sha: Option<&str>, dirty: bool) -> String {
    match sha {
        Some(sha) if dirty => format!("{sha}{DIRTY_SUFFIX}"),
        Some(sha) => sha.to_owned(),
        None => UNKNOWN_STAMP.to_owned(),
    }
}

/// Whether a stamp supplied by the build environment names a commit.
///
/// The override exists so a packaging pipeline building from an exported tree
/// can supply the commit it exported. It is not free text: `bee --version` is
/// read as provenance, so a value that cannot be an abbreviated object name is
/// refused and the build falls back to the checkout, or to `unknown`.
pub fn is_plausible_stamp(value: &str) -> bool {
    let hex = value.strip_suffix(DIRTY_SUFFIX).unwrap_or(value);
    (MIN_STAMP_HEX..=MAX_STAMP_HEX).contains(&hex.len())
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Resolve a checkout's git directory, following the `.git` *file* a linked
/// worktree carries.
///
/// Filesystem resolution rather than `git rev-parse`, so the build script can
/// name its watch files even when git is absent, and so the resolution itself
/// is testable over a fixture layout.
pub fn resolve_git_dir(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).ok()?;
            let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
            let target = Path::new(target);
            return Some(normalize(if target.is_absolute() {
                target.to_path_buf()
            } else {
                dir.join(target)
            }));
        }
    }
    None
}

/// The files whose change means the stamp no longer describes the checkout.
///
/// `HEAD` alone is not enough, and that was the defect: in a worktree — and on
/// any branch — `HEAD` holds `ref: refs/heads/<branch>`, which a commit does
/// not rewrite. The moving file is the ref itself, which for a linked worktree
/// lives in the **common** directory (`commondir`), not beside its `HEAD`; a
/// packed ref moves in `packed-refs` instead. All the candidates are returned;
/// the caller watches the ones that exist, because cargo re-runs a build
/// script forever when told to watch a path that does not.
pub fn stamp_watch_paths(git_dir: &Path) -> Vec<PathBuf> {
    let common = common_dir(git_dir);
    let mut paths = vec![git_dir.join("HEAD")];
    if let Some(reference) = head_ref(&git_dir.join("HEAD")) {
        let local = git_dir.join(&reference);
        let shared = common.join(&reference);
        if shared != local {
            paths.push(shared);
        }
        paths.push(local);
    }
    paths.push(common.join("packed-refs"));
    paths
}

/// The repository directory a linked worktree's gitdir shares refs with.
fn common_dir(git_dir: &Path) -> PathBuf {
    let Ok(text) = std::fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let Some(target) = text.lines().next().map(str::trim) else {
        return git_dir.to_path_buf();
    };
    let target = Path::new(target);
    normalize(if target.is_absolute() {
        target.to_path_buf()
    } else {
        git_dir.join(target)
    })
}

/// Resolve `.` and `..` lexically.
///
/// Lexical rather than `canonicalize`, because a watch path is a name the
/// build script hands cargo, and cargo compares names: `<gitdir>/../../refs/…`
/// and `<repo>/.git/refs/…` are the same file and must be the same string.
/// Symlinked checkouts keep the name they were given, which is the name cargo
/// already sees elsewhere in the build.
fn normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push(component);
                }
            }
            std::path::Component::CurDir => {}
            component => out.push(component),
        }
    }
    out
}

/// The ref `HEAD` points at, or `None` for a detached `HEAD`.
fn head_ref(head: &Path) -> Option<String> {
    let text = std::fs::read_to_string(head).ok()?;
    Some(text.lines().next()?.strip_prefix("ref:")?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A main checkout plus one linked worktree, in the layout git writes.
    struct FakeRepo {
        root: tempfile::TempDir,
    }

    impl FakeRepo {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("temp dir");
            let git = root.path().join("main/.git");
            fs::create_dir_all(git.join("refs/heads/lane")).expect("refs dir");
            fs::write(git.join("HEAD"), "ref: refs/heads/main\n").expect("main HEAD");
            fs::write(git.join("refs/heads/main"), format!("{}\n", "a".repeat(40)))
                .expect("main ref");
            fs::write(git.join("packed-refs"), "# pack-refs with: peeled\n").expect("packed-refs");
            fs::write(
                git.join("refs/heads/lane/batch2-a1-cli"),
                format!("{}\n", "b".repeat(40)),
            )
            .expect("lane ref");

            let worktree_git = git.join("worktrees/batch2-a1-cli");
            fs::create_dir_all(&worktree_git).expect("worktree gitdir");
            fs::write(
                worktree_git.join("HEAD"),
                "ref: refs/heads/lane/batch2-a1-cli\n",
            )
            .expect("worktree HEAD");
            fs::write(worktree_git.join("commondir"), "../..\n").expect("commondir");

            let worktree = root.path().join("worktree");
            fs::create_dir_all(&worktree).expect("worktree dir");
            fs::write(
                worktree.join(".git"),
                format!("gitdir: {}\n", worktree_git.display()),
            )
            .expect(".git file");
            Self { root }
        }

        fn main_git(&self) -> PathBuf {
            self.root.path().join("main/.git")
        }

        fn worktree(&self) -> PathBuf {
            self.root.path().join("worktree")
        }
    }

    #[test]
    fn a_worktrees_git_dir_is_resolved_through_its_dot_git_file() {
        let repo = FakeRepo::new();
        let resolved = resolve_git_dir(&repo.worktree()).expect("a worktree has a git dir");
        assert_eq!(
            resolved,
            repo.main_git().join("worktrees/batch2-a1-cli"),
            "a linked worktree's `.git` is a file naming the real gitdir"
        );
    }

    #[test]
    fn a_plain_checkouts_git_dir_is_the_directory_itself() {
        let repo = FakeRepo::new();
        let resolved =
            resolve_git_dir(&repo.root.path().join("main")).expect("a checkout has a git dir");
        assert_eq!(resolved, repo.main_git());
    }

    /// The defect the review found: only `HEAD` was watched, and in a worktree
    /// `HEAD` is a symref that a commit on the same branch does not rewrite —
    /// so the stamp survived every commit the branch took.
    #[test]
    fn the_ref_head_resolves_to_is_watched_not_just_head() {
        let repo = FakeRepo::new();
        let git_dir = repo.main_git().join("worktrees/batch2-a1-cli");
        let watched = stamp_watch_paths(&git_dir);

        assert!(
            watched.contains(&git_dir.join("HEAD")),
            "HEAD itself is still watched: {watched:?}"
        );
        assert!(
            watched.contains(&repo.main_git().join("refs/heads/lane/batch2-a1-cli")),
            "the file a commit actually rewrites must be watched: {watched:?}"
        );
        assert!(
            watched.contains(&repo.main_git().join("packed-refs")),
            "a packed ref moves in packed-refs: {watched:?}"
        );
    }

    #[test]
    fn a_detached_head_watches_head_and_packed_refs_only() {
        let repo = FakeRepo::new();
        let git_dir = repo.main_git();
        fs::write(git_dir.join("HEAD"), format!("{}\n", "c".repeat(40))).expect("detached HEAD");
        let watched = stamp_watch_paths(&git_dir);
        assert!(watched.contains(&git_dir.join("HEAD")), "{watched:?}");
        assert!(
            !watched
                .iter()
                .any(|path| path.to_string_lossy().contains("refs/heads/")),
            "a detached HEAD names no branch to watch: {watched:?}"
        );
    }

    /// A commit name alone claims the binary is that commit. It is not, when
    /// the tree it was built from carried uncommitted changes.
    #[test]
    fn a_dirty_checkout_is_stamped_dirty() {
        assert_eq!(stamp(Some("6a683c9e3"), true), "6a683c9e3-dirty");
        assert_eq!(stamp(Some("6a683c9e3"), false), "6a683c9e3");
    }

    /// `unknown` is already the absence of a commit; `unknown-dirty` would
    /// dress that absence up as a measurement.
    #[test]
    fn an_unknown_commit_is_never_dressed_up() {
        assert_eq!(stamp(None, true), UNKNOWN_STAMP);
        assert_eq!(stamp(None, false), UNKNOWN_STAMP);
    }

    /// The environment override exists for packaging pipelines that build from
    /// an exported tree. It is not a free-text field: a value that cannot be a
    /// commit is refused, so `bee --version` can never print a slogan.
    #[test]
    fn only_a_commit_shaped_override_is_accepted() {
        assert!(is_plausible_stamp("6a683c9e3"));
        assert!(is_plausible_stamp(&"a".repeat(40)));
        assert!(is_plausible_stamp("6a683c9e3-dirty"));
        assert!(!is_plausible_stamp("totally-not-a-sha"));
        assert!(
            !is_plausible_stamp("6A683C9E3"),
            "uppercase is not our form"
        );
        assert!(!is_plausible_stamp("abc"), "too short to name a commit");
        assert!(!is_plausible_stamp(""));
    }
}
