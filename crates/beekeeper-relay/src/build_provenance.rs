// Where the relay's build identity comes from, and which files make it stale.
//
// This file is `build.rs`'s own resolution, `include!`d into it, and also
// compiled into the crate proper under `#[cfg(test)]` (`lib.rs`) so it is
// unit-testable: `cargo test` never runs a build script, and the same
// class of defect the buzz-cli build script review found (a stamp that named
// a commit the binary did not contain, and a stamp that never went stale
// because only a symref was watched) is exactly the kind this file exists to
// keep out. See `crates/beekeeper-cli/src/build_provenance.rs` for the sibling
// this was adapted from — kept as a separate copy rather than a shared crate
// because a build script cannot depend on its own crate, and the two have
// different stamp shapes: buzz-cli accepts a short, `-dirty`-suffixed form
// for `bee --version`; the relay's NIP-11 `software_commit` is public,
// unauthenticated, disclosed protocol metadata, so it is deliberately
// narrower — a full 40-hex commit, or the literal `unknown`. Never a short
// form: a public NIP-11 reader has no checkout to disambiguate an abbreviated
// SHA against, and `bee git check --ref`'s ancestry comparison
// (`commands/git_setup.rs`) needs the full object name to look it up.
//
// Plain comments, not `//!`: this file is included into `build.rs` mid-stream,
// where an inner doc comment is not allowed.

use std::path::{Path, PathBuf};

/// What the build stamps when nothing can name a commit.
pub const UNKNOWN_STAMP: &str = "unknown";

/// A full SHA-1 object name, in hex.
const FULL_SHA_HEX: usize = 40;

/// Whether `value` is a full, lowercase-hex commit object name.
///
/// Anchors `software_commit`'s NIP-11 contract ("40-hex or `unknown`"): a
/// short SHA, mixed case, or non-hex value is refused rather than served —
/// serving a value that cannot be looked up unambiguously would be worse
/// than disclosing `unknown`.
pub fn is_full_sha(value: &str) -> bool {
    value.len() == FULL_SHA_HEX
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A `git rev-list --count` result, parsed.
///
/// The count's contract mirrors [`is_full_sha`]'s: a value that is not
/// exactly what git prints is refused rather than served. Rejects the empty
/// string, [`UNKNOWN_STAMP`], signs, leading zeros (git never pads), inner
/// whitespace, anything non-decimal, anything overflowing `u32` — and `0`.
///
/// `0` is refused because `rev-list --count` of a real commit is at minimum
/// 1 (the commit itself), so a `0` can only come from a broken pipeline; and
/// unlike a malformed string, a `0` served as data would silently take part
/// in a consumer's subtraction and read as agreement.
pub fn parse_commit_count(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || value == UNKNOWN_STAMP {
        return None;
    }
    if value.len() > 1 && value.starts_with('0') {
        return None;
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let count: u32 = value.parse().ok()?;
    (count >= 1).then_some(count)
}

/// Whether `git_dir` belongs to a shallow clone.
///
/// A shallow checkout's `rev-list --count` returns the size of the *graft*,
/// not the commit's true ordinal — `actions/checkout` defaults to
/// `fetch-depth: 1`, which would stamp a count of `1` and make every client
/// comparing against it announce a drift of the entire history. Refusing to
/// count at all is the only honest answer.
///
/// A filesystem check rather than `git rev-parse --is-shallow-repository`, so
/// it holds even where spawning `git` is undesirable or impossible, and so it
/// is testable over a fixture layout without a real git binary. `build.rs`
/// asks git as well; either saying shallow drops the count.
///
/// Resolved through [`common_dir`] for the same reason
/// [`stamp_watch_paths`] is: a linked worktree's own gitdir holds neither the
/// marker nor the refs, both of which live in the common directory. Checking
/// beside the worktree's `HEAD` finds nothing and silently reports "not
/// shallow" — which is exactly the wrong direction to be wrong in.
pub fn is_shallow_checkout(git_dir: &Path) -> bool {
    common_dir(git_dir).join("shallow").exists()
}

/// Resolve a checkout's git directory, following the `.git` *file* a linked
/// worktree carries.
///
/// Filesystem resolution rather than `git rev-parse --git-dir`, so the build
/// script can name its watch files even in an environment where invoking
/// `git` is undesirable, and so the resolution itself is testable over a
/// fixture layout without a real git binary.
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
/// `HEAD` alone is not enough: in a worktree — and on any branch — `HEAD`
/// holds `ref: refs/heads/<branch>`, which a commit does not rewrite. The
/// moving file is the ref itself, which for a linked worktree lives in the
/// **common** directory (`commondir`), not beside its `HEAD`; a packed ref
/// moves in `packed-refs` instead. All the candidates are returned; the
/// caller watches the ones that exist, because cargo re-runs a build script
/// forever when told to watch a path that does not.
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

/// RFC 3339, UTC, second precision — the shape `build_time` (NIP-11) and
/// `bee git check --ref`'s date-fallback comparison both expect.
///
/// A hand-rolled Gregorian conversion rather than a `chrono`/`time`
/// build-dependency: `build.rs` compiles before the crate's own dependency
/// graph is available to it, and pulling either crate in as a
/// build-dependency-only edge would be a second copy of the same code this
/// comment is explaining. The algorithm is Howard Hinnant's `civil_from_days`
/// (public domain), the same one those crates use internally.
pub fn rfc3339_utc_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    rfc3339_utc(now.as_secs())
}

/// `rfc3339_utc_now`'s pure half, taking the epoch second directly so it is
/// testable against known instants without mocking the clock.
pub fn rfc3339_utc(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Days since the Unix epoch (1970-01-01) to a proleptic Gregorian
/// `(year, month, day)`. Howard Hinnant's `civil_from_days`, public domain:
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn is_full_sha_accepts_only_lowercase_40_hex() {
        assert!(is_full_sha(&"a".repeat(40)));
        assert!(is_full_sha("42dd921d831c483e6e16111491b39947b4cf1f86"));
        assert!(!is_full_sha(&"a".repeat(39)), "too short");
        assert!(!is_full_sha(&"a".repeat(41)), "too long");
        assert!(!is_full_sha(&"A".repeat(40)), "uppercase is not our form");
        assert!(!is_full_sha("6a683c9e3"), "a short SHA is not accepted");
        assert!(!is_full_sha("not-hex-at-all-not-hex-at-all-not-hex-a"));
        assert!(!is_full_sha(""));
    }

    #[test]
    fn parse_commit_count_accepts_only_what_git_actually_prints() {
        assert_eq!(parse_commit_count("1"), Some(1));
        assert_eq!(parse_commit_count("40312"), Some(40_312));
        assert_eq!(
            parse_commit_count(" 40312\n"),
            Some(40_312),
            "git's trailing newline"
        );

        assert_eq!(parse_commit_count(""), None);
        assert_eq!(parse_commit_count(UNKNOWN_STAMP), None);
        assert_eq!(
            parse_commit_count("0"),
            None,
            "rev-list --count of a real commit is at least 1; a 0 would subtract as data"
        );
        assert_eq!(parse_commit_count("007"), None, "git never pads");
        assert_eq!(parse_commit_count("-1"), None);
        assert_eq!(parse_commit_count("+1"), None);
        assert_eq!(parse_commit_count("1 2"), None, "inner whitespace");
        assert_eq!(parse_commit_count("1e3"), None);
        assert_eq!(parse_commit_count("12a"), None);
        assert_eq!(
            parse_commit_count("4294967296"),
            None,
            "one past u32::MAX is refused, not wrapped"
        );
    }

    #[test]
    fn a_shallow_checkout_is_recognized_from_the_main_checkout_and_from_a_worktree() {
        let repo = FakeRepo::new();
        let worktree_git = repo.main_git().join("worktrees/batch3-l24-relay-build");
        assert!(
            !is_shallow_checkout(&repo.main_git()),
            "a full checkout has no shallow marker"
        );
        assert!(!is_shallow_checkout(&worktree_git));

        // The marker lives in the *common* directory, never beside a linked
        // worktree's own HEAD. Looking in the wrong place reports "not
        // shallow" and stamps a graft size as if it were an ordinal — the
        // failure this whole guard exists to prevent.
        fs::write(
            repo.main_git().join("shallow"),
            format!("{}\n", "c".repeat(40)),
        )
        .expect("write shallow marker");
        assert!(
            is_shallow_checkout(&repo.main_git()),
            "a fetch-depth:1 clone must never be counted"
        );
        assert!(
            is_shallow_checkout(&worktree_git),
            "a worktree of a shallow clone is just as uncountable"
        );
    }

    #[test]
    fn rfc3339_utc_known_instants() {
        // 2026-09-03T02:51:29Z — the committer time of 42dd921d8, used
        // verbatim as `git_setup::REQUIRE_VERDICT_COMMIT_TIME` on the CLI
        // side; pinning it here catches either side drifting alone.
        assert_eq!(rfc3339_utc(1_788_403_889), "2026-09-03T02:51:29Z");
        // The Unix epoch itself.
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn unknown_stamp_is_the_literal_build_info_falls_back_to() {
        assert_eq!(UNKNOWN_STAMP, "unknown");
    }

    /// `rfc3339_utc_now` (the real clock read `build.rs` calls) is the thin,
    /// untestable-for-an-exact-value half of `rfc3339_utc` (pinned above) —
    /// this only checks the shape survives a real `SystemTime::now()` call.
    #[test]
    fn rfc3339_utc_now_has_the_right_shape() {
        let stamp = rfc3339_utc_now();
        assert_eq!(stamp.len(), "2026-09-03T02:51:29Z".len(), "{stamp}");
        assert!(stamp.starts_with("20"), "{stamp}"); // any year we will run in
        assert!(stamp.ends_with('Z'), "{stamp}");
        assert_eq!(stamp.as_bytes()[4], b'-');
        assert_eq!(stamp.as_bytes()[10], b'T');
    }

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
                git.join("refs/heads/lane/batch3-l24-relay-build"),
                format!("{}\n", "b".repeat(40)),
            )
            .expect("lane ref");

            let worktree_git = git.join("worktrees/batch3-l24-relay-build");
            fs::create_dir_all(&worktree_git).expect("worktree gitdir");
            fs::write(
                worktree_git.join("HEAD"),
                "ref: refs/heads/lane/batch3-l24-relay-build\n",
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
            repo.main_git().join("worktrees/batch3-l24-relay-build"),
        );
    }

    #[test]
    fn the_ref_head_resolves_to_is_watched_not_just_head() {
        let repo = FakeRepo::new();
        let git_dir = repo.main_git().join("worktrees/batch3-l24-relay-build");
        let watched = stamp_watch_paths(&git_dir);

        assert!(watched.contains(&git_dir.join("HEAD")), "{watched:?}");
        assert!(
            watched.contains(
                &repo
                    .main_git()
                    .join("refs/heads/lane/batch3-l24-relay-build")
            ),
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
}
