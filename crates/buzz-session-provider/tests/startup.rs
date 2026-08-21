use std::process::Command;

use buzz_session_provider::state::acquire_state_dir_lock;
use nostr::ToBech32;

#[test]
fn startup_selects_a_rustls_crypto_provider() {
    let temp = tempfile::tempdir().expect("temporary state directory");
    let secret = nostr::Keys::generate()
        .secret_key()
        .to_bech32()
        .expect("test nsec");

    let output = Command::new(env!("CARGO_BIN_EXE_buzz-session-provider"))
        .env("BUZZ_PRIVATE_KEY", secret)
        .env("BUZZ_RELAY_URL", "wss://127.0.0.1:9")
        .env("BUZZ_CSP_STATE_DIR", temp.path())
        .env("BUZZ_CSP_DEFAULT_MODEL", "default")
        .output()
        .expect("run provider");

    assert!(
        !output.status.success(),
        "the test relay must be unreachable"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Could not automatically determine the process-level CryptoProvider"),
        "rustls provider selection regressed: {stderr}"
    );
}

/// A second provider pointed at an owned state directory must fail fast with a
/// clear message and a nonzero exit — before it touches the relay, the command
/// ledger, or the outbox.
#[test]
fn a_second_instance_on_the_same_state_dir_fails_fast() {
    let temp = tempfile::tempdir().expect("temporary state directory");
    let _held = acquire_state_dir_lock(temp.path()).expect("hold the state dir lock");
    let secret = nostr::Keys::generate()
        .secret_key()
        .to_bech32()
        .expect("test nsec");

    let output = Command::new(env!("CARGO_BIN_EXE_buzz-session-provider"))
        .env("BUZZ_PRIVATE_KEY", secret)
        .env("BUZZ_RELAY_URL", "wss://127.0.0.1:9")
        .env("BUZZ_CSP_STATE_DIR", temp.path())
        .env("BUZZ_CSP_DEFAULT_MODEL", "default")
        .output()
        .expect("run provider");

    assert!(
        !output.status.success(),
        "a second instance must exit nonzero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("already owns") || stdout.contains("already owns"),
        "the refusal must name the conflict, got stderr: {stderr} stdout: {stdout}"
    );
}
