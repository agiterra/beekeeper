//! What the ref-state reader says, and what the prune plan refuses to delete.

use super::*;

fn ref_state(name: &str, sha: &str, as_of: Option<i64>) -> PulseRefState {
    PulseRefState {
        ref_name: name.to_owned(),
        sha: sha.to_owned(),
        pusher_pubkey: "11".repeat(32),
        as_of,
    }
}

const NOW: i64 = 1_756_800_960;
const DAY: i64 = 24 * 60 * 60;

#[test]
fn a_relay_signed_ref_state_decodes_to_its_branches_and_its_pusher() {
    let event = json!({
        "kind": 30618,
        "created_at": 1_756_800_600,
        "tags": [
            ["d", "beekeeper"],
            ["refs/heads/main", "c1d2e3f4a5b60718293a4b5c6d7e8f9001122334"],
            ["refs/heads/wip/builder/1f2e3d4c", "9a1c4e7b2d3f40516273849506172839405a6b7c"],
            ["refs/tags/v1", "aaaa"],
            ["HEAD", "ref: refs/heads/main"],
            ["p", "22".repeat(32)],
        ],
    });
    let refs = decode_ref_state(&event);
    assert_eq!(refs.len(), 2, "only refs/heads/* are branches: {refs:?}");
    assert_eq!(refs[0].ref_name, "refs/heads/main");
    assert_eq!(refs[1].ref_name, "refs/heads/wip/builder/1f2e3d4c");
    assert!(refs
        .iter()
        .all(|state| state.pusher_pubkey == "22".repeat(32)));
    assert!(refs.iter().all(|state| state.as_of == Some(1_756_800_600)));
}

#[test]
fn an_event_with_no_tags_decodes_to_nothing_rather_than_to_a_guess() {
    assert!(decode_ref_state(&json!({"kind": 30618})).is_empty());
    assert!(decode_ref_state(&json!({"tags": []})).is_empty());
}

#[test]
fn a_merged_ref_and_a_thirty_one_day_old_ref_are_both_deleted() {
    let refs = vec![
        ref_state("refs/heads/wip/builder/aaaa1111", "merged", Some(NOW - 60)),
        ref_state(
            "refs/heads/wip/builder/bbbb2222",
            "old",
            Some(NOW - 31 * DAY),
        ),
    ];
    let merged = |sha: &str| sha == "merged";
    let plan = plan_wip_prune(&refs, NOW, &merged);
    assert_eq!(plan.delete.len(), 2, "{plan:?}");
    assert_eq!(
        plan.delete[0].reason,
        "its commit is merged into the base branch"
    );
    assert_eq!(
        plan.delete[1].reason,
        "ref state is older than the 30-day window"
    );
    assert!(plan.keep.is_empty());
}

#[test]
fn a_ref_outside_the_wip_namespace_is_refused_and_never_deleted() {
    let refs = vec![
        ref_state("refs/heads/main", "abc", Some(NOW - 400 * DAY)),
        ref_state("refs/heads/feature/wip/x", "def", Some(NOW - 400 * DAY)),
    ];
    let merged = |_: &str| true;
    let plan = plan_wip_prune(&refs, NOW, &merged);
    assert!(
        plan.delete.is_empty(),
        "prune-wip owns one namespace: {plan:?}"
    );
    assert_eq!(plan.refused.len(), 2);
    assert!(plan.refused[0]
        .reason
        .starts_with("outside refs/heads/wip/: prune-wip never deletes a ref it does not own"));
}

#[test]
fn a_ref_with_no_readable_date_is_kept_because_unknown_is_not_old() {
    let refs = vec![ref_state("refs/heads/wip/builder/cccc3333", "live", None)];
    let merged = |_: &str| false;
    let plan = plan_wip_prune(&refs, NOW, &merged);
    assert!(plan.delete.is_empty());
    assert_eq!(
        plan.keep[0].reason,
        "unmerged, and its ref state carries no readable date"
    );
}

#[test]
fn a_ref_inside_the_window_is_kept_and_the_window_is_named() {
    let refs = vec![ref_state(
        "refs/heads/wip/builder/dddd4444",
        "fresh",
        Some(NOW - 29 * DAY),
    )];
    let merged = |_: &str| false;
    let plan = plan_wip_prune(&refs, NOW, &merged);
    assert_eq!(plan.keep[0].reason, "unmerged and inside the 30-day window");
    let printed = prune_plan_json(&plan);
    assert_eq!(printed["retentionDays"], 30);
    assert_eq!(printed["delete"].as_array().expect("delete").len(), 0);
    assert_eq!(printed["keep"][0]["ref"], "refs/heads/wip/builder/dddd4444");
}
