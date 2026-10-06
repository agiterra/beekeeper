use super::*;
use beekeeper_core::project_artifact_pin_fold::PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA;

fn row(target: &str, kind: &str, pinned: bool, rank: &str) -> PinRow {
    PinRow {
        target: target.to_owned(),
        target_kind: kind.to_owned(),
        pinned,
        rank: rank.to_owned(),
        by: "1".repeat(64),
        updated_at: 100,
    }
}

fn snap(pins: Vec<PinRow>) -> Snapshot {
    Snapshot {
        coordinate: format!("30621:{}:tank-loop", "a".repeat(64)),
        repo: format!("30617:{}:tank-loop-beekeeper-agents", "a".repeat(64)),
        digest: ProjectArtifactPinDigest {
            schema: PROJECT_ARTIFACT_PIN_DIGEST_SCHEMA.to_owned(),
            project: format!("30621:{}:tank-loop", "a".repeat(64)),
            repo: format!("30617:{}:tank-loop-beekeeper-agents", "a".repeat(64)),
            ignored: 0,
            other_repo: 0,
            ranks_without_pin: 0,
            pins,
        },
        latest: HashMap::new(),
        truncated: false,
    }
}

#[test]
fn a_first_pin_takes_the_first_rank_and_later_ones_append() {
    let empty = snap(vec![]);
    let first = rank_at(&empty, "docs/a.md", None).expect("a rank");
    assert_eq!(first, beekeeper_core::fractional_rank::FIRST_RANK);

    let one = snap(vec![row("docs/a.md", "file", true, "a0")]);
    let appended = rank_at(&one, "docs/b.md", None).expect("a rank");
    assert!(appended.as_str() > "a0", "{appended}");
}

#[test]
fn an_index_places_a_pin_between_its_new_neighbours() {
    let three = snap(vec![
        row("docs/a.md", "file", true, "a0"),
        row("docs/b.md", "file", true, "a1"),
        row("docs/c.md", "file", true, "a2"),
    ]);
    let first = rank_at(&three, "docs/d.md", Some(0)).expect("a rank");
    assert!(first.as_str() < "a0", "{first}");
    let middle = rank_at(&three, "docs/d.md", Some(1)).expect("a rank");
    assert!(middle.as_str() > "a0" && middle.as_str() < "a1", "{middle}");
    // An index past the end is the end, not an error: a caller asking for
    // "last" should not have to count the rows first.
    let last = rank_at(&three, "docs/d.md", Some(99)).expect("a rank");
    assert!(last.as_str() > "a2", "{last}");
}

#[test]
fn moving_a_pin_never_measures_against_its_own_row() {
    // `b` moving to index 0 must land before `a`. If its own row counted as a
    // neighbour, the rank would be computed between `a` and `b` instead and
    // the row would not move at all.
    let three = snap(vec![
        row("docs/a.md", "file", true, "a0"),
        row("docs/b.md", "file", true, "a1"),
        row("docs/c.md", "file", true, "a2"),
    ]);
    let moved = rank_at(&three, "docs/b.md", Some(0)).expect("a rank");
    assert!(moved.as_str() < "a0", "{moved}");

    // And moving it to the end lands after `c`, not after itself.
    let end = rank_at(&three, "docs/b.md", Some(2)).expect("a rank");
    assert!(end.as_str() > "a2", "{end}");
}

#[test]
fn an_unpinned_row_is_not_a_neighbour() {
    let mixed = snap(vec![
        row("docs/a.md", "file", true, "a0"),
        row("docs/gone.md", "file", false, "a1"),
        row("docs/c.md", "file", true, "a2"),
    ]);
    // Index 1 sits between the two *pinned* rows, so the unpinned one in
    // between cannot push a new pin past it.
    let middle = rank_at(&mixed, "docs/d.md", Some(1)).expect("a rank");
    assert!(middle.as_str() > "a0" && middle.as_str() < "a2", "{middle}");
}

#[test]
fn the_honesty_counters_appear_only_when_they_are_not_zero() {
    let mut quiet = snap(vec![]);
    let mut out = serde_json::json!({});
    honesty(&quiet, &mut out);
    assert_eq!(out, serde_json::json!({}));

    quiet.digest.ignored = 2;
    quiet.digest.ranks_without_pin = 1;
    quiet.truncated = true;
    let mut out = serde_json::json!({});
    honesty(&quiet, &mut out);
    assert_eq!(out["ignored"], 2);
    assert_eq!(out["ranks_without_pin"], 1);
    assert_eq!(out["truncated"], true);
    assert!(out.get("other_repo").is_none());
}
