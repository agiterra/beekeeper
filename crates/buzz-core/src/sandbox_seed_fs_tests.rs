//! The ignore rule, which is the measured fact the whole seeding design turns
//! on. The patterns here are this repository's real ones.

use super::*;

/// This repository's own `.gitignore`, as `git check-ignore -v` reports it.
fn beekeeper_patterns(query: &str) -> Option<String> {
    match query {
        "target/" => Some("/target/".to_owned()),
        "node_modules/" | "desktop/node_modules/" | "admin-web/node_modules/" => {
            Some("node_modules/".to_owned())
        }
        // `desktop/.gitignore` writes this one without a trailing slash, so it
        // hides a link there as well as a directory.
        "desktop/node_modules" => Some("node_modules".to_owned()),
        ".hermit/" => Some(".hermit/".to_owned()),
        ".hermit/rust" | ".hermit/rust/" => Some(".hermit/".to_owned()),
        ".env" | ".env/" => Some(".env".to_owned()),
        _ => None,
    }
}

#[test]
fn a_build_directory_that_does_not_exist_yet_is_still_ignored() {
    // The bug this rule exists to fix: asked without a trailing slash, git
    // evaluates `target` as a non-directory and `/target/` does not match, so
    // every entry on a freshly cut sandbox read as unignored.
    assert!(ignored_from_patterns(
        "target",
        NodeKind::Directory,
        beekeeper_patterns
    ));
}

#[test]
fn a_link_where_the_rule_is_directory_only_is_not_ignored() {
    assert!(!ignored_from_patterns(
        "node_modules",
        NodeKind::Symlink,
        beekeeper_patterns
    ));
}

#[test]
fn a_directory_where_the_rule_is_directory_only_is_ignored() {
    assert!(ignored_from_patterns(
        "node_modules",
        NodeKind::Directory,
        beekeeper_patterns
    ));
}

#[test]
fn a_link_under_an_ignored_parent_is_ignored_despite_the_trailing_slash() {
    // `.hermit/` hides the directory above it, so the pattern's trailing slash
    // says nothing about what shape `.hermit/rust` may be. This is what lets
    // CARGO_HOME be shared as a link at all.
    assert!(ignored_from_patterns(
        ".hermit/rust",
        NodeKind::Symlink,
        beekeeper_patterns
    ));
}

#[test]
fn a_rule_written_without_a_trailing_slash_hides_a_link_too() {
    assert!(ignored_from_patterns(
        "desktop/node_modules",
        NodeKind::Symlink,
        beekeeper_patterns
    ));
}

#[test]
fn a_file_is_judged_as_a_non_directory() {
    assert!(ignored_from_patterns(
        ".env",
        NodeKind::File,
        beekeeper_patterns
    ));
}

#[test]
fn a_path_no_pattern_matches_is_not_ignored_in_any_shape() {
    for kind in [NodeKind::Directory, NodeKind::Symlink, NodeKind::File] {
        assert!(!ignored_from_patterns("src", kind, beekeeper_patterns));
    }
}

#[test]
fn a_nested_path_whose_parent_is_not_ignored_is_judged_on_its_own() {
    assert!(ignored_from_patterns(
        "admin-web/node_modules",
        NodeKind::Directory,
        beekeeper_patterns
    ));
    assert!(!ignored_from_patterns(
        "admin-web/node_modules",
        NodeKind::Symlink,
        beekeeper_patterns
    ));
}
