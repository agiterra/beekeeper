use buzz_session_provider_pkg::session_title_mode::{read, SESSION_TITLE_MODE_FILE};

use super::*;

#[test]
fn the_effective_mode_preserves_an_existing_opt_in() {
    use CodingSessionNamingProvider as P;
    use CodingSessionTitleMode as M;
    assert_eq!(effective_title_mode(None, P::Off), M::Agent);
    assert_eq!(effective_title_mode(None, P::Anthropic), M::MyModel);
    assert_eq!(effective_title_mode(None, P::OpenAiCompatible), M::MyModel);
    for mode in [M::Agent, M::MyModel, M::Off] {
        assert_eq!(effective_title_mode(Some(mode), P::Anthropic), mode);
        assert_eq!(effective_title_mode(Some(mode), P::Off), mode);
    }
}

#[test]
fn only_my_model_consults_the_naming_model() {
    assert!(require_naming_model(CodingSessionTitleMode::MyModel).is_ok());
    assert!(require_naming_model(CodingSessionTitleMode::Agent).is_err());
    assert!(require_naming_model(CodingSessionTitleMode::Off).is_err());
}

#[test]
fn my_model_without_an_endpoint_is_refused() {
    assert!(validate_mode_with_provider(
        CodingSessionTitleMode::MyModel,
        CodingSessionNamingProvider::Off
    )
    .is_err());
    assert!(validate_mode_with_provider(
        CodingSessionTitleMode::Off,
        CodingSessionNamingProvider::Off
    )
    .is_ok());
    assert!(validate_mode_with_provider(
        CodingSessionTitleMode::Agent,
        CodingSessionNamingProvider::Off
    )
    .is_ok());
}

#[test]
fn what_the_desktop_writes_the_provider_reads() {
    let root = tempfile::tempdir().expect("tempdir");
    let dirs = vec![root.path().join("p1"), root.path().join("p2")];
    for dir in &dirs {
        std::fs::create_dir_all(dir).expect("dir");
    }
    for mode in [
        CodingSessionTitleMode::Off,
        CodingSessionTitleMode::MyModel,
        CodingSessionTitleMode::Agent,
    ] {
        write_title_mode_to_dirs(&dirs, mode).expect("write");
        for dir in &dirs {
            assert_eq!(read(dir).expect("read"), mode);
        }
    }
}

#[test]
fn a_failed_write_is_reported_and_the_others_still_land() {
    let root = tempfile::tempdir().expect("tempdir");
    let good = root.path().join("good");
    std::fs::create_dir_all(&good).expect("dir");
    // A plain file where a state directory should be cannot hold the mode.
    let bad = root.path().join("bad");
    std::fs::write(&bad, b"not a directory").expect("file");
    let error = write_title_mode_to_dirs(&[bad, good.clone()], CodingSessionTitleMode::Off)
        .expect_err("reported");
    assert!(error.contains("agent host was not told"), "{error}");
    assert_eq!(read(&good).expect("read"), CodingSessionTitleMode::Off);
    assert!(good.join(SESSION_TITLE_MODE_FILE).exists());
}

#[test]
fn no_identity_yet_is_not_an_error() {
    write_title_mode_to_dirs(&[], CodingSessionTitleMode::Off).expect("nothing to write");
}

#[test]
fn host_files_that_agree_are_no_mismatch() {
    let root = tempfile::tempdir().expect("tempdir");
    let dirs = vec![root.path().join("p1"), root.path().join("p2")];
    for dir in &dirs {
        std::fs::create_dir_all(dir).expect("dir");
    }
    // No file at all reads as the default, exactly as the provider reads it.
    assert_eq!(
        host_mode_mismatch_in_dirs(&dirs, CodingSessionTitleMode::Agent),
        None
    );
    write_title_mode_to_dirs(&dirs, CodingSessionTitleMode::Off).expect("write");
    assert_eq!(
        host_mode_mismatch_in_dirs(&dirs, CodingSessionTitleMode::Off),
        None
    );
    assert_eq!(
        host_mode_mismatch_in_dirs(&[], CodingSessionTitleMode::Off),
        None
    );
}

#[test]
fn a_host_left_on_another_mode_is_a_durable_mismatch() {
    // The record says Off, but one identity's file was never rewritten (the
    // save's publish failed): every later settings read must say so.
    let root = tempfile::tempdir().expect("tempdir");
    let told = root.path().join("told");
    let stale = root.path().join("stale");
    std::fs::create_dir_all(&told).expect("dir");
    std::fs::create_dir_all(&stale).expect("dir");
    write_title_mode_to_dirs(
        &[told.clone(), stale.clone()],
        CodingSessionTitleMode::Agent,
    )
    .expect("write");
    write_title_mode_to_dirs(std::slice::from_ref(&told), CodingSessionTitleMode::Off)
        .expect("write");
    let mismatch = host_mode_mismatch_in_dirs(&[told, stale.clone()], CodingSessionTitleMode::Off)
        .expect("mismatch");
    assert!(mismatch.contains("not on off"), "{mismatch}");
    assert!(
        mismatch.contains(&format!("{} is on agent", stale.display())),
        "{mismatch}"
    );
    assert!(!mismatch.contains("told is on"), "{mismatch}");
}

#[test]
fn an_unreadable_host_file_is_a_mismatch() {
    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("p");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join(SESSION_TITLE_MODE_FILE), b"not json").expect("file");
    let mismatch =
        host_mode_mismatch_in_dirs(&[dir], CodingSessionTitleMode::Agent).expect("mismatch");
    assert!(mismatch.contains("is not JSON"), "{mismatch}");
}

#[test]
fn provisioning_fails_closed_unless_the_file_already_holds_the_stored_mode() {
    let root = tempfile::tempdir().expect("tempdir");
    // A fresh identity (no file) would read agent: an unreadable record or a
    // failed publish leaves it on off instead.
    let fresh = root.path().join("fresh");
    std::fs::create_dir_all(&fresh).expect("dir");
    assert_eq!(
        fail_closed_in_dir(&fresh, None).expect("fallback"),
        CodingSessionTitleMode::Off
    );
    assert_eq!(read(&fresh).expect("read"), CodingSessionTitleMode::Off);
    // The person chose Off and this identity's file was never written: it
    // would read agent, so it is set to off.
    let unwritten = root.path().join("unwritten");
    std::fs::create_dir_all(&unwritten).expect("dir");
    assert_eq!(
        fail_closed_in_dir(&unwritten, Some(CodingSessionTitleMode::Off)).expect("fallback"),
        CodingSessionTitleMode::Off
    );
    assert_eq!(read(&unwritten).expect("read"), CodingSessionTitleMode::Off);
    // A stale file on another mode is replaced by off, not left titling.
    let stale = root.path().join("stale");
    std::fs::create_dir_all(&stale).expect("dir");
    write_title_mode_to_dirs(std::slice::from_ref(&stale), CodingSessionTitleMode::Agent)
        .expect("write");
    assert_eq!(
        fail_closed_in_dir(&stale, Some(CodingSessionTitleMode::MyModel)).expect("fallback"),
        CodingSessionTitleMode::Off
    );
    // Another identity's directory failed, this one was told: leave it.
    let told = root.path().join("told");
    std::fs::create_dir_all(&told).expect("dir");
    write_title_mode_to_dirs(std::slice::from_ref(&told), CodingSessionTitleMode::Agent)
        .expect("write");
    assert_eq!(
        fail_closed_in_dir(&told, Some(CodingSessionTitleMode::Agent)).expect("kept"),
        CodingSessionTitleMode::Agent
    );
    assert_eq!(read(&told).expect("read"), CodingSessionTitleMode::Agent);
}
