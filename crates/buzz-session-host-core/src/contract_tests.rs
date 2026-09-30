//! The launcher contract, defended where it lives.
//!
//! These tests moved here with the code they cover, from the desktop app's
//! `session_provider::tests`. They are the assertions a reader of the spawn
//! path cannot make by eye: the exact `BUZZ_AUTH_TAG` shape, the record file's
//! camelCase wire keys, and which variables are exported only when somebody
//! actually chose a value. Every one of them now also covers `buzz-host`,
//! which is the point of the move — the properties belong to the contract, not
//! to whichever launcher happens to be reading it.
//!
//! What deliberately stayed in the desktop crate: everything that goes through
//! `mint_provider_record`. Minting an identity is commissioning, the desktop
//! is where that happens, and those tests assert the desktop's minting against
//! this crate's env builder — which is a different claim than the ones here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::env::{
    build_provider_env, resolve_app_checkout, ProviderEnvInputs, BEE_VAR, DEFAULT_RUST_LOG,
    INHERITED_KEYS_TO_CLEAR, PROJECTS_FILE_NAME, SHARED_WORKDIRS_VAR,
};
use crate::record::{
    canonical_relay_key, CodingSessionProviderRecord, CodingSessionProviderStore, STORE_VERSION,
};

const RELAY: &str = "wss://relay.example/";

fn sample_record() -> CodingSessionProviderRecord {
    CodingSessionProviderRecord {
        provider_pubkey: "a".repeat(64),
        instance_id: "a".repeat(16),
        auth_tag: Some(r#"["auth","0011","","beef"]"#.to_string()),
        created_at: "2026-08-11T00:00:00Z".to_string(),
        relay_url: RELAY.to_string(),
        private_key_nsec: "nsec1provider".to_string(),
    }
}

#[test]
fn store_round_trips_through_json() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());

    let json = serde_json::to_string_pretty(&store).expect("serialize");
    let parsed: CodingSessionProviderStore = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed, store);
    assert_eq!(parsed.version, STORE_VERSION);
    let record = parsed.get(RELAY).expect("record for relay");
    assert_eq!(record.instance_id, "a".repeat(16));
}

/// The wire shape is camelCase and matches what the TS wrapper reads. A silent
/// rename here would leave an already-provisioned desktop looking
/// un-provisioned, which mints a second identity.
#[test]
fn store_serializes_camel_case_keys() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());
    let json = serde_json::to_string(&store).expect("serialize");

    for key in [
        "\"providerPubkey\"",
        "\"instanceId\"",
        "\"authTag\"",
        "\"createdAt\"",
        "\"relayUrl\"",
    ] {
        assert!(json.contains(key), "missing {key} in {json}");
    }
}

/// An empty nsec is the keyring-backed steady state and must not be written to
/// disk as an empty string — `skip_serializing_if` is what keeps the key out of
/// the JSON entirely.
#[test]
fn store_omits_a_blank_inline_key() {
    let mut record = sample_record();
    record.private_key_nsec.clear();
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, record);

    let json = serde_json::to_string(&store).expect("serialize");
    assert!(!json.contains("privateKeyNsec"), "{json}");
}

/// Relay keys are normalized so `wss://R/` and `wss://r` are one identity, not
/// two.
#[test]
fn relay_keys_are_normalized() {
    assert_eq!(
        canonical_relay_key("  WSS://Relay.Example/  "),
        "wss://relay.example"
    );
    let mut store = CodingSessionProviderStore::default();
    store.upsert("wss://Relay.Example/", sample_record());
    assert!(store.get("wss://relay.example").is_some());
}

/// The three session settings round-trip through the record file, and an
/// unset one stays absent rather than being written as a null.
///
/// Absence is load-bearing: `None` means "the provider's own default", and a
/// stored `0` means "no limit". A serializer that wrote `null` for the first
/// would make the two indistinguishable to anything reading the file by hand,
/// and a change to the provider's default would silently not apply.
#[test]
fn store_round_trips_the_session_settings_and_omits_unset_ones() {
    let mut store = CodingSessionProviderStore::default();
    store.upsert(RELAY, sample_record());
    let json = serde_json::to_string(&store).expect("serialize");
    for key in ["maxSessions", "turnIdleTimeoutSecs", "turnBudget"] {
        assert!(!json.contains(key), "unset {key} must be absent: {json}");
    }

    store.max_sessions = Some(9);
    store.turn_idle_timeout_secs = Some(3_600);
    store.turn_budget = Some(50);
    let json = serde_json::to_string_pretty(&store).expect("serialize");
    assert!(json.contains("\"turnBudget\""), "{json}");
    let parsed: CodingSessionProviderStore = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, store);
    assert_eq!(parsed.turn_budget, Some(50));

    // Zero is "no budget", a choice, and survives as one.
    store.turn_budget = Some(0);
    let parsed: CodingSessionProviderStore =
        serde_json::from_str(&serde_json::to_string(&store).expect("serialize"))
            .expect("deserialize");
    assert_eq!(parsed.turn_budget, Some(0));
}

/// A config file written before this feature existed must still load, and must
/// not resurrect as a different provider.
#[test]
fn legacy_store_without_optional_fields_loads() {
    let json = r#"{
        "version": 1,
        "providers": {
            "wss://relay.example": {
                "providerPubkey": "bb",
                "instanceId": "bb",
                "createdAt": "2026-01-01T00:00:00Z"
            }
        }
    }"#;
    let store: CodingSessionProviderStore = serde_json::from_str(json).expect("deserialize");
    let record = store.get("wss://relay.example").expect("record");
    assert_eq!(record.auth_tag, None);
    assert!(record.private_key_nsec.is_empty());
    assert!(record.relay_url.is_empty());
    // Including the settings added since: absent means "the provider's own
    // default", never a locally invented number.
    assert_eq!(store.max_sessions, None);
    assert_eq!(store.turn_idle_timeout_secs, None);
    assert_eq!(store.turn_budget, None);
}

fn sample_runtimes() -> Vec<buzz_core::coding_session_runtime::RuntimeDescriptor> {
    use buzz_core::coding_session_runtime::{CliEnvVar, RuntimeDescriptor, SteerIdleGuard};
    vec![
        RuntimeDescriptor {
            steer_idle_guard: Some(SteerIdleGuard::PromptRequired),
            instance_ref: "claude-primary".into(),
            driver: "claude-agent-acp".into(),
            runtime: "claude".into(),
            agent_command: "/opt/buzz/bin/claude-agent-acp".into(),
            agent_args: Vec::new(),
            cli_env: Some(CliEnvVar {
                name: "CLAUDE_CODE_EXECUTABLE".into(),
                value: "/usr/local/bin/claude".into(),
            }),
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: true,
            capabilities: None,
        },
        RuntimeDescriptor {
            steer_idle_guard: None,
            instance_ref: "goose-primary".into(),
            driver: "goose-acp".into(),
            runtime: "goose".into(),
            agent_command: "/opt/homebrew/bin/goose".into(),
            agent_args: vec!["acp".into()],
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
        },
    ]
}

fn env_for(record: &CodingSessionProviderRecord) -> BTreeMap<String, String> {
    build_provider_env(&ProviderEnvInputs {
        record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: Some(PathBuf::from("/opt/buzz/bin/claude-agent-acp")),
        context_mcp_command: Some(PathBuf::from("/opt/buzz/bin/buzz-dev-mcp")),
        claude_code_executable: Some(PathBuf::from("/usr/local/bin/claude")),
        runtimes: sample_runtimes(),
        augmented_path: Some("/opt/buzz/bin:/usr/bin".into()),
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    })
}

#[test]
fn env_carries_the_required_provider_contract() {
    let record = sample_record();
    let env = env_for(&record);

    assert_eq!(
        env.get("BUZZ_PRIVATE_KEY").map(String::as_str),
        Some("nsec1provider")
    );
    assert_eq!(env.get("BUZZ_RELAY_URL").map(String::as_str), Some(RELAY));
    assert_eq!(
        env.get("BUZZ_CSP_STATE_DIR").map(String::as_str),
        Some("/tmp/session-provider/aaaa")
    );
    assert_eq!(
        env.get("BUZZ_CSP_PROJECTS_FILE").map(String::as_str),
        Some(
            Path::new("/tmp/session-provider/aaaa")
                .join(PROJECTS_FILE_NAME)
                .to_string_lossy()
                .as_ref()
        )
    );
    assert_eq!(
        env.get("BUZZ_CSP_INSTANCE_ID").map(String::as_str),
        Some(record.instance_id.as_str())
    );
    assert_eq!(
        env.get("BUZZ_CSP_AGENT_COMMAND").map(String::as_str),
        Some("/opt/buzz/bin/claude-agent-acp")
    );
    assert_eq!(
        env.get("BUZZ_CSP_CONTEXT_MCP_COMMAND").map(String::as_str),
        Some("/opt/buzz/bin/buzz-dev-mcp")
    );
    assert_eq!(
        env.get("CLAUDE_CODE_EXECUTABLE").map(String::as_str),
        Some("/usr/local/bin/claude")
    );
    assert!(env.contains_key("RUST_LOG"));
    // The provider's adapters are `env node` shims; the augmented PATH is how
    // they find `node` when the desktop was launched with a bare GUI PATH.
    assert_eq!(
        env.get("PATH").map(String::as_str),
        Some("/opt/buzz/bin:/usr/bin")
    );
}

/// `BUZZ_CSP_RUNTIMES` must round-trip through the exact parser the sidecar
/// uses — the env map is the real interface, so this is the drift check.
#[test]
fn env_carries_a_parseable_runtime_list() {
    let env = env_for(&sample_record());
    let raw = env.get("BUZZ_CSP_RUNTIMES").expect("runtimes in env");
    let parsed = buzz_core::coding_session_runtime::parse_runtime_descriptors(raw)
        .expect("the sidecar parser must accept what the host writes");
    assert_eq!(parsed, sample_runtimes());
    // The legacy variables stay exported alongside the list, so an older
    // sidecar binary keeps working under a newer desktop.
    assert!(env.contains_key("BUZZ_CSP_AGENT_COMMAND"));
    assert!(env.contains_key("CLAUDE_CODE_EXECUTABLE"));
}

/// An empty runtime list writes no variable at all: the sidecar treats absence
/// as "synthesize the legacy claude default", while an empty JSON array would
/// be a startup error.
#[test]
fn env_omits_an_empty_runtime_list() {
    let record = sample_record();
    let env = build_provider_env(&ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    });
    assert!(!env.contains_key("BUZZ_CSP_RUNTIMES"));
    // Without an augmented PATH the child inherits the process PATH unchanged.
    assert!(!env.contains_key("PATH"));
}

/// A record with no attestation must produce no variable at all, so the child's
/// config sees "absent" rather than an empty string it would reject.
#[test]
fn env_omits_the_auth_tag_when_unattested() {
    let mut record = sample_record();
    record.auth_tag = None;
    assert!(!env_for(&record).contains_key("BUZZ_AUTH_TAG"));
}

/// The person's ceiling has to reach the child, and an unset one must stay
/// unset — exporting a number equal to the provider's default would make a
/// later change to that default silently not apply (asked for 2026-08-24).
/// The debug switch has to survive the trip from the host's environment into
/// the child, and has to be absent — not `false` — when nobody asked. A
/// provider that never asked must send no `emitRawSDKMessages` key at all.
#[test]
fn env_exports_the_raw_frame_switch_only_when_it_is_asked_for() {
    let record = sample_record();
    let base = |emit_raw_sdk_frames| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(false)).contains_key("BUZZ_CSP_EMIT_RAW_SDK_FRAMES"));
    assert_eq!(
        build_provider_env(&base(true))
            .get("BUZZ_CSP_EMIT_RAW_SDK_FRAMES")
            .map(String::as_str),
        Some("true")
    );
}

/// `RUST_LOG` used to be set unconditionally, which silently discarded a
/// filter chosen for a debugging session — the one moment anybody cares what
/// the child logs. It is a default now, and the host's choice wins.
#[test]
fn a_chosen_log_filter_beats_the_default() {
    let record = sample_record();
    let base = |rust_log: Option<String>| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert_eq!(
        build_provider_env(&base(None))
            .get("RUST_LOG")
            .map(String::as_str),
        Some(DEFAULT_RUST_LOG),
        "an unset filter keeps the quiet default"
    );
    assert_eq!(
        build_provider_env(&base(Some("debug,acp::sdk_frame=trace".into())))
            .get("RUST_LOG")
            .map(String::as_str),
        Some("debug,acp::sdk_frame=trace"),
        "a filter chosen for a debugging session must reach the child"
    );
}

#[test]
fn env_exports_the_session_ceiling_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |max_sessions| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_MAX_SESSIONS"));
    assert_eq!(
        build_provider_env(&base(Some(9)))
            .get("BUZZ_CSP_MAX_SESSIONS")
            .map(String::as_str),
        Some("9")
    );
    // Zero is unlimited, not "unset": it must be exported like any other choice.
    assert_eq!(
        build_provider_env(&base(Some(0)))
            .get("BUZZ_CSP_MAX_SESSIONS")
            .map(String::as_str),
        Some("0")
    );
}

/// Same contract for the per-turn silence budget: two of Andy's turns died as
/// "no agent activity for 900s" while a long command ran, so the number is the
/// person's — and an unset one must stay unset.
#[test]
fn env_exports_the_turn_idle_timeout_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |turn_idle_timeout_secs| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_IDLE_TIMEOUT"));
    assert_eq!(
        build_provider_env(&base(Some(3_600)))
            .get("BUZZ_CSP_IDLE_TIMEOUT")
            .map(String::as_str),
        Some("3600")
    );
}

/// And the same contract again for the crew turn budget (D9). Zero is the
/// spelling of "no budget" and is a choice, so it is exported; unset is not.
#[test]
fn env_exports_the_crew_turn_budget_only_when_one_is_chosen() {
    let record = sample_record();
    let base = |turn_budget| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget,
        app_checkout: None,
    };

    assert!(!build_provider_env(&base(None)).contains_key("BUZZ_CSP_TURN_BUDGET"));
    assert_eq!(
        build_provider_env(&base(Some(50)))
            .get("BUZZ_CSP_TURN_BUDGET")
            .map(String::as_str),
        Some("50")
    );
    assert_eq!(
        build_provider_env(&base(Some(0)))
            .get("BUZZ_CSP_TURN_BUDGET")
            .map(String::as_str),
        Some("0")
    );
}

/// Item 87(d), found live 2026-08-28 21:2x. A Team launch seated its lead in
/// the checkout this app was running from — the operator's own hot tree — and
/// the provider had no way to know that directory was special. The provider
/// owns the refusal (`session::seated_workdir_refusal`); only the host knows
/// which directory it is, so the host hands it down.
#[test]
fn the_checkout_the_app_runs_from_is_handed_to_the_provider() {
    let record = sample_record();
    let base = |app_checkout: Option<PathBuf>| ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: Vec::new(),
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout,
    };

    assert_eq!(
        build_provider_env(&base(Some(PathBuf::from(
            "/Users/b/Projects/beekeeper/beekeeper"
        ))))
        .get(SHARED_WORKDIRS_VAR)
        .map(String::as_str),
        Some("/Users/b/Projects/beekeeper/beekeeper")
    );
    // A bundled app launched from Finder resolves no checkout. Absent, not
    // empty: an empty list would read as "the host looked and found nothing
    // shared", which is a different claim.
    assert!(!build_provider_env(&base(None)).contains_key(SHARED_WORKDIRS_VAR));
}

/// The resolver walks up from the process's working directory to the
/// repository that contains it, and refuses to call a home directory a
/// checkout.
#[test]
fn an_app_checkout_is_the_repository_the_process_is_running_inside() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let repo = tmp.path().join("Projects/beekeeper/beekeeper");
    std::fs::create_dir_all(repo.join(".git")).expect("repo");
    let inner = repo.join("desktop/src-tauri");
    std::fs::create_dir_all(&inner).expect("inner");

    assert_eq!(
        resolve_app_checkout(&inner, Some(tmp.path())),
        Some(repo.clone())
    );
    assert_eq!(resolve_app_checkout(&repo, Some(tmp.path())), Some(repo));

    // Outside any repository — a bundled app's `/` — there is nothing to name.
    let bare = tmp.path().join("elsewhere");
    std::fs::create_dir_all(&bare).expect("bare");
    assert_eq!(resolve_app_checkout(&bare, Some(tmp.path())), None);

    // A home directory that happens to be a git repository is still the
    // operator's home, and the provider already refuses that by name.
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join(".git")).expect("home repo");
    assert_eq!(resolve_app_checkout(&home, Some(&home)), None);
}

/// An operator's ambient `BEE` must not survive into the provider child.
///
/// `BEE` names the `bee` a seat runs. It sits outside the `BUZZ_` prefix, so
/// the provider's own agent fence (`buzz_session_provider::agent_fence`) does
/// not cover it and an inherited value would pass straight through to every
/// seat this host starts — which is exactly the 2026-09-01 failure with the
/// variable's name on it: a binary nobody chose, answering as though somebody
/// had. The provider resolves its own (the sidecar beside its executable, else
/// `PATH`); the desktop's only job is to stop a stale one arriving.
///
/// Asserted against the list the spawn path actually iterates
/// (`supervisor.rs`, `for key in INHERITED_KEYS_TO_CLEAR`), not against a
/// literal, so a key removed from the list fails this test rather than
/// silently stopping being cleared.
#[test]
fn an_ambient_bee_is_cleared_before_the_provider_child_starts() {
    assert!(
        INHERITED_KEYS_TO_CLEAR.contains(&BEE_VAR),
        "BEE must be cleared from the inherited environment: {INHERITED_KEYS_TO_CLEAR:?}"
    );
    assert_eq!(
        BEE_VAR, "BEE",
        "this name is kept byte-for-byte in step with buzz_session_provider::seat_bee::BEE_ENV"
    );
}

/// The desktop never *sets* `BEE`. Only the provider knows which directory its
/// own executable sits in, so only the provider can name the sidecar beside
/// it; a value invented here would be a guess wearing the host's authority.
#[test]
fn the_desktop_names_no_bee_of_its_own() {
    let env = env_for(&sample_record());
    assert!(
        !env.contains_key(BEE_VAR),
        "the choice of bee belongs to the provider, not to this host: {env:?}"
    );
}

// ── the native-steer idle guard on the wire ──────────────────────────────────

/// The idle guard is the fact that turns native steering on, so the wire
/// must carry it for the runtime whose adapter was verified to honour it and
/// carry *nothing* — not `null` — for one that was not.
#[test]
fn env_declares_the_steer_idle_guard_for_claude_and_omits_it_for_codex() {
    use buzz_core::coding_session_runtime::{RuntimeDescriptor, SteerIdleGuard};
    let record = sample_record();
    let env = build_provider_env(&ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: vec![
            RuntimeDescriptor {
                steer_idle_guard: Some(SteerIdleGuard::PromptRequired),
                instance_ref: "claude-primary".into(),
                driver: "claude-agent-acp".into(),
                runtime: "claude".into(),
                agent_command: "claude-agent-acp".into(),
                agent_args: Vec::new(),
                cli_env: None,
                default_model: "default".into(),
                allowed_models: vec!["default".into()],
                discover_models: true,
                capabilities: None,
            },
            RuntimeDescriptor {
                steer_idle_guard: None,
                instance_ref: "codex-primary".into(),
                driver: "codex-acp".into(),
                runtime: "codex".into(),
                agent_command: "codex-acp".into(),
                agent_args: Vec::new(),
                cli_env: None,
                default_model: "default".into(),
                allowed_models: vec!["default".into()],
                discover_models: true,
                capabilities: None,
            },
        ],
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    });
    let raw = env.get("BUZZ_CSP_RUNTIMES").expect("runtimes in env");
    let list: Vec<serde_json::Value> = serde_json::from_str(raw).expect("json array");
    let claude = list
        .iter()
        .find(|entry| entry["instanceRef"] == "claude-primary")
        .expect("claude row");
    let codex = list
        .iter()
        .find(|entry| entry["instanceRef"] == "codex-primary")
        .expect("codex row");
    assert_eq!(claude["steerIdleGuard"], "promptRequired");
    assert!(
        raw.contains(r#""steerIdleGuard":"promptRequired""#),
        "the exact wire spelling is the contract: {raw}"
    );
    assert!(
        !codex
            .as_object()
            .expect("object")
            .contains_key("steerIdleGuard"),
        "codex must carry no guard key at all, not null: {codex}"
    );
    // And the sidecar's own parser reads both rows back exactly.
    let parsed = buzz_core::coding_session_runtime::parse_runtime_descriptors(raw)
        .expect("the sidecar parser must accept what the host writes");
    assert_eq!(
        parsed[0].steer_idle_guard,
        Some(SteerIdleGuard::PromptRequired)
    );
    assert_eq!(parsed[1].steer_idle_guard, None);
}
