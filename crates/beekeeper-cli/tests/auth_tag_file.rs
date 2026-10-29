//! `BEEKEEPER_AUTH_TAG_FILE` through the real binary (AGI-198).
//!
//! The client re-reads the file before every request (unit-tested in
//! `client.rs`); this exercises the precedence `bee` applies at startup: the
//! file wins over `BEEKEEPER_AUTH_TAG`, an unreadable file falls back to it
//! with a warning that names the path and never the tag, and an `--auth-tag`
//! typed on the command line wins over a file merely inherited from the
//! environment. A malformed tag is refused with exit 3 before any network I/O;
//! an accepted one gets as far as the (unreachable) relay, exit 2.

use std::process::{Command, Output};

use nostr::Keys;

const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn valid_tag() -> String {
    let agent = Keys::parse(KEY).unwrap();
    let owner = Keys::parse(&"2".repeat(64)).unwrap();
    beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &agent.public_key(), "").unwrap()
}

fn bee(env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bee"));
    command
        .env("BEEKEEPER_PRIVATE_KEY", KEY)
        .env("BEEKEEPER_RELAY_URL", "http://127.0.0.1:1/")
        .env("BEEKEEPER_CONNECT_TIMEOUT_SECS", "1")
        .env_remove("BEEKEEPER_AUTH_TAG")
        .env_remove("BEEKEEPER_AUTH_TAG_FILE")
        .env_remove("BUZZ_AUTH_TAG")
        .env_remove("BUZZ_AUTH_TAG_FILE");
    for (name, value) in env {
        command.env(name, value);
    }
    command
        .args(args)
        .args(["channels", "list"])
        .output()
        .expect("bee runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_file_wins_over_the_environment_value() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth-tag.json");
    std::fs::write(&path, "garbage\n").unwrap();
    let tag = valid_tag();
    let out = bee(
        &[
            ("BEEKEEPER_AUTH_TAG", &tag),
            ("BEEKEEPER_AUTH_TAG_FILE", path.to_str().unwrap()),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("BEEKEEPER_AUTH_TAG is malformed"),
        "{}",
        stderr(&out)
    );

    std::fs::write(&path, format!("{tag}\n")).unwrap();
    let out = bee(
        &[
            ("BEEKEEPER_AUTH_TAG", "garbage"),
            ("BEEKEEPER_AUTH_TAG_FILE", path.to_str().unwrap()),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

#[test]
fn an_unreadable_file_falls_back_to_the_environment_value_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("no-such-tag.json");
    let tag = valid_tag();
    let out = bee(
        &[
            ("BEEKEEPER_AUTH_TAG", &tag),
            ("BEEKEEPER_AUTH_TAG_FILE", missing.to_str().unwrap()),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("BEEKEEPER_AUTH_TAG_FILE="), "{err}");
    assert!(err.contains(missing.to_str().unwrap()), "{err}");
    assert!(err.contains("using BEEKEEPER_AUTH_TAG"), "{err}");
    let signature = &tag[tag.len() - 70..tag.len() - 2];
    assert!(
        !err.contains(signature),
        "the tag must never be logged: {err}"
    );
}

#[test]
fn an_explicit_auth_tag_flag_wins_over_an_inherited_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth-tag.json");
    std::fs::write(&path, "garbage").unwrap();
    let tag = valid_tag();
    let out = bee(
        &[("BEEKEEPER_AUTH_TAG_FILE", path.to_str().unwrap())],
        &["--auth-tag", &tag],
    );
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));

    // ...but a file named on the command line still wins.
    let out = bee(
        &[],
        &[
            "--auth-tag",
            &tag,
            "--auth-tag-file",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
}
