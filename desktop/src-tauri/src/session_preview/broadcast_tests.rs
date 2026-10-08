use super::capture::{self, prepare_frame};
use super::pacing::WatchRefusal;
use super::*;

use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;
use beekeeper_core_pkg::session_preview::validate_session_preview_announce_envelope;
use beekeeper_core_pkg::surface_watch::{
    validate_surface_frame_envelope, Surface, SurfaceFrameHeader, PREVIEW_FRAME_BUDGET_BYTES,
    PREVIEW_FRAME_MAX_LONG_EDGE, SURFACE_FRAME_MAX_PER_MIN,
};
use beekeeper_sdk_pkg::surface::{build_session_preview_announce, build_surface_frame};
use nostr::Keys;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const T0: u64 = 1_791_374_400_000;

fn watcher(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn ready_facts() -> PreviewFacts {
    PreviewFacts {
        status: PreviewStatus::Ready,
        url: Some("http://localhost:5173/settings?tab=2#x".into()),
        title: Some("Settings".into()),
        hidden: false,
        occluded: false,
        target_key: None,
        slot: Some((1280, 800)),
        not_macos: false,
    }
}

fn configured_entry() -> ShareEntry {
    let mut entry = ShareEntry::new(T0);
    entry.config.session_ref = Some(SESSION_REF.into());
    entry.config.provider = Some(Keys::generate().public_key());
    entry
}

fn frame(hash: u8) -> PreparedFrame {
    PreparedFrame {
        hash: vec![hash; 32],
        content: "/9j/AAAA".into(),
        width: 640,
        height: 400,
    }
}

fn captures(actions: &[TickAction]) -> usize {
    actions
        .iter()
        .filter(|a| matches!(a, TickAction::Capture { .. }))
        .count()
}

// ------------------------------------------------------------- cadence

#[test]
fn cadence_spaces_attempts_by_the_base_interval() {
    let mut cadence = Cadence::new();
    assert_eq!(cadence.interval_ms, 2_000);
    assert!(cadence.ready(T0));
    cadence.attempt(T0);
    assert!(!cadence.ready(T0 + 1_999));
    assert!(cadence.ready(T0 + 2_000));
}

#[test]
fn cadence_caps_frames_at_twenty_per_rolling_minute() {
    let mut cadence = Cadence::new();
    let mut sent = 0;
    // Try every 500 ms for a minute: spacing alone would allow 30.
    for i in 0..120u64 {
        let now = T0 + i * 500;
        if cadence.ready(now) {
            cadence.attempt(now);
            cadence.record(now);
            sent += 1;
        }
    }
    assert_eq!(sent, SURFACE_FRAME_MAX_PER_MIN);
    assert!(cadence.cap_reached(T0 + 59_999));
    // The window rolls: a minute after the first frame there is room.
    assert!(!cadence.cap_reached(T0 + 60_000));
}

#[test]
fn cadence_backs_off_to_eight_seconds_and_releases_after_a_minute() {
    let mut cadence = Cadence::new();
    assert!(cadence.rate_limited(T0));
    assert_eq!(cadence.interval_ms, 4_000);
    assert!(cadence.rate_limited(T0 + 1_000));
    assert_eq!(cadence.interval_ms, 8_000);
    assert!(!cadence.rate_limited(T0 + 2_000), "8 s is the ceiling");
    assert_eq!(cadence.interval_ms, MAX_BACKOFF_CADENCE_MS);
    // Held 60 s from the last refusal.
    assert!(!cadence.tick(T0 + 2_000 + 59_999));
    assert_eq!(cadence.interval_ms, 8_000);
    assert!(cadence.tick(T0 + 2_000 + 60_000));
    assert_eq!(cadence.interval_ms, 2_000);
    assert!(!cadence.tick(T0 + 200_000));
}

#[test]
fn rate_limited_publish_reports_back_off_the_channel() {
    let channel = "0e1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e70";
    with_entry(channel, |_| ());
    note_publish(channel, true, "");
    note_publish(channel, false, "invalid: bad frame");
    assert_eq!(with_entry(channel, |e| e.cadence.interval_ms), 2_000);
    note_publish(channel, false, "rate-limited: slow down");
    assert_eq!(with_entry(channel, |e| e.cadence.interval_ms), 4_000);
}

// ------------------------------------------------------------ watchers

#[test]
fn watchers_expire_at_forty_five_seconds_and_stop_removes_at_once() {
    let mut watchers = Watchers::default();
    assert_eq!(watchers.touch(&watcher(1), T0), Ok(true));
    assert_eq!(watchers.touch(&watcher(2), T0), Ok(true));
    assert_eq!(watchers.touch(&watcher(1), T0 + 10_000), Ok(false));
    assert!(!watchers.expire(T0 + 44_999));
    assert!(watchers.expire(T0 + 45_000), "watcher 2 went silent");
    assert_eq!(watchers.list(), vec![watcher(1)]);
    assert!(watchers.remove(&watcher(1)));
    assert!(!watchers.remove(&watcher(1)));
    assert!(watchers.is_empty());
}

#[test]
fn watchers_are_capped_at_one_hundred_twenty_eight() {
    let mut watchers = Watchers::default();
    for i in 0..MAX_WATCHERS {
        watchers
            .touch(&format!("{i:064x}"), T0)
            .expect("under the cap");
    }
    assert_eq!(watchers.touch(&watcher(0xff), T0), Err(WatchRefusal::Full));
    // A known watcher still refreshes.
    assert_eq!(watchers.touch(&format!("{:064x}", 0), T0 + 1), Ok(false));
}

#[test]
fn snapshot_requests_are_limited_per_watcher() {
    let mut watchers = Watchers::default();
    assert!(watchers.allow_snapshot(&watcher(1), T0));
    assert!(!watchers.allow_snapshot(&watcher(1), T0 + 9_999));
    assert!(watchers.allow_snapshot(&watcher(2), T0 + 1), "per watcher");
    assert!(watchers.allow_snapshot(&watcher(1), T0 + 10_000));

    let mut entry = configured_entry();
    let first = entry
        .apply_watch(&watcher(3), WatchAction::Snapshot, T0)
        .expect("ok");
    assert!(first.snapshot);
    let second = entry
        .apply_watch(&watcher(3), WatchAction::Snapshot, T0 + 5_000)
        .expect("ok");
    assert!(!second.snapshot, "a refusal publishes nothing");
}

// ----------------------------------------------------------- the loop

#[test]
fn zero_watchers_means_no_capture_and_capture_stops_when_the_last_expires() {
    let mut entry = configured_entry();
    assert_eq!(captures(&entry.plan_tick(ready_facts(), T0)), 0);
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    let actions = entry.plan_tick(ready_facts(), T0);
    assert_eq!(captures(&actions), 1);
    assert!(entry.capture_active);
    entry.finish_capture(Some(&frame(1)), T0 + 100);
    // Keepalives lapse: at 45 s the watcher expires and capture stops once.
    let actions = entry.plan_tick(ready_facts(), T0 + 45_000);
    assert!(actions.contains(&TickAction::CaptureStopped));
    assert_eq!(captures(&actions), 0);
    assert!(!entry.capture_active);
    let again = entry.plan_tick(ready_facts(), T0 + 46_000);
    assert!(!again.contains(&TickAction::CaptureStopped));
    assert_eq!(captures(&again), 0);
}

#[test]
fn stop_removes_the_watcher_and_capture_stops_on_the_next_tick() {
    let mut entry = configured_entry();
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    entry.plan_tick(ready_facts(), T0);
    entry.finish_capture(Some(&frame(1)), T0 + 10);
    let stop = entry
        .apply_watch(&watcher(1), WatchAction::Stop, T0 + 500)
        .expect("stop");
    assert!(stop.state_changed);
    let actions = entry.plan_tick(ready_facts(), T0 + 600);
    assert!(actions.contains(&TickAction::CaptureStopped));
}

#[test]
fn unchanged_pictures_send_nothing_unless_a_resync_is_pending() {
    let mut entry = configured_entry();
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    entry.plan_tick(ready_facts(), T0);
    let first = entry
        .finish_capture(Some(&frame(7)), T0 + 10)
        .expect("first");
    assert_eq!(first.seq, 1);
    assert_eq!(first.frame_type, SurfaceFrameType::Frame);

    // Same pixels → no frame.
    let actions = entry.plan_tick(ready_facts(), T0 + 2_000);
    assert_eq!(captures(&actions), 1);
    assert!(entry.finish_capture(Some(&frame(7)), T0 + 2_010).is_none());

    // Changed pixels → frame.
    entry.plan_tick(ready_facts(), T0 + 4_000);
    let changed = entry
        .finish_capture(Some(&frame(8)), T0 + 4_010)
        .expect("changed");
    assert_eq!(changed.seq, 2);

    // Resync → the same pixels go out again, once.
    entry
        .apply_watch(&watcher(1), WatchAction::Resync, T0 + 5_000)
        .expect("resync");
    let actions = entry.plan_tick(ready_facts(), T0 + 6_000);
    assert!(actions.contains(&TickAction::Capture {
        force: true,
        last_hash: Some(vec![8; 32])
    }));
    let resent = entry
        .finish_capture(Some(&frame(8)), T0 + 6_010)
        .expect("resync");
    assert_eq!(resent.seq, 3);
    entry.plan_tick(ready_facts(), T0 + 8_000);
    assert!(entry.finish_capture(Some(&frame(8)), T0 + 8_010).is_none());
}

#[test]
fn a_new_watcher_gets_a_frame_even_if_nothing_changed() {
    let mut entry = configured_entry();
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    entry.plan_tick(ready_facts(), T0);
    entry
        .finish_capture(Some(&frame(1)), T0 + 10)
        .expect("first");
    entry
        .apply_watch(&watcher(2), WatchAction::Watch, T0 + 2_500)
        .expect("second watcher");
    entry.plan_tick(ready_facts(), T0 + 3_000);
    assert!(entry.finish_capture(Some(&frame(1)), T0 + 3_010).is_some());
}

#[test]
fn hidden_sends_paused_once_and_closing_sends_end_once() {
    let mut entry = configured_entry();
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    entry.plan_tick(ready_facts(), T0);
    entry.finish_capture(Some(&frame(1)), T0 + 10);
    let epoch = entry.epoch;

    let hidden = PreviewFacts {
        hidden: true,
        ..ready_facts()
    };
    let paused = |actions: &[TickAction]| {
        actions
            .iter()
            .filter(
                |a| matches!(a, TickAction::Frame(p) if p.frame_type == SurfaceFrameType::Paused),
            )
            .count()
    };
    assert_eq!(paused(&entry.plan_tick(hidden.clone(), T0 + 2_000)), 1);
    assert_eq!(paused(&entry.plan_tick(hidden.clone(), T0 + 4_000)), 0);
    assert_eq!(captures(&entry.plan_tick(hidden, T0 + 6_000)), 0);

    // Visible again: capture resumes, and the first picture is forced.
    let actions = entry.plan_tick(ready_facts(), T0 + 8_000);
    assert!(actions
        .iter()
        .any(|a| matches!(a, TickAction::Capture { force: true, .. })));
    entry
        .finish_capture(Some(&frame(1)), T0 + 8_010)
        .expect("resumed");

    let closed = PreviewFacts {
        status: PreviewStatus::ClosedByPerson,
        ..ready_facts()
    };
    let ends = |actions: &[TickAction]| {
        actions
            .iter()
            .filter_map(|a| match a {
                TickAction::Frame(p) if p.frame_type == SurfaceFrameType::End => Some(p.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let end = ends(&entry.plan_tick(closed.clone(), T0 + 12_000));
    assert_eq!(end.len(), 1);
    assert_eq!(end[0].epoch, epoch);
    assert!(ends(&entry.plan_tick(closed, T0 + 14_000)).is_empty());

    // Reopened: a new epoch, seq from 1.
    entry.plan_tick(ready_facts(), T0 + 16_000);
    let next = entry
        .finish_capture(Some(&frame(2)), T0 + 16_010)
        .expect("new");
    assert!(next.epoch > epoch);
    assert_eq!(next.seq, 1);
}

// -------------------------------------------------------------- budget

/// A deterministic noisy image JPEG-encoded at high quality: the worst case
/// for the budget.
fn noisy_jpeg(width: u32, height: u32) -> Vec<u8> {
    let mut state: u32 = 0x1234_5678;
    let image = image::RgbImage::from_fn(width, height, |_, _| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let [r, g, b, _] = state.to_le_bytes();
        image::Rgb([r, g, b])
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
        .encode(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .expect("encode");
    out
}

#[test]
fn a_noisy_retina_picture_fits_the_frame_budget() {
    let jpeg = noisy_jpeg(2560, 1600);
    let prepared = prepare_frame(&jpeg, None, false, PREVIEW_FRAME_BUDGET_BYTES)
        .expect("prepared")
        .expect("a first frame always goes");
    assert!(
        prepared.content.len() <= PREVIEW_FRAME_BUDGET_BYTES,
        "{} bytes",
        prepared.content.len()
    );
    assert!(prepared.width.max(prepared.height) <= PREVIEW_FRAME_MAX_LONG_EDGE);
    assert!(prepared.content.starts_with("/9j/"));
    // Aspect preserved (16:10).
    let ratio = prepared.width as f64 / prepared.height as f64;
    assert!((ratio - 1.6).abs() < 0.02, "ratio {ratio}");

    // The relay's own validator accepts it.
    let keys = Keys::generate();
    let header = SurfaceFrameHeader {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        surface: Surface::Preview,
        key: SESSION_REF.into(),
        frame_type: SurfaceFrameType::Frame,
        seq: 1,
        epoch: T0,
        cadence_ms: 2_000,
        width: prepared.width,
        height: prepared.height,
        captured_at_ms: T0,
        actor: None,
        commit: None,
    };
    let event = build_surface_frame(&header, &prepared.content)
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    assert_eq!(
        validate_surface_frame_envelope(&event).expect("valid"),
        header
    );

    // The same pixels again: change-only skips, unless forced.
    let again = prepare_frame(
        &jpeg,
        Some(&prepared.hash),
        false,
        PREVIEW_FRAME_BUDGET_BYTES,
    )
    .expect("ok");
    assert!(again.is_none());
    assert!(prepare_frame(
        &jpeg,
        Some(&prepared.hash),
        true,
        PREVIEW_FRAME_BUDGET_BYTES
    )
    .expect("ok")
    .is_some());
}

#[test]
fn a_tight_budget_steps_size_down_too() {
    let jpeg = noisy_jpeg(640, 400);
    let prepared = prepare_frame(&jpeg, None, false, 16 * 1024)
        .expect("prepared")
        .expect("frame");
    assert!(prepared.content.len() <= 16 * 1024);
    assert!(prepared.width < 640, "shrunk to {}", prepared.width);
}

// ------------------------------------------------------------ announce

fn agent_target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: "S".into(),
        generation: 2,
    }
}

fn sign_and_validate(
    spec: &AnnounceSpec,
) -> beekeeper_core_pkg::session_preview::SessionPreviewAnnounce {
    let keys = Keys::generate();
    let announce = spec.to_announce(spec.viewport.unwrap_or((1280, 800)));
    let event = build_session_preview_announce(&announce)
        .expect("builder")
        .sign_with_keys(&keys)
        .expect("sign");
    let parsed = validate_session_preview_announce_envelope(&event).expect("valid");
    assert_eq!(parsed, announce);
    parsed
}

#[test]
fn announces_validate_for_person_and_agent_bindings_open_and_closed() {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");

    // Person binding: no cs-target. The page loses host, port and query.
    let mut record = PreviewRecord::new(CHANNEL);
    record.status = PreviewStatus::Ready;
    record.url = Some("http://127.0.0.1:5173/settings/profile?tab=2#top".into());
    record.title = Some("Settings\n/Users/brian/proj".into());
    record.binding = Binding::Person {
        target: agent_target(),
    };
    record.slot = Some(super::super::geometry::SlotRect {
        x: 0.0,
        y: 0.0,
        width: 1279.6,
        height: 800.2,
    });
    let mut entry = configured_entry();
    entry.facts = Some(PreviewFacts::of(&record));
    let open = entry.desired_announce(channel).expect("open");
    assert_eq!(open.target_key, None);
    assert_eq!(open.page.as_deref(), Some("local:/settings/profile"));
    assert_eq!(open.viewport, Some((1280, 800)));
    let title = open.title.clone().expect("an open announce has a title");
    assert!(!title.contains('\n'));
    assert!(!title.contains("/Users/"));
    let parsed = sign_and_validate(&open);
    assert_eq!(parsed.status, AnnounceStatus::Open);
    assert_eq!(parsed.stream, PreviewStream::Frames);

    let closed = sign_and_validate(&open.closed());
    assert_eq!(closed.status, AnnounceStatus::Closed);
    assert_eq!((closed.page, closed.title), (None, None));

    // Agent binding: cs-target is the opening execution's target key.
    record.binding = Binding::Agent {
        target: agent_target(),
        execution_id: "exec-1".into(),
    };
    entry.facts = Some(PreviewFacts::of(&record));
    let agent = entry.desired_announce(channel).expect("open");
    assert_eq!(
        agent.target_key.as_deref(),
        Some("coding-session/v1|6:claude6:inst-11:S1:2")
    );
    assert_eq!(sign_and_validate(&agent).stream, PreviewStream::Frames);
    // Share off announces nothing at all — not even a snapshots-only open
    // carrying the page and title.
    entry.config.share = false;
    assert!(entry.desired_announce(channel).is_none());
    entry.config.share = true;

    // A public page keeps its host but never its port or query.
    record.url = Some("https://example.com:8443/docs?q=1".into());
    entry.facts = Some(PreviewFacts::of(&record));
    let public = entry.desired_announce(channel).expect("open");
    assert_eq!(public.page.as_deref(), Some("https://example.com/docs"));
    sign_and_validate(&public);

    // An unparseable page becomes local:/ rather than leaking anything.
    record.url = Some("about:blank".into());
    entry.facts = Some(PreviewFacts::of(&record));
    assert_eq!(
        entry
            .desired_announce(channel)
            .expect("open")
            .page
            .as_deref(),
        Some("local:/")
    );

    // No sessionRef, or nothing open → nothing to announce.
    record.status = PreviewStatus::Absent;
    entry.facts = Some(PreviewFacts::of(&record));
    assert!(entry.desired_announce(channel).is_none());
    entry.config.session_ref = None;
    entry.facts = Some(ready_facts());
    assert!(entry.desired_announce(channel).is_none());
}

#[test]
fn long_titles_are_cut_to_two_hundred_bytes_on_a_character_boundary() {
    let title = clean_title(&"é".repeat(150));
    assert!(title.len() <= 200);
    assert!(title.chars().all(|c| c == 'é'));
    assert_eq!(clean_title("  a\tb  "), "a b");
}

fn spec(status: AnnounceStatus, title: &str) -> AnnounceSpec {
    AnnounceSpec {
        channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
        session_ref: SESSION_REF.into(),
        status,
        target_key: None,
        provider: None,
        page: (status == AnnounceStatus::Open).then(|| "local:/".into()),
        title: (status == AnnounceStatus::Open).then(|| title.into()),
        viewport: Some((1280, 800)),
        stream: PreviewStream::Frames,
    }
}

#[test]
fn announces_are_debounced_latest_wins_and_a_close_is_never_lost() {
    let mut debounce = AnnounceDebounce::default();
    // Nothing open and never announced: nothing to say.
    assert_eq!(debounce.offer(None, T0), None);

    // First open goes at once.
    let first = debounce.offer(Some(spec(AnnounceStatus::Open, "A")), T0);
    assert_eq!(first, Some(spec(AnnounceStatus::Open, "A")));
    // Same announcement again (only the viewport differs): nothing.
    let mut resized = spec(AnnounceStatus::Open, "A");
    resized.viewport = Some((900, 700));
    assert_eq!(debounce.offer(Some(resized), T0 + 100), None);
    assert_eq!(debounce.pending, None);

    // Two title changes inside the window coalesce to the latest.
    assert_eq!(
        debounce.offer(Some(spec(AnnounceStatus::Open, "B")), T0 + 1_000),
        None
    );
    assert_eq!(
        debounce.offer(Some(spec(AnnounceStatus::Open, "C")), T0 + 2_000),
        None
    );
    assert_eq!(debounce.due(T0 + 4_999), None);
    assert_eq!(
        debounce.due(T0 + 5_000),
        Some(spec(AnnounceStatus::Open, "C"))
    );
    assert_eq!(debounce.due(T0 + 20_000), None);

    // Close inside the window: held, then published — not lost.
    assert_eq!(debounce.offer(None, T0 + 6_000), None);
    let closed = debounce.due(T0 + 10_000).expect("the close goes out");
    assert_eq!(closed.status, AnnounceStatus::Closed);
    assert_eq!((closed.page, closed.title), (None, None));

    // Closed already: closing again says nothing.
    assert_eq!(debounce.offer(None, T0 + 20_000), None);

    // Open then close then open inside one window: the relay already says
    // open after the reopen, so the coalesced result is nothing new… unless
    // the open changed.
    assert!(debounce
        .offer(Some(spec(AnnounceStatus::Open, "C")), T0 + 30_000)
        .is_some());
    assert_eq!(debounce.offer(None, T0 + 31_000), None);
    assert_eq!(
        debounce.offer(Some(spec(AnnounceStatus::Open, "C")), T0 + 32_000),
        None
    );
    assert_eq!(debounce.due(T0 + 40_000), None);
}

#[test]
fn a_failed_close_is_retried_not_dropped() {
    let mut debounce = AnnounceDebounce::default();
    debounce.offer(Some(spec(AnnounceStatus::Open, "A")), T0);
    let closed = debounce.offer(None, T0 + 6_000).expect("close at once");
    debounce.failed(closed);
    // The preview stays closed; the next offer must not forget the close.
    assert_eq!(debounce.offer(None, T0 + 7_000), None);
    let retried = debounce.due(T0 + 11_000).expect("retried");
    assert_eq!(retried.status, AnnounceStatus::Closed);
}

#[test]
fn share_state_names_why_sharing_is_limited() {
    let mut entry = ShareEntry::new(T0);
    entry.facts = Some(ready_facts());
    let state = entry.share_state(CHANNEL, T0);
    assert!(state.share, "share defaults on");
    assert_eq!(state.announced, "none");
    assert_eq!(state.stream, "frames");
    assert_eq!(state.cadence_ms, 2_000);
    let code = state.unavailable.map(|u| u.code);
    if cfg!(target_os = "macos") {
        assert_eq!(code, Some("no_session_ref"));
        entry.config.session_ref = Some(SESSION_REF.into());
        assert_eq!(
            entry.share_state(CHANNEL, T0).unavailable.map(|u| u.code),
            Some("no_provider")
        );
        entry.config.provider = Some(Keys::generate().public_key());
        assert_eq!(entry.share_state(CHANNEL, T0).unavailable, None);
    } else {
        assert_eq!(code, Some("not_macos"));
    }
    entry.config.share = false;
    assert_eq!(entry.share_state(CHANNEL, T0).stream, "none");
    let json = serde_json::to_value(entry.share_state(CHANNEL, T0)).expect("json");
    for key in [
        "channelId",
        "sessionRef",
        "share",
        "announced",
        "stream",
        "watchers",
        "cadenceMs",
        "framesLastMinute",
        "lastFrameAt",
        "unavailable",
    ] {
        assert!(json.get(key).is_some(), "missing {key}");
    }
}

/// The tag names of an announce as it would travel.
fn wire_tag_names(spec: &AnnounceSpec) -> Vec<String> {
    spec.to_announce(spec.viewport.unwrap_or((1280, 800)))
        .tags()
        .into_iter()
        .map(|row| row[0].clone())
        .collect()
}

#[test]
fn share_off_never_announces_open_and_says_nothing_about_the_page() {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let mut entry = configured_entry();
    entry.config.share = false;
    entry.facts = Some(ready_facts());
    // Off from the start: nothing to announce, before or after navigating.
    assert!(entry.desired_announce(channel).is_none());
    assert_eq!(
        entry.debounce.offer(entry.desired_announce(channel), T0),
        None
    );
    entry.facts = Some(PreviewFacts {
        url: Some("http://localhost:5173/billing".into()),
        title: Some("Billing".into()),
        ..ready_facts()
    });
    assert_eq!(
        entry
            .debounce
            .offer(entry.desired_announce(channel), T0 + 10_000),
        None
    );
    assert_eq!(entry.debounce.due(T0 + 20_000), None);
}

#[test]
fn turning_share_off_after_on_publishes_exactly_one_close_without_page_or_title() {
    let channel = Uuid::parse_str(CHANNEL).expect("uuid");
    let mut entry = configured_entry();
    entry.facts = Some(ready_facts());
    let open = entry
        .debounce
        .offer(entry.desired_announce(channel), T0)
        .expect("share on announces open");
    assert_eq!(open.status, AnnounceStatus::Open);
    assert!(wire_tag_names(&open).contains(&"page".to_string()));

    entry.config.share = false;
    let closed = entry
        .debounce
        .offer(entry.desired_announce(channel), T0 + 6_000)
        .expect("share off sends one close");
    assert_eq!(closed.status, AnnounceStatus::Closed);
    let names = wire_tag_names(&closed);
    assert!(
        !names.iter().any(|name| name == "page" || name == "title"),
        "a close leaked the page: {names:?}"
    );
    sign_and_validate(&closed);

    // Navigating while off publishes nothing more.
    entry.facts = Some(PreviewFacts {
        url: Some("http://localhost:5173/billing".into()),
        title: Some("Billing".into()),
        ..ready_facts()
    });
    for at in [T0 + 12_000, T0 + 20_000, T0 + 40_000] {
        assert_eq!(
            entry.debounce.offer(entry.desired_announce(channel), at),
            None
        );
        assert_eq!(entry.debounce.due(at), None);
    }
}

#[test]
fn share_off_sends_no_frames_and_takes_no_pictures_even_with_watchers() {
    let mut entry = configured_entry();
    entry
        .apply_watch(&watcher(1), WatchAction::Watch, T0)
        .expect("watch");
    entry.plan_tick(ready_facts(), T0);
    entry.finish_capture(Some(&frame(1)), T0 + 10);

    entry.config.share = false;
    for at in [T0 + 2_000, T0 + 4_000, T0 + 6_000] {
        let actions = entry.plan_tick(ready_facts(), at);
        assert!(
            !actions
                .iter()
                .any(|a| matches!(a, TickAction::Frame(_) | TickAction::Capture { .. })),
            "share off still streams: {actions:?}"
        );
    }
    // A capture already in flight when sharing went off sends nothing.
    assert_eq!(entry.finish_capture(Some(&frame(2)), T0 + 6_010), None);
}

#[test]
fn share_off_refuses_snapshots_before_any_picture_is_taken() {
    let mut entry = configured_entry();
    assert_eq!(capture::snapshot_refusal(&entry.config), None);
    entry.config.share = false;
    assert_eq!(
        capture::snapshot_refusal(&entry.config),
        Some(SHARE_OFF_SENTENCE)
    );
}
