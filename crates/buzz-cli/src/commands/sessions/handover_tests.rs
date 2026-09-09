//! Unit tests for `bee sessions handover`: arguments, decisions and refusals.
//!
//! Nothing here touches a relay, and nothing here builds a signed event —
//! what goes on the wire, and what is read back off it, lives in
//! `handover_wire_tests.rs` so neither file passes 1,000 lines.

use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_handover::{
    CodingSessionHandoverArtifact, CodingSessionHandoverArtifactKind,
    CodingSessionHandoverContinuation, CodingSessionHandoverMode, CodingSessionHandoverTestOutcome,
    MAX_HANDOVER_MISSING,
};
use buzz_core::coding_session_handover_fold::{HandoverContinuationEntry, HandoverStanding};
use buzz_core::coding_session_team_transaction::CodingSessionTeamFoldContext;
use serde_json::json;

use super::super::operations_reads::SessionAuthority;
use super::*;

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "1f2e3d4c-5b6a-4798-8765-43210fedcba9";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn hex64(byte: &str) -> String {
    byte.repeat(32)
}

// ── argument validation ──────────────────────────────────────────────────

#[test]
fn tests_parse_on_the_first_two_colons_so_a_command_may_contain_colons() {
    let parsed = super::super::handover_checkpoint::parse_tests(&[
        "unit:passed:cargo test -p buzz-cli -- --nocapture: verbose".to_owned(),
    ])
    .expect("parse");
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].name, "unit");
    assert_eq!(parsed[0].outcome, CodingSessionHandoverTestOutcome::Passed);
    assert_eq!(
        parsed[0].command,
        "cargo test -p buzz-cli -- --nocapture: verbose"
    );
}

#[test]
fn an_unknown_test_outcome_is_refused_rather_than_read_as_not_run() {
    let error =
        super::super::handover_checkpoint::parse_tests(&["unit:green:cargo test".to_owned()])
            .expect_err("must refuse");
    assert!(
        matches!(error, CliError::Usage(ref message) if message.contains("passed, failed or not-run")),
        "got {error}"
    );
}

#[test]
fn a_malformed_test_row_is_refused() {
    for bad in ["unit", "unit:passed", "unit::cargo test", ":passed:cargo"] {
        assert!(
            super::super::handover_checkpoint::parse_tests(&[bad.to_owned()]).is_err(),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn a_bare_decision_id_records_that_no_summary_was_stated() {
    let parsed = super::super::handover_checkpoint::parse_decisions(&[hex64("1a")]).expect("parse");
    assert_eq!(parsed[0].event_id, hex64("1a"));
    assert!(
        parsed[0].summary.contains("no summary was stated"),
        "an invented summary would put words in the author's mouth: {}",
        parsed[0].summary
    );
}

#[test]
fn a_decision_summary_is_carried_verbatim() {
    let parsed = super::super::handover_checkpoint::parse_decisions(&[format!(
        "{}:kept the temp index",
        hex64("1a")
    )])
    .expect("parse");
    assert_eq!(parsed[0].summary, "kept the temp index");
}

#[test]
fn a_decision_that_is_not_an_event_id_is_refused() {
    assert!(
        super::super::handover_checkpoint::parse_decisions(&["nope:summary".to_owned()]).is_err()
    );
}

#[test]
fn the_wait_ceiling_is_refused_rather_than_clamped() {
    assert!(super::super::handover_claim::bounded_wait(0).is_err());
    assert!(super::super::handover_claim::bounded_wait(
        super::super::handover_claim::MAX_CLAIM_WAIT_SECONDS + 1
    )
    .is_err());
    assert_eq!(
        super::super::handover_claim::bounded_wait(30).expect("in range"),
        30
    );
}

#[test]
fn missing_lines_are_bounded_without_hiding_the_overflow() {
    let many: Vec<String> = (0..MAX_HANDOVER_MISSING + 5)
        .map(|index| format!("path-{index}"))
        .collect();
    let bounded = super::super::handover_checkpoint::bound_missing(many);
    assert_eq!(bounded.len(), MAX_HANDOVER_MISSING);
    assert!(
        bounded
            .last()
            .expect("last")
            .contains("more path(s) not preserved"),
        "a truncated enumeration must still say there was more: {bounded:?}"
    );
}

#[test]
fn recovered_lines_are_bounded_the_same_way() {
    let many: Vec<String> = (0..10).map(|index| format!("line-{index}")).collect();
    let bounded = super::super::handover_continue::bound_lines(many, 4);
    assert_eq!(bounded.len(), 4);
    assert!(bounded[3].contains("more line(s)"));
}

// ── the native/reconstruct decision ──────────────────────────────────────

#[test]
fn native_is_refused_when_no_live_lease_answers() {
    let error = super::super::handover_continue::decide_plan(true, false, false, true)
        .expect_err("must refuse");
    assert!(
        matches!(error, CliError::Usage(ref message)
            if message.contains("--native requires") && message.contains("no live")),
        "the refusal must name the missing lease rather than falling back silently: got {error}"
    );
}

#[test]
fn native_is_chosen_only_when_reachable_and_already_granted() {
    let (plan, why) =
        super::super::handover_continue::decide_plan(false, false, true, true).expect("decide");
    assert_eq!(
        plan,
        super::super::handover_continue::ContinuationPlan::Native
    );
    assert!(why.contains("live 24223 lease"), "{why}");

    // Reachable but ungranted: the existing grant is the resource consent this
    // increment reuses, so without it the work is reconstructed instead.
    let (plan, why) =
        super::super::handover_continue::decide_plan(false, false, true, false).expect("decide");
    assert_eq!(
        plan,
        super::super::handover_continue::ContinuationPlan::Reconstruct
    );
    assert!(why.contains("holds no grant"), "{why}");

    // Unreachable: reconstruct, whatever the grants say.
    let (plan, why) =
        super::super::handover_continue::decide_plan(false, false, false, true).expect("decide");
    assert_eq!(
        plan,
        super::super::handover_continue::ContinuationPlan::Reconstruct
    );
    assert!(why.contains("no live 24223 lease"), "{why}");
}

#[test]
fn reconstruct_skips_the_reachability_read_entirely() {
    let (plan, why) =
        super::super::handover_continue::decide_plan(false, true, true, true).expect("decide");
    assert_eq!(
        plan,
        super::super::handover_continue::ContinuationPlan::Reconstruct
    );
    assert!(why.contains("reachability was not consulted"), "{why}");
}

#[test]
fn native_and_reconstruct_together_are_refused() {
    assert!(super::super::handover_continue::decide_plan(true, true, true, true).is_err());
}

// ── refusals ─────────────────────────────────────────────────────────────

fn state_with(claim: ClaimState, retirement: Option<String>) -> HandoverState {
    state_with_genesis(claim, retirement, None)
}

/// A state whose genesis read came back empty, with no receipt to explain it.
fn state_with_unreadable_genesis() -> HandoverState {
    state_with_genesis(
        ClaimState::NoClaim,
        None,
        Some(super::GENESIS_UNAVAILABLE_REASON.to_owned()),
    )
}

fn state_with_genesis(
    claim: ClaimState,
    retirement: Option<String>,
    genesis_unavailable: Option<String>,
) -> HandoverState {
    HandoverState {
        channel: CHANNEL.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: GENESIS.to_owned(),
        founder: hex64("f0"),
        authority: Some(SessionAuthority {
            context: CodingSessionTeamFoldContext {
                channel_ref: CHANNEL.to_owned(),
                session_ref: SESSION.to_owned(),
                genesis_ref: GENESIS.to_owned(),
                founder_pubkey: hex64("f0"),
                active_seats: Vec::new(),
                active_grants: Vec::new(),
                verifier_required: false,
            },
            claim: claim.clone(),
            claim_since: Some(1_700_000_000),
            grants: vec![(hex64("0b"), 1_700_000_000)],
            seats: Vec::new(),
            head_event_id: Some(hex64("11")),
            head_seq: 3,
            policy_grants: Vec::new(),
        }),
        fold: HandoverFold {
            claim,
            retired: retirement.is_some(),
            ..HandoverFold::default()
        },
        retirement,
        genesis_unavailable,
    }
}

#[test]
fn every_verb_refuses_a_retired_umbrella() {
    let state = state_with(
        ClaimState::NoClaim,
        Some("the relay published a signed deletion receipt".to_owned()),
    );
    let error = super::super::handover_claim::refuse_retired(&state).expect_err("must refuse");
    assert!(
        matches!(error, CliError::Usage(ref message)
            if message.contains("deleted") && message.contains("nothing here is claimed")),
        "got {error}"
    );
    assert_eq!(crate::error::exit_code(&error), 1);
}

#[test]
fn a_live_umbrella_is_not_refused() {
    let state = state_with(ClaimState::NoClaim, None);
    assert!(super::super::handover_claim::refuse_retired(&state).is_ok());
    assert!(super::super::handover_claim::refuse_unverifiable_genesis(&state).is_ok());
}

#[test]
fn an_empty_genesis_read_is_never_reported_as_a_deletion() {
    // Root's rule: absence is not deletion authority. A relay that is behind,
    // partitioned, or scoping the read away produces the same empty answer as
    // one that applied a deletion, and only a signed receipt tells them apart.
    let state = state_with_unreadable_genesis();
    assert!(
        state.retirement.is_none(),
        "no signed deletion receipt named this session, so it is not retired"
    );
    assert!(
        super::super::handover_claim::refuse_retired(&state).is_ok(),
        "the retired path must not fire on an absent genesis"
    );
    assert!(
        !state.fold.retired,
        "the fold is not told this umbrella was deleted"
    );
}

#[test]
fn claim_and_continue_fail_closed_on_an_unreadable_genesis_with_exit_two() {
    let state = state_with_unreadable_genesis();
    let error =
        super::super::handover_claim::refuse_unverifiable_genesis(&state).expect_err("must refuse");
    assert!(
        matches!(error, CliError::Unverifiable(_)),
        "the refusal is its own state, not a usage error and not a deletion: got {error}"
    );
    let message = error.to_string();
    assert!(
        message.contains(super::GENESIS_UNAVAILABLE_REASON),
        "the exact sentence is what a reader acts on: {message}"
    );
    assert!(
        message.contains("not proof the session was deleted"),
        "the refusal must rule the deletion reading out in words: {message}"
    );
    assert_eq!(
        crate::error::exit_code(&error),
        2,
        "a failed relay read exits 2 (network/relay), not 1 and not the retired path"
    );
}

#[test]
fn status_keeps_deleted_and_unreadable_apart_in_one_sentence_each() {
    assert_eq!(
        state_with(ClaimState::NoClaim, None).existence_line(),
        "retired: no"
    );

    let retired = state_with(
        ClaimState::NoClaim,
        Some("the relay published a signed deletion receipt (abc)".to_owned()),
    );
    let line = retired.existence_line();
    assert!(line.starts_with("retired: yes"), "{line}");

    let unreadable = state_with_unreadable_genesis().existence_line();
    assert!(
        unreadable.contains("Genesis not readable on the relay (not proven deleted)"),
        "the unreadable case must say it is not proven deleted: {unreadable}"
    );
    assert!(
        !unreadable.starts_with("retired: yes"),
        "and it must never read as a deletion: {unreadable}"
    );
}

#[test]
fn standing_is_the_founder_or_a_live_operator_and_nothing_else() {
    let state = state_with(ClaimState::NoClaim, None);
    assert!(
        state.has_standing(&hex64("f0")),
        "the founder always has it"
    );
    assert!(state.has_standing(&hex64("0b")), "a live operator has it");
    assert!(
        !state.has_standing(&hex64("cc")),
        "a stranger does not, and a seat is deliberately not enough"
    );
}

#[test]
fn a_lost_race_exits_five_and_never_retries() {
    // The exit code is the contract `bee` publishes (5 = write conflict), so
    // it is asserted rather than left to the mapping table.
    let error = CliError::Conflict("this takeover lost the race at the relay".to_owned());
    assert_eq!(crate::error::exit_code(&error), 5);
}

// ── idempotent rerun ─────────────────────────────────────────────────────

fn continuation_entry(claim_ref: &str, author: &str) -> HandoverContinuationEntry {
    HandoverContinuationEntry {
        event_id: hex64("ee"),
        author: author.to_owned(),
        created_at: 1_700_000_100,
        claim_ref: claim_ref.to_owned(),
        mode: CodingSessionHandoverMode::Reconstructed,
        standing: HandoverStanding::Authorized,
        body: CodingSessionHandoverContinuation {
            claim_ref: claim_ref.to_owned(),
            mode: CodingSessionHandoverMode::Reconstructed,
            checkpoint_ref: None,
            target: CodingSessionTarget {
                driver: "claude-agent-acp".to_owned(),
                instance_id: "inst".to_owned(),
                session_id: "sess".to_owned(),
                generation: 1,
            },
            recovered: Vec::new(),
            missing: Vec::new(),
            note: None,
        },
    }
}

#[test]
fn a_rerun_prints_the_existing_continuation_and_creates_no_second_execution() {
    let claimant = hex64("0b");
    let claim = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: claimant.clone(),
        body_pubkey: hex64("bb"),
        accepted_event_id: hex64("aa"),
        seq: 4,
    });
    let mut state = state_with(claim, None);
    state.fold.continuations = vec![continuation_entry(&hex64("aa"), &claimant)];
    state.fold.active_continuation = Some(hex64("ee"));

    let printed = super::super::handover_continue::already_continued(&state, &claimant)
        .expect("rerun answer");
    let value: serde_json::Value = serde_json::from_str(&printed).expect("json");
    assert_eq!(value["rerun"], json!(true));
    assert_eq!(value["eventId"], json!(hex64("ee")));
    assert!(
        value["message"]
            .as_str()
            .expect("message")
            .contains("no second execution"),
        "{value}"
    );
}

#[test]
fn a_rerun_by_somebody_else_is_not_treated_as_already_done() {
    let claimant = hex64("0b");
    let claim = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: claimant.clone(),
        body_pubkey: hex64("bb"),
        accepted_event_id: hex64("aa"),
        seq: 4,
    });
    let mut state = state_with(claim, None);
    state.fold.continuations = vec![continuation_entry(&hex64("aa"), &claimant)];
    state.fold.active_continuation = Some(hex64("ee"));
    assert!(
        super::super::handover_continue::already_continued(&state, &hex64("cc")).is_none(),
        "the rerun shortcut belongs to the claimant alone"
    );
}

#[test]
fn a_claim_with_no_continuation_yet_is_not_a_rerun() {
    let claimant = hex64("0b");
    let claim = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: claimant.clone(),
        body_pubkey: hex64("bb"),
        accepted_event_id: hex64("aa"),
        seq: 4,
    });
    let state = state_with(claim, None);
    assert!(
        super::super::handover_continue::already_continued(&state, &claimant).is_none(),
        "an interrupted first run must be resumable, not short-circuited"
    );
}

// ── the base a patch sits on ─────────────────────────────────────────────

fn artifact(kind: CodingSessionHandoverArtifactKind, tag: &str) -> CodingSessionHandoverArtifact {
    CodingSessionHandoverArtifact {
        kind,
        repo_ref: tag.to_owned(),
        r#ref: (kind == CodingSessionHandoverArtifactKind::WipRef)
            .then(|| "refs/heads/wip/owner/1f2e3d4c".to_owned()),
        sha: (kind == CodingSessionHandoverArtifactKind::WipRef).then(|| "a".repeat(40)),
        event_id: (kind == CodingSessionHandoverArtifactKind::Patch).then(|| hex64("ef")),
        hash: (kind == CodingSessionHandoverArtifactKind::Blob).then(|| hex64("bc")),
        base_sha: (kind != CodingSessionHandoverArtifactKind::WipRef).then(|| "a".repeat(40)),
        bytes: (kind != CodingSessionHandoverArtifactKind::WipRef).then_some(64),
    }
}

#[test]
fn the_wip_ref_is_checked_out_first_whatever_order_the_record_lists() {
    use super::super::handover_reconstruct::split_artifacts;

    // A record that lists the patch before the ref it applies to. Artifact
    // order is the author's, and nobody promised it (REVIEW N9).
    let artifacts = vec![
        artifact(CodingSessionHandoverArtifactKind::Patch, "patch-first"),
        artifact(CodingSessionHandoverArtifactKind::WipRef, "the-base"),
        artifact(CodingSessionHandoverArtifactKind::Blob, "blob-last"),
    ];
    let (bases, overlays) = split_artifacts(&artifacts);
    assert_eq!(bases.len(), 1);
    assert_eq!(bases[0].repo_ref, "the-base", "the base is taken first");
    assert_eq!(
        overlays
            .iter()
            .map(|artifact| artifact.repo_ref.as_str())
            .collect::<Vec<_>>(),
        vec!["patch-first", "blob-last"],
        "and the overlays keep their own relative order"
    );
}

#[test]
fn a_patch_is_never_applied_onto_a_base_that_did_not_land() {
    use super::super::handover_reconstruct::BaseState;

    assert!(
        BaseState::CheckedOut.may_apply_overlays(),
        "the base landed, so its diffs go on top of it"
    );
    assert!(
        BaseState::NoWipRef.may_apply_overlays(),
        "no base was named, so the caller's own checkout is the base"
    );
    assert!(
        !BaseState::CheckoutFailed.may_apply_overlays(),
        "a named base that did not land stops the apply: a three-way merge onto some other \
         commit would report 'recovered' over a tree that is not this work"
    );
}

// ── binding the create to the recovered checkout ─────────────────────────

/// The provider's own decoder, so what this command writes is checked against
/// the type that will read it rather than against a copy of it.
fn provider_reads(body: &str) -> serde_json::Value {
    serde_json::from_str(body).expect("the provider parses this file with serde_json")
}

#[test]
fn the_written_projects_file_carries_the_pending_entry_in_the_providers_key_set() {
    use super::super::handover_reconstruct::bind_pending_directory;

    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("recovered");
    std::fs::create_dir_all(&work).expect("mkdir");
    let path = dir.path().join("projects.json");

    let binding = bind_pending_directory(&path, "cmd-1", &work).expect("bind");
    let body = std::fs::read_to_string(&path).expect("read back");
    let document = provider_reads(&body);

    // The provider's exact key set: version, pending, projects, channels,
    // camelCase, all `#[serde(default)]`
    // (`crates/buzz-session-provider/src/commands.rs`, `ProjectsFile`).
    assert_eq!(document["version"], serde_json::json!(1));
    assert_eq!(
        document["pending"]["cmd-1"],
        serde_json::json!(binding.directory.to_string_lossy()),
        "the pending hint is keyed by the create's commandId, which is what \
         ProjectsFile::resolve consults first"
    );
    assert!(
        binding.directory.is_absolute(),
        "a relative path is ignored by usable_directory, so the binding stores an absolute one"
    );
    // Round-trips through a struct with the provider's shape.
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    #[derive(Default)]
    struct ProvidersShape {
        version: u32,
        pending: std::collections::BTreeMap<String, std::path::PathBuf>,
        projects: std::collections::BTreeMap<String, std::path::PathBuf>,
        channels: std::collections::BTreeMap<uuid::Uuid, std::path::PathBuf>,
    }
    let decoded: ProvidersShape = serde_json::from_str(&body).expect("provider decode");
    assert_eq!(decoded.version, 1);
    assert_eq!(decoded.pending.get("cmd-1"), Some(&binding.directory));
    assert!(decoded.projects.is_empty());
    assert!(decoded.channels.is_empty());
}

#[test]
fn an_existing_projects_file_keeps_every_entry_it_already_had() {
    use super::super::handover_reconstruct::bind_pending_directory;

    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("recovered");
    std::fs::create_dir_all(&work).expect("mkdir");
    let path = dir.path().join("projects.json");
    // Including a key this build does not know: read-modify-write is done on
    // the JSON, not through a typed struct, so a newer provider's field is not
    // silently dropped by an older CLI.
    let existing = serde_json::json!({
        "version": 2,
        "pending": { "older-command": "/tmp/older" },
        "projects": { "30078:ab:proj": "/tmp/project-a" },
        "channels": { "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86": "/tmp/channel-a" },
        "somethingNewer": { "kept": true },
    });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&existing).expect("json"),
    )
    .expect("seed");

    let binding = bind_pending_directory(&path, "cmd-2", &work).expect("bind");
    let after = provider_reads(&std::fs::read_to_string(&path).expect("read back"));

    assert_eq!(
        after["version"],
        serde_json::json!(2),
        "the owner's version stands"
    );
    assert_eq!(after["projects"], existing["projects"]);
    assert_eq!(after["channels"], existing["channels"]);
    assert_eq!(after["somethingNewer"], existing["somethingNewer"]);
    assert_eq!(
        after["pending"]["older-command"],
        serde_json::json!("/tmp/older"),
        "another create's pending hint is not this command's to remove"
    );
    assert_eq!(
        after["pending"]["cmd-2"],
        serde_json::json!(binding.directory.to_string_lossy())
    );
}

#[test]
fn a_malformed_projects_file_is_refused_rather_than_replaced() {
    use super::super::handover_reconstruct::bind_pending_directory;

    let dir = tempfile::tempdir().expect("tempdir");
    let work = dir.path().join("recovered");
    std::fs::create_dir_all(&work).expect("mkdir");
    let path = dir.path().join("projects.json");
    std::fs::write(&path, "{ this is not json").expect("seed");

    let error = bind_pending_directory(&path, "cmd-3", &work).expect_err("must refuse");
    assert!(error.to_string().contains("does not parse"), "got {error}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "{ this is not json",
        "the owner's file is left exactly as it was"
    );
}

#[test]
fn no_projects_file_refuses_with_the_remedy_before_anything_is_claimed() {
    use super::super::handover_reconstruct::{projects_file_remedy, resolve_projects_file};

    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("recovered");
    std::fs::create_dir_all(&cwd).expect("mkdir");

    // The env default is the provider's own variable; with neither it nor the
    // flag, this refuses. Read without mutating the process environment, so
    // the test cannot race another.
    let remedy = projects_file_remedy(&cwd);
    assert!(
        remedy.contains(
            "the provider resolves its working directory from its projects file, \
                         not from this command"
        ) && remedy.contains("--projects-file")
            && remedy.contains("BUZZ_CSP_PROJECTS_FILE")
            && remedy.contains("<state-dir>/projects.json")
            && remedy.contains(&cwd.display().to_string()),
        "the refusal names the mechanism, the flag, the variable, the desktop path and the \
         directory: {remedy}"
    );

    // An explicit flag always wins, whatever the environment says.
    let explicit = dir.path().join("explicit.json");
    assert_eq!(
        resolve_projects_file(Some(&explicit), &cwd).expect("explicit"),
        explicit
    );
}

#[test]
fn a_pending_entry_beats_the_projects_mapping_for_the_same_create() {
    use super::super::handover_reconstruct::bind_pending_directory;

    // Two folders: the projects file maps the project to A, and `--cwd` is B.
    // `ProjectsFile::resolve` tries `pending[commandId]` before
    // `projects[projectRef]`
    // (`crates/buzz-session-provider/src/commands.rs`, the `candidates` array
    // in `resolve`), so the entry this writes is the one that wins.
    let dir = tempfile::tempdir().expect("tempdir");
    let folder_a = dir.path().join("project-a");
    let folder_b = dir.path().join("recovered-b");
    std::fs::create_dir_all(&folder_a).expect("mkdir a");
    std::fs::create_dir_all(&folder_b).expect("mkdir b");
    let path = dir.path().join("projects.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "version": 1,
            "pending": {},
            "projects": { "30078:ab:proj": folder_a.to_string_lossy() },
            "channels": {},
        })
        .to_string(),
    )
    .expect("seed");

    let binding = bind_pending_directory(&path, "cmd-4", &folder_b).expect("bind");
    let after = provider_reads(&std::fs::read_to_string(&path).expect("read"));

    assert_eq!(
        after["pending"]["cmd-4"],
        serde_json::json!(binding.directory.to_string_lossy()),
        "the pending hint is the recovered checkout"
    );
    assert_eq!(
        after["projects"]["30078:ab:proj"],
        serde_json::json!(folder_a.to_string_lossy()),
        "and the project's own mapping is untouched — the pending hint wins by precedence, \
         not by overwriting somebody's configuration"
    );
    assert_ne!(
        binding.directory.canonicalize().expect("b"),
        folder_a.canonicalize().expect("a"),
        "the two folders really are different"
    );
}

#[test]
fn an_execution_running_somewhere_else_is_named_in_missing() {
    use super::super::handover_reconstruct::workdir_mismatch_line;

    let head = "a".repeat(40);
    // Where it should be.
    assert!(workdir_mismatch_line(
        Some("handover/1f2e3d4c"),
        Some(&head),
        "handover/1f2e3d4c",
        Some(&head)
    )
    .is_none());

    // The old mapped folder: a different branch entirely.
    let line = workdir_mismatch_line(
        Some("main"),
        Some(&"b".repeat(40)),
        "handover/1f2e3d4c",
        Some(&head),
    )
    .expect("a mismatch must be named");
    assert_eq!(
        line,
        format!(
            "the execution reports branch main at {}, not the recovered checkout — it is \
             running somewhere else",
            "b".repeat(40)
        )
    );

    // Right branch, wrong commit — still somewhere else.
    assert!(workdir_mismatch_line(
        Some("handover/1f2e3d4c"),
        Some(&"c".repeat(40)),
        "handover/1f2e3d4c",
        Some(&head)
    )
    .is_some());

    // No worktree probe at all is a mismatch: absence is not confirmation.
    assert!(workdir_mismatch_line(None, None, "handover/1f2e3d4c", Some(&head)).is_some());

    // A checkpoint with no headSha has one fewer fact to compare, not a
    // mismatch to invent.
    assert!(
        workdir_mismatch_line(Some("handover/1f2e3d4c"), None, "handover/1f2e3d4c", None).is_none()
    );
}
