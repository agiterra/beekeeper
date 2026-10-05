//! SV-56 / D9: the session-title mode file.

use super::*;

#[test]
fn every_mode_round_trips_through_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    for mode in [
        SessionTitleMode::Agent,
        SessionTitleMode::MyModel,
        SessionTitleMode::Off,
    ] {
        write(dir.path(), mode).expect("write");
        assert_eq!(read(dir.path()), Ok(mode));
        let text = std::fs::read_to_string(dir.path().join(SESSION_TITLE_MODE_FILE)).expect("read");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value["version"], 1);
        assert_eq!(value["mode"], mode.as_str());
    }
    assert!(
        !dir.path()
            .join(format!("{SESSION_TITLE_MODE_FILE}.tmp"))
            .exists(),
        "the staged file is renamed away"
    );
}

#[cfg(unix)]
#[test]
fn the_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), SessionTitleMode::Off).expect("write");
    let meta = std::fs::metadata(dir.path().join(SESSION_TITLE_MODE_FILE)).expect("meta");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
}

#[test]
fn an_absent_file_is_the_agent_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(read(dir.path()), Ok(SessionTitleMode::Agent));
    assert_eq!(SessionTitleMode::default(), SessionTitleMode::Agent);
}

#[test]
fn only_agent_lets_the_provider_generate() {
    assert!(SessionTitleMode::Agent.provider_generates());
    assert!(!SessionTitleMode::MyModel.provider_generates());
    assert!(!SessionTitleMode::Off.provider_generates());
}

#[test]
fn a_malformed_file_is_an_error_not_a_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join(SESSION_TITLE_MODE_FILE);
    for bad in [
        "not json",
        "",
        r#"{"mode":"agent"}"#,
        r#"{"version":2,"mode":"agent"}"#,
        r#"{"version":"1","mode":"agent"}"#,
        r#"{"version":1,"mode":"sometimes"}"#,
        r#"{"version":1,"mode":"Agent"}"#,
        r#"{"version":1}"#,
        r#"[1,"agent"]"#,
    ] {
        std::fs::write(&file, bad).expect("write");
        assert!(read(dir.path()).is_err(), "{bad:?} must be an error");
    }
}

#[test]
fn an_unreadable_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A directory where the file should be cannot be read as text.
    std::fs::create_dir(dir.path().join(SESSION_TITLE_MODE_FILE)).expect("mkdir");
    assert!(read(dir.path()).is_err());
}

#[test]
fn admission_puts_the_host_switch_above_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(admission(true, dir.path()), Admission::Generate);
    assert_eq!(
        admission(false, dir.path()),
        Admission::HostOff,
        "env off wins over an absent file"
    );
    write(dir.path(), SessionTitleMode::Agent).expect("write");
    assert_eq!(admission(true, dir.path()), Admission::Generate);
    assert_eq!(
        admission(false, dir.path()),
        Admission::HostOff,
        "env off wins over a file saying agent"
    );
    write(dir.path(), SessionTitleMode::Off).expect("write");
    assert_eq!(
        admission(true, dir.path()),
        Admission::Declined(SessionTitleMode::Off)
    );
    write(dir.path(), SessionTitleMode::MyModel).expect("write");
    assert_eq!(
        admission(true, dir.path()),
        Admission::Declined(SessionTitleMode::MyModel)
    );
    std::fs::write(dir.path().join(SESSION_TITLE_MODE_FILE), "{").expect("write");
    assert!(matches!(
        admission(true, dir.path()),
        Admission::Unreadable(_)
    ));
}
