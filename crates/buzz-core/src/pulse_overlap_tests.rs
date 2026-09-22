//! What the overlap row says, what it refuses to guess, and what it never sends.

use super::*;

fn side(
    session: &str,
    author: &str,
    sha: &str,
    as_of: Option<i64>,
    files: &[&str],
) -> PulseOverlapSide {
    PulseOverlapSide {
        session_key: session.to_owned(),
        author_pubkey: author.to_owned(),
        sha: sha.to_owned(),
        as_of,
        files: files.iter().map(|path| (*path).to_owned()).collect(),
    }
}

fn names(entries: Vec<(&str, &str)>) -> PulseMissionNames {
    PulseMissionNames {
        names: entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
        viewer: None,
    }
}

const LEFT_SESSION: &str = "0f1e2d3c-4b5a-4978-8796-a5b4c3d2e1f0";
const RIGHT_SESSION: &str = "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9";
const LEFT_AUTHOR: &str = "11aa22bb33cc44dd55ee66ff7788990011223344556677889900112233445566";
const RIGHT_AUTHOR: &str = "22bb33cc44dd55ee66ff77889900112233445566778899001122334455667788";

#[test]
fn two_umbrellas_touching_one_file_give_one_row_naming_both_seats() {
    let sides = vec![
        side(
            LEFT_SESSION,
            LEFT_AUTHOR,
            "9a1c4e7b2d3f40516273849506172839405a6b7c",
            Some(9_640),
            &["crates/buzz-core/src/pulse.rs", "plans/SESSION_STATE.md"],
        ),
        side(
            RIGHT_SESSION,
            RIGHT_AUTHOR,
            "b7c8d9e0f1a2334455667788990011223344556f",
            None,
            &["crates/buzz-core/src/pulse.rs", "justfile"],
        ),
    ];
    let facts = fold_pulse_overlaps(&sides);
    assert_eq!(facts.len(), 1, "one collision, one row");
    assert_eq!(facts[0].paths, vec!["crates/buzz-core/src/pulse.rs"]);
    assert_eq!(facts[0].sides.len(), 2);

    let rows = render_pulse_overlap_rows(
        &facts,
        &names(vec![(LEFT_AUTHOR, "Bob"), (RIGHT_AUTHOR, "Ira")]),
        10_000,
    );
    assert_eq!(
        rows[0].lines[0].text,
        "Overlap · crates/buzz-core/src/pulse.rs: Bob (0f1e2d3c) 9a1c4e7b 6m ago, and Ira (1a2b3c4d) b7c8d9e0"
    );
    assert_eq!(rows[0].seats.len(), 2, "both seats are named");
    assert_eq!(rows[0].seats[0].age_seconds, Some(360));
    assert_eq!(
        rows[0].seats[1].age_seconds, None,
        "an unreadable date renders no age at all"
    );
}

#[test]
fn the_same_path_inside_one_umbrella_gives_no_row() {
    let sides = vec![
        side(
            LEFT_SESSION,
            LEFT_AUTHOR,
            "aaaa1111",
            Some(1),
            &["crates/buzz-core/src/pulse.rs"],
        ),
        side(
            LEFT_SESSION,
            RIGHT_AUTHOR,
            "bbbb2222",
            Some(2),
            &["crates/buzz-core/src/pulse.rs"],
        ),
    ];
    // One team's own two seats editing one file is that team's business, and
    // the newest checkpoint per umbrella is the only one considered anyway.
    assert!(fold_pulse_overlaps(&sides).is_empty());
}

#[test]
fn a_reader_who_cannot_read_one_side_gets_no_row() {
    // "Unreadable" means the caller supplies no side for that umbrella at all:
    // there is no half-row, no placeholder and no guess about what it holds.
    let sides = vec![side(
        LEFT_SESSION,
        LEFT_AUTHOR,
        "aaaa1111",
        Some(1),
        &["crates/buzz-core/src/pulse.rs"],
    )];
    assert!(fold_pulse_overlaps(&sides).is_empty());
}

#[test]
fn a_directory_prefix_is_never_an_overlap() {
    let sides = vec![
        side(
            LEFT_SESSION,
            LEFT_AUTHOR,
            "aaaa1111",
            Some(1),
            &["crates/buzz-core/src/pulse.rs"],
        ),
        side(
            RIGHT_SESSION,
            RIGHT_AUTHOR,
            "bbbb2222",
            Some(2),
            &["crates/buzz-core/src/pulse_fold.rs", "crates/buzz-core/src"],
        ),
    ];
    assert!(
        fold_pulse_overlaps(&sides).is_empty(),
        "exact equality only — never a directory-prefix guess"
    );
}

#[test]
fn shared_paths_are_bounded_with_a_visible_truncation() {
    let many: Vec<String> = (0..9).map(|index| format!("crates/f{index}.rs")).collect();
    let borrowed: Vec<&str> = many.iter().map(String::as_str).collect();
    let sides = vec![
        side(LEFT_SESSION, LEFT_AUTHOR, "aaaa1111", Some(1), &borrowed),
        side(RIGHT_SESSION, RIGHT_AUTHOR, "bbbb2222", Some(2), &borrowed),
    ];
    let facts = fold_pulse_overlaps(&sides);
    assert_eq!(facts[0].paths.len(), MAX_PULSE_OVERLAP_PATHS);
    assert_eq!(facts[0].paths_truncated, 3);
    let rows = render_pulse_overlap_rows(&facts, &names(vec![]), 10_000);
    assert_eq!(
        rows[0].lines[1].text, "3 more shared paths not shown",
        "truncation is visible, never silent"
    );
    assert_eq!(rows[0].lines[1].id, "overlap-truncated");
}

#[test]
fn the_newest_checkpoint_per_umbrella_is_the_one_paired() {
    let sides = vec![
        side(LEFT_SESSION, LEFT_AUTHOR, "old", Some(1), &["a.rs"]),
        side(LEFT_SESSION, LEFT_AUTHOR, "new", Some(9), &["b.rs"]),
        side(RIGHT_SESSION, RIGHT_AUTHOR, "other", Some(5), &["b.rs"]),
    ];
    let facts = fold_pulse_overlaps(&sides);
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].paths, vec!["b.rs"]);
    assert_eq!(facts[0].sides[0].sha, "new");
}

#[test]
fn an_overlap_row_produces_no_wake_of_any_kind() {
    // The rule, and the line nothing crosses: a lead may note or message the
    // other lead; no record ever places work on another umbrella's seat. This
    // asserts on the module itself, because the guarantee is the absence of a
    // publish rather than the shape of a value.
    // Code only: the module's own prose says what a lead *may* do, and the
    // assertion is about what this file can *execute*.
    let code: String = include_str!("pulse_overlap.rs")
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//") && !trimmed.starts_with("///") && !trimmed.starts_with("//!")
        })
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "wake_text",
        "team_wake",
        "EventBuilder",
        "sign_with_keys",
        "publish",
        "send",
    ] {
        assert!(
            !code.contains(forbidden),
            "pulse_overlap must not be able to send anything: found {forbidden}"
        );
    }

    let sides = vec![
        side(LEFT_SESSION, LEFT_AUTHOR, "aaaa1111", Some(1), &["a.rs"]),
        side(RIGHT_SESSION, RIGHT_AUTHOR, "bbbb2222", Some(2), &["a.rs"]),
    ];
    let rows = render_pulse_overlap_rows(&fold_pulse_overlaps(&sides), &names(vec![]), 10_000);
    let value = serde_json::to_value(&rows).expect("serialize");
    let text = value.to_string();
    for forbidden in ["\"target\"", "\"wake\"", "\"commandId\"", "\"assignment\""] {
        assert!(
            !text.contains(forbidden),
            "an overlap row carries no target: found {forbidden} in {text}"
        );
    }
    assert!(PULSE_OVERLAP_NO_WAKE.contains("wakes nobody"));
}

#[test]
fn checkpoint_files_is_absent_until_the_owning_lane_lands_it() {
    // `checkpoint.files` is Lane L5's key. Until it lands, no checkpoint on the
    // wire names a path, so no overlap row can be computed at all — the honest
    // failure, because paths nobody published are paths nobody may be told
    // about.
    use nostr::{EventBuilder, Keys, Kind};
    let keys = Keys::generate();
    let event = EventBuilder::new(
        Kind::Custom(44246),
        serde_json::json!({"body": {"phase": "green"}}).to_string(),
    )
    .sign_with_keys(&keys)
    .expect("sign");
    assert_eq!(pulse_checkpoint_files(&event), None);

    let with_files = EventBuilder::new(
        Kind::Custom(44246),
        serde_json::json!({"body": {"files": ["crates/buzz-core/src/pulse.rs"]}}).to_string(),
    )
    .sign_with_keys(&keys)
    .expect("sign");
    assert_eq!(
        pulse_checkpoint_files(&with_files),
        Some(vec!["crates/buzz-core/src/pulse.rs".to_owned()])
    );

    // A path longer than §1e's 256-byte bound makes the whole list unusable
    // rather than silently truncating somebody's file name.
    let long = "x".repeat(257);
    let hostile = EventBuilder::new(
        Kind::Custom(44246),
        serde_json::json!({"body": {"files": [long]}}).to_string(),
    )
    .sign_with_keys(&keys)
    .expect("sign");
    assert_eq!(pulse_checkpoint_files(&hostile), None);
}
