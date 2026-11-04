//! `bee host commission --owner-key-file` through the real binary, against a
//! throwaway home and a scripted relay: only fake owner keys, no real Host, no
//! model turn. What a later client depends on is asserted here: one JSON object
//! per outcome, a stable code on refusal, nothing written before a key refusal,
//! the identity kept across retries, and the key in no output or state file.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use beekeeper_host_core::{layout, record};
use nostr::{Keys, ToBech32};
use serde_json::Value;

/// Answers every request with the current status and body, and counts them, so
/// a test can say the relay was or was not contacted.
struct Relay {
    url: String,
    answer: Arc<Mutex<(u16, String)>>,
    requests: Arc<AtomicUsize>,
}

impl Relay {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let answer = Arc::new(Mutex::new((200u16, "[]".to_owned())));
        let requests = Arc::new(AtomicUsize::new(0));
        let (reply, count) = (answer.clone(), requests.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                read_request(&mut stream);
                count.fetch_add(1, Ordering::SeqCst);
                let (status, body) = reply.lock().expect("answer").clone();
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: \
                     {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            url,
            answer,
            requests,
        }
    }

    fn answer(&self, status: u16, body: &str) {
        *self.answer.lock().expect("answer") = (status, body.to_owned());
    }

    fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

fn read_request(stream: &mut std::net::TcpStream) {
    let mut seen = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let Ok(read) = stream.read(&mut chunk) else {
            return;
        };
        if read == 0 {
            return;
        }
        seen.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&seen).to_ascii_lowercase();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                return;
            }
        }
    }
}

struct Rig {
    dir: tempfile::TempDir,
    relay: Relay,
    home: PathBuf,
    driver: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        std::fs::create_dir(&home).expect("home");
        let driver = dir.path().join("acme-acp");
        std::fs::write(&driver, "#!/bin/sh\nexit 0\n").expect("driver");
        std::fs::set_permissions(&driver, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        Self {
            dir,
            relay: Relay::start(),
            home,
            driver,
        }
    }

    fn key_file(&self, name: &str, contents: &str, mode: u32) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, contents).expect("key file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        path
    }

    /// A clean environment, no stdin: a prompt would fail here rather than hang.
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bee"));
        command
            .env_clear()
            .env("HOME", self.dir.path())
            .env("BEEKEEPER_RELAY_URL", &self.relay.url)
            .env("BEEKEEPER_CONNECT_TIMEOUT_SECS", "2")
            .stdin(Stdio::null());
        command
    }

    fn commission_args(&self) -> Vec<String> {
        vec![
            "host".into(),
            "commission".into(),
            "--home".into(),
            self.home.display().to_string(),
            "--runtime".into(),
            "acme".into(),
            "--driver".into(),
            self.driver.display().to_string(),
        ]
    }

    fn store_path(&self) -> PathBuf {
        layout::host_dir(&self.home, layout::Instance::Production)
            .join("service/session-provider")
            .join(record::STORE_FILE_NAME)
    }

    /// Damage the one recorded identity, as a bad restore or a bad edit would.
    fn edit_record(&self, change: impl Fn(&mut Value)) {
        let path = self.store_path();
        let mut store: Value =
            serde_json::from_slice(&std::fs::read(&path).expect("store")).expect("store JSON");
        for (_, entry) in store["providers"]
            .as_object_mut()
            .expect("providers")
            .iter_mut()
        {
            change(entry);
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&store).expect("encode")).expect("write");
    }

    fn with_file(&self, file: &Path, extra: &[&str]) -> Output {
        self.command()
            .args(self.commission_args())
            .arg("--owner-key-file")
            .arg(file)
            .args(extra)
            .output()
            .expect("bee runs")
    }

    /// Every file under the home, by contents, so "changed nothing" is a fact.
    fn tree(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    into.insert(path.clone(), Vec::new());
                    walk(&path, into);
                } else {
                    into.insert(path.clone(), std::fs::read(&path).expect("read"));
                }
            }
        }
        let mut tree = BTreeMap::new();
        walk(&self.home, &mut tree);
        tree
    }

    /// The tree by name and link-or-contents, for a home holding a planted link
    /// or directory where a file belongs.
    fn tree_without_following_links(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(dir: &Path, into: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
                let path = entry.path();
                let kind = entry.file_type().expect("type");
                if kind.is_dir() {
                    into.insert(path.clone(), Vec::new());
                    walk(&path, into);
                } else if kind.is_symlink() {
                    into.insert(path.clone(), b"link".to_vec());
                } else {
                    into.insert(path.clone(), std::fs::read(&path).expect("read"));
                }
            }
        }
        let mut tree = BTreeMap::new();
        walk(&self.home, &mut tree);
        tree
    }

    fn assert_absent_from_state(&self, secrets: &[String]) {
        for (path, bytes) in self.tree() {
            let text = String::from_utf8_lossy(&bytes);
            for secret in secrets {
                assert!(!text.contains(secret.as_str()), "secret in {path:?}");
            }
        }
    }
}

fn hex(keys: &Keys) -> String {
    keys.secret_key().to_secret_hex()
}

fn nsec(keys: &Keys) -> String {
    keys.secret_key().to_bech32().expect("bech32")
}

fn secrets_of(keys: &Keys) -> Vec<String> {
    vec![hex(keys), nsec(keys)]
}

fn assert_no_leak(output: &Output, secrets: &[String]) {
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    for secret in secrets {
        assert!(!stdout.contains(secret.as_str()), "stdout leaked: {stdout}");
        assert!(!stderr.contains(secret.as_str()), "stderr leaked: {stderr}");
    }
}

fn success(output: &Output) -> Value {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout is one JSON object")
}

/// Asserts the refusal shape: nothing on stdout, one JSON object on stderr.
fn refusal(output: &Output, exit: i32, category: &str, code: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(exit), "{stderr}");
    assert!(output.stdout.is_empty(), "stdout on refusal");
    let error: Value = serde_json::from_str(stderr.trim()).expect("stderr is one JSON object");
    assert_eq!(error["error"], category, "{stderr}");
    assert_eq!(error["retryable"], false, "{stderr}");
    let message = error["message"].as_str().expect("message").to_owned();
    assert!(message.contains(&format!("{code}: ")), "{message}");
    message
}

#[test]
fn a_key_file_commissions_and_success_claims_no_model_or_worker() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &format!("{}\n", nsec(&owner)), 0o600);

    let output = rig.with_file(&file, &[]);
    let body = success(&output);

    assert_eq!(body["commissioned"], true);
    assert_eq!(body["relay_admitted"], true);
    assert_eq!(body["identity"], "minted");
    assert_eq!(body["owner_pubkey"], owner.public_key().to_hex());
    assert_eq!(body["owner_key_source"], "file");
    assert_eq!(body["runtime_account"], "unconfirmed");
    assert_eq!(body["worker"], "not_started");
    assert!(body["next_steps"].as_array().is_some_and(|s| !s.is_empty()));
    assert_eq!(rig.relay.requests(), 1);
    assert_no_leak(&output, &secrets_of(&owner));
    rig.assert_absent_from_state(&secrets_of(&owner));
}

#[test]
fn a_retry_by_the_same_owner_keeps_the_identity_and_host_key() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);

    let first = success(&rig.with_file(&file, &[]));
    let stores = |tree: BTreeMap<PathBuf, Vec<u8>>| {
        tree.into_iter()
            .filter(|(path, _)| path.to_string_lossy().contains("provider-key"))
            .collect::<Vec<_>>()
    };
    let key_before = stores(rig.tree());
    assert!(!key_before.is_empty(), "the host's own key is on disk");

    let second = success(&rig.with_file(&file, &[]));

    assert_eq!(second["identity"], "retained");
    assert_eq!(second["provider_pubkey"], first["provider_pubkey"]);
    assert_eq!(second["instance_id"], first["instance_id"]);
    assert_eq!(stores(rig.tree()), key_before);
}

#[test]
fn a_different_owner_is_refused_and_changes_nothing() {
    let rig = Rig::new();
    let (owner, stranger) = (Keys::generate(), Keys::generate());
    let owner_file = rig.key_file("owner.key", &hex(&owner), 0o600);
    let stranger_file = rig.key_file("stranger.key", &hex(&stranger), 0o600);
    success(&rig.with_file(&owner_file, &[]));
    let (before, requests) = (rig.tree(), rig.relay.requests());

    for extra in [&[][..], &["--check"][..]] {
        let output = rig.with_file(&stranger_file, extra);
        let message = refusal(&output, 3, "auth_error", "HOST_OWNER_MISMATCH");
        assert!(message.contains("--owner-key-file"), "{message}");
        assert_no_leak(&output, &secrets_of(&stranger));
    }

    assert_eq!(rig.tree(), before);
    assert_eq!(rig.relay.requests(), requests);
}

#[test]
fn an_unusable_key_file_is_refused_before_any_write_or_request() {
    let rig = Rig::new();
    let real = Keys::generate();
    let garbage = "not-a-key-but-still-private-looking";
    let readable = rig.key_file("readable.key", &hex(&real), 0o644);
    let empty = rig.key_file("empty.key", "", 0o600);
    let invalid = rig.key_file("invalid.key", garbage, 0o600);
    let zero = rig.key_file("zero.key", &"0".repeat(64), 0o600);
    let oversized = rig.key_file("big.key", &"a".repeat(513), 0o600);
    let link = rig.dir.path().join("link.key");
    let target = rig.key_file("target.key", &hex(&real), 0o600);
    symlink(&target, &link).expect("symlink");
    let directory = rig.dir.path().join("directory.key");
    std::fs::create_dir(&directory).expect("dir");
    let missing = rig.dir.path().join("absent.key");

    let mut secrets = secrets_of(&real);
    secrets.push(garbage.to_owned());
    for path in [
        &readable, &empty, &invalid, &zero, &oversized, &link, &directory, &missing,
    ] {
        for extra in [&[][..], &["--check"][..]] {
            let output = rig.with_file(path, extra);
            refusal(&output, 3, "key_error", "OWNER_KEY_UNAVAILABLE");
            assert_no_leak(&output, &secrets);
        }
    }
    // An empty value never falls through to another source: the parser refuses it.
    let output = rig
        .command()
        .env("BEEKEEPER_PRIVATE_KEY", hex(&real))
        .args(rig.commission_args())
        .args(["--owner-key-file", ""])
        .output()
        .expect("bee runs");
    assert_eq!(output.status.code(), Some(1));
    assert_no_leak(&output, &secrets);

    assert!(rig.tree().is_empty(), "the home stays empty");
    assert_eq!(rig.relay.requests(), 0);
}

#[test]
fn a_key_passed_where_the_path_belongs_is_not_repeated() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let mut near_miss = nsec(&owner);
    near_miss.pop();
    let upper = nsec(&owner).to_ascii_uppercase();
    assert!(
        Keys::parse(&upper).is_ok(),
        "an uppercase nsec is a usable key"
    );
    for value in [
        hex(&owner),
        hex(&owner).to_ascii_uppercase(),
        nsec(&owner),
        upper.clone(),
        near_miss.clone(),
    ] {
        let output = rig.with_file(Path::new(&value), &[]);
        refusal(&output, 3, "key_error", "OWNER_KEY_UNAVAILABLE");
        assert_no_leak(
            &output,
            &[
                hex(&owner),
                hex(&owner).to_ascii_uppercase(),
                near_miss.clone(),
                upper.clone(),
            ],
        );
    }
    assert!(rig.tree().is_empty());
}

#[test]
fn a_typed_private_key_beside_a_file_is_a_conflict_that_repeats_neither() {
    let rig = Rig::new();
    let (owner, typed) = (Keys::generate(), Keys::generate());
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);

    let output = rig
        .command()
        .arg("--private-key")
        .arg(hex(&typed))
        .args(rig.commission_args())
        .arg("--owner-key-file")
        .arg(&file)
        .output()
        .expect("bee runs");

    refusal(&output, 1, "user_error", "OWNER_KEY_CONFLICT");
    assert_no_leak(&output, &[hex(&owner), hex(&typed)]);
    assert!(rig.tree().is_empty());
    assert_eq!(rig.relay.requests(), 0);
}

#[test]
fn the_file_wins_over_the_environment_which_is_not_even_read() {
    let rig = Rig::new();
    let (owner, other) = (Keys::generate(), Keys::generate());
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);

    for (env_key, tag) in [
        (hex(&other), None),
        ("not-a-key".to_owned(), None),
        (hex(&other), Some("not-a-tag")),
    ] {
        let rig_for_case = Rig::new();
        let mut command = rig_for_case.command();
        command.env("BEEKEEPER_PRIVATE_KEY", &env_key);
        if let Some(tag) = tag {
            command.env("BEEKEEPER_AUTH_TAG", tag);
        }
        let output = command
            .args(rig_for_case.commission_args())
            .arg("--owner-key-file")
            .arg(&file)
            .output()
            .expect("bee runs");
        let body = success(&output);
        assert_eq!(body["owner_pubkey"], owner.public_key().to_hex());
        assert_eq!(body["owner_key_source"], "file");
        assert_no_leak(&output, &[hex(&other), hex(&owner)]);
    }
}

#[test]
fn the_environment_route_still_works_for_a_child_process() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let output = rig
        .command()
        .env("BEEKEEPER_PRIVATE_KEY", hex(&owner))
        .args(rig.commission_args())
        .output()
        .expect("bee runs");
    let body = success(&output);
    assert_eq!(body["owner_pubkey"], owner.public_key().to_hex());
    assert_eq!(body["owner_key_source"], "environment_or_flag");
    assert_no_leak(&output, &secrets_of(&owner));
    rig.assert_absent_from_state(&secrets_of(&owner));
}

#[test]
fn naming_no_key_is_refused_and_the_default_key_file_is_not_inferred() {
    let rig = Rig::new();
    let planted = Keys::generate();
    let dot_nostr = rig.dir.path().join(".nostr");
    std::fs::create_dir(&dot_nostr).expect("dir");
    let default_file = dot_nostr.join("key");
    std::fs::write(&default_file, hex(&planted)).expect("key");
    std::fs::set_permissions(&default_file, std::fs::Permissions::from_mode(0o600)).expect("chmod");

    let output = rig
        .command()
        .args(rig.commission_args())
        .output()
        .expect("bee runs");
    let message = refusal(&output, 3, "auth_error", "OWNER_KEY_REQUIRED");
    assert!(message.contains("--owner-key-file"), "{message}");

    for bad in ["", "not-a-key", &"0".repeat(64)] {
        let output = rig
            .command()
            .env("BEEKEEPER_PRIVATE_KEY", bad)
            .args(rig.commission_args())
            .output()
            .expect("bee runs");
        refusal(&output, 3, "auth_error", "OWNER_KEY_INVALID");
    }
    assert!(rig.tree().is_empty());
    assert_eq!(rig.relay.requests(), 0);
}

#[test]
fn check_reports_the_plan_and_changes_nothing() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);

    let output = rig.with_file(&file, &["--check"]);
    let plan = success(&output);
    assert_eq!(plan["check"], true);
    assert_eq!(plan["changed"], false);
    assert_eq!(plan["can_commission"], true);
    assert_eq!(plan["identity"], "would_mint");
    assert_eq!(plan["provider_pubkey"], Value::Null);
    assert_eq!(plan["owner_pubkey"], owner.public_key().to_hex());
    assert!(plan.get("commissioned").is_none(), "{plan}");
    for name in [
        "home_writable",
        "relay_admission",
        "runtime_account",
        "credential_file",
        "runtime_install_root",
    ] {
        assert!(
            plan["not_checked"]
                .as_array()
                .expect("not_checked")
                .contains(&Value::from(name)),
            "{name}: {plan}"
        );
    }
    assert_no_leak(&output, &secrets_of(&owner));
    assert!(rig.tree().is_empty());
    assert_eq!(rig.relay.requests(), 0);

    let done = success(&rig.with_file(&file, &[]));
    let (before, requests) = (rig.tree(), rig.relay.requests());
    let replan = success(&rig.with_file(&file, &["--check"]));
    assert_eq!(replan["identity"], "would_retain");
    assert_eq!(replan["provider_pubkey"], done["provider_pubkey"]);
    assert_eq!(rig.tree(), before);
    assert_eq!(rig.relay.requests(), requests);
}

#[test]
fn a_relay_refusal_keeps_the_identity_and_the_same_command_finishes_the_job() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    rig.relay
        .answer(403, r#"{"error":"relay_membership_required"}"#);

    let output = rig.with_file(&file, &[]);
    let message = refusal(&output, 3, "auth_error", "RELAY_ADMISSION_REFUSED");
    assert!(message.contains("same command"), "{message}");
    assert_no_leak(&output, &secrets_of(&owner));

    let kept = success(&rig.with_file(&file, &["--check"]));
    assert_eq!(kept["identity"], "would_retain");

    rig.relay.answer(200, "[]");
    let body = success(&rig.with_file(&file, &[]));
    assert_eq!(body["identity"], "retained");
    assert_eq!(body["provider_pubkey"], kept["provider_pubkey"]);
}

#[test]
fn an_unreachable_relay_is_retryable_and_keeps_the_identity() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    let closed = TcpListener::bind("127.0.0.1:0").expect("bind");
    let closed_url = format!("http://{}", closed.local_addr().expect("addr"));
    drop(closed);

    let output = rig
        .command()
        .env("BEEKEEPER_RELAY_URL", &closed_url)
        .args(rig.commission_args())
        .arg("--owner-key-file")
        .arg(&file)
        .output()
        .expect("bee runs");

    assert_eq!(output.status.code(), Some(2));
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON");
    assert_eq!(error["error"], "network_error");
    assert_eq!(error["retryable"], true);
    assert_no_leak(&output, &secrets_of(&owner));
    rig.assert_absent_from_state(&secrets_of(&owner));
}

#[test]
fn help_names_the_file_route_and_the_check() {
    let output = Rig::new()
        .command()
        .args(["host", "commission", "--help"])
        .output()
        .expect("bee runs");
    let help = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0));
    assert!(help.contains("--owner-key-file"), "{help}");
    assert!(help.contains("--check"), "{help}");
    assert!(help.contains("never prompts"), "{help}");
}

fn step<'a>(body: &'a Value, argv: &[&str]) -> Option<&'a Value> {
    body["next_steps"]
        .as_array()
        .expect("next_steps")
        .iter()
        .find(|step| step["argv"] == serde_json::json!(argv))
}

/// Forged but naming the right owner: the signature is all that is wrong.
#[test]
fn a_forged_attestation_is_refused_alike_by_check_and_apply() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    success(&rig.with_file(&file, &[]));
    rig.edit_record(|entry| {
        let mut tag: Vec<String> =
            serde_json::from_str(entry["authTag"].as_str().expect("tag")).expect("tag JSON");
        tag[3] = "0".repeat(128);
        entry["authTag"] = Value::from(serde_json::to_string(&tag).expect("encode"));
    });
    let (before, requests) = (rig.tree(), rig.relay.requests());

    for extra in [&["--check"][..], &[][..]] {
        let output = rig.with_file(&file, extra);
        refusal(&output, 3, "auth_error", "HOST_ATTESTATION_INVALID");
        assert_no_leak(&output, &secrets_of(&owner));
    }

    assert_eq!(rig.tree(), before);
    assert_eq!(rig.relay.requests(), requests);
}

#[test]
fn a_damaged_identity_is_refused_by_name_and_changes_nothing() {
    type Damage = fn(&mut Value);
    let cases: [(&str, Damage); 7] = [
        ("HOST_ATTESTATION_INVALID", |entry| {
            entry.as_object_mut().expect("record").remove("authTag");
        }),
        ("HOST_ATTESTATION_INVALID", |entry| {
            entry["authTag"] = Value::from("nope");
        }),
        ("HOST_KEY_INVALID", |entry| {
            entry["privateKeyNsec"] = Value::from("garbage");
        }),
        ("HOST_KEY_MISMATCH", |entry| {
            entry["privateKeyNsec"] = Value::from(nsec(&Keys::generate()));
        }),
        ("HOST_OWNER_MISMATCH", |entry| {
            entry["authTag"] = Value::from("[]");
        }),
        ("HOST_RELAY_SCOPE_MISMATCH", |entry| {
            entry["relayUrl"] = Value::from("wss://elsewhere.example.test");
        }),
        ("HOST_KEY_UNAVAILABLE", |entry| {
            entry["privateKeyNsec"] = Value::from("");
        }),
    ];
    for (code, damage) in cases {
        let rig = Rig::new();
        let owner = Keys::generate();
        let file = rig.key_file("owner.key", &hex(&owner), 0o600);
        success(&rig.with_file(&file, &[]));
        rig.edit_record(damage);
        if code == "HOST_KEY_UNAVAILABLE" {
            std::fs::remove_file(layout::host_key_file_path(
                &rig.home,
                layout::Instance::Production,
            ))
            .expect("remove key");
        }
        let (before, requests) = (rig.tree(), rig.relay.requests());
        for extra in [&["--check"][..], &[][..]] {
            let output = rig.with_file(&file, extra);
            refusal(&output, 3, "auth_error", code);
            assert_no_leak(&output, &secrets_of(&owner));
        }
        assert_eq!(rig.tree(), before, "{code}");
        assert_eq!(rig.relay.requests(), requests, "{code}");
    }
}

/// A mint must not be saved ahead of this refusal, and `--check` must see it.
#[test]
fn a_custody_target_that_is_not_a_regular_file_is_refused_before_any_write() {
    type Plant = fn(&Path, &Path);
    let as_directory: Plant = |target, _| std::fs::create_dir_all(target).expect("dir");
    let as_link: Plant = |target, elsewhere| {
        std::fs::create_dir_all(target.parent().expect("parent")).expect("parent");
        symlink(elsewhere, target).expect("symlink");
    };
    let key = |home: &Path| layout::host_key_file_path(home, layout::Instance::Production);
    let config = |home: &Path| layout::host_config_path(home, layout::Instance::Production);
    let target_of: [fn(&Path) -> PathBuf; 2] = [key, config];
    for target in target_of {
        for plant in [as_directory, as_link] {
            let rig = Rig::new();
            let owner = Keys::generate();
            let file = rig.key_file("owner.key", &hex(&owner), 0o600);
            let elsewhere = rig.key_file("elsewhere", "x", 0o600);
            plant(&target(&rig.home), &elsewhere);
            let before = rig.tree_without_following_links();
            for extra in [&["--check"][..], &[][..]] {
                let output = rig.with_file(&file, extra);
                refusal(&output, 1, "refused", "HOST_CUSTODY_TARGET_INVALID");
                assert_no_leak(&output, &secrets_of(&owner));
            }
            assert_eq!(rig.tree_without_following_links(), before);
            assert!(!rig.store_path().exists(), "no minted identity saved");
            assert_eq!(rig.relay.requests(), 0);
        }
    }
}

#[test]
fn a_key_file_of_exactly_the_limit_is_accepted() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let key = hex(&owner);
    let padded = format!("{key}{}", "\n".repeat(512 - key.len()));
    let file = rig.key_file("exact.key", &padded, 0o600);
    assert_eq!(success(&rig.with_file(&file, &["--check"]))["check"], true);
}

/// Followed literally, a step reaches the commissioned home; without its
/// environment the same command answers about a different one.
#[test]
fn next_steps_carry_the_commissioned_home_as_argv_and_env() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    let body = success(&rig.with_file(&file, &[]));
    let home = rig.home.display().to_string();

    for argv in [
        &["bee", "host", "status"][..],
        &["bee", "host", "bind"][..],
        &["bee", "host", "install"][..],
        &["beekeeper-host", "run"][..],
    ] {
        let found = step(&body, argv).unwrap_or_else(|| panic!("{argv:?}: {body}"));
        assert_eq!(
            found["env"]["BEEKEEPER_HOST_HOME"],
            home.as_str(),
            "{found}"
        );
        assert_eq!(found["env"]["BEEKEEPER_HOST_INSTANCE"], "production");
    }
    assert!(
        step(
            &body,
            &[
                "sudo",
                "bee",
                "host",
                "install",
                "--system",
                "--user",
                "<account>"
            ]
        )
        .is_none(),
        "the system daemon cannot carry a custom home: {body}"
    );
    assert!(
        step(&body, &["bee", "host", "install"]).expect("install")["when"]
            .as_str()
            .expect("when")
            .contains("activates nothing"),
        "{body}"
    );

    let status = step(&body, &["bee", "host", "status"]).expect("status");
    let mut command = rig.command();
    for (name, value) in status["env"].as_object().expect("env") {
        command.env(name, value.as_str().expect("value"));
    }
    let followed = command.args(["host", "status"]).output().expect("bee runs");
    assert!(
        !String::from_utf8_lossy(&followed.stderr).contains("commission it first"),
        "{}",
        String::from_utf8_lossy(&followed.stderr)
    );
    let bare = rig
        .command()
        .args(["host", "status"])
        .output()
        .expect("bee runs");
    assert!(
        String::from_utf8_lossy(&bare.stderr).contains("commission it first"),
        "the environment is what makes the step land"
    );
}

#[test]
fn next_steps_name_the_dev_instance_when_commissioned_for_it() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    let output = rig
        .command()
        .env("BEEKEEPER_HOST_INSTANCE", "dev")
        .args(rig.commission_args())
        .arg("--owner-key-file")
        .arg(&file)
        .output()
        .expect("bee runs");
    let body = success(&output);
    let status = step(&body, &["bee", "host", "status"]).expect("status");
    assert_eq!(status["env"]["BEEKEEPER_HOST_INSTANCE"], "dev");
    assert!(layout::host_config_path(&rig.home, layout::Instance::Dev).exists());
}

/// The commissioned home is the account's own: a registration can start it, and
/// on a Mac the headless route is offered, marked as needing a person's root.
#[test]
fn next_steps_for_the_os_home_offer_install_and_on_macos_the_headless_route() {
    let rig = Rig::new();
    let owner = Keys::generate();
    let file = rig.key_file("owner.key", &hex(&owner), 0o600);
    let output = rig
        .command()
        .env("HOME", &rig.home)
        .args(rig.commission_args())
        .arg("--owner-key-file")
        .arg(&file)
        .output()
        .expect("bee runs");
    let body = success(&output);

    let install = step(&body, &["bee", "host", "install"]).expect("install");
    assert!(install["when"]
        .as_str()
        .expect("when")
        .contains("enable-linger"));
    assert!(step(&body, &["beekeeper-host", "run"]).is_none(), "{body}");
    let system = step(
        &body,
        &[
            "sudo",
            "bee",
            "host",
            "install",
            "--system",
            "--user",
            "<account>",
        ],
    );
    if cfg!(target_os = "macos") {
        let system = system.expect("system route on macOS");
        assert_eq!(system["human_action_required"], "root_authorization");
        assert_eq!(system["placeholders"], serde_json::json!(["<account>"]));
    } else {
        assert!(system.is_none(), "{body}");
    }
}

/// Item 92's lesson: advice about the wrong gate is worse than none.
#[test]
fn join_advice_follows_only_a_membership_refusal() {
    for (status, body, membership) in [
        (403, r#"{"error":"relay_membership_required"}"#, true),
        (403, r#"{"error":"restricted: not a relay member"}"#, true),
        (
            401,
            r#"{"error":"nip98 created_at outside the allowed window"}"#,
            false,
        ),
        (403, r#"{"error":"attestation does not verify"}"#, false),
    ] {
        let rig = Rig::new();
        let owner = Keys::generate();
        let file = rig.key_file("owner.key", &hex(&owner), 0o600);
        rig.relay.answer(status, body);
        let output = rig.with_file(&file, &[]);
        let message = refusal(&output, 3, "auth_error", "RELAY_ADMISSION_REFUSED");
        assert_eq!(message.contains("bee community"), membership, "{message}");
        assert!(message.contains("same command"), "{message}");
        assert_no_leak(&output, &secrets_of(&owner));
    }
}

/// Pins behaviour that predates the key file and is disclosed, not endorsed: the
/// owner guard is per relay. A policy change belongs to its own follow-up.
#[test]
fn the_owner_guard_is_per_relay_and_a_second_relay_replaces_active_custody() {
    let rig = Rig::new();
    let (first, second) = (Keys::generate(), Keys::generate());
    let first_file = rig.key_file("first.key", &hex(&first), 0o600);
    let second_file = rig.key_file("second.key", &hex(&second), 0o600);
    let kept = success(&rig.with_file(&first_file, &[]));
    let key_path = layout::host_key_file_path(&rig.home, layout::Instance::Production);
    let before = std::fs::read(&key_path).expect("key");

    let other = Relay::start();
    let output = rig
        .command()
        .env("BEEKEEPER_RELAY_URL", &other.url)
        .args(rig.commission_args())
        .arg("--owner-key-file")
        .arg(&second_file)
        .output()
        .expect("bee runs");

    assert_eq!(success(&output)["identity"], "minted");
    assert_ne!(std::fs::read(&key_path).expect("key"), before);
    let store = String::from_utf8(std::fs::read(rig.store_path()).expect("store")).expect("text");
    assert!(
        store.contains(kept["provider_pubkey"].as_str().expect("pubkey")),
        "the first owner's record stays in the store"
    );
}
