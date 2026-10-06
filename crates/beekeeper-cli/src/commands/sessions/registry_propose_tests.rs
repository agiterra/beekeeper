//! Tests for [`super`] — the only writer, proven against a **fake relay**.
//!
//! Every event here is a hand-built JSON value in the shape the relay hands
//! back. Nothing in this file talks to a relay, and no test publishes anything:
//! `propose` is a reading of signed events, so what it must be tested against
//! is a set of signed events.

use std::path::PathBuf;

use serde_json::json;

use super::*;

use beekeeper_core::registry_bench::bench_gate_name;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn bench_root() -> PathBuf {
    repo_root().join("team/registry-bench")
}

fn registry_hash(path: &Path) -> String {
    use sha2::{Digest as _, Sha256};
    let text = std::fs::read_to_string(path).expect("read");
    hex::encode(Sha256::digest(text.as_bytes()))
}

fn shipped_registry() -> Registry {
    let text = std::fs::read_to_string(repo_root().join("team/model-registry.yaml")).expect("read");
    beekeeper_core::coding_session_routing::parse_registry(&text).expect("parses")
}

const SEAT: &str = "11111111111111111111111111111111111111111111111111111111111111aa";
const OTHER: &str = "22222222222222222222222222222222222222222222222222222222222222bb";

fn gate_event(id: &str, signer: &str, gate: &str, summary: &str) -> Value {
    json!({
        "id": id,
        "pubkey": signer,
        "content": json!({
            "schema": "csob/1",
            "sessionRef": "s",
            "genesisRef": "g",
            "type": "gate",
            "assignmentRef": Value::Null,
            "body": { "rows": [{
                "gate": gate,
                "outcome": if summary.contains("failed: none") { "passed" } else { "failed" },
                "command": "/usr/bin/true --acp --model opus[1m]",
                "summary": summary,
                "durationMs": 4200,
            }]},
        })
        .to_string(),
    })
}

fn manifest_event(id: &str, signer: &str, role: &str, version: u32, hash: &str, run: u32) -> Value {
    json!({
        "id": id,
        "pubkey": signer,
        "content": json!({
            "schema": "csob/1",
            "sessionRef": "s",
            "genesisRef": "g",
            "type": "checkpoint",
            "assignmentRef": Value::Null,
            "body": {
                "phase": "gates",
                "testsWritten": 0, "testsRed": 0, "testsGreen": 0,
                "lastCommand": "/usr/bin/true --acp --model opus[1m]",
                "lastSummary": "registry-bench run",
                "note": json!({
                    "kind": "registry-bench-manifest",
                    "role": role,
                    "benchVersion": version,
                    "benchHash": hash,
                    "runtime": "claude-primary",
                    "model": "opus[1m]",
                    "repeat": 3,
                    "run": run,
                }).to_string(),
            },
        })
        .to_string(),
    })
}

/// Three clean runs of the verifier bench, as the relay would hand them back.
fn clean_session(hash: &str) -> Vec<Value> {
    let mut events = Vec::new();
    for run in 0..3 {
        events.push(manifest_event(
            &format!("m{run}"),
            SEAT,
            "verifier",
            2,
            hash,
            run,
        ));
        events.push(gate_event(
            &format!("g{run}"),
            SEAT,
            &bench_gate_name("verifier", "planted-defect", run),
            "22/22 · failed: none",
        ));
    }
    events
}

// ── reading the relay ────────────────────────────────────────────────────────

#[test]
fn evidence_is_read_from_signed_events_and_ordered_by_run() {
    let evidence = read_evidence(&clean_session("abc"), "verifier");
    assert_eq!(evidence.gate_rows.len(), 3);
    assert_eq!(
        evidence
            .gate_rows
            .iter()
            .map(|row| row.run)
            .collect::<Vec<u32>>(),
        vec![0, 1, 2]
    );
    assert_eq!(evidence.bench_version, Some(2));
    assert_eq!(evidence.bench_hash.as_deref(), Some("abc"));
}

/// A gate row for another role, or a gate name this lane did not write, is
/// ignored — never counted as a run behind a measurement.
#[test]
fn a_row_that_is_not_this_roles_bench_is_ignored_rather_than_guessed_at() {
    let mut events = clean_session("abc");
    events.push(gate_event(
        "x1",
        SEAT,
        "registry-bench/builder/red-test#0",
        "1/1 · failed: none",
    ));
    events.push(gate_event("x2", SEAT, "just ci", "green"));
    let evidence = read_evidence(&events, "verifier");
    assert_eq!(evidence.gate_rows.len(), 3, "{:?}", evidence.gate_rows);
}

/// Two bench versions on one session is two benches, and a row built across
/// them measures nothing in particular.
#[test]
fn two_bench_versions_on_one_session_leave_no_single_version() {
    let mut events = clean_session("abc");
    events.push(manifest_event("m9", SEAT, "verifier", 3, "abc", 9));
    let evidence = read_evidence(&events, "verifier");
    assert_eq!(evidence.bench_version, None);
    assert_eq!(evidence.versions_seen.len(), 2);
}

// ── the numbers are re-derived from the wire ─────────────────────────────────

/// The medians come from the gate rows' own summaries plus the checked-in
/// rubric — signed events and checked-in code, and nothing else.
#[test]
fn the_medians_are_recomputed_from_the_gate_rows_own_summaries() {
    let tasks = load_role_bench(&bench_root(), "verifier").expect("bench").1;
    let evidence = read_evidence(&clean_session("abc"), "verifier");
    let traits = traits_from_relay(&evidence, &tasks);
    for name in ["reasoning", "judgment", "verification", "discipline"] {
        assert!((traits[name].score - 5.0).abs() < f64::EPSILON, "{name}");
        assert_eq!(traits[name].n, 3);
    }
    assert!(!traits.contains_key("taste"));
}

/// A failed criterion on one run and not another shows up as a spread, which
/// is the honest shape of a non-deterministic model.
#[test]
fn a_criterion_that_failed_on_one_run_only_shows_up_as_spread() {
    let mut events = clean_session("abc");
    // Replace run 1's row with one that failed the judgment criterion.
    events.retain(|event| event["id"] != "g1");
    events.push(gate_event(
        "g1",
        SEAT,
        &bench_gate_name("verifier", "planted-defect", 1),
        "16/22 · failed: reports-no-absent-defect",
    ));
    let tasks = load_role_bench(&bench_root(), "verifier").expect("bench").1;
    let traits = traits_from_relay(&read_evidence(&events, "verifier"), &tasks);
    // Partial credit: the failing run scores judgment 4 of 10 → 2.6, so the
    // three runs are 5.0, 2.6, 5.0 → median 5.0, min 2.6, spread 2.4.
    assert!((traits["judgment"].score - 5.0).abs() < f64::EPSILON);
    assert!((traits["judgment"].min - 2.6).abs() < f64::EPSILON);
    assert!(
        traits["judgment"].is_unstable(),
        "a spread of 2.4 must mark the set unstable"
    );
}

// ── what it refuses ──────────────────────────────────────────────────────────

fn refusals_for(events: &[Value], repeat: u32, tree_hash: &str) -> Vec<String> {
    let registry = shipped_registry();
    let (set, tasks) = load_role_bench(&bench_root(), "verifier").expect("bench");
    let evidence = read_evidence(events, "verifier");
    let traits = traits_from_relay(&evidence, &tasks);
    proposal_refusals(&ProposalInput {
        role: "verifier",
        bench_version: evidence.bench_version.unwrap_or(set.bench_version),
        tree_bench_version: set.bench_version,
        bench_hash: evidence.bench_hash.as_deref().unwrap_or(""),
        tree_bench_hash: tree_hash,
        repeat,
        tasks: &set.tasks,
        gate_rows: &evidence.gate_rows,
        minimums: &registry.classes["verifier"].minimums,
        traits: &traits,
    })
}

#[test]
fn a_clean_three_run_session_is_refused_for_nothing() {
    assert!(refusals_for(&clean_session("abc"), 3, "abc").is_empty());
}

#[test]
fn fewer_runs_than_asked_for_is_named_by_task() {
    let refusals = refusals_for(&clean_session("abc"), 5, "abc");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("task planted-defect has 3 run(s)")),
        "{refusals:?}"
    );
}

#[test]
fn a_bench_that_changed_under_the_measurement_is_refused() {
    let refusals = refusals_for(&clean_session("abcdef012345"), 3, "999999999999");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("changed under the measurement")),
        "{refusals:?}"
    );
}

#[test]
fn rows_from_two_signing_keys_are_refused() {
    let mut events = clean_session("abc");
    events.push(gate_event(
        "g9",
        OTHER,
        &bench_gate_name("verifier", "planted-defect", 3),
        "22/22 · failed: none",
    ));
    let refusals = refusals_for(&events, 3, "abc");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("2 signing keys")),
        "{refusals:?}"
    );
}

#[test]
fn an_unstable_set_is_refused_with_the_spread_that_made_it_unstable() {
    let mut events = clean_session("abc");
    events.retain(|event| event["id"] != "g1");
    events.push(gate_event(
        "g1",
        SEAT,
        &bench_gate_name("verifier", "planted-defect", 1),
        "16/22 · failed: reports-no-absent-defect",
    ));
    let refusals = refusals_for(&events, 3, "abc");
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("UNSTABLE on")),
        "{refusals:?}"
    );
    assert!(
        refusals
            .iter()
            .any(|refusal| refusal.contains("judgment 2.6–5")),
        "{refusals:?}"
    );
}

// ── the fragment, and what it says about itself ──────────────────────────────

fn block() -> MeasuredBlock {
    MeasuredBlock {
        role: "verifier".to_owned(),
        bench_version: 1,
        bench_hash: "abc".to_owned(),
        measured_at: "2026-09-02".to_owned(),
        measured_by: SEAT.to_owned(),
        runs: vec!["g0".to_owned(), "g1".to_owned(), "g2".to_owned()],
        traits: BTreeMap::from([
            (
                "reasoning".to_owned(),
                MeasuredTrait {
                    score: 4.6,
                    n: 3,
                    min: 4.4,
                    max: 4.8,
                },
            ),
            (
                "judgment".to_owned(),
                MeasuredTrait {
                    score: 4.8,
                    n: 3,
                    min: 4.7,
                    max: 4.9,
                },
            ),
            (
                "verification".to_owned(),
                MeasuredTrait {
                    score: 4.9,
                    n: 3,
                    min: 4.8,
                    max: 5.0,
                },
            ),
        ]),
    }
}

#[test]
fn the_fragment_carries_every_run_and_a_derived_confidence() {
    let yaml = render_measured_yaml("claude-primary/opus[1m]", &block());
    assert!(yaml.contains("benchVersion: 1"));
    assert!(yaml.contains("        - \"g0\""));
    assert!(yaml.contains("reasoning: { score: 4.6, n: 3, min: 4.4, max: 4.8 }"));
    assert!(yaml.contains("status: measured, confidence: medium"));
    assert!(
        !yaml.contains("operational_opinion"),
        "a measured row does not still claim to be an opinion"
    );
}

/// The disclosure says which of the ten are measurements and which are still
/// opinions. A half-measured row reading as measured is the same lie as a
/// badge with no event behind it.
#[test]
fn the_disclosure_separates_the_measured_traits_from_the_opinions() {
    let text = provenance_disclosure(&shipped_registry(), &block());
    assert!(
        text.contains("measured by registry-bench/verifier v1: judgment, reasoning, verification")
    );
    assert!(
        text.contains(
            "STILL OPINIONS on this row: coding, taste, agency, discipline, context, velocity, \
             costEfficiency"
        ),
        "unexpected: {text}"
    );
    assert!(text.contains("derived from n"));
}

// ── --write ──────────────────────────────────────────────────────────────────

#[test]
fn write_amends_the_named_row_and_refuses_to_stack_a_second_measurement() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("model-registry.yaml");
    std::fs::copy(repo_root().join("team/model-registry.yaml"), &path).expect("copy");

    let hash = registry_hash(&path);
    apply_fragment(&path, &hash, "claude-primary/opus[1m]", &block()).expect("applies");
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("      benchHash: \"abc\""));
    assert!(text.contains("status: measured, confidence: medium"));

    // Still a registry, and only the one row moved.
    let registry = beekeeper_core::coding_session_routing::parse_registry(&text)
        .unwrap_or_else(|error| panic!("the amended file must still parse: {error}"));
    let measured: Vec<String> = registry
        .targets
        .iter()
        .filter(|target| target.is_measured())
        .map(beekeeper_core::coding_session_routing::RegistryTarget::label)
        .collect();
    assert_eq!(measured, vec!["claude-primary/opus[1m]".to_owned()]);

    let error = apply_fragment(
        &path,
        &registry_hash(&path),
        "claude-primary/opus[1m]",
        &block(),
    )
    .expect_err("a second measurement must be refused");
    assert!(error
        .to_string()
        .contains("already carries a measured block"));
}

#[test]
fn write_refuses_a_row_that_is_not_there_rather_than_guessing_where_it_goes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("model-registry.yaml");
    std::fs::copy(repo_root().join("team/model-registry.yaml"), &path).expect("copy");
    let error = apply_fragment(
        &path,
        &registry_hash(&path),
        "codex-primary/gpt-6-nova",
        &block(),
    )
    .expect_err("refuses");
    assert!(error.to_string().contains("refusing to guess"));
}

// ── fix round 1 ──────────────────────────────────────────────────────────────

/// F2 — a summary-less row used to fold as a perfect run: three
/// `bee sessions observe gate` calls with no `--summary` minted a verifier row
/// clearing every ratified minimum. It now contributes **no score at all**, and
/// the proposal is refused by row id.
#[test]
fn summary_less_rows_score_nothing_and_refuse_the_proposal() {
    let mut events = Vec::new();
    for run in 0..3 {
        events.push(manifest_event(
            &format!("m{run}"),
            SEAT,
            "verifier",
            2,
            "abc",
            run,
        ));
        // A gate row exactly as `observe gate` mints it without `--summary`.
        let mut event = gate_event(
            &format!("g{run}"),
            SEAT,
            &bench_gate_name("verifier", "planted-defect", run),
            "22/22 · failed: none",
        );
        let mut payload: Value =
            serde_json::from_str(event["content"].as_str().expect("content")).expect("json");
        payload["body"]["rows"][0]["summary"] = Value::Null;
        event["content"] = Value::String(payload.to_string());
        events.push(event);
    }
    let evidence = read_evidence(&events, "verifier");
    let tasks = load_role_bench(&bench_root(), "verifier").expect("bench").1;

    // Nothing is scored — not "everything passed".
    let traits = traits_from_relay(&evidence, &tasks);
    assert!(
        traits.is_empty(),
        "a row that cannot say what failed is not evidence: {traits:?}"
    );

    let refusals = gate_row_refusals(&evidence.gate_rows);
    assert_eq!(refusals.len(), 3, "{refusals:?}");
    assert!(
        refusals[0].contains("has no parseable summary"),
        "{refusals:?}"
    );
}

/// F3 — `outcome` is consulted, and a row whose two fields disagree is REFUSED
/// by name rather than coerced into agreement with one of them. Where they
/// agree, the summary's per-criterion detail is what gets scored — the same
/// arithmetic `measure` ran, so both sides read one number from one row.
#[test]
fn a_failed_row_scores_exactly_what_its_summary_names() {
    let mut events = Vec::new();
    for run in 0..3 {
        events.push(manifest_event(
            &format!("m{run}"),
            SEAT,
            "verifier",
            2,
            "abc",
            run,
        ));
        events.push(gate_event(
            &format!("g{run}"),
            SEAT,
            &bench_gate_name("verifier", "planted-defect", run),
            "0/22 · failed: names-the-defect, reports-no-absent-defect, \
             verdict-is-changes-requested, wrote-nothing-else, exited-clean",
        ));
    }
    let evidence = read_evidence(&events, "verifier");
    assert!(evidence.gate_rows.iter().all(BenchGateRow::says_failed));
    let tasks = load_role_bench(&bench_root(), "verifier").expect("bench").1;
    // This row names EVERY criterion as failed, so 1.0 across the board — and
    // it is 1.0 because the summary says so, not because the word `failed`
    // overrode it.
    let traits = traits_from_relay(&evidence, &tasks);
    for name in ["judgment", "reasoning", "verification", "discipline"] {
        assert!(
            (traits[name].score - 1.0).abs() < f64::EPSILON,
            "{name}: {:?}",
            traits[name]
        );
    }
    assert!(
        gate_row_refusals(&evidence.gate_rows).is_empty(),
        "the row is consistent"
    );
}

/// The contradiction itself: neither field wins, the row is refused by name.
#[test]
fn a_row_whose_outcome_and_summary_disagree_is_refused_not_coerced() {
    let mut passed_but_failures = gate_row_fixture();
    passed_but_failures.outcome = "passed".to_owned();
    passed_but_failures.summary = Some("16/22 · failed: reports-no-absent-defect".to_owned());
    let refusals = gate_row_refusals(&[passed_but_failures]);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(refusals[0].contains("says outcome passed"), "{refusals:?}");
    assert!(refusals[0].contains("cannot say both"), "{refusals:?}");

    let mut failed_but_clean = gate_row_fixture();
    failed_but_clean.outcome = "failed".to_owned();
    failed_but_clean.summary = Some("22/22 · failed: none".to_owned());
    let refusals = gate_row_refusals(&[failed_but_clean]);
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(refusals[0].contains("says outcome failed"), "{refusals:?}");
    assert!(refusals[0].contains("cannot say both"), "{refusals:?}");
}

fn gate_row_fixture() -> BenchGateRow {
    BenchGateRow {
        event_id: "g0".to_owned(),
        signer: SEAT.to_owned(),
        gate: bench_gate_name("verifier", "planted-defect", 0),
        role: "verifier".to_owned(),
        task_id: "planted-defect".to_owned(),
        run: 0,
        outcome: "passed".to_owned(),
        summary: Some("22/22 · failed: none".to_owned()),
    }
}

/// F7 — the registry moved between the read and the write. `propose` runs a
/// relay query in between, and a human runs `--write` after reading the output.
#[test]
fn write_refuses_when_the_registry_moved_since_it_was_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("model-registry.yaml");
    std::fs::copy(repo_root().join("team/model-registry.yaml"), &path).expect("copy");
    let hash_at_read = registry_hash(&path);

    // Somebody else edits the file while the proposal is being built.
    let text = std::fs::read_to_string(&path).expect("read");
    std::fs::write(&path, format!("{text}\n# a comment somebody added\n")).expect("write");

    let error = apply_fragment(&path, &hash_at_read, "claude-primary/opus[1m]", &block())
        .expect_err("a moved file must refuse");
    let message = error.to_string();
    assert!(
        message.contains("changed since this proposal read it"),
        "{message}"
    );
    assert!(message.contains("re-run propose"), "{message}");

    // And nothing was written.
    assert!(!std::fs::read_to_string(&path)
        .expect("read")
        .contains("benchHash"));
}

/// F15 — `apply_fragment` used to keep only what preceded `rating:`, silently
/// deleting every key after it. Safe against today's file by accident; a
/// landmine the day somebody appends one.
#[test]
fn write_preserves_a_key_that_follows_the_rating_block() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("model-registry.yaml");
    let text = std::fs::read_to_string(repo_root().join("team/model-registry.yaml")).expect("read");
    // Append a key after `rating:` on the row we are about to amend.
    let anchor = "  - provider: claude-primary\n    model: opus[1m]\n";
    let start = text.find(anchor).expect("anchor");
    let end = text[start..]
        .find("\n  - provider: ")
        .map_or(text.len(), |offset| start + offset + 1);
    let row = &text[start..end];
    let widened = format!("{}\n    notes: \"keep me\"\n", row.trim_end());
    let widened_text = format!("{}{widened}{}", &text[..start], &text[end..]);
    std::fs::write(&path, &widened_text).expect("write");

    apply_fragment(
        &path,
        &registry_hash(&path),
        "claude-primary/opus[1m]",
        &block(),
    )
    .expect("applies");
    let after = std::fs::read_to_string(&path).expect("read");
    assert!(
        after.contains("    notes: \"keep me\""),
        "the key after rating: must survive:\n{after}"
    );
    assert!(after.contains("status: measured"));
    // `notes` is deliberately a key this schema does not know — it stands for
    // the key somebody appends after `rating` one day, which is exactly the
    // case F15 is about. Strip it and the file is still a registry, so nothing
    // else in the row moved either.
    let without_synthetic: String = after
        .lines()
        .filter(|line| !line.contains("notes: \"keep me\""))
        .map(|line| format!("{line}\n"))
        .collect();
    beekeeper_core::coding_session_routing::parse_registry(&without_synthetic)
        .unwrap_or_else(|error| panic!("must still parse: {error}\n{without_synthetic}"));
}

/// F5 — the proposal's signature covers everything that decides what gets
/// written, so two proposals a reader would tell apart have different digests.
#[test]
fn the_proposal_digest_covers_every_field_that_decides_the_row() {
    let base = proposal_digest("claude-primary/opus[1m]", &block());
    assert_eq!(base, proposal_digest("claude-primary/opus[1m]", &block()));
    assert_ne!(base, proposal_digest("claude-primary/sonnet", &block()));

    let mut moved_score = block();
    moved_score.traits.insert(
        "reasoning".to_owned(),
        MeasuredTrait {
            score: 4.7,
            n: 3,
            min: 4.4,
            max: 4.8,
        },
    );
    assert_ne!(
        base,
        proposal_digest("claude-primary/opus[1m]", &moved_score)
    );

    let mut moved_runs = block();
    moved_runs.runs.push("g3".to_owned());
    assert_ne!(
        base,
        proposal_digest("claude-primary/opus[1m]", &moved_runs)
    );

    let mut moved_bench = block();
    moved_bench.bench_hash = "def".to_owned();
    assert_ne!(
        base,
        proposal_digest("claude-primary/opus[1m]", &moved_bench)
    );
}
