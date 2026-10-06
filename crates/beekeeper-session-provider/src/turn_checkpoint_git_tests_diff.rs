//! A tree-to-tree diff names only UTF-8 paths and counts the rest. Every
//! repository here is a throwaway created by the test in its own temporary
//! directory; trees are built with `mktree`, so a Latin-1 name needs no
//! filesystem that accepts one (APFS refuses it).

use std::io::Write as _;
use std::process::{Command, Stdio};

use super::*;
use crate::execution_scope_host::UNBOUNDED_FOR_TESTS;

/// `caf\xe9.txt` and `caf\xe8.txt`: Latin-1 names that both decode lossily
/// to `caf\u{FFFD}.txt`.
const CAFE_E_ACUTE: &[u8] = b"caf\xe9.txt";
const CAFE_E_GRAVE: &[u8] = b"caf\xe8.txt";

#[test]
fn parsers_count_names_that_are_not_utf8_instead_of_colliding_them() {
    let numstat = [
        &b"1\t0\t"[..],
        CAFE_E_ACUTE,
        b"\x002\t0\t",
        CAFE_E_GRAVE,
        b"\x003\t0\tok.txt\x00",
    ]
    .concat();
    let names = [
        &b"A\0"[..],
        CAFE_E_ACUTE,
        b"\0A\0",
        CAFE_E_GRAVE,
        b"\0A\0ok.txt\0",
    ]
    .concat();
    let result = merge_diff(&parse_name_status(&names), &parse_numstat(&numstat));
    assert_eq!(
        result.not_listed, 2,
        "both Latin-1 names counted, once each"
    );
    assert_eq!(
        result.files,
        vec![ChangedFile {
            path: "ok.txt".to_owned(),
            status: FileChange::Added,
            from: None,
            additions: Some(3),
            deletions: Some(0),
        }],
        "only the UTF-8 name is listed, with its own counts"
    );
    assert!(
        result
            .files
            .iter()
            .all(|file| !file.path.contains('\u{FFFD}')),
        "no lossily decoded name is published"
    );
}

#[test]
fn a_rename_is_named_only_when_both_paths_are_utf8() {
    // R: old Latin-1 → new UTF-8; R: old UTF-8 → new Latin-1; R: both UTF-8.
    let names = [
        &b"R100\0"[..],
        CAFE_E_ACUTE,
        b"\0cafe.txt\0R100\0plain.txt\0",
        CAFE_E_GRAVE,
        b"\0R090\0old.txt\0new.txt\0",
    ]
    .concat();
    let numstat = [
        &b"0\t0\t\0"[..],
        CAFE_E_ACUTE,
        b"\0cafe.txt\x000\t0\t\0plain.txt\0",
        CAFE_E_GRAVE,
        b"\x001\t1\t\0old.txt\0new.txt\0",
    ]
    .concat();
    let result = merge_diff(&parse_name_status(&names), &parse_numstat(&numstat));
    assert_eq!(result.not_listed, 2);
    assert_eq!(
        result.files,
        vec![ChangedFile {
            path: "new.txt".to_owned(),
            status: FileChange::Renamed,
            from: Some("old.txt".to_owned()),
            additions: Some(1),
            deletions: Some(1),
        }]
    );
}

#[test]
fn counts_join_on_raw_bytes_not_on_a_lossy_name() {
    // A UTF-8 file literally named `caf\u{FFFD}.txt` beside a Latin-1 one:
    // keyed lossily, the Latin-1 file's counts could land on the real one.
    let replacement = "caf\u{FFFD}.txt".as_bytes();
    let numstat = [
        &b"9\t9\t"[..],
        CAFE_E_ACUTE,
        b"\x001\t0\t",
        replacement,
        b"\0",
    ]
    .concat();
    let names = [&b"A\0"[..], CAFE_E_ACUTE, b"\0A\0", replacement, b"\0"].concat();
    let result = merge_diff(&parse_name_status(&names), &parse_numstat(&numstat));
    assert_eq!(result.not_listed, 1);
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].path, "caf\u{FFFD}.txt");
    assert_eq!(
        (result.files[0].additions, result.files[0].deletions),
        (Some(1), Some(0)),
        "the real file keeps its own counts"
    );
}

/// `git` in `cwd`, hermetic, with optional stdin; returns trimmed stdout.
fn git_in(cwd: &Path, args: &[&str], stdin: Option<&[u8]>) -> String {
    let mut command = Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let mut child = command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git spawns");
    if let Some(bytes) = stdin {
        child
            .stdin
            .take()
            .expect("stdin piped")
            .write_all(bytes)
            .expect("stdin written");
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A tree holding `entries` (raw name → blob id), built without touching the
/// filesystem's name rules.
fn mktree(cwd: &Path, entries: &[(&[u8], &str)]) -> String {
    let mut input = Vec::new();
    for (name, blob) in entries {
        input.extend_from_slice(format!("100644 blob {blob}\t").as_bytes());
        input.extend_from_slice(name);
        input.push(0);
    }
    git_in(cwd, &["mktree", "-z"], Some(&input))
}

#[tokio::test]
async fn two_latin1_files_added_in_a_turn_are_counted_not_collided() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path();
    git_in(cwd, &["init", "-q", "-b", "main", "."], None);
    let one = git_in(cwd, &["hash-object", "-w", "--stdin"], Some(b"one\n"));
    let two = git_in(
        cwd,
        &["hash-object", "-w", "--stdin"],
        Some(b"two\nlines\n"),
    );
    let base = mktree(cwd, &[(b"keep.txt", &one)]);
    let end = mktree(
        cwd,
        &[
            (b"keep.txt", &one),
            (CAFE_E_ACUTE, &one),
            (CAFE_E_GRAVE, &two),
            (b"ok.txt", &two),
        ],
    );
    let result = diff_tree_files(cwd, Some(&UNBOUNDED_FOR_TESTS), &base, &end)
        .await
        .expect("diff");
    assert_eq!(result.not_listed, 2);
    assert_eq!(
        result.files,
        vec![ChangedFile {
            path: "ok.txt".to_owned(),
            status: FileChange::Added,
            from: None,
            additions: Some(2),
            deletions: Some(0),
        }]
    );
}
