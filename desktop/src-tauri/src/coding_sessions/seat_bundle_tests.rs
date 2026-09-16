//! Bundle cleanup, against throwaway app-data directories only.
//!
//! Every test builds its own `<app data>/agents/seats/...` tree in a
//! `TempDir`. Nothing here reads this machine's real app data directory, and
//! nothing here needs a Tauri `AppHandle`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{
    bundle_directory_name, list_orphan_seat_bundles, remove_orphan_seat_bundle, remove_seat_bundle,
    seat_bundle_dir, seat_bundles_root, SEAT_BUNDLES_DIR, SEAT_BUNDLE_MANIFEST,
    SEAT_BUNDLE_SKILLS_DIR,
};

/// Build a bundle on disk the way the provider writes one.
fn write_bundle(app_data: &Path, session_id: &str) -> PathBuf {
    let dir = seat_bundle_dir(app_data, session_id);
    std::fs::create_dir_all(dir.join(SEAT_BUNDLE_SKILLS_DIR).join("reviewing")).unwrap();
    std::fs::write(
        dir.join(SEAT_BUNDLE_SKILLS_DIR)
            .join("reviewing")
            .join("SKILL.md"),
        "# reviewing\n",
    )
    .unwrap();
    std::fs::write(dir.join(SEAT_BUNDLE_MANIFEST), "{\"packRef\":\"x\"}").unwrap();
    dir
}

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

#[test]
fn the_bundle_root_is_the_path_the_provider_writes_to() {
    // The provider composes `<app data dir>/agents/seats/<session id>`. This
    // host finds a bundle by recomputing that path, so the composition is the
    // contract, not just the constant.
    assert_eq!(SEAT_BUNDLES_DIR, "agents/seats");
    assert_eq!(SEAT_BUNDLE_SKILLS_DIR, "skills");
    let app_data = Path::new("/tmp/app-data");
    assert_eq!(
        seat_bundle_dir(app_data, "11111111-2222-3333-4444-555555555555"),
        app_data
            .join("agents/seats")
            .join("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(seat_bundles_root(app_data), app_data.join("agents/seats"));
}

#[test]
fn a_session_id_becomes_exactly_one_directory_component() {
    // Byte-for-byte the provider's sanitizer: alphanumeric, `-` and `_` keep
    // their character, everything else becomes `_`, and an empty result gets a
    // name of its own. A disagreement here looks in the wrong place.
    assert_eq!(bundle_directory_name("abc-DEF_123"), "abc-DEF_123");
    // `../../` is six characters, each mapped to one `_`.
    assert_eq!(
        bundle_directory_name("../../etc/passwd"),
        "______etc_passwd"
    );
    assert_eq!(bundle_directory_name("a/b"), "a_b");
    assert_eq!(bundle_directory_name(""), "unnamed-session");
    assert_eq!(bundle_directory_name("///"), "___");
    assert!(!bundle_directory_name("../escape").contains('/'));
}

#[test]
fn a_closed_seats_bundle_is_removed() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let dir = write_bundle(app_data, "session-one");
    assert!(dir.is_dir());

    let outcome = remove_seat_bundle(app_data, Some("session-one"), false);

    assert!(outcome.removed);
    assert_eq!(outcome.token, "removed");
    assert_eq!(outcome.path.as_deref(), Some(dir.to_str().unwrap()));
    assert!(!dir.exists());
}

#[test]
fn a_live_sessions_bundle_is_never_removed() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let dir = write_bundle(app_data, "session-live");

    let outcome = remove_seat_bundle(app_data, Some("session-live"), true);

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "session_live");
    assert!(
        dir.join(SEAT_BUNDLE_MANIFEST).is_file(),
        "a live session's skills must still be on disk"
    );
}

#[test]
fn an_absent_bundle_is_disclosed_rather_than_reported_as_removed() {
    let temp = TempDir::new().unwrap();

    let outcome = remove_seat_bundle(temp.path(), Some("never-had-one"), false);

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "absent");
    assert!(
        outcome.detail.contains("No bundle directory"),
        "unexpected sentence: {}",
        outcome.detail
    );
}

#[test]
fn a_seat_with_no_recorded_session_id_names_no_bundle() {
    let temp = TempDir::new().unwrap();
    // A bundle that would be removed if the host guessed at a name.
    let other = write_bundle(temp.path(), "somebody-elses-session");

    let outcome = remove_seat_bundle(temp.path(), None, false);

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "unnamed_session");
    assert_eq!(outcome.path, None);
    assert!(other.is_dir(), "nothing else may be removed instead");
}

#[test]
fn a_blank_session_id_is_treated_as_no_session_id() {
    let temp = TempDir::new().unwrap();
    let outcome = remove_seat_bundle(temp.path(), Some("   "), false);
    assert_eq!(outcome.token, "unnamed_session");
    assert!(!outcome.removed);
}

#[test]
fn a_bundle_path_outside_the_seats_root_is_refused() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    // A directory a traversing name would reach if the name were not sanitized
    // to one component.
    let outside = app_data.join("agents").join("nests");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "keep").unwrap();

    // The sanitizer maps this to one component, so it can only ever resolve
    // inside the root — the refusal path is proven separately below.
    let outcome = remove_seat_bundle(app_data, Some("../nests"), false);

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "absent");
    assert!(
        outside.join("keep.txt").is_file(),
        "a traversing session id must never reach a sibling directory"
    );
}

#[test]
fn an_orphan_name_that_climbs_out_of_the_root_is_refused() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let outside = app_data.join("agents").join("nests");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), "keep").unwrap();

    let outcome = remove_orphan_seat_bundle(app_data, "../nests");

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "outside_root");
    assert!(
        outside.join("keep.txt").is_file(),
        "the refusal must leave the sibling directory alone"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_bundle_is_refused_rather_than_followed() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let elsewhere = temp.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("precious.txt"), "precious").unwrap();

    let root = seat_bundles_root(app_data);
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.join("session-link")).unwrap();

    let outcome = remove_seat_bundle(app_data, Some("session-link"), false);

    assert!(!outcome.removed);
    assert_eq!(outcome.token, "outside_root");
    assert!(
        elsewhere.join("precious.txt").is_file(),
        "a symlinked bundle must not be followed out of the root"
    );
}

#[test]
fn orphans_are_listed_before_anything_is_removed() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let kept = write_bundle(app_data, "recorded-session");
    let orphan_a = write_bundle(app_data, "gone-session-a");
    let orphan_b = write_bundle(app_data, "gone-session-b");

    let orphans = list_orphan_seat_bundles(app_data, &ids(&["recorded-session"]));

    assert_eq!(orphans.len(), 2, "only unrecorded bundles are orphans");
    assert_eq!(orphans[0].name, "gone-session-a");
    assert_eq!(orphans[1].name, "gone-session-b");
    assert!(orphans.iter().all(|o| o.looks_like_bundle));
    // Listing removes nothing.
    assert!(kept.is_dir());
    assert!(orphan_a.is_dir());
    assert!(orphan_b.is_dir());
}

#[test]
fn a_listed_orphan_is_removed_by_name_and_the_recorded_one_is_not() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let kept = write_bundle(app_data, "recorded-session");
    let orphan = write_bundle(app_data, "gone-session");

    let orphans = list_orphan_seat_bundles(app_data, &ids(&["recorded-session"]));
    assert_eq!(orphans.len(), 1);

    let outcome = remove_orphan_seat_bundle(app_data, &orphans[0].name);

    assert!(outcome.removed);
    assert_eq!(outcome.token, "removed");
    assert!(!orphan.exists());
    assert!(
        kept.is_dir(),
        "a recorded session's bundle is never an orphan"
    );
}

#[test]
fn a_directory_that_is_not_a_bundle_is_listed_but_marked() {
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let root = seat_bundles_root(app_data);
    std::fs::create_dir_all(root.join("something-else")).unwrap();

    let orphans = list_orphan_seat_bundles(app_data, &BTreeSet::new());

    assert_eq!(orphans.len(), 1);
    assert_eq!(orphans[0].name, "something-else");
    assert!(
        !orphans[0].looks_like_bundle,
        "a directory with no skills/ or manifest.json is not claimed to be one"
    );
}

#[test]
fn a_missing_root_lists_nothing_rather_than_failing() {
    let temp = TempDir::new().unwrap();
    let orphans = list_orphan_seat_bundles(temp.path(), &ids(&["anything"]));
    assert!(orphans.is_empty());
}

#[test]
fn a_recorded_session_id_is_matched_through_the_same_sanitizer() {
    // The record holds the raw session id; the directory holds the sanitized
    // name. Matching the two raw would orphan every bundle whose id needed
    // sanitizing.
    let temp = TempDir::new().unwrap();
    let app_data = temp.path();
    let dir = write_bundle(app_data, "session/with/slashes");
    assert_eq!(dir.file_name().unwrap(), "session_with_slashes");

    let orphans = list_orphan_seat_bundles(app_data, &ids(&["session/with/slashes"]));

    assert!(
        orphans.is_empty(),
        "the recorded id must match its own sanitized directory"
    );
}
