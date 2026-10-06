//! Unit and Postgres-backed tests for [`super`] — split out of `policy.rs`
//! so the handler itself reads in one screenful and neither file grows
//! without bound. A child module (`#[path]`), so `use super::*` reads the
//! parent's private items exactly as the inline `mod tests` it replaces did.
//!
//! `pub(crate)` on the fixture helpers is deliberate: the sibling
//! `verdict_admission_tests` drives the same handler through the same
//! Postgres fixtures rather than growing a second, drifting copy of them.

use super::*;

fn make_request() -> HookCallbackRequest {
    HookCallbackRequest {
        repo_id: "test-repo".to_string(),
        repo_owner: "a".repeat(64),
        community_id: uuid::Uuid::from_u128(1).to_string(),
        pusher_pubkey: "b".repeat(64),
        ref_updates: vec![HookRefUpdate {
            old_oid: "1".repeat(40),
            new_oid: "2".repeat(40),
            ref_name: "refs/heads/main".to_string(),
            is_ancestor: true,
        }],
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        signature: String::new(),
    }
}

fn sign_request(req: &mut HookCallbackRequest, secret: &[u8]) {
    let mac = compute_hmac(secret, req);
    req.signature = hex::encode(mac);
}

#[test]
fn hmac_valid_signature_accepted() {
    let secret = b"test-secret-key";
    let mut req = make_request();
    sign_request(&mut req, secret);
    assert!(verify_hmac(secret, &req));
}

#[test]
fn hmac_wrong_secret_rejected() {
    let mut req = make_request();
    sign_request(&mut req, b"correct-secret");
    assert!(!verify_hmac(b"wrong-secret", &req));
}

/// Deploy-skew guard for the unbound-repo deny body. The token
/// (`no_channel_binding`, underscores) and the legacy phrase
/// (`no channel binding`, spaces) do NOT contain each other, so the body
/// must carry both: the token for structured consumers (Desktop's merge
/// classifier and dialog matcher), the phrase for desktops already in
/// the field that prose-match it. Relay ships continuously and Desktop
/// on release cadence — dropping the phrase strands every old desktop
/// on a new relay. Asserted against the shared consts, not re-typed
/// literals, so the const and this test cannot drift apart separately.
#[test]
fn no_channel_binding_body_satisfies_old_and_new_matchers() {
    assert!(
        GIT_NO_CHANNEL_BINDING_BODY.starts_with(&format!(
            "{}: ",
            beekeeper_core::git_perms::GIT_NO_CHANNEL_BINDING_TOKEN
        )),
        "new structured consumers match the token prefix"
    );
    assert!(
        GIT_NO_CHANNEL_BINDING_BODY.contains("no channel binding"),
        "shipped desktops prose-match this exact phrase (spaces, not underscores)"
    );
}

#[test]
fn hmac_tampered_repo_id_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.repo_id = "evil-repo".to_string();
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_tampered_pusher_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.pusher_pubkey = "c".repeat(64);
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_tampered_ref_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.ref_updates[0].ref_name = "refs/heads/evil".to_string();
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_tampered_is_ancestor_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.ref_updates[0].is_ancestor = false; // Flip FF → NFF
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_tampered_owner_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.repo_owner = "c".repeat(64);
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_tampered_timestamp_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.timestamp += 1;
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_invalid_hex_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    req.signature = "not-valid-hex!!!".to_string();
    assert!(!verify_hmac(secret, &req));
}

/// Tampering the server-resolved community changes the HMAC input, so a
/// hook callback cannot be replayed across communities even though the
/// localhost policy endpoint itself has no inbound Host header.
#[test]
fn hmac_tampered_community_rejected() {
    let secret = b"test-secret";
    let mut req = make_request();
    sign_request(&mut req, secret);
    req.community_id = uuid::Uuid::from_u128(2).to_string();
    assert!(!verify_hmac(secret, &req));
}

#[test]
fn hmac_deterministic_across_ref_order() {
    let secret = b"test-secret";
    let mut req1 = make_request();
    req1.ref_updates.push(HookRefUpdate {
        old_oid: "3".repeat(40),
        new_oid: "4".repeat(40),
        ref_name: "refs/heads/develop".to_string(),
        is_ancestor: false,
    });
    let mut req2 = req1.clone();
    // Reverse the ref order — HMAC should be the same (sorted internally).
    req2.ref_updates.reverse();
    let mac1 = compute_hmac(secret, &req1);
    let mac2 = compute_hmac(secret, &req2);
    assert_eq!(mac1, mac2);
}

#[test]
fn generate_hook_hmac_matches_verify() {
    let secret = b"test-secret";
    let mut req = make_request();
    let sig = generate_hook_hmac(
        secret,
        &req.repo_id,
        &req.repo_owner,
        &req.community_id,
        &req.pusher_pubkey,
        &req.ref_updates,
        req.timestamp,
    );
    req.signature = sig;
    assert!(verify_hmac(secret, &req));
}

/// Cross-boundary HMAC integration test.
///
/// Runs the bash HMAC computation logic (extracted from the pre-receive hook)
/// and compares its output against Rust's `generate_hook_hmac`. This is the
/// most critical test — it verifies the bash/Rust format agreement that the
/// entire security model depends on.
#[test]
fn bash_hmac_matches_rust_hmac() {
    let secret = "cross-boundary-test-secret-key-1234";
    let repo_id = "my-project";
    let repo_owner = "ab".repeat(32); // 64 hex chars
    let pusher = "cd".repeat(32); // 64 hex chars
    let community_id = uuid::Uuid::from_u128(1).to_string();
    let timestamp: u64 = 1700000000;

    // Two refs, intentionally out of sorted order to test sorting.
    let ref_updates = vec![
        HookRefUpdate {
            old_oid: "b".repeat(40),
            new_oid: "c".repeat(40),
            ref_name: "refs/heads/main".to_string(),
            is_ancestor: true,
        },
        HookRefUpdate {
            old_oid: "a".repeat(40),
            new_oid: "d".repeat(40),
            ref_name: "refs/heads/feature".to_string(),
            is_ancestor: false,
        },
    ];

    // Compute Rust-side HMAC.
    let rust_sig = generate_hook_hmac(
        secret.as_bytes(),
        repo_id,
        &repo_owner,
        &community_id,
        &pusher,
        &ref_updates,
        timestamp,
    );

    // Bash script that replicates the hook's HMAC computation.
    // This is the exact logic from hook.rs PRE_RECEIVE_HOOK, extracted into
    // a standalone script with hardcoded values.
    let bash_script = format!(
        r#"
export LC_ALL=C
BUZZ_REPO_ID="{repo_id}"
BUZZ_REPO_OWNER="{repo_owner}"
BUZZ_COMMUNITY_ID="{community_id}"
BUZZ_PUSHER_PUBKEY="{pusher}"
BUZZ_HOOK_SECRET="{secret}"
TIMESTAMP="{timestamp}"

# Simulate the HMAC_FILE with two refs (unsorted, like the hook writes them)
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
HMAC_FILE="$WORK_DIR/hmac"

# Write refs in the order they'd arrive (main first, feature second)
echo "refs/heads/main {old1} {new1} 1" >> "$HMAC_FILE"
echo "refs/heads/feature {old2} {new2} 0" >> "$HMAC_FILE"

# Build HMAC input — exact logic from hook script
REPO_ID_LEN=${{#BUZZ_REPO_ID}}
HMAC_INPUT="${{REPO_ID_LEN}}:${{BUZZ_REPO_ID}}|${{BUZZ_REPO_OWNER}}|${{BUZZ_COMMUNITY_ID}}|${{BUZZ_PUSHER_PUBKEY}}|"
sort "$HMAC_FILE" | while IFS=' ' read -r ref_name old_oid new_oid is_anc; do
REF_LEN=${{#ref_name}}
printf '%s%s%s:%s%s' "$old_oid" "$new_oid" "$REF_LEN" "$ref_name" "$is_anc"
done > "$HMAC_FILE.concat"
HMAC_INPUT="${{HMAC_INPUT}}$(cat "$HMAC_FILE.concat")|${{TIMESTAMP}}"

# Compute HMAC-SHA256
printf '%s' "$HMAC_INPUT" | openssl dgst -sha256 -hmac "$BUZZ_HOOK_SECRET" -hex 2>/dev/null | sed 's/.*= //'
"#,
        repo_id = repo_id,
        repo_owner = repo_owner,
        community_id = community_id,
        pusher = pusher,
        secret = secret,
        timestamp = timestamp,
        old1 = "b".repeat(40),
        new1 = "c".repeat(40),
        old2 = "a".repeat(40),
        new2 = "d".repeat(40),
    );

    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&bash_script)
        .output()
        .expect("failed to run bash");

    assert!(
        output.status.success(),
        "bash script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bash_sig = String::from_utf8_lossy(&output.stdout).trim().to_string();

    assert_eq!(
        rust_sig, bash_sig,
        "HMAC mismatch!\n  Rust: {rust_sig}\n  Bash: {bash_sig}\n\
         The pre-receive hook and policy endpoint disagree on the canonical format."
    );
}

/// Cross-boundary test with a single ref (simpler case).
#[test]
fn bash_hmac_single_ref() {
    let secret = "single-ref-secret";
    let repo_id = "test-repo";
    let repo_owner = "a".repeat(64);
    let pusher = "b".repeat(64);
    let community_id = uuid::Uuid::from_u128(1).to_string();
    let timestamp: u64 = 1700000001;

    let ref_updates = vec![HookRefUpdate {
        old_oid: "1".repeat(40),
        new_oid: "2".repeat(40),
        ref_name: "refs/heads/main".to_string(),
        is_ancestor: true,
    }];

    let rust_sig = generate_hook_hmac(
        secret.as_bytes(),
        repo_id,
        &repo_owner,
        &community_id,
        &pusher,
        &ref_updates,
        timestamp,
    );

    let bash_script = format!(
        r#"
export LC_ALL=C
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
HMAC_FILE="$WORK_DIR/hmac"
echo "refs/heads/main {old} {new} 1" >> "$HMAC_FILE"
BUZZ_REPO_ID="{repo_id}"
REPO_ID_LEN=${{#BUZZ_REPO_ID}}
HMAC_INPUT="${{REPO_ID_LEN}}:${{BUZZ_REPO_ID}}|{owner}|{community_id}|{pusher}|"
sort "$HMAC_FILE" | while IFS=' ' read -r ref_name old_oid new_oid is_anc; do
REF_LEN=${{#ref_name}}
printf '%s%s%s:%s%s' "$old_oid" "$new_oid" "$REF_LEN" "$ref_name" "$is_anc"
done > "$HMAC_FILE.concat"
HMAC_INPUT="${{HMAC_INPUT}}$(cat "$HMAC_FILE.concat")|{timestamp}"
printf '%s' "$HMAC_INPUT" | openssl dgst -sha256 -hmac "{secret}" -hex 2>/dev/null | sed 's/.*= //'
"#,
        old = "1".repeat(40),
        new = "2".repeat(40),
        repo_id = repo_id,
        owner = repo_owner,
        community_id = community_id,
        pusher = pusher,
        timestamp = timestamp,
        secret = secret,
    );

    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&bash_script)
        .output()
        .expect("failed to run bash");

    assert!(
        output.status.success(),
        "bash script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bash_sig = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(
        rust_sig, bash_sig,
        "Single-ref HMAC mismatch!\n  Rust: {rust_sig}\n  Bash: {bash_sig}"
    );
}

// ── hook_policy_check binding gate (requires Postgres) ──────────────

const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

pub(crate) async fn policy_test_state() -> Arc<AppState> {
    let mut config = crate::config::Config::from_env().expect("default config loads");
    config.require_relay_membership = false;
    config.redis_url = "redis://127.0.0.1:1".to_string();
    config.database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| TEST_DB_URL.to_string());
    let pool = sqlx::PgPool::connect(&config.database_url)
        .await
        .expect("connect test DB");
    let db = beekeeper_db::Db::from_pool(pool.clone());
    let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("redis pool");
    let pubsub = Arc::new(
        beekeeper_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
            .await
            .expect("pubsub manager"),
    );
    let audit = beekeeper_audit::AuditService::new(pool.clone());
    let auth = beekeeper_auth::AuthService::new(config.auth.clone());
    let search = beekeeper_search::SearchService::new(pool.clone());
    let workflow_engine = Arc::new(beekeeper_workflow::WorkflowEngine::new(
        db.clone(),
        beekeeper_workflow::WorkflowConfig::default(),
    ));
    let media_storage = beekeeper_media::MediaStorage::new(&config.media).expect("media storage");
    let (state, _audit_shutdown) = AppState::new(
        config,
        db,
        redis_pool,
        audit,
        pubsub,
        auth,
        search,
        workflow_engine,
        nostr::Keys::generate(),
        media_storage,
    );
    Arc::new(state)
}

/// Creating `refs/heads/main` — the default operation, minimum role
/// `Member` (`git_perms::default_min_role`).
pub(crate) fn create_main() -> HookRefUpdate {
    HookRefUpdate {
        old_oid: "0".repeat(40),
        new_oid: "2".repeat(40),
        ref_name: "refs/heads/main".to_string(),
        is_ancestor: false,
    }
}

/// Fast-forwarding `refs/heads/main` — minimum role `Member`, the
/// ordinary push a seat must keep.
pub(crate) fn fast_forward_main() -> HookRefUpdate {
    HookRefUpdate {
        old_oid: "1".repeat(40),
        new_oid: "2".repeat(40),
        ref_name: "refs/heads/main".to_string(),
        is_ancestor: true,
    }
}

/// Deleting `refs/heads/main` — minimum role `Admin`.
pub(crate) fn delete_main() -> HookRefUpdate {
    HookRefUpdate {
        old_oid: "1".repeat(40),
        new_oid: "0".repeat(40),
        ref_name: "refs/heads/main".to_string(),
        is_ancestor: false,
    }
}

/// Force-pushing `refs/heads/main` — minimum role `Admin`. Used to prove
/// a grant carries its *tier*, not merely permission to push at all.
pub(crate) fn force_push_main() -> HookRefUpdate {
    HookRefUpdate {
        old_oid: "1".repeat(40),
        new_oid: "2".repeat(40),
        ref_name: "refs/heads/main".to_string(),
        is_ancestor: false,
    }
}

/// Announce `repo_id` with the given tags, then run the policy check for
/// an arbitrary pusher and ref update.
pub(crate) async fn push_response(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    repo_owner_keys: &nostr::Keys,
    repo_id: &str,
    announcement_tags: Vec<nostr::Tag>,
    pusher_hex: &str,
    ref_update: HookRefUpdate,
) -> axum::response::Response {
    use nostr::{EventBuilder, Kind, Tag};

    let mut tags = vec![Tag::parse(["d", repo_id]).unwrap()];
    tags.extend(announcement_tags);
    let event = EventBuilder::new(Kind::Custom(30617), "")
        .tags(tags)
        .sign_with_keys(repo_owner_keys)
        .expect("sign 30617");
    state
        .db
        .insert_event(community, &event, None)
        .await
        .expect("insert 30617");

    let mut req = HookCallbackRequest {
        repo_id: repo_id.to_string(),
        repo_owner: repo_owner_keys.public_key().to_hex(),
        community_id: community.as_uuid().to_string(),
        pusher_pubkey: pusher_hex.to_string(),
        ref_updates: vec![ref_update],
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        signature: String::new(),
    };
    let secret = state.config.git_hook_hmac_secret.clone();
    sign_request(&mut req, secret.as_bytes());
    hook_policy_check(State(Arc::clone(state)), Json(req)).await
}

/// Announce `repo_id` with the given tags, then push to it as its own
/// announcement author and return the response.
pub(crate) async fn owner_push_response(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    keys: &nostr::Keys,
    repo_id: &str,
    binding_tags: Vec<nostr::Tag>,
) -> axum::response::Response {
    let owner_hex = keys.public_key().to_hex();
    push_response(
        state,
        community,
        keys,
        repo_id,
        binding_tags,
        &owner_hex,
        create_main(),
    )
    .await
}

pub(crate) async fn body_string(response: axum::response::Response) -> (StatusCode, String) {
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

// ── project-roster grants (NIP-MP) ───────────────────────────────────

pub(crate) struct ProjectFixture {
    state: Arc<AppState>,
    community: beekeeper_core::CommunityId,
    /// The community's own host, for cases that must drive `ingest_event`
    /// (which needs a resolved tenant) rather than `insert_event`.
    host: String,
    coordinate: String,
    creator: nostr::Keys,
    collaborator: nostr::Keys,
    viewer: nostr::Keys,
    /// Announcement author, deliberately off the roster so the roster —
    /// not authorship — is what these tests measure. (Pushing *as* the
    /// author still short-circuits to Owner; that is tested in
    /// `policy_gate_tests.rs`.)
    repo_owner: nostr::Keys,
}

pub(crate) async fn project_fixture(visibility: &str) -> ProjectFixture {
    use beekeeper_core::channel::ProjectRole;
    use nostr::Keys;

    let state = policy_test_state().await;
    let host = format!("policy-mp-{}.example", uuid::Uuid::new_v4().simple());
    let community = state
        .db
        .ensure_configured_community(&host)
        .await
        .expect("community")
        .id;

    let creator = Keys::generate();
    let collaborator = Keys::generate();
    let viewer = Keys::generate();
    let repo_owner = Keys::generate();

    let dtag = format!("proj-{}", uuid::Uuid::new_v4().simple());
    state
        .db
        .upsert_project_acl(
            community,
            &creator.public_key().to_bytes(),
            &dtag,
            visibility,
            &[
                (
                    collaborator.public_key().to_bytes().to_vec(),
                    ProjectRole::Collaborator,
                ),
                (viewer.public_key().to_bytes().to_vec(), ProjectRole::Viewer),
            ],
            1,
        )
        .await
        .expect("project acl");

    let coordinate = format!("30621:{}:{dtag}", creator.public_key().to_hex());
    ProjectFixture {
        state,
        community,
        host,
        coordinate,
        creator,
        collaborator,
        viewer,
        repo_owner,
    }
}

pub(crate) fn project_tag(coordinate: &str) -> Vec<nostr::Tag> {
    vec![nostr::Tag::parse(["project", coordinate]).unwrap()]
}

pub(crate) fn fresh_repo() -> String {
    format!("repo-{}", uuid::Uuid::new_v4().simple())
}

// ── NIP-OA seats (a hired seat pushing on its owner's grant) ─────────

/// Register `agent` as a managed seat of `owner`, the way the git request
/// extractor does when a push carries a verified NIP-OA attestation.
pub(crate) async fn seat_of(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    agent: &nostr::Keys,
    owner: &nostr::Keys,
) {
    for pk in [agent.public_key(), owner.public_key()] {
        state
            .db
            .ensure_user(community, &pk.to_bytes())
            .await
            .expect("user");
    }
    assert!(
        state
            .db
            .set_agent_owner(
                community,
                &agent.public_key().to_bytes(),
                &owner.public_key().to_bytes(),
            )
            .await
            .expect("set agent owner"),
        "fixture: the seat must be newly attested"
    );
}

// ── L6.3: the relay logs who pushed, on every decision ───────────────

/// Capture `tracing` output on this thread so a log line can be asserted on.
#[derive(Clone, Default)]
struct CapturedLog(Arc<std::sync::Mutex<Vec<u8>>>);

impl CapturedLog {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("log buffer").clone()).into_owned()
    }
}

impl std::io::Write for CapturedLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Run `body` with `tracing` captured on this thread, and return what it wrote.
fn captured_log(body: impl FnOnce()) -> String {
    use tracing_subscriber::layer::SubscriberExt;

    let sink = CapturedLog::default();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(sink.clone())
            .with_ansi(false)
            .with_level(true),
    );
    {
        let _guard = tracing::subscriber::set_default(subscriber);
        body();
    }
    sink.text()
}

/// Run 3 could answer "who pushed `main`?" only from the derived kind:30618
/// event, because the relay logged nothing at the moment it *allowed* the
/// push. The allowed case is therefore the one that matters: every field is
/// asserted on it, not only on a refusal.
#[test]
fn an_allowed_ref_update_is_logged_with_pusher_and_both_object_ids() {
    let req = make_request();
    let logged = captured_log(|| log_ref_updates(&req, &PolicyOutcome::Allowed));

    for field in [
        "git ref update",
        "repo=test-repo",
        "ref_name=refs/heads/main",
        &format!("old={}", "1".repeat(40)),
        &format!("new={}", "2".repeat(40)),
        &format!("pusher={}", "b".repeat(64)),
        "decision=\"allowed\"",
    ] {
        assert!(
            logged.contains(field),
            "the allowed push must log {field}:\n{logged}"
        );
    }
}

/// Every ref in one push gets its own line, and a denial carries its reason
/// so the log answers "what was refused, and why" without a second lookup.
#[test]
fn every_ref_in_a_denied_push_is_logged_with_its_reason() {
    let mut req = make_request();
    req.ref_updates.push(HookRefUpdate {
        old_oid: "3".repeat(40),
        new_oid: "4".repeat(40),
        ref_name: "refs/heads/topic".to_string(),
        is_ancestor: true,
    });
    let outcome = PolicyOutcome::Denied(vec![Denial {
        ref_name: "refs/heads/main".to_string(),
        reason: "require-verdict is set".to_string(),
    }]);
    let logged = captured_log(|| log_ref_updates(&req, &outcome));

    assert_eq!(
        logged.matches("git ref update").count(),
        2,
        "one line per ref update:\n{logged}"
    );
    assert!(logged.contains("ref_name=refs/heads/topic"), "{logged}");
    assert!(logged.contains("decision=\"denied\""), "{logged}");
    assert!(logged.contains("require-verdict is set"), "{logged}");
}

/// The Postgres-backed push-gate cases, in their own file so neither this
/// one nor `policy.rs` grows past the repository's 1,000-line ceiling. A
/// child module, so the fixtures above are reached by `use super::*` exactly
/// as they were when every case lived here.
#[path = "policy_gate_tests.rs"]
mod gate;
