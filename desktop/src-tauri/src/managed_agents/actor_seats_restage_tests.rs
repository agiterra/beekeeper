//! Tests for re-staging custody after a coding-session provider restart.
//!
//! Split out of `actor_seats_restage.rs` to keep both files under the
//! repository's 1000-line ceiling; `use super::*` keeps every claim against
//! the same module it was written for. Like every other test in
//! `managed_agents`, none of this builds a live `AppHandle`
//! (`session_provider::tests` module docs) — `restage_actor_seats_with` is the
//! decision core [`restage_actor_seats_for_provider`] delegates to precisely
//! so the staging and skip logic is testable without one; the async wrapper's
//! own I/O (reading `seat-requests.json`, hydrating keys, reading/writing the
//! actor-seats file) is exercised at the level of the functions it calls,
//! which are already covered here and in `actor_seats_tests.rs`.

use std::collections::BTreeMap;

use super::*;
use crate::managed_agents::actor_seats::{
    build_actor_seat_entry, write_actor_seats, SeatPackOrigin,
};

const PUBKEY_A: &str = "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";
const PUBKEY_B: &str = "bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66aa11";
const PUBKEY_C: &str = "cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66aa11bb22";
const RELAY: &str = "wss://relay.example";

fn agent_record(pubkey: &str, nsec: &str) -> crate::managed_agents::types::ManagedAgentRecord {
    let mut record = crate::managed_agents::types::AgentDefinition {
        id: "def".into(),
        display_name: "Builder".into(),
        avatar_url: None,
        system_prompt: String::new(),
        runtime: None,
        model: None,
        provider: None,
        name_pool: vec![],
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: Default::default(),
        respond_to: None,
        respond_to_allowlist: vec![],
        parallelism: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
    .into_agent_record();
    record.pubkey = pubkey.to_string();
    record.private_key_nsec = nsec.to_string();
    record
}

fn request(command_id: &str, actor: &str, role: &str, project_ref: Option<&str>) -> SeatRequest {
    SeatRequest {
        command_id: command_id.to_string(),
        actor: actor.to_string(),
        role: role.to_string(),
        project_ref: project_ref.map(str::to_string),
        session_id: "sess-1".to_string(),
        generation: 1,
        pack_ref: Some(pack_ref(role)),
        fenced: false,
    }
}

fn pack_ref(role: &str) -> packs_cache::PackRef {
    packs_cache::PackRef {
        repo: format!("30617:{PUBKEY_A}:packs"),
        sha: "a".repeat(40),
        role: role.to_string(),
        path: format!("personas/roles/{role}"),
    }
}

fn staged_plan(persona: &str) -> SeatPackPreview {
    SeatPackPreview {
        pack_staged: true,
        origin: SeatPackOrigin::Installed,
        role: Some("builder".into()),
        pack_dir: Some(format!("/tmp/{persona}-pack")),
        persona_id: Some(persona.to_string()),
        pack_ref: Some(pack_ref("builder")),
        refusal: None,
        reason: None,
        warnings: Vec::new(),
        compose_digest: None,
        source_kind: None,
        agents_repo: crate::managed_agents::packs_cache::AgentsRepoAccess::None,
    }
}

/// (a) A request with no seat staged under its `commandId` gets one, filed
/// with the right pubkey, relay, and persona (standing in for "role" — the
/// entry itself carries no bare role field, only the pack the role picked).
#[test]
fn a_missing_seat_is_staged_under_its_command_id() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let requests = vec![request("csl-1", PUBKEY_A, "builder", None)];
    let mut resolved = BTreeMap::new();
    resolved.insert("csl-1".to_string(), Ok(staged_plan("builder")));

    let (file, report) = restage_actor_seats_with(
        &requests,
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &resolved,
    );

    assert_eq!(report.requested, 1);
    assert_eq!(report.staged, 1);
    assert_eq!(report.already_present, 0);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let entry = file.pending.get("csl-1").expect("staged under csl-1");
    assert_eq!(entry.pubkey, PUBKEY_A);
    assert_eq!(entry.relay_url, RELAY);
    assert_eq!(entry.persona_id.as_deref(), Some("builder"));
}

/// (b) A request whose `commandId` already has a seat is left alone — the
/// existing entry is not touched, even if a caller supplied a resolution for
/// it (it must not have to, but the count is right either way).
#[test]
fn an_already_staged_seat_is_left_untouched() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let existing_entry = build_actor_seat_entry(
        PUBKEY_A,
        "nsec1already-staged",
        None,
        RELAY,
        None,
        None,
        None,
    )
    .expect("existing entry");
    let mut existing = ActorSeatsFile::default();
    existing
        .pending
        .insert("csl-2".to_string(), existing_entry.clone());
    let requests = vec![request("csl-2", PUBKEY_A, "builder", None)];

    let (file, report) =
        restage_actor_seats_with(&requests, &existing, &records, RELAY, &BTreeMap::new());

    assert_eq!(report.requested, 1);
    assert_eq!(report.staged, 0);
    assert_eq!(report.already_present, 1);
    assert!(report.skipped.is_empty());
    assert_eq!(file.pending.get("csl-2"), Some(&existing_entry));
}

/// (c) An actor this computer does not manage is skipped, never substituted.
#[test]
fn an_unmanaged_actor_is_skipped_with_the_create_time_sentence() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let requests = vec![request("csl-3", PUBKEY_B, "builder", None)];

    let (file, report) = restage_actor_seats_with(
        &requests,
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &BTreeMap::new(),
    );

    assert_eq!(report.staged, 0);
    assert!(!file.pending.contains_key("csl-3"));
    assert_eq!(report.skipped.len(), 1);
    let (command_id, reason) = &report.skipped[0];
    assert_eq!(command_id, "csl-3");
    assert_eq!(
        reason,
        &format!("agent {PUBKEY_B} is not a managed agent on this computer")
    );
}

/// (c) A managed actor whose key the keyring could not produce this boot
/// (empty `private_key_nsec` after hydration) is skipped with
/// `build_actor_seat_entry`'s own sentence — the same one the create-time
/// path refuses with.
#[test]
fn an_actor_with_an_empty_key_is_skipped_with_build_actor_seat_entrys_sentence() {
    let records = vec![agent_record(PUBKEY_C, "")];
    let requests = vec![request("csl-4", PUBKEY_C, "builder", None)];
    let mut resolved = BTreeMap::new();
    resolved.insert("csl-4".to_string(), Ok(staged_plan("builder")));

    let (file, report) = restage_actor_seats_with(
        &requests,
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &resolved,
    );

    assert_eq!(report.staged, 0);
    assert!(!file.pending.contains_key("csl-4"));
    assert_eq!(report.skipped.len(), 1);
    let (command_id, reason) = &report.skipped[0];
    assert_eq!(command_id, "csl-4");
    assert!(reason.contains("has no private key available"), "{reason}");
}

/// (d) A `projectRef` whose pack source could not be read from the relay is
/// skipped with the reader's own sentence — never a substitute pack, and
/// nothing is written for that row.
#[test]
fn a_failed_project_pack_source_read_skips_the_row_and_stages_nothing() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let requests = vec![request(
        "csl-5",
        PUBKEY_A,
        "builder",
        Some("30621:deadbeef:project"),
    )];
    let mut resolved = BTreeMap::new();
    let read_failure =
        "the project's pack source could not be read from the relay: no connection".to_string();
    resolved.insert("csl-5".to_string(), Err(read_failure.clone()));

    let (file, report) = restage_actor_seats_with(
        &requests,
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &resolved,
    );

    assert_eq!(report.staged, 0);
    assert!(!file.pending.contains_key("csl-5"));
    assert_eq!(report.skipped, vec![("csl-5".to_string(), read_failure)]);
}

/// (e) A missing `seat-requests.json` is an empty ledger, not an error — the
/// provider has not written one yet, or every open row was already served.
/// `restage_actor_seats_for_provider` returns `Ok(RestageReport::default())`
/// as soon as this is empty, before it ever reads or writes the actor-seats
/// file, which this pins at the level the core logic can be tested at.
#[test]
fn a_missing_seat_requests_file_reads_as_empty() {
    let dir = std::env::temp_dir().join(format!(
        "buzz-seat-requests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    let path = seat_requests_path(&dir);
    let file = read_seat_requests(&path).expect("missing file is not an error");
    assert!(file.requests.is_empty());

    // And the core, given no requests at all, reports nothing and leaves the
    // seats file byte-for-byte as it found it.
    let existing = ActorSeatsFile::default();
    let (updated, report) = restage_actor_seats_with(&[], &existing, &[], RELAY, &BTreeMap::new());
    assert_eq!(report, RestageReport::default());
    assert_eq!(updated, existing);
}

/// (f) Whatever `restage_actor_seats_for_provider` stages goes through the
/// same [`write_actor_seats`] every other seat write does — owner-only, same
/// as the create-time path's own round-trip test in `actor_seats_tests.rs`.
#[test]
fn the_restaged_seats_file_is_written_owner_only() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let requests = vec![request("csl-6", PUBKEY_A, "builder", None)];
    let mut resolved = BTreeMap::new();
    resolved.insert("csl-6".to_string(), Ok(staged_plan("builder")));
    let (updated, report) = restage_actor_seats_with(
        &requests,
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &resolved,
    );
    assert_eq!(report.staged, 1);

    let dir = std::env::temp_dir().join(format!(
        "buzz-actor-seats-restage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = actor_seats_path(&dir);
    write_actor_seats(&path, &updated).expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the restaged seat file must be owner-only"
        );
    }
    assert_eq!(read_actor_seats(&path).expect("read back"), updated);
    std::fs::remove_dir_all(&dir).ok();
}

/// Pins the provider's wire shape for one open row, exactly as
/// `docs/CI_CONTINUATION_RECOVERY_SPEC.md` §3 states it — including the two
/// nullable fields (`projectRef`, `packRef`) as `null`.
#[test]
fn the_provider_wire_shape_parses() {
    let json = serde_json::json!({
        "version": 1,
        "requests": [
            {
                "commandId": "csl-42",
                "actor": PUBKEY_A,
                "role": "builder",
                "projectRef": null,
                "sessionId": "sess-7",
                "generation": 2,
                "packRef": null,
            },
            {
                "commandId": "csl-43",
                "actor": PUBKEY_B,
                "role": "architect",
                "projectRef": "30621:deadbeef:project",
                "sessionId": "sess-8",
                "generation": 1,
                "packRef": {
                    "repo": "30617:deadbeef:packs",
                    "sha": "a".repeat(40),
                    "role": "architect",
                    "path": "personas/roles/architect",
                },
            },
        ],
    });
    let file: SeatRequestsFile =
        serde_json::from_value(json).expect("the provider's shape deserializes");
    assert_eq!(file.version, 1);
    assert_eq!(file.requests.len(), 2);
    assert_eq!(file.requests[0].command_id, "csl-42");
    assert_eq!(file.requests[0].project_ref, None);
    assert!(file.requests[0].pack_ref.is_none());
    assert_eq!(
        file.requests[1].project_ref.as_deref(),
        Some("30621:deadbeef:project")
    );
    assert_eq!(
        file.requests[1].pack_ref.as_ref().map(|p| p.role.as_str()),
        Some("architect")
    );
}

#[test]
fn recovery_rejects_newer_or_unverifiable_role_instructions() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let mut req = request("restore", PUBKEY_A, "builder", None);
    let mut newer = staged_plan("builder");
    newer.pack_ref.as_mut().expect("pack").sha = "b".repeat(40);
    for plan in [newer, {
        let mut local = staged_plan("builder");
        local.pack_ref = None;
        local
    }] {
        let resolved = BTreeMap::from([("restore".into(), Ok(plan))]);
        let (file, report) = restage_actor_seats_with(
            &[req.clone()],
            &ActorSeatsFile::default(),
            &records,
            RELAY,
            &resolved,
        );
        assert!(file.pending.is_empty());
        assert!(report.skipped[0].1.contains("ACTOR_UNAVAILABLE"));
        req.pack_ref = None;
    }
}

#[test]
fn original_repository_pin_survives_a_moving_project_source() {
    let req = request("restore", PUBKEY_A, "builder", None);
    let original = req.pack_ref.as_ref().expect("pack");
    let pinned = pinned_pack_source(&req)
        .expect("valid")
        .expect("repository pin");
    assert_eq!(pinned.repo, original.repo);
    assert_eq!(pinned.sha.as_deref(), Some(original.sha.as_str()));
    assert!(pinned.git_ref.is_none());
    assert_eq!(pinned.path, "personas/roles");
    assert!(ensure_restage_relay("wss://other.test", RELAY).is_err());
}

/// A fenced generation is stated by the provider and skipped here: its
/// umbrella has been handed over, so the seat cannot take a turn on this
/// computer and filing its signing key would be putting a live credential on
/// disk for work this body is not allowed to do.
///
/// Skipped and *named*, not dropped: the report is where an operator finds out
/// the fence is why nothing restarted. The unfenced row beside it still
/// stages, so this is a per-row decision rather than a whole-file one.
#[test]
fn a_fenced_request_is_skipped_and_named_while_its_neighbour_still_stages() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let mut fenced = request("csl-fenced", PUBKEY_A, "builder", None);
    fenced.fenced = true;
    let open = request("csl-open", PUBKEY_A, "builder", None);
    let mut resolved = BTreeMap::new();
    resolved.insert("csl-open".to_string(), Ok(staged_plan("builder")));
    // Deliberately also resolvable: the skip must not depend on the caller
    // having declined to resolve a pack for it.
    resolved.insert("csl-fenced".to_string(), Ok(staged_plan("builder")));

    let (file, report) = restage_actor_seats_with(
        &[fenced, open],
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &resolved,
    );

    assert_eq!(report.requested, 2);
    assert_eq!(report.staged, 1);
    assert!(
        file.pending.contains_key("csl-open"),
        "the unfenced generation still gets its custody"
    );
    assert!(
        !file.pending.contains_key("csl-fenced"),
        "a fenced generation gets no key material"
    );
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].0, "csl-fenced");
    // The exact sentence, not a substring: this one is read by an operator,
    // and it shipped once with eighteen spaces in the middle of it because a
    // formatter rewrapped the literal.
    assert_eq!(
        report.skipped[0].1,
        "HANDOVER_FENCED: this session has been handed over, so its seat is not re-staged on \
         this computer"
    );
    assert!(
        !report.skipped[0].1.contains("  "),
        "no run of spaces survives into the sentence: {:?}",
        report.skipped[0].1
    );
}

/// A retired generation never appears in the file at all — the provider omits
/// it, because unlike a fence it is never coming back. Nothing is staged, and
/// nothing is reported as skipped either, because there was no row to skip.
#[test]
fn a_retired_generation_has_no_row_to_read() {
    let records = vec![agent_record(PUBKEY_A, "nsec1secret")];
    let (file, report) = restage_actor_seats_with(
        &[],
        &ActorSeatsFile::default(),
        &records,
        RELAY,
        &BTreeMap::new(),
    );
    assert_eq!(report.requested, 0);
    assert_eq!(report.staged, 0);
    assert!(report.skipped.is_empty());
    assert!(file.pending.is_empty());
}

/// A provider built before the fence existed writes no `fenced` key. Reading
/// its file must mean "not fenced", not a parse failure.
#[test]
fn a_row_without_the_fenced_key_reads_as_unfenced() {
    let json = serde_json::json!({
        "version": 1,
        "requests": [
            {
                "commandId": "csl-legacy",
                "actor": PUBKEY_A,
                "role": "builder",
                "sessionId": "sess-1",
                "generation": 1,
            },
            {
                "commandId": "csl-fenced",
                "actor": PUBKEY_B,
                "role": "builder",
                "sessionId": "sess-2",
                "generation": 1,
                "fenced": true,
            },
        ],
    });
    let file: SeatRequestsFile =
        serde_json::from_value(json).expect("the provider's shape deserializes");
    assert!(!file.requests[0].fenced, "absent means not fenced");
    assert!(file.requests[1].fenced);
}
