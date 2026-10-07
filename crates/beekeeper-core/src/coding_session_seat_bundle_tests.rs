//! The sanitizer's whole contract, owned here now that both sides share it.
//!
//! These moved from `beekeeper-session-provider`'s `session.rs` and the desktop's
//! `coding_sessions/seat_bundle_tests.rs`. Each side keeps a test that the
//! path *it* composes matches a literal, which is what proves the move changed
//! no behaviour; the character-level rules live here once.

use std::path::Path;

use super::{
    bundle_directory_name, seat_bundle_dir_in, seat_bundles_root_in, SEAT_BUNDLES_DIR,
    SEAT_BUNDLE_SKILLS_DIR, UNNAMED_SESSION_BUNDLE,
};

#[test]
fn the_directory_names_are_the_ones_on_disk_today() {
    // Bundles already exist on real machines. These two strings are the
    // directory layout, so changing either orphans every bundle written
    // before the change.
    assert_eq!(SEAT_BUNDLES_DIR, "agents/seats");
    assert_eq!(SEAT_BUNDLE_SKILLS_DIR, "skills");
    assert_eq!(UNNAMED_SESSION_BUNDLE, "unnamed-session");
}

#[test]
fn an_ordinary_session_id_is_left_alone() {
    // The real case: provider-minted ids are UUIDs and must pass through
    // untouched, or every existing bundle stops being found.
    assert_eq!(
        bundle_directory_name("11111111-2222-3333-4444-555555555555"),
        "11111111-2222-3333-4444-555555555555"
    );
    assert_eq!(bundle_directory_name("abc-DEF_123"), "abc-DEF_123");
}

#[test]
fn every_other_character_becomes_one_underscore() {
    // One character in, one character out — the mapping never changes a name's
    // length, so two distinct ids cannot collapse onto one directory by
    // shortening.
    assert_eq!(bundle_directory_name("a/b"), "a_b");
    assert_eq!(bundle_directory_name("a b"), "a_b");
    assert_eq!(bundle_directory_name("a.b"), "a_b");
    // `../../` is six characters, each mapped to one `_`.
    assert_eq!(
        bundle_directory_name("../../etc/passwd"),
        "______etc_passwd"
    );
    for id in ["../../escape", "..", "a/b", "/abs", "C:\\x", "a\0b"] {
        let name = bundle_directory_name(id);
        assert_eq!(
            name.chars().count(),
            id.chars().count(),
            "{id} changed length"
        );
        assert!(!name.contains('/'), "{id} kept a separator");
        assert!(!name.contains('\\'), "{id} kept a separator");
        assert!(!name.contains('\0'), "{id} kept a NUL");
        assert_ne!(name, "..", "{id} stayed a parent reference");
        assert!(!Path::new(&name).is_absolute(), "{id} stayed absolute");
    }
}

#[test]
fn an_id_that_sanitizes_to_nothing_gets_a_name_of_its_own() {
    // An empty name would join to the seats root itself — every seat's
    // bundle, rather than this one's.
    assert_eq!(bundle_directory_name(""), UNNAMED_SESSION_BUNDLE);
    assert_ne!(bundle_directory_name(""), "");
}

#[test]
fn a_run_of_separators_stays_a_run_of_underscores() {
    assert_eq!(bundle_directory_name("///"), "___");
}

#[test]
fn a_bundle_is_always_exactly_one_component_under_the_root() {
    let app_data = Path::new("/tmp/app-data");
    let root = seat_bundles_root_in(app_data);
    assert_eq!(root, app_data.join("agents/seats"));

    for hostile in ["../../escape", "..", "a/b", "", "/abs"] {
        let resolved = seat_bundle_dir_in(app_data, hostile);
        assert!(
            resolved.starts_with(&root),
            "{hostile} escaped to {}",
            resolved.display()
        );
        assert_eq!(
            resolved.components().count(),
            root.components().count() + 1,
            "{hostile} produced more than one directory name"
        );
        assert_eq!(resolved.parent(), Some(root.as_path()));
    }
}

#[test]
fn the_composed_path_is_the_layout_both_sides_expect() {
    let app_data = Path::new("/tmp/app-data");
    assert_eq!(
        seat_bundle_dir_in(app_data, "11111111-2222-3333-4444-555555555555"),
        Path::new("/tmp/app-data/agents/seats/11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(
        seat_bundle_dir_in(app_data, "session/with/slashes"),
        Path::new("/tmp/app-data/agents/seats/session_with_slashes")
    );
}
