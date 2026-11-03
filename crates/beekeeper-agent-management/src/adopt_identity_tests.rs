use super::*;
use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;

/// A key store that counts every read, and never touches a real Keychain.
struct Fake {
    answer: Result<Option<String>, KeySourceFailure>,
    reads: Cell<u32>,
}

impl Fake {
    fn holding(key: &str) -> Self {
        Self {
            answer: Ok(Some(key.to_string())),
            reads: Cell::new(0),
        }
    }
    fn answering(answer: Result<Option<String>, KeySourceFailure>) -> Self {
        Self {
            answer,
            reads: Cell::new(0),
        }
    }
}

impl AgentKeySource for Fake {
    fn agent_key(&self, _actor: &str) -> Result<Option<Zeroizing<String>>, KeySourceFailure> {
        self.reads.set(self.reads.get() + 1);
        self.answer.clone().map(|key| key.map(Zeroizing::new))
    }
}

struct World {
    state: tempfile::TempDir,
    owner: Keys,
    actor: Keys,
}

fn world() -> World {
    World {
        state: tempfile::tempdir().unwrap(),
        owner: Keys::generate(),
        actor: Keys::generate(),
    }
}

impl World {
    fn hex(&self) -> String {
        self.actor.public_key().to_hex()
    }
    /// The actor's real secret, which must never appear in any message.
    fn secret(&self) -> String {
        self.actor.secret_key().to_secret_hex()
    }
    fn dir(&self) -> std::path::PathBuf {
        self.state.path().join("actors").join(self.hex())
    }
    fn run(&self, source: &Fake) -> Result<bool, Error> {
        establish_custody(self.state.path(), &self.hex(), &self.owner, source)
    }
    fn refusal(&self, source: &Fake) -> String {
        let message = self.run(source).unwrap_err().to_string();
        assert!(
            !message.contains(&self.secret()),
            "a refusal quoted the key"
        );
        message
    }
    fn tag(&self, by: &Keys) -> String {
        beekeeper_sdk::nip_oa::compute_auth_tag(by, &self.actor.public_key(), "").unwrap()
    }
    fn place(&self, name: &str, bytes: &str) {
        std::fs::create_dir_all(self.dir()).unwrap();
        let path = self.dir().join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn custody_is_written_owner_only_attested_and_from_one_read() {
    let w = world();
    let source = Fake::holding(&w.secret());
    assert!(w.run(&source).unwrap());
    assert_eq!(source.reads.get(), 1);
    for name in ["key", "auth-tag.json"] {
        let mode = std::fs::metadata(w.dir().join(name))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{name} is not owner only");
    }
    let (key, auth) = actor_custody(w.state.path(), &w.hex(), None).unwrap();
    assert_eq!(key.trim(), w.secret());
    let attester = beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &w.actor.public_key()).unwrap();
    assert_eq!(attester, w.owner.public_key());
}

#[test]
fn a_rerun_reads_nothing_and_changes_nothing() {
    let w = world();
    let source = Fake::holding(&w.secret());
    w.run(&source).unwrap();
    let before = std::fs::read(w.dir().join("auth-tag.json")).unwrap();
    assert!(!w.run(&source).unwrap());
    assert_eq!(source.reads.get(), 1);
    assert_eq!(
        std::fs::read(w.dir().join("auth-tag.json")).unwrap(),
        before
    );
}

#[test]
fn a_crash_after_the_key_finishes_without_reading_the_store() {
    let w = world();
    w.place("key", &w.secret());
    let source = Fake::answering(Err(KeySourceFailure::Unavailable));
    assert!(!w.run(&source).unwrap());
    assert_eq!(source.reads.get(), 0);
    assert!(actor_custody(w.state.path(), &w.hex(), None).is_ok());
}

#[test]
fn a_crash_after_the_attestation_keeps_it_and_reads_only_the_key() {
    let w = world();
    let tag = w.tag(&w.owner);
    w.place("auth-tag.json", &tag);
    let source = Fake::holding(&w.secret());
    assert!(w.run(&source).unwrap());
    assert_eq!(
        std::fs::read_to_string(w.dir().join("auth-tag.json")).unwrap(),
        tag
    );
}

#[test]
fn every_store_failure_is_named_and_writes_nothing() {
    for (answer, code) in [
        (Ok(None), "KEYCHAIN_ENTRY_ABSENT"),
        (Err(KeySourceFailure::Unavailable), "KEYCHAIN_UNAVAILABLE"),
        (Err(KeySourceFailure::Malformed), "KEYCHAIN_MALFORMED"),
        (Err(KeySourceFailure::Unsupported), "KEYCHAIN_UNSUPPORTED"),
    ] {
        let w = world();
        assert!(w.refusal(&Fake::answering(answer)).contains(code));
        assert!(!w.dir().exists(), "{code} left custody behind");
    }
}

#[test]
fn a_key_that_derives_another_identity_is_refused_and_not_quoted() {
    let w = world();
    let other = Keys::generate().secret_key().to_secret_hex();
    let message = w.refusal(&Fake::holding(&other));
    assert!(message.contains("KEY_IDENTITY_MISMATCH"));
    assert!(!message.contains(&other));
    assert!(!w.dir().join("key").exists());
    let garbage = "not-a-key-sentinel";
    let message = w.refusal(&Fake::holding(garbage));
    assert!(message.contains("KEY_IDENTITY_MISMATCH"));
    assert!(!message.contains(garbage));
}

#[test]
fn conflicting_custody_refuses_and_the_original_stays() {
    let w = world();
    let other = Keys::generate().secret_key().to_secret_hex();
    w.place("key", &other);
    let source = Fake::holding(&w.secret());
    assert!(w.refusal(&source).contains("CUSTODY_CONFLICT"));
    assert_eq!(source.reads.get(), 0);
    assert_eq!(std::fs::read_to_string(w.dir().join("key")).unwrap(), other);
}

#[test]
fn another_owners_attestation_refuses_before_any_read() {
    let w = world();
    let tag = w.tag(&Keys::generate());
    w.place("auth-tag.json", &tag);
    let source = Fake::holding(&w.secret());
    assert!(w.refusal(&source).contains("CUSTODY_CONFLICT"));
    assert_eq!(source.reads.get(), 0);
    assert_eq!(
        std::fs::read_to_string(w.dir().join("auth-tag.json")).unwrap(),
        tag
    );
}

#[test]
fn a_broad_key_file_refuses() {
    let w = world();
    w.place("key", &w.secret());
    std::fs::set_permissions(w.dir().join("key"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(w
        .refusal(&Fake::holding(&w.secret()))
        .contains("CUSTODY_CONFLICT"));
}

#[test]
fn a_symlink_anywhere_in_custody_is_never_written_through() {
    let w = world();
    let elsewhere = tempfile::tempdir().unwrap();
    let target = elsewhere.path().join("target");
    std::fs::create_dir_all(w.dir()).unwrap();
    std::os::unix::fs::symlink(&target, w.dir().join("key")).unwrap();
    assert!(w
        .refusal(&Fake::holding(&w.secret()))
        .contains("CUSTODY_UNSAFE"));
    assert!(!target.exists());

    let w = world();
    std::fs::create_dir_all(w.state.path().join("actors")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), w.dir()).unwrap();
    assert!(w
        .refusal(&Fake::holding(&w.secret()))
        .contains("CUSTODY_UNSAFE"));
    assert!(!elsewhere.path().join("key").exists());
}

#[test]
fn an_invalid_actor_is_a_usage_error_before_anything_else() {
    let w = world();
    let source = Fake::holding(&w.secret());
    assert!(establish_custody(w.state.path(), "../../etc", &w.owner, &source).is_err());
    assert_eq!(source.reads.get(), 0);
}

fn write_state(w: &World, sessions: serde_json::Value) {
    std::fs::write(
        w.state.path().join("state.json"),
        serde_json::to_vec(&json!({"sessions":sessions})).unwrap(),
    )
    .unwrap();
}

#[test]
fn the_baseline_holds_this_identitys_sessions_and_is_written_once() {
    let w = world();
    w.run(&Fake::holding(&w.secret())).unwrap();
    let owner = w.owner.public_key().to_hex();
    write_state(
        &w,
        json!({
            "old-open": {"actor": w.hex(), "closed": false},
            "old-closed": {"actor": w.hex(), "closed": true},
            "someone-else": {"actor": "cd".repeat(32), "closed": false},
        }),
    );
    assert_eq!(write_baseline(w.state.path(), &w.hex(), &owner).unwrap(), 2);
    assert!(adoption(w.state.path(), &w.hex()).unwrap().is_some());

    write_state(
        &w,
        json!({
            "old-open": {"actor": w.hex(), "closed": false},
            "new": {"actor": w.hex(), "closed": false},
        }),
    );
    assert_eq!(write_baseline(w.state.path(), &w.hex(), &owner).unwrap(), 2);
    let prior = beekeeper_host_core::agent_setup::adopted_prior_sessions(w.state.path(), &w.hex());
    assert!(prior.contains("old-open") && prior.contains("old-closed"));
    assert!(!prior.contains("new") && !prior.contains("someone-else"));
    let open = beekeeper_host_core::agent_setup::read_open_executions(w.state.path()).unwrap();
    assert_eq!(
        open.iter()
            .map(|row| row.session_id.as_str())
            .collect::<Vec<_>>(),
        ["new"],
        "a session from before the adoption does not count as open"
    );

    assert!(write_baseline(w.state.path(), &w.hex(), &"ef".repeat(32))
        .unwrap_err()
        .to_string()
        .contains("CUSTODY_CONFLICT"));
}

#[test]
fn no_file_means_no_exclusion() {
    let w = world();
    write_state(&w, json!({"open": {"actor": w.hex(), "closed": false}}));
    let open = beekeeper_host_core::agent_setup::read_open_executions(w.state.path()).unwrap();
    assert_eq!(open.len(), 1);
    assert!(adoption(w.state.path(), &w.hex()).unwrap().is_none());
}

#[test]
fn stop_keeps_only_what_this_host_holds_from_after_the_adoption() {
    let set = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>();
    let (current, prior) = (
        set(&["baseline", "new"]),
        set(&["baseline", "gone-locally"]),
    );
    for (session, stoppable) in [
        ("another-provider", false),
        ("gone-locally", false),
        ("baseline", false),
        ("new", true),
    ] {
        assert_eq!(
            is_stoppable(session, &current, &prior),
            stoppable,
            "{session}"
        );
    }
}

#[test]
fn a_bad_adoption_record_is_an_error_and_never_absent() {
    for body in [
        "{not json",
        r#"{"actor":"someone","owner":"o","priorSessions":[]}"#,
        r#"{"actor":"x"}"#,
    ] {
        let w = world();
        w.place(ADOPTION_FILE, body);
        let error = adoption(w.state.path(), &w.hex()).unwrap_err().to_string();
        assert!(
            error.contains("ADOPTION_RECORD_UNREADABLE"),
            "{body}: {error}"
        );
    }
    // A failure that is not "absent": a directory stands where the file should be.
    let w = world();
    std::fs::create_dir_all(w.dir().join(ADOPTION_FILE)).unwrap();
    assert!(adoption(w.state.path(), &w.hex()).is_err());
    let owner = w.owner.public_key().to_hex();
    assert!(write_baseline(w.state.path(), &w.hex(), &owner).is_err());
    assert!(
        w.dir().join(ADOPTION_FILE).is_dir(),
        "it was not overwritten"
    );
}

#[test]
fn a_bad_record_excludes_nothing_so_every_session_still_counts() {
    let w = world();
    w.place(ADOPTION_FILE, "{not json");
    write_state(&w, json!({"open": {"actor": w.hex(), "closed": false}}));
    let prior = beekeeper_host_core::agent_setup::adopted_prior_sessions(w.state.path(), &w.hex());
    assert!(prior.is_empty());
    let open = beekeeper_host_core::agent_setup::read_open_executions(w.state.path()).unwrap();
    assert_eq!(open.len(), 1);
}

fn signed_head(w: &World) -> nostr::Event {
    nostr::EventBuilder::new(nostr::Kind::Custom(30177), r#"{"name":"Keystone"}"#)
        .tags([nostr::Tag::parse(["d", &w.hex()]).unwrap()])
        .sign_with_keys(&w.owner)
        .unwrap()
}

#[test]
fn the_prior_definition_is_kept_exactly_and_only_once() {
    let w = world();
    std::fs::create_dir_all(w.dir()).unwrap();
    let first = signed_head(&w);
    keep_prior_definition(w.state.path(), &w.hex(), &first).unwrap();
    let path = w.dir().join(PRE_ADOPTION_DEFINITION);
    let kept: nostr::Event = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(kept, first);
    assert!(kept.verify().is_ok(), "the copy is still the signed event");
    let bytes = std::fs::read(&path).unwrap();
    keep_prior_definition(w.state.path(), &w.hex(), &signed_head(&w)).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "a later head never replaces it"
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o077,
        0
    );
}

#[test]
fn a_definition_that_cannot_be_kept_refuses() {
    let w = world();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(w.state.path().join("actors")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), w.dir()).unwrap();
    let error = keep_prior_definition(w.state.path(), &w.hex(), &signed_head(&w)).unwrap_err();
    assert!(error.to_string().contains("CUSTODY_UNSAFE"));
    assert!(!elsewhere.path().join(PRE_ADOPTION_DEFINITION).exists());
}

#[test]
fn an_aged_install_is_never_retried_over_an_owners_change() {
    let same = "aa";
    assert!(may_retry_install(Some(same), same, true, true).is_ok());
    assert!(may_retry_install(None, same, false, false).is_ok());
    let changed = may_retry_install(Some(same), "newer-owner-revision", true, true).unwrap_err();
    assert!(changed.to_string().contains("OWNER_CHANGED_DEFINITION"));
    let removed = may_retry_install(Some(same), same, false, true).unwrap_err();
    assert!(removed.to_string().contains("MEMBERSHIP_UNKNOWN"));
}

#[test]
fn stop_refuses_when_it_cannot_read_what_this_host_holds() {
    let home = tempfile::tempdir().unwrap();
    let context = Context {
        home: home.path().to_path_buf(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let actor = "ab".repeat(32);
    // No Host here at all: nothing is adopted, and Stop is unchanged.
    let (kept, excluded) = stoppable(&context, &actor, Vec::new()).unwrap();
    assert!(kept.is_empty() && excluded == 0);

    let path = beekeeper_host_core::layout::host_config_path(&context.home, context.instance);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{not a config").unwrap();
    let error = stoppable(&context, &actor, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("ADOPTION_RECORD_UNREADABLE"), "{error}");

    let state = home.path().join("provider");
    let dir = state.join("actors").join(&actor);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({
            "version": 1, "instance": "dev", "relayUrl": "wss://one.example",
            "providerPubkey": "cd".repeat(32),
            "sessionProviderBaseDir": home.path(), "providerStateDir": state,
            "runtimes": [], "writtenAt": "2026-11-03T00:00:00Z",
        }))
        .unwrap(),
    )
    .unwrap();
    stoppable(&context, &actor, Vec::new()).expect("a good config, not adopted");
    std::fs::write(dir.join(ADOPTION_FILE), "{not json").unwrap();
    let error = stoppable(&context, &actor, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("ADOPTION_RECORD_UNREADABLE"), "{error}");
    std::fs::write(
        dir.join(ADOPTION_FILE),
        serde_json::to_vec(&json!({"actor": actor, "owner": "o", "priorSessions": []})).unwrap(),
    )
    .unwrap();
    std::fs::write(state.join("state.json"), "{not json").unwrap();
    let error = stoppable(&context, &actor, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("ADOPTION_RECORD_UNREADABLE"), "{error}");
}

#[test]
fn an_aged_start_with_the_member_gone_says_the_real_way_forward() {
    let message = start_membership_unknown().to_string();
    assert!(message.contains("MEMBERSHIP_UNKNOWN"));
    assert!(message.contains("bee channels add-member") && message.contains("Retry"));
    assert!(
        !message.contains("plan again"),
        "plan is refused once installed"
    );
}
