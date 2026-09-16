//! Keep a file the provider writes into a seat's worktree out of `git status`.
//!
//! Finding 76 (live run 6, 2026-09-04): the provider writes files into a
//! seat's working directory. They are not tracked and not in the repository's
//! `.gitignore`, so `git status --porcelain` in a seated worktree is never
//! empty. The gate observer stamps every kind-44246 row with `dirty` from
//! exactly that command ([`crate::git_probe`], untracked files included), and
//! the relay refuses an observed-dirty row — so every seat's push was refused
//! by the host's own staging. The seat's report said it verbatim: "The
//! worktree carried untracked .agents/skills/ pack files throughout, so it was
//! never clean by git status --porcelain."
//!
//! The fix is git's own per-repository exclude file, which ignores paths
//! without touching anything tracked: the line is appended to the file
//! `git rev-parse --git-path info/exclude` resolves for that working
//! directory. A directory that is not a git worktree is left alone.
//!
//! **The skills no longer go through here.** A seat's role pack is written
//! into a bundle outside every checkout ([`crate::session::seat_bundle_dir`]),
//! which is a better answer than excluding it: `info/exclude` is *shared* by a
//! linked worktree and the main checkout it was added from, so every one of
//! this repository's worktrees inherited a `.agents/` line added by one seat.
//! What is left is the Claude write fence's own settings file, which has to
//! live in the worktree because the harness reads it from there
//! ([`crate::agent_fence::WRITE_FENCE_SETTINGS_FILE`]) — one line, one caller.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::git_probe::GIT_REPO_SELECTION_VARS;

/// What one call to [`append_exclude_line`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExcludeOutcome {
    /// `cwd` is not inside a git worktree; nothing was written.
    NotARepository,
    /// The line was already there; nothing was written.
    AlreadyExcluded { exclude_file: PathBuf },
    /// The line was appended (the file and its directory created if missing).
    Added { exclude_file: PathBuf },
}

/// Make git ignore `line` under `cwd`, if `cwd` is a worktree.
///
/// Appends `line` to the repository's `info/exclude` unless a line equal to it
/// is already present. Other lines in that file are never rewritten — the file
/// is opened for append, and the only byte added before the new line is a
/// newline when the existing content does not end with one.
///
/// `info/exclude` is shared: git resolves one file for the main checkout and
/// every linked worktree of a repository, so a line added from a seat's
/// worktree is a line added for the person's own checkout too. That is the
/// cost of this mechanism, and why only a file that genuinely has to live in
/// the worktree is worth paying it for.
///
/// Errors are I/O on the exclude file only; a `cwd` outside any repository is
/// [`ExcludeOutcome::NotARepository`], not an error.
pub(crate) fn append_exclude_line(cwd: &Path, line: &str) -> std::io::Result<ExcludeOutcome> {
    let Some(exclude_file) = exclude_path(cwd) else {
        return Ok(ExcludeOutcome::NotARepository);
    };
    let existing = match std::fs::read_to_string(&exclude_file) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if existing.lines().any(|existing| existing.trim() == line) {
        return Ok(ExcludeOutcome::AlreadyExcluded { exclude_file });
    }
    if let Some(parent) = exclude_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_file)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        file.write_all(b"\n")?;
    }
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(ExcludeOutcome::Added { exclude_file })
}

/// The exclude file git would consult for `cwd`, or `None` outside a repo.
///
/// `git rev-parse --git-path info/exclude` answers for the main checkout and
/// for a linked worktree alike (where `info/` lives in the common dir). The
/// answer is relative to `cwd` when git prints it that way, so it is joined
/// back onto `cwd` before use. The repo-selection variables are cleared for
/// the reason [`GIT_REPO_SELECTION_VARS`] documents: under a git hook, an
/// inherited `GIT_DIR` would point this write at the wrong repository.
fn exclude_path(cwd: &Path) -> Option<PathBuf> {
    let mut command = Command::new("git");
    for var in GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let output = command
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let printed = String::from_utf8(output.stdout).ok()?;
    let printed = printed.trim();
    if printed.is_empty() {
        return None;
    }
    let path = Path::new(printed);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_fence::WRITE_FENCE_EXCLUDE_LINE;

    /// The one line this module still adds, for the one file that has to live
    /// in the seat's worktree.
    fn exclude_the_fence_file(cwd: &Path) -> std::io::Result<ExcludeOutcome> {
        append_exclude_line(cwd, WRITE_FENCE_EXCLUDE_LINE)
    }

    /// `git` in `cwd`, identity forced, the repo-selection variables cleared
    /// and the developer's own configuration out of the way, so a test never
    /// touches — or reads a verdict from — the machine it runs on.
    fn git(cwd: &Path, args: &[&str]) {
        let mut command = std::process::Command::new("git");
        for var in GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let status = hermetic(&mut command)
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env("GIT_AUTHOR_NAME", "exclude")
            .env("GIT_AUTHOR_EMAIL", "exclude@example.invalid")
            .env("GIT_COMMITTER_NAME", "exclude")
            .env("GIT_COMMITTER_EMAIL", "exclude@example.invalid")
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// Take the developer's global and system git configuration out of the
    /// answer.
    ///
    /// Found the hard way: this machine's `~/.config/git/ignore` holds
    /// `**/.claude/settings.local.json`, so the fixture that proves these
    /// tests have something to exclude was already clean before the exclude
    /// ran, and three tests failed on one developer's home directory. A test
    /// about git's behaviour must ask git, not ask this computer.
    fn hermetic(command: &mut std::process::Command) -> &mut std::process::Command {
        // Same three as `crate::agent_fence`'s test helper, and for the same
        // measured reason: `~/.config/git/ignore` is read through XDG when
        // `core.excludesFile` is unset, so pointing the config files away is
        // not enough on its own. The repository fixture also sets
        // `core.excludesFile` locally, which is what covers the production
        // `git` that `crate::git_probe` runs.
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "XDG_CONFIG_HOME",
                std::env::temp_dir().join("buzz-git-exclude-tests-no-xdg"),
            )
    }

    /// `git status --porcelain` in `cwd`, verbatim.
    fn porcelain(cwd: &Path) -> String {
        let mut command = std::process::Command::new("git");
        for var in GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let output = hermetic(&mut command)
            .arg("-C")
            .arg(cwd)
            .args(["status", "--porcelain"])
            .output()
            .expect("git status");
        assert!(output.status.success(), "git status failed");
        String::from_utf8(output.stdout).expect("utf-8")
    }

    /// A throwaway repository with one commit.
    ///
    /// `core.excludesFile` is emptied in the repository's own config, not just
    /// in this test's environment, because [`crate::git_probe`] runs its own
    /// `git` — and a developer whose global ignore already covers the fence
    /// file would otherwise see these tests pass on a fixture that was never
    /// dirty.
    fn repo(cwd: &Path) {
        git(cwd, &["init", "-q", "-b", "exclude-branch", "."]);
        git(cwd, &["config", "core.excludesFile", "/dev/null"]);
        std::fs::write(cwd.join("a.txt"), "a").expect("write");
        git(cwd, &["add", "a.txt"]);
        git(cwd, &["commit", "-q", "--no-gpg-sign", "-m", "one"]);
    }

    /// What the provider writes into a seat's worktree: the Claude write
    /// fence's settings file, and nothing else since the skills moved into a
    /// bundle of their own.
    fn fence_file(cwd: &Path) {
        let path = cwd.join(WRITE_FENCE_EXCLUDE_LINE);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("fence dir");
        std::fs::write(path, "{}\n").expect("fence file");
    }

    #[test]
    fn a_fenced_worktree_is_clean_by_porcelain() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        fence_file(dir.path());
        assert!(
            porcelain(dir.path()).contains(".claude/"),
            "the fixture is not dirty, so the test proves nothing"
        );

        let outcome = exclude_the_fence_file(dir.path()).expect("exclude");
        assert!(
            matches!(outcome, ExcludeOutcome::Added { .. }),
            "{outcome:?}"
        );
        assert_eq!(porcelain(dir.path()), "");
    }

    #[test]
    fn running_twice_adds_the_line_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        fence_file(dir.path());

        let first = exclude_the_fence_file(dir.path()).expect("first");
        let second = exclude_the_fence_file(dir.path()).expect("second");
        let ExcludeOutcome::Added { exclude_file } = first else {
            panic!("first call adds: {first:?}");
        };
        assert_eq!(
            second,
            ExcludeOutcome::AlreadyExcluded {
                exclude_file: exclude_file.clone()
            }
        );
        let content = std::fs::read_to_string(&exclude_file).expect("exclude file");
        assert_eq!(
            content
                .lines()
                .filter(|line| *line == WRITE_FENCE_EXCLUDE_LINE)
                .count(),
            1,
            "{content:?}"
        );
        assert_eq!(porcelain(dir.path()), "");
    }

    #[test]
    fn an_existing_exclude_file_keeps_its_other_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        fence_file(dir.path());
        let exclude_file = dir.path().join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().expect("parent")).expect("info dir");
        // No trailing newline on purpose: the append must not glue the line
        // onto the operator's last pattern.
        std::fs::write(&exclude_file, "# mine\n*.scratch\nnotes.local").expect("seed");

        let outcome = exclude_the_fence_file(dir.path()).expect("exclude");
        assert_eq!(
            outcome,
            ExcludeOutcome::Added {
                exclude_file: exclude_file.clone()
            }
        );
        assert_eq!(
            std::fs::read_to_string(&exclude_file).expect("exclude file"),
            "# mine\n*.scratch\nnotes.local\n.claude/settings.local.json\n"
        );
        assert_eq!(porcelain(dir.path()), "");
    }

    /// The line is matched as a whole line: a pattern that merely contains it
    /// (a neighbouring one, say) does not count as already present.
    #[test]
    fn a_neighbouring_pattern_is_not_mistaken_for_the_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        let exclude_file = dir.path().join(".git/info/exclude");
        std::fs::create_dir_all(exclude_file.parent().expect("parent")).expect("info dir");
        std::fs::write(&exclude_file, ".claude/settings.local.json.bak\n").expect("seed");

        let outcome = exclude_the_fence_file(dir.path()).expect("exclude");
        assert!(
            matches!(outcome, ExcludeOutcome::Added { .. }),
            "{outcome:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&exclude_file).expect("exclude file"),
            ".claude/settings.local.json.bak\n.claude/settings.local.json\n"
        );
    }

    #[test]
    fn a_non_git_directory_is_left_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        fence_file(dir.path());
        let before: Vec<_> = walk(dir.path());

        let outcome = exclude_the_fence_file(dir.path()).expect("no error outside a repo");
        assert_eq!(outcome, ExcludeOutcome::NotARepository);
        assert_eq!(walk(dir.path()), before, "something was written");
        assert!(!dir.path().join(".git").exists());
    }

    /// Every path under `root`, sorted, so "nothing changed" is a comparison.
    fn walk(root: &Path) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read_dir") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                paths.push(path);
            }
        }
        paths.sort();
        paths
    }

    /// The exclude is resolved through git, so a *linked* worktree — what the
    /// desktop actually gives a seat — writes into the common dir's
    /// `info/exclude`, and its porcelain comes out clean too.
    #[test]
    fn a_linked_worktree_is_clean_by_porcelain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).expect("main");
        repo(&main);
        let seat = dir.path().join("seat");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "seat-branch",
                seat.to_str().expect("utf-8 path"),
            ],
        );
        fence_file(&seat);
        assert!(porcelain(&seat).contains(".claude/"));

        let outcome = exclude_the_fence_file(&seat).expect("exclude");
        assert!(
            matches!(outcome, ExcludeOutcome::Added { .. }),
            "{outcome:?}"
        );
        assert_eq!(porcelain(&seat), "");
    }

    /// The end the fix is for: the gate observer's `dirty` reads `Some(false)`
    /// for a worktree with a materialized pack in it.
    #[tokio::test]
    async fn the_git_probe_reads_a_fenced_worktree_as_clean() {
        let dir = tempfile::tempdir().expect("tempdir");
        repo(dir.path());
        fence_file(dir.path());
        assert_eq!(
            crate::git_probe::probe(dir.path()).await.dirty,
            Some(true),
            "the fixture is not dirty before the exclude, so the test proves nothing"
        );

        exclude_the_fence_file(dir.path()).expect("exclude");

        let observed = crate::git_probe::probe(dir.path()).await;
        assert_eq!(observed.dirty, Some(false));
        assert_eq!(observed.branch.as_deref(), Some("exclude-branch"));
    }
}
