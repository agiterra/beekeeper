use std::sync::atomic::Ordering;
use std::sync::Arc;

use super::super::test_support::{ok, sized_image, FakeRunner, IPHONE17_UDID};
use super::*;

#[test]
fn watchers_expire_at_45_seconds_and_stop_leaves_at_once() {
    let mut registry = WatcherRegistry::default();
    registry.touch("a", 1_000);
    registry.touch("b", 20_000);
    assert_eq!(registry.live(45_999), 2);
    assert_eq!(
        registry.live(46_000),
        1,
        "a expires 45 s after its last keepalive"
    );
    registry.stop("b");
    assert_eq!(registry.live(46_001), 0);
}

#[test]
fn the_gate_is_change_only_cadenced_and_budgeted_per_minute() {
    let mut gate = CaptureGate::new(3_000);
    let h = |n: u8| [n; 32];
    assert_eq!(gate.admit(0, h(1)), GateDecision::Send);
    assert_eq!(
        gate.admit(3_000, h(1)),
        GateDecision::Unchanged,
        "a static screen sends nothing"
    );
    assert_eq!(
        gate.admit(1_000, h(2)),
        GateDecision::Throttled,
        "faster than the cadence"
    );
    assert_eq!(gate.admit(3_000, h(2)), GateDecision::Send);
    gate.resync();
    assert_eq!(
        gate.admit(6_000, h(2)),
        GateDecision::Send,
        "resync sends an unchanged frame"
    );
    // A 500 ms cadence still cannot exceed 20 per minute.
    let mut fast = CaptureGate::new(500);
    let sent = (0..60u64)
        .filter(|i| fast.admit(i * 500, [(*i % 250) as u8 + 1; 32]) == GateDecision::Send)
        .count();
    assert_eq!(sent, SURFACE_FRAME_MAX_PER_MIN as usize);
    assert_eq!(
        fast.admit(60_000, [251; 32]),
        GateDecision::Send,
        "the minute window slides"
    );
}

#[test]
fn a_full_size_iphone_capture_fits_the_frame_budget() {
    // 1320×2868 is the spec's named worst case (iPhone Pro Max class);
    // noise is the worst case for JPEG.
    let mut seed: u32 = 7;
    let noise = image::RgbImage::from_fn(1320, 2868, |_, _| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        image::Rgb([(seed >> 16) as u8, (seed >> 8) as u8, seed as u8])
    });
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgb8(noise)
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .expect("encode");
    let (_, rgb) = prepare_frame(&jpeg, DEVICE_FRAME_MAX_LONG_EDGE).expect("prepare");
    assert_eq!(rgb.height(), DEVICE_FRAME_MAX_LONG_EDGE);
    let frame = encode_frame(&rgb, DEVICE_FRAME_BUDGET_BYTES).expect("fits");
    assert!(
        frame.base64.len() <= DEVICE_FRAME_BUDGET_BYTES,
        "{}",
        frame.base64.len()
    );
    assert!(
        frame.base64.starts_with("/9j/"),
        "a JPEG in standard base64"
    );
    assert!(frame.width <= 900 && frame.height <= 900);
}

#[test]
fn identical_pixels_hash_identically_and_different_ones_do_not() {
    let a = sized_image(image::ImageFormat::Png, 1200, 2600, 1);
    let b = sized_image(image::ImageFormat::Png, 1200, 2600, 2);
    let (ha, _) = prepare_frame(&a, 900).expect("a");
    let (ha2, _) = prepare_frame(&a, 900).expect("a again");
    let (hb, _) = prepare_frame(&b, 900).expect("b");
    assert_eq!(ha, ha2);
    assert_ne!(ha, hb);
}

async fn collect_until_ended(rx: &mut mpsc::Receiver<CaptureOutput>) -> Vec<CaptureOutput> {
    let mut out = Vec::new();
    while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
        let ended = matches!(item, CaptureOutput::Ended { .. });
        out.push(item);
        if ended {
            break;
        }
    }
    out
}

fn target() -> CaptureTarget {
    CaptureTarget {
        channel: "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f".into(),
        slot: "0123456789abcdef".into(),
        udid: IPHONE17_UDID.into(),
    }
}

#[tokio::test]
async fn capture_sends_one_frame_for_a_static_screen_then_pauses_when_the_watcher_expires() {
    let runner = Arc::new(FakeRunner::xcode27());
    let handle = CaptureHandle::default();
    // A watcher heard 44.5 s ago: live now, expired within a second.
    handle
        .watchers
        .lock()
        .expect("lock")
        .touch("w", now_ms() - 44_500);
    let (tx, mut rx) = mpsc::channel(16);
    let task = spawn_capture(
        Simctl::new(runner.clone()),
        target(),
        handle,
        tx,
        Duration::from_millis(100),
    );
    let out = collect_until_ended(&mut rx).await;
    task.await.expect("task");
    let frames: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            CaptureOutput::Frame(header, kind, content) => {
                Some((header.clone(), *kind, content.clone()))
            }
            CaptureOutput::Ended { .. } => None,
        })
        .collect();
    assert_eq!(
        frames.iter().filter(|f| f.1 == FrameType::Frame).count(),
        1,
        "unchanged screens are not resent"
    );
    let (first, _, content) = &frames[0];
    assert_eq!(
        (first.seq, first.cadence_ms),
        (1, 500),
        "cadence is clamped to the wire minimum"
    );
    assert!(content.starts_with("/9j/"));
    let (last, kind, content) = frames.last().expect("last");
    assert_eq!((*kind, content.as_str()), (FrameType::Paused, ""));
    assert!(last.seq > first.seq && last.epoch == first.epoch);
    assert!(matches!(out.last(), Some(CaptureOutput::Ended { .. })));
    assert!(
        runner
            .calls()
            .iter()
            .filter(|c| c.contains("screenshot --type=jpeg"))
            .count()
            >= 2
    );
}

#[tokio::test]
async fn a_changed_screen_sends_again_and_close_ends_with_t_end() {
    let runner = Arc::new(FakeRunner::new().on(
        "xcrun",
        "screenshot --type=jpeg",
        ok(&sized_image(image::ImageFormat::Jpeg, 60, 120, 0)),
    ));
    runner.once(
        "screenshot --type=jpeg",
        ok(&sized_image(image::ImageFormat::Jpeg, 60, 120, 99)),
    );
    let handle = CaptureHandle::default();
    handle.watchers.lock().expect("lock").touch("w", now_ms());
    let (tx, mut rx) = mpsc::channel(16);
    let task = spawn_capture(
        Simctl::new(runner),
        target(),
        handle.clone(),
        tx,
        Duration::from_millis(50),
    );
    let mut frames = 0;
    while frames < 2 {
        match tokio::time::timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(CaptureOutput::Frame(_, FrameType::Frame, _))) => frames += 1,
            Ok(Some(_)) => {}
            _ => panic!("expected two frames"),
        }
    }
    handle.closed.store(true, Ordering::SeqCst);
    handle.wake.notify_one();
    let out = collect_until_ended(&mut rx).await;
    task.await.expect("task");
    assert!(out
        .iter()
        .any(|o| matches!(o, CaptureOutput::Frame(_, FrameType::End, c) if c.is_empty())));
}
