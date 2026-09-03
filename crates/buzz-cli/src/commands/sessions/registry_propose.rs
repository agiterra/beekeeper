//! `bee sessions registry propose` — the only writer, and it writes nothing it
//! did not read.
//!
//! `propose` reads the 44246 rows **back off the relay**, never the measuring
//! run's own memory. A proposal built from a process's recollection of what it
//! just did is the same class of claim as *"cargo test -p buzz-cli green after
//! the rebase"* in live run 3 — which was false, and which a signed row would
//! have caught. So: signed events, or no row.
//!
//! It refuses, exit 4, naming which check failed:
//!
//! * fewer than `--repeat` runs for any task in the bench version;
//! * rows from more than one signing key;
//! * a `benchHash` on the rows differing from the tree's;
//! * any trait the role's gate reads that the bench does not evidence — a
//!   verifier row measuring nothing about `verification` is not a verifier row;
//! * an unstable set.
//!
//! # Who may propose
//!
//! Brian's addendum of 2026-09-01, ruling 3: **a seat may `measure`** — its own
//! signature goes on the 44246 rows — **and only the founder's key may
//! `propose`**. One proposal, one signer. This command signs with the invoking
//! key, and the founder-only rule is checked on that key; there is no
//! `--founder` flag to name a second one, because a flag naming an identity you
//! are not signing with is a claim, not a check (F5). The first half falls out
//! of the single-signer check above.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use std::path::Path;

use serde_json::{json, Value};

use buzz_core::coding_session_routing::{base_id, Registry};
use buzz_core::kind::KIND_CODING_SESSION_OBSERVATION;
use buzz_core::registry_bench::{
    accumulate_totals, aggregate_runs, derived_confidence, gate_row_refusals, parse_bench_gate,
    proposal_refusals, traits_from_totals, BenchGateRow, CriterionOutcome, MeasuredBlock,
    MeasuredTrait, ProposalInput, RunScore, BENCH_MIN_REPEAT,
};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::validate_uuid;

use super::registry::load_registry;
use super::registry_measure::{bench_root_for, hash_role_bench, load_role_bench, BenchTaskSpec};

/// What one measuring session left on the relay, read back.
#[derive(Debug, Clone, Default)]
pub struct RelayEvidence {
    /// Every registry-bench gate row for the role, in relay order.
    pub gate_rows: Vec<BenchGateRow>,
    /// The `benchVersion` every manifest agreed on, or `None` when they did
    /// not agree — which is itself a refusal.
    pub bench_version: Option<u32>,
    /// As [`Self::bench_version`], for the hash.
    pub bench_hash: Option<String>,
    /// Every distinct `benchVersion` seen, so a disagreement can be named.
    pub versions_seen: BTreeSet<u32>,
    /// Every distinct `benchHash` seen.
    pub hashes_seen: BTreeSet<String>,
}

/// Pull the 44246 observations for one session and pick out this role's bench
/// evidence.
///
/// Everything here is a *reading* of signed events. Anything that does not
/// parse as a registry-bench row is ignored rather than guessed at — a
/// mis-shaped row must never become a run behind a measurement.
pub fn read_evidence(events: &[Value], role: &str) -> RelayEvidence {
    let mut evidence = RelayEvidence::default();
    for event in events {
        let Some(signer) = event.get("pubkey").and_then(Value::as_str) else {
            continue;
        };
        let Some(event_id) = event.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(payload) = event
            .get("content")
            .and_then(Value::as_str)
            .and_then(|content| serde_json::from_str::<Value>(content).ok())
        else {
            continue;
        };
        match payload.get("type").and_then(Value::as_str) {
            Some("gate") => {
                let rows = payload
                    .get("body")
                    .and_then(|body| body.get("rows"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for row in rows {
                    let Some(gate) = row.get("gate").and_then(Value::as_str) else {
                        continue;
                    };
                    let Ok((row_role, task_id, run)) = parse_bench_gate(gate) else {
                        continue;
                    };
                    if row_role != role {
                        continue;
                    }
                    evidence.gate_rows.push(BenchGateRow {
                        event_id: event_id.to_owned(),
                        signer: signer.to_owned(),
                        gate: gate.to_owned(),
                        role: row_role,
                        task_id,
                        run,
                        outcome: row
                            .get("outcome")
                            .and_then(Value::as_str)
                            .unwrap_or("not-run")
                            .to_owned(),
                        summary: row
                            .get("summary")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    });
                }
            }
            Some("checkpoint") => {
                let Some(manifest) = payload
                    .get("body")
                    .and_then(|body| body.get("note"))
                    .and_then(Value::as_str)
                    .and_then(|note| serde_json::from_str::<Value>(note).ok())
                else {
                    continue;
                };
                if manifest.get("kind").and_then(Value::as_str) != Some("registry-bench-manifest")
                    || manifest.get("role").and_then(Value::as_str) != Some(role)
                {
                    continue;
                }
                if let Some(version) = manifest.get("benchVersion").and_then(Value::as_u64) {
                    evidence
                        .versions_seen
                        .insert(u32::try_from(version).unwrap_or(u32::MAX));
                }
                if let Some(hash) = manifest.get("benchHash").and_then(Value::as_str) {
                    evidence.hashes_seen.insert(hash.to_owned());
                }
            }
            _ => {}
        }
    }
    // A single value only when every manifest agreed. Two versions on one
    // session is two different benches, and a row built across them would be a
    // measurement of nothing in particular.
    if evidence.versions_seen.len() == 1 {
        evidence.bench_version = evidence.versions_seen.iter().next().copied();
    }
    if evidence.hashes_seen.len() == 1 {
        evidence.bench_hash = evidence.hashes_seen.iter().next().cloned();
    }
    // Gate rows in a stable order: run, then task, then id. The relay's own
    // order is arrival order, and arrival order is not evidence of anything.
    evidence.gate_rows.sort_by(|left, right| {
        (left.run, &left.task_id, &left.event_id).cmp(&(right.run, &right.task_id, &right.event_id))
    });
    evidence
}

/// Recompute every per-trait median from what the **relay** holds, not from
/// the measuring run's memory.
///
/// A gate row's `summary` names the criteria that failed on that run
/// specifically; the checked-in rubric supplies each criterion's weight and
/// traits. Together those are the whole scoring input, so a proposal's numbers
/// are re-derived from signed events plus checked-in code and nothing else.
///
/// A run with no row for a task contributes nothing to that task — and
/// `proposal_refusals` refuses the proposal for the missing run, so a thin run
/// can never quietly raise a median.
pub fn traits_from_relay(
    evidence: &RelayEvidence,
    tasks: &[BenchTaskSpec],
) -> BTreeMap<String, MeasuredTrait> {
    let runs: BTreeSet<u32> = evidence.gate_rows.iter().map(|row| row.run).collect();
    let mut per_run: Vec<BTreeMap<String, f64>> = Vec::new();
    for run in runs {
        let mut totals: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for task in tasks {
            let Some(row) = evidence
                .gate_rows
                .iter()
                .find(|row| row.run == run && row.task_id == task.id)
            else {
                continue;
            };
            // F2 — a row that cannot say what failed contributes NOTHING. It
            // used to fold as a perfect run, so three `observe gate` calls with
            // no `--summary` minted a row clearing every ratified minimum.
            // `gate_row_refusals` refuses the proposal by row id; this is the
            // second half of the same rule, so no path can score it.
            let Some(failed) = row.failed_criteria() else {
                continue;
            };
            // F3 — the `outcome` word is checked against the summary and a
            // disagreement is REFUSED by name (`gate_row_refusals`), not
            // resolved in favour of one field. So by the time anything is
            // scored the two agree, and the summary's per-criterion detail is
            // what the weights are applied to — the same arithmetic `measure`
            // ran on the same rows.
            let score = RunScore {
                outcomes: task
                    .criteria
                    .iter()
                    .map(|criterion| CriterionOutcome {
                        id: criterion.id.clone(),
                        passed: !failed.contains(&criterion.id),
                        detail: String::new(),
                    })
                    .collect(),
                traits: BTreeMap::new(),
                failed,
                weight_passed: 0,
                weight_total: 0,
            };
            accumulate_totals(&mut totals, &task.criteria, &score);
        }
        if !totals.is_empty() {
            per_run.push(traits_from_totals(&totals));
        }
    }
    aggregate_runs(&per_run)
}

/// Render one `measured:` block as a YAML fragment in the registry's own style.
///
/// Hand-rendered rather than serialised because the registry file is
/// hand-edited and its flow-style maps are load-bearing for review: a
/// serialiser would reflow the whole file and bury a one-row change.
pub fn render_measured_yaml(target_label: &str, block: &MeasuredBlock) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# --- {target_label} ---");
    let _ = writeln!(out, "    measured:");
    let _ = writeln!(out, "      role: {}", block.role);
    let _ = writeln!(out, "      benchVersion: {}", block.bench_version);
    let _ = writeln!(out, "      benchHash: \"{}\"", block.bench_hash);
    let _ = writeln!(out, "      measuredAt: \"{}\"", block.measured_at);
    let _ = writeln!(out, "      measuredBy: \"{}\"", block.measured_by);
    let _ = writeln!(out, "      runs:");
    for run in &block.runs {
        let _ = writeln!(out, "        - \"{run}\"");
    }
    let _ = writeln!(out, "      traits:");
    for (name, measured) in &block.traits {
        let _ = writeln!(
            out,
            "        {name}: {{ score: {}, n: {}, min: {}, max: {} }}",
            measured.score, measured.n, measured.min, measured.max
        );
    }
    let _ = writeln!(out, "    rating:");
    let _ = writeln!(
        out,
        "      {{ status: measured, confidence: {}, author: bench, date: \"{}\" }}",
        block.confidence(),
        block.measured_at
    );
    out
}

/// The sentence that says which of a row's ten numbers are measurements and
/// which are still opinions.
///
/// A half-measured row reading as measured is the same lie as a badge with no
/// event behind it, so this is printed with every proposal and never elided.
pub fn provenance_disclosure(registry: &Registry, block: &MeasuredBlock) -> String {
    let measured = block.measured_trait_names();
    let opinions: Vec<String> = registry
        .traits
        .iter()
        .filter(|name| !measured.contains(name))
        .cloned()
        .collect();
    format!(
        "measured by registry-bench/{} v{}: {}. STILL OPINIONS on this row: {}. \
         rating.status becomes `measured` and confidence is derived from n ({}), never chosen.",
        block.role,
        block.bench_version,
        if measured.is_empty() {
            "nothing".to_owned()
        } else {
            measured.join(", ")
        },
        if opinions.is_empty() {
            "nothing".to_owned()
        } else {
            opinions.join(", ")
        },
        derived_confidence(block.samples())
    )
}

/// `bee sessions registry propose`.
///
/// # Errors
///
/// [`CliError::Usage`] for a bad flag; [`CliError::Other`] (exit 4) listing
/// every reason the proposal was refused.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn cmd_registry_propose(
    client: &BuzzClient,
    role: &str,
    runtime: &str,
    model: &str,
    channel: &str,
    session_ref: &str,
    repeat: u32,
    registry_path: Option<&str>,
    write: bool,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;

    let (path, registry) = load_registry(registry_path)?;
    let registry_text = std::fs::read_to_string(&path)
        .map_err(|error| CliError::Other(format!("cannot read {}: {error}", path.display())))?;
    // F7 — the hash of the file AS READ. `apply_fragment` refuses if it moved
    // between this read and the write; `propose` runs a relay query in between
    // and a human runs `--write` after it.
    let registry_hash_at_read = hex::encode(Sha256::digest(registry_text.as_bytes()));
    let bench_root = bench_root_for(&path);
    let (set, tasks) = load_role_bench(&bench_root, role)?;
    let tree_hash = hash_role_bench(&bench_root, role)?;

    // F5 — who is invoking. `propose` signs with this key, and ruling 3 is
    // checked against it. `--founder` was a self-asserted string nobody
    // compared to anything.
    let invoking_key = client.keys().public_key().to_hex();

    let events = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_OBSERVATION],
            "#h": [channel],
            "#d": [session_ref],
        }))
        .await?;
    let evidence = read_evidence(&events, role);

    // The founder is the author of the session's own genesis event — a fact on
    // the wire, not a flag. Ruling 3: a seat may `measure` (its key signs the
    // 44246 rows); only the founder may `propose`.
    let founder = founder_of_session(client, channel, session_ref).await?;

    let mut refusals: Vec<String> = Vec::new();
    // F5 — checked against the INVOKING key, not against the rows' signer. The
    // old check demanded the rows' signer equal `--founder`, which is ruling 3
    // backwards: it meant a seat-measured set could never be proposed at all,
    // and a seat following the SKILL burned a bench run it could not use.
    if invoking_key != founder {
        refusals.push(format!(
            "this key is {invoking_key} and the session's founder is {founder}: a seat may run \
             measure, and only the founder's key may propose a row (Brian's ruling of \
             2026-09-01). The rows you measured are on the relay and keep — ask the founder to \
             run this command."
        ));
    }
    // F2/F3/F8 — the rows themselves, before anything is built from them.
    refusals.extend(gate_row_refusals(&evidence.gate_rows));
    if evidence.gate_rows.is_empty() {
        refusals.push(format!(
            "no registry-bench gate rows for {role} on session {session_ref}: a proposal is \
             built from signed events read back off the relay, never from a run's own memory"
        ));
    }
    if evidence.versions_seen.len() > 1 {
        refusals.push(format!(
            "the rows span bench versions {:?}: two versions is two benches",
            evidence.versions_seen
        ));
    }
    if evidence.hashes_seen.len() > 1 {
        refusals.push("the rows carry more than one benchHash".to_owned());
    }
    if evidence.bench_hash.is_none() && !evidence.gate_rows.is_empty() {
        refusals.push(
            "no run manifest names a benchHash, so nothing proves the task set did not change \
             under the measurement"
                .to_owned(),
        );
    }

    let traits = traits_from_relay(&evidence, &tasks);
    let bench_version = evidence.bench_version.unwrap_or(set.bench_version);
    let minimums = registry
        .classes
        .get(role)
        .map(|class| class.minimums.clone())
        .unwrap_or_default();
    refusals.extend(proposal_refusals(&ProposalInput {
        role,
        bench_version,
        tree_bench_version: set.bench_version,
        bench_hash: evidence.bench_hash.as_deref().unwrap_or(""),
        tree_bench_hash: &tree_hash,
        repeat: repeat.max(BENCH_MIN_REPEAT),
        tasks: &set.tasks,
        gate_rows: &evidence.gate_rows,
        minimums: &minimums,
        traits: &traits,
    }));

    let target_label = registry
        .targets
        .iter()
        .find(|target| target.provider == runtime && base_id(&target.model) == base_id(model))
        .map(buzz_core::coding_session_routing::RegistryTarget::label);
    if target_label.is_none() {
        refusals.push(format!(
            "no registry row for {runtime}/{model}: propose amends a row, it does not create one \
             — add the row first, or this would write a measurement onto nothing"
        ));
    }

    if !refusals.is_empty() {
        // Exit 4, and nothing is written. A partial row is a lie with a
        // decimal point in it.
        return Err(CliError::Other(format!(
            "refusing to propose a {role} row; {} reason(s):\n  - {}",
            refusals.len(),
            refusals.join("\n  - ")
        )));
    }

    let block = MeasuredBlock {
        role: role.to_owned(),
        bench_version,
        bench_hash: tree_hash,
        measured_at: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        measured_by: invoking_key.clone(),
        runs: evidence
            .gate_rows
            .iter()
            .map(|row| row.event_id.clone())
            .collect(),
        traits,
    };
    let label = target_label.unwrap_or_default();
    let fragment = render_measured_yaml(&label, &block);
    let disclosure = provenance_disclosure(&registry, &block);
    // F5 — the proposal is SIGNED by the invoking key. It is not published:
    // this lane writes nothing to a relay. The signature is over the digest
    // below, so a reader can check that the key that claimed the founder role
    // is the key that produced this exact fragment for this exact row.
    let digest = proposal_digest(&label, &block);
    let signature = client
        .keys()
        .sign_schnorr(&nostr::secp256k1::Message::from_digest(digest))
        .to_string();

    let applied = if write {
        apply_fragment(&path, &registry_hash_at_read, &label, &block)?;
        true
    } else {
        false
    };

    let report = json!({
        "role": role,
        "target": label,
        "benchVersion": block.bench_version,
        "benchHash": block.bench_hash,
        "runs": block.runs.len(),
        "confidence": block.confidence(),
        "fragment": fragment,
        "disclosure": disclosure,
        "written": applied,
        "registry": path.display().to_string(),
        // Who proposed this, provably, and over what.
        "proposedBy": invoking_key,
        "proposalDigest": hex::encode(digest),
        "proposalSignature": signature,
        "signedNotPublished": "this proposal is signed by the invoking key and published                                nowhere; it is a claim about a file, not an event",
    });
    match format {
        crate::OutputFormat::Compact => println!(
            "{}",
            json!({
                "target": label,
                "written": applied,
                "proposedBy": invoking_key,
                "disclosure": disclosure,
            })
        ),
        crate::OutputFormat::Json => println!("{report}"),
    }
    Ok(())
}

/// The 32 bytes a proposal's signature covers.
///
/// Every field that decides what gets written: the row, the bench and its
/// hash, the run ids in order, and each trait's four numbers. Two proposals
/// that differ anywhere a reader would care about differ here.
pub fn proposal_digest(label: &str, block: &MeasuredBlock) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"buzz/registry-proposal/1\0");
    for field in [
        label,
        block.role.as_str(),
        &block.bench_version.to_string(),
        block.bench_hash.as_str(),
        block.measured_at.as_str(),
        block.measured_by.as_str(),
    ] {
        hasher.update(field.as_bytes());
        hasher.update([0]);
    }
    for run in &block.runs {
        hasher.update(run.as_bytes());
        hasher.update([0]);
    }
    for (name, measured) in &block.traits {
        hasher.update(name.as_bytes());
        hasher.update(
            format!(
                "\0{}\0{}\0{}\0{}\0",
                measured.score, measured.n, measured.min, measured.max
            )
            .as_bytes(),
        );
    }
    hasher.finalize().into()
}

/// The founder of a coding session: the author of its genesis event.
///
/// A fact on the wire rather than a flag. `--founder` used to be a
/// self-asserted 64-hex string nothing compared to anything, on a command that
/// signed nothing — so the one ruling with no test was also the one whose
/// enforcement was decorative.
///
/// # Errors
///
/// [`CliError::NotFound`] when the session has no genesis event this key can
/// see — a session whose founder cannot be established is one nobody may
/// propose for.
pub async fn founder_of_session(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
) -> Result<String, CliError> {
    let events = super::fetch_channel_events(
        client,
        channel,
        &[buzz_core::kind::KIND_CODING_SESSION_GENESIS],
    )
    .await?;
    let genesis_id = super::crew::resolve_umbrella_genesis(&events, session_ref)?;
    events
        .iter()
        .find(|event| event.get("id").and_then(Value::as_str) == Some(genesis_id.as_str()))
        .and_then(|event| event.get("pubkey").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "the genesis event {genesis_id} for session {session_ref} carries no author, so \
                 this session has no founder to check a proposal against"
            ))
        })
}

/// Apply the fragment in place, refusing if the file's hash moved since it was
/// read.
///
/// F7: the doc claimed this and the body did not do it. `propose` runs a relay
/// query between the read and the write, and a human runs `--write` after
/// reading the output — the file can move in between, and silently rewriting
/// somebody else's edit is exactly the class of thing this lane exists to stop.
///
/// # Errors
///
/// [`CliError::Other`] when the file moved since it was read, when the row is
/// not there, or when it already carries a measurement.
pub fn apply_fragment(
    path: &Path,
    hash_at_read: &str,
    label: &str,
    block: &MeasuredBlock,
) -> Result<(), CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| CliError::Other(format!("cannot read {}: {error}", path.display())))?;
    let hash_now = hex::encode(Sha256::digest(text.as_bytes()));
    if hash_now != hash_at_read {
        return Err(CliError::Other(format!(
            "{} changed since this proposal read it ({} then, {} now): refusing to write over an \
             edit this proposal never saw — re-run propose",
            path.display(),
            &hash_at_read[..12.min(hash_at_read.len())],
            &hash_now[..12.min(hash_now.len())]
        )));
    }
    let Some((provider, model)) = label.split_once('/') else {
        return Err(CliError::Other(format!("{label} is not provider/model")));
    };
    let anchor = format!("  - provider: {provider}\n    model: {model}\n");
    let Some(start) = text.find(&anchor) else {
        return Err(CliError::Other(format!(
            "cannot find the row for {label} in {}: refusing to guess where it goes",
            path.display()
        )));
    };
    // The row ends at the next row's `  - provider:` or at end of file.
    let tail_start = start + anchor.len();
    let end = text[tail_start..]
        .find("\n  - provider: ")
        .map_or(text.len(), |offset| tail_start + offset + 1);
    let row = &text[start..end];
    if row.contains("\n    measured:\n") {
        return Err(CliError::Other(format!(
            "{label} already carries a measured block; remove it first rather than stacking two \
             measurements nobody can tell apart"
        )));
    }

    // F15 — replace ONLY the `rating:` block, preserving whatever follows it.
    // The previous version kept `row.split("\n    rating:\n").next()`, which
    // silently deleted every key after `rating` — safe against today's file,
    // where `rating` happens to be last in all eleven rows, and a landmine the
    // day somebody appends one.
    let rendered = render_measured_yaml(label, block);
    let mut measured_block = String::new();
    let mut rating_block = String::new();
    for line in rendered.lines() {
        if line.starts_with("# ---") {
            continue;
        }
        if line.starts_with("    rating:") || !rating_block.is_empty() {
            rating_block.push_str(line);
            rating_block.push('\n');
        } else {
            measured_block.push_str(line);
            measured_block.push('\n');
        }
    }

    let replaced_row = match row.find("\n    rating:\n") {
        Some(offset) => {
            let head = &row[..offset + 1];
            let after_rating = &row[offset + 1..];
            // The rating block runs until the next line indented four spaces
            // that is a key, i.e. `    <name>:` — anything more deeply indented
            // belongs to it.
            let tail = tail_after_block(after_rating);
            format!("{head}{measured_block}{rating_block}{tail}")
        }
        // A row with no rating block at all: append both, keep everything.
        None => format!("{}\n{measured_block}{rating_block}", row.trim_end()),
    };
    let updated = format!("{}{replaced_row}{}", &text[..start], &text[end..]);
    std::fs::write(path, updated)
        .map_err(|error| CliError::Other(format!("cannot write {}: {error}", path.display())))?;
    Ok(())
}

/// Everything after the `    rating:` block that opens `text`.
///
/// The block is its opening line plus every following line indented deeper than
/// four spaces (or blank). The first line at four-space depth or shallower ends
/// it, and everything from there is the tail this function returns — the tail
/// F15 says must survive.
fn tail_after_block(text: &str) -> String {
    let mut lines = text.lines();
    // The `    rating:` line itself.
    lines.next();
    let mut rest = String::new();
    let mut in_tail = false;
    for line in lines {
        if !in_tail {
            let indent = line.len() - line.trim_start().len();
            if !line.trim().is_empty() && indent <= 4 {
                in_tail = true;
            } else {
                continue;
            }
        }
        rest.push_str(line);
        rest.push('\n');
    }
    rest
}

#[cfg(test)]
#[path = "registry_propose_tests.rs"]
mod registry_propose_tests;
