//! One fixture per disposition, plus the two rules that outrank the rest:
//! dirty work is never `Prunable`, and the hot checkout is never anything but
//! `Protected`.

use super::*;

/// A settled, pushed, clean, recorded, out-of-grace tree — the only shape the
/// host is ever allowed to remove on its own.
fn prunable() -> SeatWorktreeFacts {
    SeatWorktreeFacts {
        session_settled: true,
        execution_live: false,
        tip_on_relay: true,
        dirty_files: 0,
        recorded: true,
        is_protected: false,
        settled_for_secs: Some(SEAT_WORKTREE_GRACE_SECS + 1),
    }
}

#[test]
fn a_settled_pushed_clean_recorded_tree_past_grace_is_prunable() {
    assert_eq!(
        classify_seat_worktree(&prunable()),
        SeatWorktreeDisposition::Prunable
    );
    assert!(classify_seat_worktree(&prunable()).is_host_prunable());
    assert_eq!(classify_seat_worktree(&prunable()).token(), "prunable");
}

#[test]
fn three_dirty_files_are_held_never_prunable() {
    let facts = SeatWorktreeFacts {
        dirty_files: 3,
        ..prunable()
    };
    let disposition = classify_seat_worktree(&facts);
    assert_eq!(
        disposition,
        SeatWorktreeDisposition::Held { dirty_files: 3 },
        "uncommitted work is owed to a person, whatever else is true"
    );
    assert!(
        !disposition.is_host_prunable(),
        "a held tree is never removed by a sweep"
    );
}

#[test]
fn held_outranks_a_tip_that_is_not_on_the_relay() {
    let facts = SeatWorktreeFacts {
        dirty_files: 9,
        tip_on_relay: false,
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::Held { dirty_files: 9 },
        "the count is what the person needs to see first"
    );
}

#[test]
fn the_hot_checkout_is_protected_under_every_other_input() {
    for settled in [false, true] {
        for live in [false, true] {
            for pushed in [false, true] {
                for dirty in [0_u32, 7] {
                    for recorded in [false, true] {
                        let facts = SeatWorktreeFacts {
                            session_settled: settled,
                            execution_live: live,
                            tip_on_relay: pushed,
                            dirty_files: dirty,
                            recorded,
                            is_protected: true,
                            settled_for_secs: Some(SEAT_WORKTREE_GRACE_SECS * 100),
                        };
                        assert_eq!(
                            classify_seat_worktree(&facts),
                            SeatWorktreeDisposition::Protected,
                            "protected must beat {facts:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn an_unsettled_session_holds_its_tree() {
    let facts = SeatWorktreeFacts {
        session_settled: false,
        settled_for_secs: None,
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::NotSettled
    );
}

#[test]
fn a_tip_the_relay_does_not_hold_is_never_pruned() {
    let facts = SeatWorktreeFacts {
        tip_on_relay: false,
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::TipNotOnRelay
    );
}

#[test]
fn a_live_execution_beats_a_settled_session() {
    let facts = SeatWorktreeFacts {
        execution_live: true,
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::ExecutionLive
    );
}

#[test]
fn an_unrecorded_tree_is_listed_never_removed() {
    let facts = SeatWorktreeFacts {
        recorded: false,
        ..prunable()
    };
    let disposition = classify_seat_worktree(&facts);
    assert_eq!(disposition, SeatWorktreeDisposition::Unrecorded);
    assert!(
        !disposition.is_host_prunable(),
        "the 65 trees that predate the record are never host-pruned"
    );
}

#[test]
fn a_clean_pushed_tree_survives_seven_days_from_the_closure() {
    let one_day = 24 * 60 * 60;
    let facts = SeatWorktreeFacts {
        settled_for_secs: Some(one_day),
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::WithinGrace {
            remaining_secs: SEAT_WORKTREE_GRACE_SECS - one_day
        }
    );
}

#[test]
fn an_unknown_closure_age_holds_the_tree_for_a_full_window() {
    let facts = SeatWorktreeFacts {
        settled_for_secs: None,
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&facts),
        SeatWorktreeDisposition::WithinGrace {
            remaining_secs: SEAT_WORKTREE_GRACE_SECS
        },
        "an unknown age is not a long one"
    );
}

#[test]
fn the_grace_boundary_releases_exactly_at_seven_days() {
    let at = SeatWorktreeFacts {
        settled_for_secs: Some(SEAT_WORKTREE_GRACE_SECS),
        ..prunable()
    };
    assert_eq!(
        classify_seat_worktree(&at),
        SeatWorktreeDisposition::Prunable
    );
}

#[test]
fn build_output_is_reclaimable_the_moment_the_session_settles() {
    let held = SeatWorktreeFacts {
        dirty_files: 12,
        tip_on_relay: false,
        settled_for_secs: Some(1),
        ..prunable()
    };
    assert!(
        !classify_seat_worktree(&held).is_host_prunable(),
        "the tree itself is held"
    );
    assert!(
        build_output_reclaimable(&held),
        "and its build output is still rebuildable"
    );
}

#[test]
fn build_output_is_not_reclaimable_while_an_execution_runs_or_a_session_is_open() {
    assert!(!build_output_reclaimable(&SeatWorktreeFacts {
        execution_live: true,
        ..prunable()
    }));
    assert!(!build_output_reclaimable(&SeatWorktreeFacts {
        session_settled: false,
        ..prunable()
    }));
    assert!(!build_output_reclaimable(&SeatWorktreeFacts {
        is_protected: true,
        ..prunable()
    }));
}

#[test]
fn the_reclaimable_directories_are_a_closed_list() {
    assert_eq!(RECLAIMABLE_BUILD_DIRS, &["target", "desktop/node_modules"]);
}

#[test]
fn an_unmeasurable_size_reads_unknown_never_zero() {
    assert_eq!(render_reclaimable_bytes(None), "unknown");
    assert_eq!(render_reclaimable_bytes(Some(0)), "0.0 GB");
    assert_eq!(render_reclaimable_bytes(Some(18_400_000_000)), "18.4 GB");
    assert_eq!(render_reclaimable_bytes(Some(298_000_000_000)), "298.0 GB");
}

#[test]
fn every_disposition_has_its_own_token() {
    let all = [
        SeatWorktreeDisposition::Prunable,
        SeatWorktreeDisposition::Held { dirty_files: 1 },
        SeatWorktreeDisposition::NotSettled,
        SeatWorktreeDisposition::TipNotOnRelay,
        SeatWorktreeDisposition::ExecutionLive,
        SeatWorktreeDisposition::Unrecorded,
        SeatWorktreeDisposition::Protected,
        SeatWorktreeDisposition::WithinGrace { remaining_secs: 1 },
    ];
    let mut tokens: Vec<&str> = all.iter().map(|d| d.token()).collect();
    tokens.sort_unstable();
    tokens.dedup();
    assert_eq!(tokens.len(), all.len(), "tokens must not collide");
}
