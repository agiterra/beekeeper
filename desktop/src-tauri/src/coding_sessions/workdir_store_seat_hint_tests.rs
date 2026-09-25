//! A hired seat's create is staged into its own recorded worktree, or refused.
//!
//! Control run 7 (2026-09-24): the builder's hint was lost and the provider
//! fell through to the project's checkout, so the seat ran in the operator's
//! main tree. These pin the host half of the invariant.

use std::path::PathBuf;

use super::{SEAT_CWD_PROJECT_ROOT, SEAT_CWD_SHARED, SEAT_CWD_UNRECORDED};
use crate::coding_sessions::workdir_store::{
    CodingSessionSeatWorktree, CodingSessionWorkdirScope, CodingSessionWorkdirStore,
    MAX_PENDING_HINTS,
};

const PROJECT_REF: &str =
    "30621:aa00000000000000000000000000000000000000000000000000000000000000:kettle";
const SESSION: &str = "beb51e9d-8359-4629-bcda-514bb9df5381";
const ROOT: &str = "/repos/kettle";
const BUILDER_TREE: &str = "/repos/kettle-wt-coding-session-builder-1";

fn seat(path: &str) -> CodingSessionSeatWorktree {
    CodingSessionSeatWorktree {
        path: PathBuf::from(path),
        branch: "coding-session-builder-1".into(),
        repo_root: PathBuf::from(ROOT),
        created_at: "2026-09-24T18:39:30Z".into(),
        session_id: None,
        agents_clone: None,
        commit_identity: None,
        actor_pubkey: None,
    }
}

fn store_with_project() -> CodingSessionWorkdirStore {
    let mut store = CodingSessionWorkdirStore::default();
    store.set(
        CodingSessionWorkdirScope::Project,
        PROJECT_REF,
        PathBuf::from(ROOT),
    );
    store
}

#[test]
fn a_hire_with_a_recorded_worktree_is_staged_into_that_worktree() {
    let mut store = store_with_project();
    store
        .worktrees
        .insert(format!("{SESSION}/Builder"), seat(BUILDER_TREE));
    let staged = store
        .stage_seat_hint_from_record("csl-1d57e229", SESSION, "Builder", Some(PROJECT_REF))
        .expect("a recorded seat stages");
    assert_eq!(staged, PathBuf::from(BUILDER_TREE));
    assert_eq!(
        store.pending.get("csl-1d57e229"),
        Some(&PathBuf::from(BUILDER_TREE))
    );
}

#[test]
fn a_hire_with_no_worktree_record_is_refused_not_sent_to_the_project() {
    let mut store = store_with_project();
    let refused = store
        .stage_seat_hint_from_record("csl-1d57e229", SESSION, "Builder", Some(PROJECT_REF))
        .expect_err("no record must refuse");
    assert!(refused.starts_with(SEAT_CWD_UNRECORDED), "{refused}");
    assert!(store.pending.is_empty(), "a refusal stages nothing");
}

#[test]
fn a_hire_whose_record_names_the_project_root_is_refused() {
    let mut store = store_with_project();
    // Trailing slash included: the Lead record on the live store was written
    // that way, and `Path` equality must still see it as the same directory.
    store
        .worktrees
        .insert(format!("{SESSION}/Builder"), seat("/repos/kettle/"));
    let refused = store
        .stage_seat_hint_from_record("csl-1d57e229", SESSION, "Builder", Some(PROJECT_REF))
        .expect_err("the project root is never a seat's cwd");
    assert!(refused.starts_with(SEAT_CWD_PROJECT_ROOT), "{refused}");
    assert!(store.pending.is_empty());
}

#[test]
fn a_hire_whose_record_names_another_seats_tree_is_refused() {
    let mut store = store_with_project();
    store
        .worktrees
        .insert(format!("{SESSION}/Builder"), seat(BUILDER_TREE));
    store
        .worktrees
        .insert(format!("{SESSION}/Verifier"), seat(BUILDER_TREE));
    let refused = store
        .stage_seat_hint_from_record("csl-31865b30", SESSION, "Verifier", Some(PROJECT_REF))
        .expect_err("another seat's tree is refused");
    assert!(refused.starts_with(SEAT_CWD_SHARED), "{refused}");
}

/// The live cause: 64 stale hints, and the builder's fresh command id sorted
/// before all of them, so the cap evicted the hint it had just staged.
#[test]
fn a_full_hint_map_never_evicts_the_hint_it_just_staged() {
    let mut store = CodingSessionWorkdirStore::default();
    for index in 0..MAX_PENDING_HINTS {
        store.stage_hint(
            &format!("csl-9{index:07}"),
            PathBuf::from(format!("/gone/{index}")),
        );
    }
    store.stage_hint("csl-1d57e229", PathBuf::from(BUILDER_TREE));
    assert_eq!(store.pending.len(), MAX_PENDING_HINTS);
    assert_eq!(
        store.pending.get("csl-1d57e229"),
        Some(&PathBuf::from(BUILDER_TREE)),
        "the hint a create is about to resolve against was evicted"
    );
}
