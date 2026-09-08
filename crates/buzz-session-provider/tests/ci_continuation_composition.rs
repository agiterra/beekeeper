//! Real-process composition acceptance for CI-managed turn continuation.
//!
//! `docs/CI_MANAGED_CONTINUATION_IMPL.md` and
//! `docs/CI_MANAGED_CONTINUATION_SPEC.md` require exercising the actual
//! CLI/provider/local-relay composition, not a library-level simulation. That
//! composition is fundamentally three independent OS processes (the real
//! `buzz-relay` binary, the built `bee` CLI, and the real
//! `buzz-session-provider` binary) talking over a real network socket, so the
//! composition itself lives in `scripts/ci-continuation-acceptance.sh` — a
//! Rust `#[tokio::test]` gains nothing over a shell script for orchestrating
//! three subprocesses and would just duplicate its logic. This test is the
//! contractual entry point `cargo test --ignored` (and `just
//! test-ci-continuation`) expect: it builds nothing itself (the `just`
//! recipe builds `bee`/`buzz-relay`/`buzz-session-provider` first, matching
//! `test-ci-completion`'s own shape) and asserts the script's exit code.
//!
//! Read the script's header comment for what each of its eight numbered PASS
//! steps proves. Steps 1-5 (session creation, registration, a *real*
//! webhook-produced 46008, exactly-once admission with the materialized §1c
//! context delivered byte-for-byte through ACP, authenticated `started`
//! status, and the duplicate-result/duplicate-commandId once-admission fence
//! with its durable `DUPLICATE_OPERATION` refusal) exercise the ordinary,
//! no-restart path. Steps 6-8 exercise
//! `docs/CI_CONTINUATION_RECOVERY_SPEC.md` §5's kill/restart composition, all
//! against the real `buzz-session-provider` binary `kill -9`'d and respawned
//! over the same state dir:
//!
//! - **Scenario A** (step 6): a registration left `waiting`, the provider
//!   killed and restarted, THEN the CI result arrives. Exactly one
//!   `turn_started` names the original commandId, the reopened generation's
//!   44223 still names the ORIGINAL target (no generation bump), a
//!   `session_restored_native` transcript row precedes the delivered prompt,
//!   and the ACP prompt bytes equal the transcript's own materialization.
//! - **Scenario B** (step 7): the CI result arrives FIRST (the record reaches
//!   `ready` in the provider's `ci-continuations.json`), THEN the provider is
//!   killed before admission and restarted. Same assertions as scenario A.
//!   The race between observing `ready` and the kill landing before
//!   admission is tight — the store transitions `ready` → `claimed` strictly
//!   before any adapter call, so a record still `ready` immediately after the
//!   kill proves nothing started; a run where the kill lost that race is
//!   retried, up to 3 attempts.
//! - **Scenario C** (step 8): the same waiting→kill→restart→result shape, but
//!   against a second, independent provider process whose ACP stub
//!   advertises `session/load` and then rejects it. The result is a durable
//!   `turn_refused/NATIVE_RESTORE_REJECTED` naming the original commandId,
//!   never a fresh conversation.
//!
//! Every scenario's stub logs each JSON-RPC method it receives to its own
//! `methods.log` in the script's scratch workdir (`FABLE_METHODS_LOG`,
//! written by the accepting stub `fake-agent.sh` and the rejecting stub
//! `reject-agent.sh` the script generates); each scenario asserts against the
//! portion of that log written after its own restart, so a restore that fell
//! through to `session/new` — or that never reached `session/load` at all —
//! fails the composition rather than passing on a stale earlier call.
//!
//! What this composition does not cover (named, not faked): the
//! private-project read path (§3f); a real model — every ACP adapter here is
//! a bash stub; seated (agent-actor) restore — every session this script
//! creates is operator-created and unseated, so `native_restore`'s
//! actor-seat lookup is untested here (see
//! `crates/buzz-session-provider/src/tests/ci_continuation_restore_tests.rs`
//! for that).
//!
//! Run directly:
//! ```text
//! just test-ci-continuation
//! ```
//! or, with the three binaries already built:
//! ```text
//! cargo test -p buzz-session-provider --test ci_continuation_composition -- --ignored
//! ```
//! Set `CI_CONTINUATION_SCENARIOS=basic` in the environment to skip the
//! kill/restart scenarios (steps 6-8) and run only the original five.

use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "spawns real buzz-relay/bee/buzz-session-provider processes against a scratch \
            Postgres database and Redis DB 14; requires Docker Postgres+Redis+MinIO up \
            (`just _ensure-services`) and the three binaries built \
            (`just test-ci-continuation` does both)"]
fn ci_continuation_composes_through_the_real_relay_bee_and_provider() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is crates/buzz-session-provider; the script and the
    // repo root are two levels up.
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/buzz-session-provider has two ancestors up to the repo root");
    let script = repo_root.join("scripts/ci-continuation-acceptance.sh");
    assert!(
        script.is_file(),
        "composition script missing at {}",
        script.display()
    );

    let status = Command::new("bash")
        .arg(&script)
        .current_dir(repo_root)
        // Let the script fall back to its own CARGO_TARGET_DIR-relative
        // binary paths (BEE_BIN/RELAY_BIN/PROVIDER_BIN defaults); the `just`
        // recipe already builds them into the workspace target dir this
        // process itself was built into.
        .status()
        .expect("spawn scripts/ci-continuation-acceptance.sh");

    assert!(
        status.success(),
        "scripts/ci-continuation-acceptance.sh exited with {status}; its own stdout/stderr \
         above (and, on failure, the preserved WORKDIR it prints) has the step-by-step detail"
    );
}
