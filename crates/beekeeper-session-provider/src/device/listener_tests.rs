use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use tokio::sync::mpsc;

use super::super::capture::CaptureOutput;
use super::super::simctl::Simctl;
use super::super::slot::{contains_host_path, contains_uuid_shape, DevicePaths};
use super::super::test_support::{present_developer_dir, FakeRunner, IPHONE17_UDID};
use super::super::wire::{KIND_SESSION_DEVICE_COMMAND, KIND_SURFACE_WATCH};
use super::super::work::{WorkDone, Worker, WorkerConfig};
use super::*;
use crate::artifact_upload::ArtifactUploader;

const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CSL: &str = "a3c9e4d2-1111-4222-8333-444455556666";

fn target(generation: u64) -> String {
    format!("coding-session/v1|6:claude4:inst36:{SESSION}1:{generation}")
}

struct Harness {
    engine: Engine,
    work_rx: mpsc::Receiver<WorkDone>,
    capture_rx: mpsc::Receiver<CaptureOutput>,
    provider: Keys,
    seat: Keys,
    person: Keys,
    viewer: Keys,
    runner: Arc<FakeRunner>,
    dir: tempfile::TempDir,
    published: Vec<Event>,
}

fn harness(runner: FakeRunner, uploader_url: Option<&str>, disabled: Option<&str>) -> Harness {
    let dir = tempfile::tempdir().expect("state");
    let provider = Keys::generate();
    let runner = Arc::new(runner);
    let paths = DevicePaths::new(dir.path());
    let (work_tx, work_rx) = mpsc::channel(32);
    let (capture_tx, capture_rx) = mpsc::channel(32);
    let uploader = uploader_url.and_then(|url| ArtifactUploader::new(url, provider.clone(), None));
    let worker = Worker::new(
        WorkerConfig {
            me: provider.public_key().to_hex(),
            paths: paths.clone(),
            node_candidates: Vec::new(),
            disabled: disabled.map(str::to_owned),
            developer_dirs: present_developer_dir(),
        },
        runner.clone(),
        uploader,
        work_tx,
    );
    let mut engine = Engine::new(
        provider.clone(),
        paths.clone(),
        Simctl::new(runner.clone()),
        worker,
        capture_tx,
        Journal::load(paths.journal_file()),
    );
    engine.set_cadence(Duration::from_millis(50));
    Harness {
        engine,
        work_rx,
        capture_rx,
        provider,
        seat: Keys::generate(),
        person: Keys::generate(),
        viewer: Keys::generate(),
        runner,
        dir,
        published: Vec::new(),
    }
}

impl Harness {
    fn view(&self, generation: u64) -> DeviceSessionView {
        DeviceSessionView {
            channel: CHANNEL.into(),
            session_id: SESSION.into(),
            cs_target: target(generation),
            csl_command: CSL.into(),
            seat: Some(self.seat.public_key().to_hex()),
            people: BTreeSet::from([self.person.public_key().to_hex()]),
            viewers: BTreeSet::from([self.viewer.public_key().to_hex()]),
        }
    }

    fn take(&mut self, out: Vec<Outgoing>) {
        for outgoing in out {
            match outgoing {
                Outgoing::Stored { key, event, last } => {
                    if let (Some(key), true) = (key, last) {
                        self.engine.mark_published(&key);
                    }
                    self.published.push(event);
                }
                Outgoing::Ephemeral(event) => self.published.push(event),
            }
        }
    }

    /// Feed finished work back until `done` holds or 10 s pass.
    async fn pump_until(&mut self, done: impl Fn(&[Event]) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !done(&self.published) {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::select! {
                Some(work) = self.work_rx.recv() => { let out = self.engine.on_work(work); self.take(out); }
                Some(output) = self.capture_rx.recv() => { let out = self.engine.on_capture(output); self.take(out); }
                () = tokio::time::sleep(left) => panic!("timed out; published {:#?}", self.published.iter().map(|e| (e.kind, e.content.clone())).collect::<Vec<_>>()),
            }
        }
    }

    fn command(
        &self,
        author: &Keys,
        cmd: &str,
        slot: Option<&str>,
        content: &str,
        generation: u64,
    ) -> Event {
        let target = target(generation);
        let mut tags = vec![
            Tag::parse(["h", CHANNEL]).expect("t"),
            Tag::parse(["sdv-v", "sdv1"]).expect("t"),
            Tag::parse(["cs-target", target.as_str()]).expect("t"),
            Tag::parse(["sdv-cmd", cmd]).expect("t"),
        ];
        if let Some(slot) = slot {
            tags.push(Tag::parse(["sdv-slot", slot]).expect("t"));
        }
        EventBuilder::new(Kind::from(KIND_SESSION_DEVICE_COMMAND), content)
            .tags(tags)
            .sign_with_keys(author)
            .expect("sign")
    }

    fn watch(&self, watcher: &Keys, slot: &str, action: &str) -> Event {
        let me = self.provider.public_key().to_hex();
        EventBuilder::new(
            Kind::from(KIND_SURFACE_WATCH),
            format!(r#"{{"action":"{action}"}}"#),
        )
        .tags([
            Tag::parse(["h", CHANNEL]).expect("t"),
            Tag::parse(["surface", "device"]).expect("t"),
            Tag::parse(["d", slot]).expect("t"),
            Tag::parse(["p", me.as_str()]).expect("t"),
        ])
        .sign_with_keys(watcher)
        .expect("sign")
    }

    fn records(&self, sdv_type: &str) -> Vec<&Event> {
        self.published
            .iter()
            .filter(|e| e.kind.as_u16() == 44255 && tag(e, "sdv-type") == Some(sdv_type))
            .collect()
    }

    fn content(event: &Event) -> serde_json::Value {
        serde_json::from_str(&event.content).expect("json")
    }

    async fn serve_and_probe(&mut self, generation: u64) {
        let out = self.engine.serve(vec![self.view(generation)]);
        self.take(out);
        self.pump_until(|p| p.iter().any(|e| tag(e, "sdv-type") == Some("availability")))
            .await;
    }

    async fn open(&mut self) -> String {
        let open = self.command(&self.seat.clone(), "open-1", None, r#"{"op":"open"}"#, 1);
        let out = self.engine.on_event(&open);
        self.take(out);
        self.pump_until(|p| {
            p.iter().any(|e| {
                tag(e, "sdv-cmd") == Some("open-1") && Harness::content(e)["state"] == "open"
            })
        })
        .await;
        tag(self.records("state").last().expect("state"), "sdv-slot")
            .expect("slot")
            .to_owned()
    }
}

fn tag<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event
        .tags
        .iter()
        .map(Tag::as_slice)
        .find(|t| t.first().map(String::as_str) == Some(name))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

/// Every published event, every tag except the identifier tags, and every
/// content string: no UUID-shaped value and no host path. Run at the end of
/// every scenario. Each event must also pass lane P's canonical
/// `beekeeper_core` validator for its kind — the same check the relay runs at
/// ingest — so this local wire mirror cannot drift from WIRE-C5 unnoticed.
fn assert_nothing_host_local(published: &[Event]) {
    for event in published {
        let verdict = match event.kind.as_u16() {
            44253 => beekeeper_core::surface_snapshot::validate_surface_snapshot_envelope(event)
                .map(|_| ()),
            44255 => beekeeper_core::session_device::validate_session_device_record_envelope(event)
                .map(|_| ()),
            24321 => {
                beekeeper_core::surface_watch::validate_surface_frame_envelope(event).map(|_| ())
            }
            other => Err(format!("unexpected kind {other}")),
        };
        assert_eq!(
            verdict,
            Ok(()),
            "core refuses {} {:?} {}",
            event.kind,
            event.tags,
            event.content
        );
        assert!(!event.content.contains(IPHONE17_UDID), "{event:?}");
        if event.kind.as_u16() != 24321 {
            assert!(
                !contains_uuid_shape(&event.content),
                "content {}",
                event.content
            );
        }
        assert!(
            !contains_host_path(&event.content),
            "content {}",
            event.content
        );
        for t in event.tags.iter().map(Tag::as_slice) {
            for value in t.iter().skip(1) {
                assert!(!contains_host_path(value), "{t:?}");
                if !matches!(t[0].as_str(), "h" | "cs-target" | "csl-command" | "sdv-cmd") {
                    assert!(!contains_uuid_shape(value), "{t:?}");
                }
            }
        }
    }
}

#[tokio::test]
async fn availability_without_xcode_names_the_reason_and_open_is_refused_with_it() {
    let mut h = harness(FakeRunner::new(), None, None);
    h.serve_and_probe(1).await;
    let availability = Harness::content(h.records("availability")[0]);
    assert_eq!(availability["platforms"]["ios"]["available"], false);
    assert_eq!(
        availability["platforms"]["ios"]["reason"],
        "Xcode not found"
    );
    assert_eq!(availability["agentDevice"]["installed"], false);
    assert_eq!(
        availability["agentDevice"]["reason"],
        "Node 22.12 or newer not found"
    );
    assert_eq!(tag(h.records("availability")[0], "csl-command"), Some(CSL));
    let open = h.command(&h.seat.clone(), "o", None, r#"{"op":"open"}"#, 1);
    let out = h.engine.on_event(&open);
    h.take(out);
    let refused = Harness::content(h.records("refused")[0]);
    assert_eq!(
        (refused["code"].as_str(), refused["reason"].as_str()),
        (Some("platform_unavailable"), Some("Xcode not found"))
    );
    // Availability is once per generation: serving again publishes nothing.
    let out = h.engine.serve(vec![h.view(1)]);
    assert!(out.is_empty());
    assert_nothing_host_local(&h.published);
}

#[tokio::test]
async fn a_host_that_turned_devices_off_says_so() {
    let mut h = harness(
        FakeRunner::xcode27(),
        None,
        Some("devices are turned off on this machine"),
    );
    h.serve_and_probe(1).await;
    let availability = Harness::content(h.records("availability")[0]);
    assert_eq!(
        availability["platforms"]["ios"]["reason"],
        "devices are turned off on this machine"
    );
    assert!(
        !h.runner.calls().iter().any(|c| c.contains("simctl")),
        "a disabled host never touches simctl"
    );
}

#[tokio::test]
async fn open_boots_the_default_iphone_writes_a_private_slot_and_answers_once() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let slot = h.open().await;
    assert_eq!(
        slot,
        super::super::slot::slot_id(&h.provider.public_key().to_hex(), IPHONE17_UDID)
    );
    let states: Vec<String> = h
        .records("state")
        .iter()
        .map(|e| {
            Harness::content(e)["state"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(states, ["booting", "open"]);
    let open = Harness::content(h.records("state")[1]);
    assert_eq!(
        (open["model"].as_str(), open["osVersion"].as_str()),
        (Some("iPhone 17"), Some("27.0"))
    );
    assert_eq!(
        open["drivers"],
        serde_json::json!(["host-owner"]),
        "no Node: the agent cannot drive, and the record says so"
    );
    assert!(h
        .runner
        .calls()
        .contains(&format!("xcrun simctl bootstatus {IPHONE17_UDID} -b")));
    let paths = DevicePaths::new(h.dir.path());
    let file = super::super::slot::read_slot_file(&paths, SESSION, &slot).expect("slot file");
    assert_eq!(file.udid, IPHONE17_UDID);
    // The same command replayed (every reconnect replays stored commands)
    // is not run again.
    let replay = h.command(&h.seat.clone(), "open-1", None, r#"{"op":"open"}"#, 1);
    assert!(h.engine.on_event(&replay).is_empty());
    // A second open in the session reuses the device.
    let again = h.command(&h.person.clone(), "open-2", None, r#"{"op":"open"}"#, 1);
    let out = h.engine.on_event(&again);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("state").last().expect("s"))["state"],
        "open"
    );
    let terminal = h
        .published
        .iter()
        .filter(|e| {
            tag(e, "sdv-cmd") == Some("open-1") && Harness::content(e)["state"] != "booting"
        })
        .count();
    assert_eq!(terminal, 1, "exactly one terminal record per command");
    assert_nothing_host_local(&h.published);
}

#[tokio::test]
async fn standing_is_the_seat_or_a_person_and_foreign_generations_are_ignored() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let stranger = Keys::generate();
    let open = h.command(&stranger, "x", None, r#"{"op":"open"}"#, 1);
    let out = h.engine.on_event(&open);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("refused")[0])["code"],
        "no_standing"
    );
    let viewer_open = h.command(&h.viewer.clone(), "v", None, r#"{"op":"open"}"#, 1);
    let out = h.engine.on_event(&viewer_open);
    h.take(out);
    assert_eq!(h.records("refused").len(), 2, "a viewer may not drive");
    let before = h.published.len();
    let foreign = h.command(&h.seat.clone(), "f", None, r#"{"op":"open"}"#, 2);
    assert!(
        h.engine.on_event(&foreign).is_empty(),
        "another generation is another provider's"
    );
    assert_eq!(h.published.len(), before);
    let action = h.command(
        &h.seat.clone(),
        "a",
        Some("0123456789abcdef"),
        r#"{"op":"action","action":{"type":"tap"}}"#,
        1,
    );
    let out = h.engine.on_event(&action);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("refused").last().expect("r"))["code"],
        "invalid_command"
    );
}

#[tokio::test]
async fn stale_commands_are_answered_not_run() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let old = EventBuilder::new(Kind::from(KIND_SESSION_DEVICE_COMMAND), r#"{"op":"open"}"#)
        .tags([
            Tag::parse(["h", CHANNEL]).expect("t"),
            Tag::parse(["sdv-v", "sdv1"]).expect("t"),
            Tag::parse(["cs-target", target(1).as_str()]).expect("t"),
            Tag::parse(["sdv-cmd", "old"]).expect("t"),
        ])
        .custom_created_at(Timestamp::from(Timestamp::now().as_secs() - 3_600))
        .sign_with_keys(&h.seat)
        .expect("sign");
    let out = h.engine.on_event(&old);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("refused")[0])["code"],
        "invalid_command"
    );
    assert!(!h.runner.calls().iter().any(|c| c.contains("bootstatus")));
}

async fn fake_blossom() -> String {
    use axum::routing::put;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let app = axum::Router::new().route(
        "/upload",
        put(move |headers: axum::http::HeaderMap| async move {
            let sha = headers
                .get("x-sha-256")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            axum::Json(serde_json::json!({"url": format!("http://127.0.0.1:{port}/{sha}.png")}))
        }),
    );
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("ws://127.0.0.1:{port}")
}

#[tokio::test]
async fn screenshot_publishes_a_snapshot_then_a_shot_pointing_at_it() {
    let relay = fake_blossom().await;
    let mut h = harness(FakeRunner::xcode27(), Some(&relay), None);
    h.serve_and_probe(1).await;
    let slot = h.open().await;
    // A CLI may well mint its command ids as UUIDs: they are identifiers,
    // not host facts, and must still be answerable.
    let cmd = "0f9e8d7c-6b5a-4938-8271-605f4e3d2c1b";
    let shot = h.command(
        &h.viewer.clone(),
        cmd,
        Some(&slot),
        r#"{"op":"screenshot"}"#,
        1,
    );
    let out = h.engine.on_event(&shot);
    h.take(out);
    h.pump_until(|p| p.iter().any(|e| tag(e, "sdv-type") == Some("shot")))
        .await;
    assert_eq!(tag(h.records("shot")[0], "sdv-cmd"), Some(cmd));
    let snapshot = h
        .published
        .iter()
        .find(|e| e.kind.as_u16() == 44253)
        .expect("44253");
    let shot_record = h.records("shot")[0];
    let e_tag = shot_record
        .tags
        .iter()
        .map(Tag::as_slice)
        .find(|t| t[0] == "e")
        .expect("e");
    assert_eq!(
        (e_tag[1].as_str(), e_tag[3].as_str()),
        (snapshot.id.to_hex().as_str(), "snapshot")
    );
    assert_eq!(tag(snapshot, "d"), Some(slot.as_str()));
    assert_eq!(tag(snapshot, "m"), Some("image/png"));
    assert_eq!(
        tag(snapshot, "provider"),
        Some(h.provider.public_key().to_hex().as_str())
    );
    let requested = snapshot
        .tags
        .iter()
        .map(Tag::as_slice)
        .find(|t| t[0] == "p")
        .expect("p");
    assert_eq!(requested[1], h.viewer.public_key().to_hex());
    let sha = tag(snapshot, "x").expect("x");
    assert!(tag(snapshot, "url").is_some_and(|url| url.contains(sha)));
    let local = DevicePaths::new(h.dir.path())
        .shot_dir(SESSION, &slot)
        .join(format!("{sha}.png"));
    assert!(
        local.exists(),
        "bee device screenshot finds the PNG beside the slot file"
    );
    assert_nothing_host_local(&h.published);
}

#[tokio::test]
async fn screenshot_without_a_media_endpoint_or_an_open_device_is_refused() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let early = h.command(
        &h.seat.clone(),
        "s0",
        Some("0123456789abcdef"),
        r#"{"op":"screenshot"}"#,
        1,
    );
    let out = h.engine.on_event(&early);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("refused")[0])["code"],
        "no_device_open"
    );
    let slot = h.open().await;
    let shot = h.command(
        &h.seat.clone(),
        "s1",
        Some(&slot),
        r#"{"op":"screenshot"}"#,
        1,
    );
    let out = h.engine.on_event(&shot);
    h.take(out);
    h.pump_until(|p| p.iter().any(|e| tag(e, "sdv-cmd") == Some("s1")))
        .await;
    let refused = Harness::content(h.records("refused").last().expect("r"));
    assert_eq!(refused["code"], "capture_failed");
    assert!(refused["reason"]
        .as_str()
        .is_some_and(|r| r.contains("no media endpoint")));
}

#[tokio::test]
async fn close_unbinds_and_shutdown_runs_simctl() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let slot = h.open().await;
    let close = h.command(
        &h.seat.clone(),
        "c1",
        Some(&slot),
        r#"{"op":"close","shutdown":true}"#,
        1,
    );
    let out = h.engine.on_event(&close);
    h.take(out);
    h.pump_until(|p| p.iter().any(|e| tag(e, "sdv-cmd") == Some("c1")))
        .await;
    assert_eq!(
        Harness::content(h.records("state").last().expect("s"))["state"],
        "closed"
    );
    assert!(h
        .runner
        .calls()
        .contains(&format!("xcrun simctl shutdown {IPHONE17_UDID}")));
    let paths = DevicePaths::new(h.dir.path());
    assert!(!paths.slot_file(SESSION, &slot).exists());
    let again = h.command(&h.seat.clone(), "c2", Some(&slot), r#"{"op":"close"}"#, 1);
    let out = h.engine.on_event(&again);
    h.take(out);
    assert_eq!(
        Harness::content(h.records("refused").last().expect("r"))["code"],
        "no_device_open"
    );
}

#[tokio::test]
async fn a_watch_starts_capture_and_frames_are_signed_by_the_provider() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let slot = h.open().await;
    let watcher = Keys::generate();
    let out = h.engine.on_event(&h.watch(&watcher, &slot, "watch"));
    h.take(out);
    h.pump_until(|p| p.iter().any(|e| e.kind.as_u16() == 24321))
        .await;
    let frame = h
        .published
        .iter()
        .find(|e| e.kind.as_u16() == 24321)
        .expect("frame");
    assert_eq!(frame.pubkey, h.provider.public_key());
    assert_eq!(tag(frame, "d"), Some(slot.as_str()));
    assert_eq!(tag(frame, "t"), Some("frame"));
    // A watch for a slot this provider does not hold is ignored.
    assert!(h
        .engine
        .on_event(&h.watch(&watcher, "fedcba9876543210", "watch"))
        .is_empty());
    let out = h.engine.on_event(&h.watch(&watcher, &slot, "stop"));
    h.take(out);
    h.pump_until(|p| p.iter().any(|e| tag(e, "t") == Some("paused")))
        .await;
    assert_nothing_host_local(&h.published);
}

#[tokio::test]
async fn a_restart_answers_running_commands_and_readopts_open_devices() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    let slot = h.open().await;
    // A command that was running when the process died.
    let view = h.view(1);
    h.engine.journal.begin("cmd|x|lost", &view, Some("lost"));
    let paths = DevicePaths::new(h.dir.path());
    let (work_tx, _work_rx) = mpsc::channel(4);
    let (capture_tx, _capture_rx) = mpsc::channel(4);
    let runner: Arc<dyn super::super::simctl::CommandRunner> = h.runner.clone();
    let worker = Worker::new(
        WorkerConfig {
            me: h.provider.public_key().to_hex(),
            paths: paths.clone(),
            node_candidates: Vec::new(),
            disabled: None,
            developer_dirs: present_developer_dir(),
        },
        runner.clone(),
        None,
        work_tx,
    );
    let mut restarted = Engine::new(
        h.provider.clone(),
        paths.clone(),
        Simctl::new(runner),
        worker,
        capture_tx,
        Journal::load(paths.journal_file()),
    );
    let recovered = restarted.recover();
    let events: Vec<&Event> = recovered
        .iter()
        .filter_map(|o| match o {
            Outgoing::Stored { event, .. } => Some(event),
            Outgoing::Ephemeral(_) => None,
        })
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(tag(events[0], "sdv-cmd"), Some("lost"));
    assert_eq!(Harness::content(events[0])["code"], "busy");
    restarted.serve(vec![view]);
    assert_eq!(
        restarted.slot_of(SESSION).map(|d| d.slot.as_str()),
        Some(slot.as_str())
    );
    // Unpublished answers survive the restart for re-sending.
    assert!(!restarted.unpublished().is_empty());
}

#[tokio::test]
async fn a_generation_that_ends_unbinds_its_device_with_a_reason() {
    let mut h = harness(FakeRunner::xcode27(), None, None);
    h.serve_and_probe(1).await;
    h.open().await;
    let out = h.engine.serve(Vec::new());
    h.take(out);
    let closed = Harness::content(h.records("state").last().expect("s"));
    assert_eq!(
        (closed["state"].as_str(), closed["reason"].as_str()),
        (Some("closed"), Some("the session generation ended"))
    );
    assert!(h.engine.slot_of(SESSION).is_none());
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_mac_without_xcode_publishes_not_offered_and_never_runs_xcrun() {
    let runner: Arc<FakeRunner> = Arc::new(FakeRunner::xcode27());
    let dir = tempfile::tempdir().expect("dir");
    let (work_tx, mut work_rx) = mpsc::channel(4);
    let worker = Worker::new(
        WorkerConfig {
            me: Keys::generate().public_key().to_hex(),
            paths: DevicePaths::new(dir.path()),
            node_candidates: Vec::new(),
            disabled: None,
            developer_dirs: super::super::test_support::no_developer_dir(),
        },
        runner.clone(),
        None,
        work_tx,
    );
    worker.probe();
    let Some(WorkDone::Probed(availability)) = work_rx.recv().await else {
        panic!("the probe reports");
    };
    assert!(!availability.ios.available);
    // The Device surface reads this as "Not offered by <machine>: Xcode not found".
    assert_eq!(availability.ios.reason.as_deref(), Some("Xcode not found"));
    assert!(
        !runner.calls().iter().any(|c| c.starts_with("xcrun")),
        "xcrun ran on a Mac with no developer directory: {:?}",
        runner.calls()
    );
}
