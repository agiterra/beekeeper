use std::sync::Arc;

use super::super::test_support::{
    fail, no_developer_dir, ok, present_developer_dir, FakeRunner, IPHONE17_UDID, XCODE27_DEVICES,
};
use super::*;

#[test]
fn the_xcode27_fixture_parses_every_available_device_on_ios_27() {
    let devices = parse_device_list(XCODE27_DEVICES).expect("fixture parses");
    assert_eq!(devices.len(), 11);
    assert!(devices
        .iter()
        .all(|d| d.os == "iOS" && d.os_version == "27.0"));
    let iphone = devices
        .iter()
        .find(|d| d.name == "iPhone 17")
        .expect("iPhone 17");
    assert_eq!(iphone.udid, IPHONE17_UDID);
    assert_eq!(iphone.os_label(), "iOS 27.0");
    assert!(!iphone.is_booted());
}

#[test]
fn runtime_ids_read_os_and_version() {
    assert_eq!(
        parse_runtime_id("com.apple.CoreSimulator.SimRuntime.iOS-27-0"),
        Some(("iOS".into(), "27.0".into()))
    );
    assert_eq!(
        parse_runtime_id("com.apple.CoreSimulator.SimRuntime.watchOS-12-1"),
        Some(("watchOS".into(), "12.1".into()))
    );
    assert_eq!(parse_runtime_id("garbage"), None);
}

#[test]
fn unavailable_devices_and_unreadable_runtimes_are_dropped_and_newest_runtime_leads() {
    let json = br#"{"devices":{
        "com.apple.CoreSimulator.SimRuntime.iOS-26-2":[{"udid":"A","name":"iPhone 17","state":"Shutdown","isAvailable":true}],
        "com.apple.CoreSimulator.SimRuntime.iOS-27-0":[
            {"udid":"B","name":"iPhone 17","state":"Shutdown","isAvailable":true},
            {"udid":"C","name":"iPhone Old","state":"Shutdown","isAvailable":false}],
        "nonsense":[{"udid":"D","name":"x","state":"Shutdown"}]}}"#;
    let devices = parse_device_list(json).expect("parses");
    let udids: Vec<&str> = devices.iter().map(|d| d.udid.as_str()).collect();
    assert_eq!(udids, ["B", "A"]);
    assert_eq!(
        choose_device(&devices, Some("iphone 17")).map(|d| d.udid.as_str()),
        Some("B")
    );
}

#[test]
fn choosing_prefers_booted_then_the_default_model_then_any_iphone() {
    let mut devices = parse_device_list(XCODE27_DEVICES).expect("fixture");
    assert_eq!(
        choose_device(&devices, None).map(|d| d.name.as_str()),
        Some("iPhone 17")
    );
    if let Some(air) = devices.iter_mut().find(|d| d.name == "iPhone Air") {
        air.state = "Booted".into();
    }
    assert_eq!(
        choose_device(&devices, None).map(|d| d.name.as_str()),
        Some("iPhone Air")
    );
    assert_eq!(
        choose_device(&devices, Some("iPhone 18 Pro")).map(|d| d.name.as_str()),
        Some("iPhone 18 Pro")
    );
    assert!(choose_device(&devices, Some("Pixel 9")).is_none());
    devices.retain(|d| d.name != "iPhone 17" && !d.is_booted());
    assert!(choose_device(&devices, None).is_some_and(|d| d.name.starts_with("iPhone")));
}

#[test]
fn a_machine_without_xcode_probes_missing_with_a_reason() {
    let simctl = Simctl::new(Arc::new(FakeRunner::new()));
    let XcodeProbe::Missing(reason) = simctl.probe() else {
        panic!("no xcrun must be missing");
    };
    assert!(reason.starts_with("Xcode not found"), "{reason}");
    let runner = FakeRunner::new().on(
        "xcrun",
        "--find simctl",
        fail(
            72,
            "xcrun: error: unable to find utility \"simctl\", not a developer tool or in PATH",
        ),
    );
    let simctl = Simctl::with_developer_dirs(Arc::new(runner), present_developer_dir());
    let XcodeProbe::Missing(reason) = simctl.probe() else {
        panic!("a failed find must be missing");
    };
    assert!(reason.contains("unable to find utility"), "{reason}");
}

#[test]
fn boot_uses_bootstatus_and_show_falls_back_to_device_hub() {
    let runner = Arc::new(
        FakeRunner::new()
            .on("xcrun", "bootstatus", ok(b""))
            .on(
                "open",
                "-a Simulator",
                fail(1, "Unable to find application named 'Simulator'"),
            )
            .on("open", "-a DeviceHub", ok(b"")),
    );
    let simctl = Simctl::new(runner.clone());
    simctl.boot(IPHONE17_UDID).expect("boot");
    simctl.show(IPHONE17_UDID).expect("show falls back");
    let calls = runner.calls();
    assert_eq!(
        calls[0],
        format!("xcrun simctl bootstatus {IPHONE17_UDID} -b")
    );
    assert!(calls.iter().any(|c| c == "open -a DeviceHub"));
}

#[test]
fn shutdown_of_a_shut_down_device_is_not_an_error() {
    let runner = FakeRunner::new().on(
        "xcrun",
        "shutdown",
        fail(149, "Unable to shutdown device in current state: Shutdown"),
    );
    Simctl::new(Arc::new(runner))
        .shutdown(IPHONE17_UDID)
        .expect("tolerated");
}

#[test]
fn screenshots_go_to_stdout_with_the_requested_type() {
    let runner = Arc::new(FakeRunner::xcode27());
    let simctl = Simctl::new(runner.clone());
    let bytes = simctl
        .screenshot(IPHONE17_UDID, ShotFormat::Jpeg)
        .expect("shot");
    assert!(image::load_from_memory(&bytes).is_ok());
    assert!(runner.calls().contains(&format!(
        "xcrun simctl io {IPHONE17_UDID} screenshot --type=jpeg -"
    )));
}

#[test]
fn failure_reasons_never_carry_a_udid_or_a_home_path() {
    let runner = FakeRunner::new().on(
        "xcrun",
        "bootstatus",
        fail(
            164,
            "Invalid device: 70E4C638-DE97-4AB1-990D-D0FC30018372 at /Users/brian/Library/Developer/CoreSimulator",
        ),
    );
    let error = Simctl::new(Arc::new(runner))
        .boot(IPHONE17_UDID)
        .expect_err("fails");
    assert!(!error.contains(IPHONE17_UDID), "{error}");
    assert!(!error.contains("/Users/"), "{error}");
}

#[cfg(target_os = "macos")]
#[test]
fn no_developer_dir_means_xcrun_is_never_run() {
    // Apple's /usr/bin/xcrun shim pops an install dialog on a Mac without
    // Xcode or the Command Line Tools: the probe must not reach it.
    let runner = Arc::new(
        FakeRunner::new()
            .on(
                "xcode-select",
                "-p",
                fail(
                    2,
                    "xcode-select: error: unable to get active developer directory",
                ),
            )
            .on("xcrun", "--find simctl", ok(b"/usr/bin/simctl\n")),
    );
    let simctl = Simctl::with_developer_dirs(runner.clone(), no_developer_dir());
    assert_eq!(
        simctl.probe(),
        XcodeProbe::Missing("Xcode not found".into())
    );
    let calls = runner.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("xcrun") || c.contains("xcodebuild")),
        "xcrun ran with no developer directory: {calls:?}"
    );

    // xcode-select naming a directory that is gone counts as none too.
    let runner = Arc::new(
        FakeRunner::new()
            .on(
                "xcode-select",
                "-p",
                ok(b"/nonexistent/Xcode.app/Contents/Developer\n"),
            )
            .on("xcrun", "--find simctl", ok(b"/usr/bin/simctl\n")),
    );
    let simctl = Simctl::with_developer_dirs(runner.clone(), no_developer_dir());
    assert_eq!(
        simctl.probe(),
        XcodeProbe::Missing("Xcode not found".into())
    );
    assert!(!runner.calls().iter().any(|c| c.starts_with("xcrun")));

    // A developer directory xcode-select names: xcrun may run.
    let dir = tempfile::tempdir().expect("dev dir");
    let selected = format!("{}\n", dir.path().display());
    let runner = Arc::new(
        FakeRunner::new()
            .on("xcode-select", "-p", ok(selected.as_bytes()))
            .on("xcrun", "--find simctl", ok(b"/usr/bin/simctl\n")),
    );
    let simctl = Simctl::with_developer_dirs(runner.clone(), no_developer_dir());
    assert_eq!(simctl.probe(), XcodeProbe::Found);
    assert!(runner.calls().iter().any(|c| c == "xcrun --find simctl"));
}

#[test]
fn a_host_that_is_not_macos_is_never_offered_and_never_runs_xcrun() {
    let runner = Arc::new(FakeRunner::new());
    let simctl =
        Simctl::with_developer_dirs(runner.clone(), super::super::test_support::not_macos());
    match simctl.probe() {
        XcodeProbe::Missing(reason) => assert_eq!(reason, "the iOS Simulator needs macOS"),
        other => panic!("expected Missing, got {other:?}"),
    }
    assert!(runner.calls().is_empty(), "no command may run off macOS");
}
