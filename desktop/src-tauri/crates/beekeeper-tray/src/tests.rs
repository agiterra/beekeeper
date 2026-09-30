//! The layout invariant the old index arithmetic could not state.

use super::*;

fn activity(name: &str, pubkey: &str, channel: &str) -> TrayAgentActivity {
    TrayAgentActivity {
        activity_id: format!("{channel}:{pubkey}"),
        agent_name: name.into(),
        agent_pubkey: pubkey.into(),
        channel_id: channel.into(),
        channel_name: channel.into(),
        elapsed: "12s".into(),
    }
}

/// Where a subtitle goes, for a given plan: the positions of the activity
/// rows, in order. This is what `build_menu` records as it appends, and what
/// the removed arithmetic tried to recompute.
fn subtitle_positions(plan: &[PlannedRow]) -> Vec<usize> {
    plan.iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, PlannedRow::Activity(_)))
        .map(|(position, _)| position)
        .collect()
}

const TRAILING: &[MenuRow<'static>] = &[
    MenuRow::Separator,
    MenuRow::Action {
        id: "open",
        label: "Open Beekeeper",
    },
];

fn sections() -> MenuSections<'static> {
    MenuSections {
        header: None,
        running_heading: Some("Running"),
        empty_label: Some("No agents are running"),
        recent_heading: Some("Recent"),
    }
}

/// Every subtitle target is an activity row — never a separator and never a
/// heading. This is the property the old code could get wrong: it counted
/// rows by hand, and a heading added at the top shifted every index by one
/// with no failure of any kind, landing channel names on separators.
#[test]
fn every_subtitle_target_is_an_activity_row_for_every_shape() {
    for header in [None, Some("Agent host: running · 2 sessions")] {
        for running in 0..4 {
            for recent in 0..3 {
                let sections = MenuSections {
                    header,
                    ..sections()
                };
                let plan = plan_menu(&sections, running, recent, TRAILING);
                let positions = subtitle_positions(&plan);
                assert_eq!(
                    positions.len(),
                    running + recent,
                    "header={header:?} running={running} recent={recent}: {plan:?}"
                );
                for position in &positions {
                    assert!(
                        matches!(plan[*position], PlannedRow::Activity(_)),
                        "row {position} is not an activity: {plan:?}"
                    );
                }
                // And the payloads index the concatenated slice in order, so
                // a row's subtitle is its own channel and not its neighbour's.
                let payloads: Vec<usize> = plan
                    .iter()
                    .filter_map(|row| match row {
                        PlannedRow::Activity(which) => Some(*which),
                        _ => None,
                    })
                    .collect();
                assert_eq!(payloads, (0..running + recent).collect::<Vec<_>>());
            }
        }
    }
}

/// Adding a header shifts every activity row down by exactly one. Stated as a
/// test because this is the change that used to break subtitles silently — the
/// menu bar app adds a status header, and the desktop app's menu had none.
#[test]
fn a_header_shifts_the_activity_rows_and_the_targets_move_with_them() {
    let without = plan_menu(&sections(), 2, 1, TRAILING);
    let with = plan_menu(
        &MenuSections {
            header: Some("Agent host: running"),
            ..sections()
        },
        2,
        1,
        TRAILING,
    );
    let before = subtitle_positions(&without);
    let after = subtitle_positions(&with);
    assert_eq!(after, before.iter().map(|p| p + 1).collect::<Vec<_>>());
}

/// With nothing running, the empty row sits where an activity row would, and
/// must not be given a subtitle.
#[test]
fn an_empty_menu_has_no_subtitle_targets() {
    let plan = plan_menu(&sections(), 0, 0, TRAILING);
    assert!(subtitle_positions(&plan).is_empty(), "{plan:?}");
    assert!(plan
        .iter()
        .any(|row| matches!(row, PlannedRow::Disabled(label) if label.contains("No agents"))));
}

/// Recent rows follow a separator and a heading. The old arithmetic encoded
/// that as `item_index += 2`, in a different function from the one that
/// appended them.
#[test]
fn recent_rows_come_after_their_separator_and_heading() {
    let plan = plan_menu(&sections(), 1, 1, TRAILING);
    let positions = subtitle_positions(&plan);
    assert_eq!(positions.len(), 2);
    assert_eq!(
        positions[1] - positions[0],
        3,
        "one separator and one heading sit between the sections: {plan:?}"
    );
    assert!(matches!(plan[positions[0] + 1], PlannedRow::Separator));
}

/// No recent rows means no separator and no heading for them — otherwise the
/// menu grows an empty section and every index below it moves.
#[test]
fn no_recent_rows_means_no_recent_section() {
    let plan = plan_menu(&sections(), 2, 0, TRAILING);
    assert!(!plan
        .iter()
        .any(|row| matches!(row, PlannedRow::Disabled(label) if label == "Recent")));
}

/// The trailing rows are the embedder's, and they land last whatever the
/// sections above did. The desktop's menu quits the desktop; the menu bar's
/// must not, so the two differ here on purpose.
#[test]
fn trailing_rows_are_appended_last_and_verbatim() {
    let trailing = &[
        MenuRow::Separator,
        MenuRow::Action {
            id: "quit-menu-bar",
            label: "Quit Menu Bar App",
        },
        MenuRow::Disabled("does not stop your agents"),
    ];
    let plan = plan_menu(&sections(), 1, 0, trailing);
    let tail = &plan[plan.len() - 3..];
    assert_eq!(tail[0], PlannedRow::Separator);
    assert_eq!(
        tail[1],
        PlannedRow::Action {
            id: "quit-menu-bar".into(),
            label: "Quit Menu Bar App".into()
        }
    );
    assert_eq!(
        tail[2],
        PlannedRow::Disabled("does not stop your agents".into())
    );
}

// ── labels ───────────────────────────────────────────────────────────────────

/// Names are unique per project, not per computer (ledger 246), and the tray
/// spans every project — so two rows really can both say `Builder`.
#[test]
fn two_agents_sharing_a_name_are_told_apart() {
    let activities = [
        activity("Builder", &"a".repeat(64), "planning"),
        activity("Builder", &"b".repeat(64), "mobile"),
        activity("Runner", &"c".repeat(64), "design"),
    ];
    let ambiguous = ambiguous_agent_names(activities.iter());

    assert!(agent_item_label(&activities[0], &ambiguous).starts_with("Builder (aaaaaaaa"));
    assert!(agent_item_label(&activities[1], &ambiguous).starts_with("Builder (bbbbbbbb"));
    assert!(
        agent_item_label(&activities[2], &ambiguous).starts_with("Runner \u{b7}"),
        "a name nothing collides with is left plain"
    );
}

/// The same agent working in two channels is one identity, not a collision.
#[test]
fn one_agent_in_two_channels_is_not_a_collision() {
    let pubkey = "a".repeat(64);
    let activities = [
        activity("Builder", &pubkey, "planning"),
        activity("Builder", &pubkey, "mobile"),
    ];
    assert!(ambiguous_agent_names(activities.iter()).is_empty());
    assert!(agent_item_label(&activities[0], &Default::default()).starts_with("Builder \u{b7}"));
}

#[test]
fn elapsed_reads_as_a_duration_at_every_scale() {
    assert_eq!(format_elapsed(Duration::from_secs(9)), "9s");
    assert_eq!(format_elapsed(Duration::from_secs(59)), "59s");
    assert_eq!(format_elapsed(Duration::from_secs(60)), "1m 0s");
    assert_eq!(format_elapsed(Duration::from_secs(192)), "3m 12s");
    assert_eq!(format_elapsed(Duration::from_secs(3600)), "1h 0m 0s");
    assert_eq!(format_elapsed(Duration::from_secs(3661)), "1h 1m 1s");
}

#[test]
fn a_channel_item_id_round_trips_and_a_foreign_id_does_not_parse() {
    let activity = activity("Builder", &"a".repeat(64), "planning");
    let id = channel_item_id(&activity);
    assert_eq!(channel_id_from_item_id(&id), Some("planning"));
    assert_eq!(channel_id_from_item_id("tray-quit"), None);
    // An id with no separator is still a channel id, which is what an older
    // build's menu produced.
    assert_eq!(
        channel_id_from_item_id("tray-open-channel:planning"),
        Some("planning")
    );
}

#[test]
fn a_short_pubkey_is_eight_characters_and_an_ellipsis() {
    assert_eq!(short_pubkey(&"a".repeat(64)), "aaaaaaaa\u{2026}");
    // Shorter than eight: whatever there is, plus the ellipsis — never a panic
    // on a slice boundary.
    assert_eq!(short_pubkey("abc"), "abc\u{2026}");
    assert_eq!(short_pubkey(""), "\u{2026}");
}
