//! Every refusal `read_allowlisted_project_file` can make, and the one read it
//! allows.
//!
//! The point of this file is that the reader is a *gate*, not a convenience.
//! Each test below names a way a caller could reach a file it has no business
//! reading, and asserts the reader says no in words an operator can act on.

use std::fs;
use std::os::unix::fs as unix_fs;

use super::{
    read_allowlisted_project_file, MAX_PROJECT_FILE_BYTES, MODEL_REGISTRY_RELATIVE_PATH,
    READABLE_PROJECT_FILES,
};

/// A checkout with `team/model-registry.yaml` in it, holding `text`.
fn checkout_with_registry(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let team = dir.path().join("team");
    fs::create_dir_all(&team).expect("create team dir");
    fs::write(team.join("model-registry.yaml"), text).expect("write registry");
    dir
}

#[test]
fn the_allowlist_holds_exactly_the_model_registry_today() {
    assert_eq!(READABLE_PROJECT_FILES, &[MODEL_REGISTRY_RELATIVE_PATH]);
}

#[test]
fn reads_the_allowlisted_registry_and_names_the_path_it_read() {
    let dir = checkout_with_registry("version: 1\n");
    let read = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect("the registry is readable");
    assert_eq!(read.text, "version: 1\n");
    assert!(
        read.path.ends_with("team/model-registry.yaml"),
        "path should name the file that was read, got {}",
        read.path
    );
}

#[test]
fn refuses_a_relative_path_that_is_not_on_the_allowlist() {
    let dir = checkout_with_registry("version: 1\n");
    fs::write(
        dir.path().join("team").join("secrets.yaml"),
        "token: hunter2\n",
    )
    .expect("write sibling");
    let refusal = read_allowlisted_project_file(dir.path(), "team/secrets.yaml")
        .expect_err("a file off the allowlist must be refused");
    assert_eq!(refusal.code, "path-not-allowlisted");
    assert!(
        refusal.message.contains("team/model-registry.yaml"),
        "the refusal should name what IS readable, got {}",
        refusal.message
    );
}

#[test]
fn refuses_a_parent_segment() {
    let dir = checkout_with_registry("version: 1\n");
    let refusal = read_allowlisted_project_file(dir.path(), "team/../../etc/passwd")
        .expect_err("a `..` segment must be refused");
    assert_eq!(refusal.code, "path-escapes-checkout");
}

#[test]
fn refuses_an_absolute_path() {
    let dir = checkout_with_registry("version: 1\n");
    let refusal = read_allowlisted_project_file(dir.path(), "/etc/passwd")
        .expect_err("an absolute path must be refused");
    assert_eq!(refusal.code, "path-not-relative");
}

#[test]
fn refuses_an_empty_path() {
    let dir = checkout_with_registry("version: 1\n");
    let refusal =
        read_allowlisted_project_file(dir.path(), "   ").expect_err("an empty path is not a file");
    assert_eq!(refusal.code, "path-not-relative");
}

#[test]
fn refuses_a_symlink_that_leaves_the_checkout() {
    let outside = tempfile::tempdir().expect("outside tempdir");
    let secret = outside.path().join("registry.yaml");
    fs::write(&secret, "version: 1\n").expect("write outside file");

    let dir = tempfile::tempdir().expect("tempdir");
    let team = dir.path().join("team");
    fs::create_dir_all(&team).expect("create team dir");
    unix_fs::symlink(&secret, team.join("model-registry.yaml")).expect("symlink out");

    let refusal = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("a symlink out of the checkout must be refused");
    assert_eq!(refusal.code, "outside-checkout");
}

#[test]
fn follows_a_symlink_that_stays_inside_the_checkout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let team = dir.path().join("team");
    fs::create_dir_all(&team).expect("create team dir");
    fs::write(dir.path().join("real-registry.yaml"), "version: 1\n").expect("write real");
    unix_fs::symlink(
        dir.path().join("real-registry.yaml"),
        team.join("model-registry.yaml"),
    )
    .expect("symlink in");

    let read = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect("a symlink inside the checkout is still inside it");
    assert_eq!(read.text, "version: 1\n");
}

#[test]
fn refuses_a_directory_where_the_file_should_be() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("team").join("model-registry.yaml"))
        .expect("create dir in the file's place");
    let refusal = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("a directory is not a readable file");
    assert_eq!(refusal.code, "not-a-file");
}

#[test]
fn refuses_a_file_that_is_not_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let refusal = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("a missing file must be refused, not read as empty");
    assert_eq!(refusal.code, "file-missing");
    assert!(
        refusal.message.contains("team/model-registry.yaml"),
        "the refusal should name the path it looked for, got {}",
        refusal.message
    );
}

#[test]
fn refuses_a_checkout_directory_that_is_not_there() {
    let dir = tempfile::tempdir().expect("tempdir");
    let gone = dir.path().join("no-such-checkout");
    let refusal = read_allowlisted_project_file(&gone, MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("a missing checkout must be refused");
    assert_eq!(refusal.code, "checkout-missing");
}

#[test]
fn refuses_a_file_over_the_size_ceiling() {
    let big = "x".repeat((MAX_PROJECT_FILE_BYTES + 1) as usize);
    let dir = checkout_with_registry(&big);
    let refusal = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("a file over the ceiling must be refused");
    assert_eq!(refusal.code, "too-large");
    assert!(
        refusal.message.contains("262144"),
        "the refusal should name the ceiling in bytes, got {}",
        refusal.message
    );
}

#[test]
fn reads_a_file_exactly_at_the_size_ceiling() {
    let exact = "x".repeat(MAX_PROJECT_FILE_BYTES as usize);
    let dir = checkout_with_registry(&exact);
    let read = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect("the ceiling is inclusive");
    assert_eq!(read.text.len(), MAX_PROJECT_FILE_BYTES as usize);
}

#[test]
fn refuses_bytes_that_are_not_utf8_rather_than_replacing_them() {
    let dir = tempfile::tempdir().expect("tempdir");
    let team = dir.path().join("team");
    fs::create_dir_all(&team).expect("create team dir");
    fs::write(team.join("model-registry.yaml"), [0xff, 0xfe, 0x00]).expect("write bytes");
    let refusal = read_allowlisted_project_file(dir.path(), MODEL_REGISTRY_RELATIVE_PATH)
        .expect_err("invalid UTF-8 must be refused, not silently mangled");
    assert_eq!(refusal.code, "not-utf8");
}
