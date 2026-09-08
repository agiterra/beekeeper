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
//! Read the script's header comment for what each of its five numbered PASS
//! steps proves (session creation, registration, a *real* webhook-produced
//! 46008, exactly-once admission with the materialized §1c context, and the
//! duplicate-result/duplicate-commandId once-admission fence) and what this
//! composition does not cover (the private-project read path, §3f; a real
//! model — the ACP adapter is a bash stub). It also surfaced a same-second
//! receipt-ordering defect in `bee ci continuation status`, since repaired
//! by ranking receipt stages rather than event ids; the assertions here
//! read the raw 44224/44225 events, which need no such ordering.
//!
//! Run directly:
//! ```text
//! just test-ci-continuation
//! ```
//! or, with the three binaries already built:
//! ```text
//! cargo test -p buzz-session-provider --test ci_continuation_composition -- --ignored
//! ```

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
