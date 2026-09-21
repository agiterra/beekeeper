//! The flags the host's work brief prints must be flags this CLI defines.
//!
//! The brief (ledger 209, `crates/buzz-session-provider/src/work_brief.rs`)
//! hands a seat ready-to-run command lines so it does not spend a tool call on
//! `--help`. That is a saving only while the lines run: a brief that spelled a
//! renamed or invented flag would teach every seat a command that exits 2 on
//! its first try, and it would do so from the most authoritative-looking place
//! in the turn.
//!
//! The provider crate cannot depend on `buzz-cli`, so the check is a pair. The
//! provider's `work_brief_tests.rs` holds the identical `REPORT_FLAGS`,
//! `VERDICT_FLAGS` and `FLAGS_THAT_DO_NOT_EXIST` tables and asserts the brief
//! spells exactly those; this file asserts the same tables against the real
//! `clap` definitions, through the real binary. A rename breaks one side or
//! the other, and neither side can be quietly "fixed" by editing the brief.

use std::process::{Command, Output};

/// Mirror of `REPORT_FLAGS` in
/// `crates/buzz-session-provider/src/work_brief_tests.rs`.
const REPORT_FLAGS: [&str; 5] = [
    "--channel",
    "--session-ref",
    "--genesis",
    "--body",
    "--wake-to",
];

/// Mirror of `VERDICT_FLAGS` there.
const VERDICT_FLAGS: [&str; 5] = REPORT_FLAGS;

/// Mirror of `FLAGS_THAT_DO_NOT_EXIST` there: flags no `sessions` verb
/// defines, so the brief must never print one.
///
/// `--assignment` is the trap. The assignment id travels in the report body,
/// and a brief that printed a flag for it would teach every seat a command
/// that exits 1 on its first try.
const FLAGS_THAT_DO_NOT_EXIST: [&str; 1] = ["--assignment"];

/// Defined on these verbs, and still never printed by the brief.
///
/// `report`, `verdict`, `assign`, `acknowledge`, `complete` and `block` share
/// one `clap` struct (`TeamTransactionWriteArgs`), so `--verifies` is
/// *accepted* by all of them while meaning something only on `assign`. The
/// brief prints it nowhere, because a flag that parses and changes nothing is
/// worse for a seat than a flag that is refused.
const FLAGS_DEFINED_BUT_NOT_FOR_THIS_VERB: [&str; 1] = ["--verifies"];

fn bee(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bee"))
        .env_remove("BUZZ_PRIVATE_KEY")
        .env_remove("BUZZ_AUTH_TAG")
        .env("BUZZ_RELAY_URL", "http://127.0.0.1:1/")
        .args(args)
        .output()
        .expect("bee runs")
}

fn help(verb: &str) -> String {
    let output = bee(&["sessions", verb, "--help"]);
    assert!(
        output.status.success(),
        "`bee sessions {verb} --help` must succeed with no identity and no relay"
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn sessions_report_defines_every_flag_the_work_brief_prints() {
    let help = help("report");
    for flag in REPORT_FLAGS {
        assert!(
            help.contains(flag),
            "the work brief prints `bee sessions report {flag}`, which this CLI no longer \
             defines; fix the brief's section (b) in the same change that renamed it"
        );
    }
    assert!(help.contains("--example"), "{help}");
}

#[test]
fn sessions_verdict_defines_every_flag_the_work_brief_prints() {
    let help = help("verdict");
    for flag in VERDICT_FLAGS {
        assert!(
            help.contains(flag),
            "the work brief prints `bee sessions verdict {flag}`, which this CLI no longer \
             defines"
        );
    }
    assert!(help.contains("--example"), "{help}");
}

#[test]
fn the_flag_the_brief_says_does_not_exist_still_does_not_exist() {
    // If `--assignment` ever becomes real, the brief should start printing it
    // and stop saying it does not exist — this test failing is how that gets
    // noticed rather than a seat discovering it.
    for verb in ["report", "verdict"] {
        let help = help(verb);
        for absent in FLAGS_THAT_DO_NOT_EXIST {
            assert!(
                !flag_is_defined(&help, absent),
                "`bee sessions {verb}` now defines {absent}; the work brief's section (b) says \
                 it does not exist and must be corrected"
            );
        }
    }
}

#[test]
fn verifies_is_accepted_by_these_verbs_and_is_still_not_printed() {
    // Not a bug this lane fixes, and not something a brief may paper over:
    // the shared args struct means `bee sessions report --verifies <id>`
    // parses and does nothing. Pinned here so a seat is never handed it.
    for verb in ["report", "verdict"] {
        let help = help(verb);
        for shared in FLAGS_DEFINED_BUT_NOT_FOR_THIS_VERB {
            assert!(
                flag_is_defined(&help, shared),
                "`bee sessions {verb}` no longer accepts {shared}; the brief's comment about \
                 the shared args struct is out of date"
            );
        }
    }
}

/// Whether a help text *defines* a flag, rather than merely mentioning it in
/// prose. `clap` prints every option it defines at the start of its own line.
fn flag_is_defined(help: &str, flag: &str) -> bool {
    help.lines()
        .map(str::trim_start)
        .any(|line| line.starts_with(flag))
}

#[test]
fn the_example_bodies_the_brief_points_at_publish_nothing_and_exit_zero() {
    for args in [
        vec!["sessions", "report", "--example"],
        vec!["sessions", "verdict", "--example", "refutation"],
    ] {
        let output = bee(&args);
        assert!(
            output.status.success(),
            "{args:?} must exit 0 with no identity and no relay: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let body: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("{args:?} must print one JSON body: {error}"));
        assert!(body.is_object(), "{args:?} printed {body}");
    }
}

#[test]
fn a_report_example_carries_the_assignment_ref_key_the_brief_names() {
    let output = bee(&["sessions", "report", "--example"]);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("one JSON body");
    assert!(
        body.get("assignmentRef").is_some(),
        "the brief tells a seat the body's `assignmentRef` is its assignment id; the example \
         must carry that exact key: {body}"
    );
}

#[test]
fn a_refutation_example_carries_the_report_ref_key_the_brief_names() {
    let output = bee(&["sessions", "verdict", "--example", "refutation"]);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).expect("one JSON body");
    assert!(
        body.get("reportRef").is_some(),
        "the brief tells a verifier the body's `reportRef` is the report it ruled on: {body}"
    );
}
