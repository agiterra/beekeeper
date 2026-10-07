//! `bee sessions work` against the frozen contract, through the real binary.
//!
//! The unit tests exercise the module; this exercises the **command**: the
//! flags a seat types, the exit codes the contract fixes, and the two things
//! that must hold with no identity and no relay anywhere —
//! `--example` prints a complete record and exits 0, and `validate` answers
//! from git alone.
//!
//! Contract: `conformance/project-work/README.md` § (d). Exit codes are the
//! repository's: `0` ok, `1` input error, `3` auth.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The plan fixture the whole contract is written against.
const KETTLE_PLAN: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

fn bee(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bee"))
        // No identity and no relay: every assertion below must hold without
        // either, which is the point of dispatching these ahead of the key
        // gate.
        .env_remove("BEEKEEPER_PRIVATE_KEY")
        .env_remove("BEEKEEPER_AUTH_TAG")
        .env("BEEKEEPER_RELAY_URL", "http://127.0.0.1:1/")
        .args(args)
        .output()
        .expect("bee runs")
}

/// The same runner with an identity: the flag checks below live behind the
/// key gate, so a keyless run answers `auth` before it ever sees the flag.
fn bee_with_key(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_bee"))
        .env("BEEKEEPER_PRIVATE_KEY", "1".repeat(64))
        .env_remove("BEEKEEPER_AUTH_TAG")
        .env("BEEKEEPER_RELAY_URL", "http://127.0.0.1:1/")
        .args(args)
        .output()
        .expect("bee runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A throwaway repository under a temp dir — never a worktree of this one.
struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("bee-work-contract-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("plans")).expect("mkdir");
        let repo = Self(dir);
        repo.git(&["init", "--initial-branch=main"]);
        repo.git(&["config", "user.email", "lane201@example.invalid"]);
        repo.git(&["config", "user.name", "Lane 201"]);
        repo
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("utf-8 path")
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn write(&self, path: &str, body: &str) {
        let file = self.0.join(path);
        if let Some(parent) = Path::new(&file).parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(file, body).expect("write");
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Every write verb prints a complete record and exits 0 with no identity and
/// no reachable relay.
#[test]
fn every_write_verb_answers_example_offline() {
    for args in [
        vec!["sessions", "work", "adopt", "--example"],
        vec!["sessions", "work", "bind", "assignment", "--example"],
        vec!["sessions", "work", "bind", "evidence", "--example"],
    ] {
        let output = bee(&args);
        assert!(
            output.status.success(),
            "{args:?} exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        let record: serde_json::Value =
            serde_json::from_str(&stdout(&output)).expect("the example is JSON");
        assert_eq!(record["schema"], "buzz-project-work/v1", "{args:?}");
        assert!(record["body"].is_object(), "{args:?}");
        // Every nullable key is present and written as null, never absent.
        let body = &record["body"];
        match record["type"].as_str().expect("a record type") {
            "work.declared" => assert!(body.get("decisionRef").is_some()),
            "work.assignment_bound" => assert!(body.get("replacesBinding").is_some()),
            "work.evidence_bound" => assert!(body.get("completionRef").is_some()),
            other => panic!("unknown record type {other}"),
        }
    }
}

/// `validate` reads the blob at the commit, not the working copy, and reports
/// the criteria and their proof forms.
#[test]
fn validate_reads_the_blob_at_the_commit_not_the_working_copy() {
    let repo = Repo::new("at-commit");
    repo.write("plans/kettle.md", KETTLE_PLAN);
    let pinned = repo.commit("first");
    // Move the tip: the working copy is now a plan that would be refused.
    repo.write("plans/kettle.md", "not a plan at all\n");
    repo.commit("second");

    let output = bee(&[
        "sessions",
        "work",
        "validate",
        "--plan",
        "plans/kettle.md",
        "--agents-repo",
        repo.path(),
        "--commit",
        &pinned,
    ]);
    assert!(output.status.success(), "{}", stdout(&output));
    let answer: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert_eq!(answer["valid"], true);
    assert_eq!(answer["schema"], "beekeeper-plan/v1");
    assert_eq!(answer["commit"], pinned);
    assert_eq!(answer["source"], "commit");
    assert!(answer["criteria"].as_array().expect("criteria").len() >= 2);
    // A missing action blocks adoption, not drafting: the plan is valid, and
    // the command says exactly why it cannot be adopted at this commit.
    assert_eq!(answer["adoptable"], false);
    assert!(answer["unresolvedActions"]
        .as_array()
        .expect("unresolved actions")
        .iter()
        .any(|error| error
            .as_str()
            .is_some_and(|error| error.contains("unresolved-action"))));

    // The same path at the tip is refused, with a stable code and exit 1.
    let refused = bee(&[
        "sessions",
        "work",
        "validate",
        "--plan",
        "plans/kettle.md",
        "--agents-repo",
        repo.path(),
        "--commit",
        "HEAD",
    ]);
    assert_eq!(refused.status.code(), Some(1));
    let answer: serde_json::Value = serde_json::from_str(&stdout(&refused)).expect("JSON");
    assert_eq!(answer["valid"], false);
    assert!(answer["errors"][0]["code"].is_string());
}

/// With no commit, `validate` reads the working file and **says** it is
/// uncommitted and cannot be adopted.
#[test]
fn validate_of_a_working_file_says_it_cannot_be_adopted() {
    let repo = Repo::new("uncommitted");
    repo.write("plans/kettle.md", KETTLE_PLAN);
    let output = bee(&[
        "sessions",
        "work",
        "validate",
        "--plan",
        "plans/kettle.md",
        "--agents-repo",
        repo.path(),
    ]);
    assert!(output.status.success(), "{}", stdout(&output));
    let answer: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
    assert_eq!(answer["uncommitted"], true);
    assert_eq!(answer["source"], "working-copy");
    assert!(answer["message"]
        .as_str()
        .expect("a message")
        .contains("cannot be adopted"));
}

/// Every invalid plan fixture still refuses, with the code its `# REFUSED:`
/// comment names — the CLI and the frozen fixtures cannot drift apart.
#[test]
fn every_invalid_plan_fixture_refuses_with_its_own_code() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/project-work/fixtures/plans/invalid");
    let repo = Repo::new("invalid");
    let mut seen = 0;
    for entry in std::fs::read_dir(&fixtures).expect("the invalid fixtures") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("fixture");
        let expected = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("# REFUSED:"))
            .map(|rest| {
                rest.split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_owned()
            })
            .expect("every invalid fixture names its code");
        repo.write("plans/kettle.md", &body);
        let output = bee(&[
            "sessions",
            "work",
            "validate",
            "--plan",
            "plans/kettle.md",
            "--agents-repo",
            repo.path(),
        ]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}: exited 0",
            path.display()
        );
        let answer: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("JSON");
        assert_eq!(answer["valid"], false, "{}", path.display());
        let message = answer["errors"][0]["message"]
            .as_str()
            .expect("a message")
            .to_owned();
        assert!(
            message.contains(&expected),
            "{}: expected {expected}, got {message}",
            path.display()
        );
        seen += 1;
    }
    assert!(seen >= 6, "only {seen} invalid plan fixtures were read");
}

/// A verb that needs the relay still needs an identity: that refusal is exit
/// 3, and it is never a silent empty answer.
#[test]
fn a_relay_verb_without_a_key_is_an_auth_error() {
    let output = bee(&[
        "sessions",
        "work",
        "status",
        "--channel",
        "22222222-3333-4444-8555-666666666666",
        "--session-ref",
        "11111111-2222-4333-8444-555555555555",
    ]);
    assert_eq!(output.status.code(), Some(3), "{}", stdout(&output));
}

// ── ledger 213(e): a flag that parses everywhere but means one thing ───────

/// `--verifies` is refused, by name, on every verb but `assign`.
///
/// Six verbs share `TeamTransactionWriteArgs`, so clap parses the flag on all
/// of them (lane 209). A flag that is accepted and changes nothing is a lie
/// in the interface: the refusal says where it applies and where the id the
/// caller meant actually travels.
#[test]
fn verifies_is_refused_on_every_verb_but_assign() {
    let report_id = "cd".repeat(32);
    for verb in ["report", "verdict", "acknowledge", "complete", "block"] {
        let output = bee_with_key(&[
            "sessions",
            verb,
            "--channel",
            "22222222-3333-4444-8555-666666666666",
            "--session-ref",
            "11111111-2222-4333-8444-555555555555",
            "--genesis",
            &"ab".repeat(32),
            "--body",
            "{}",
            "--verifies",
            &report_id,
        ]);
        assert_eq!(output.status.code(), Some(1), "{verb} accepted --verifies");
        // The refusal is printed on stderr for some verbs and stdout for
        // others; what the contract fixes is the sentence, not the stream.
        let printed = format!(
            "{}{}",
            stdout(&output),
            String::from_utf8_lossy(&output.stderr)
        );
        let message = printed.as_str();
        assert!(
            message.contains("--verifies applies to `sessions assign`"),
            "{verb}: {message}"
        );
        assert!(message.contains("assignmentRef"), "{verb}: {message}");
    }
}

/// A report and a verdict take no `--assignment` flag at all: the assignment
/// id travels in the body, and the example says so.
#[test]
fn a_report_and_a_verdict_name_their_assignment_in_the_body() {
    for verb in ["report", "verdict"] {
        let rejected = bee_with_key(&[
            "sessions",
            verb,
            "--channel",
            "22222222-3333-4444-8555-666666666666",
            "--session-ref",
            "11111111-2222-4333-8444-555555555555",
            "--genesis",
            &"ab".repeat(32),
            "--body",
            "{}",
            "--assignment",
            &"cd".repeat(32),
        ]);
        assert_ne!(
            rejected.status.code(),
            Some(0),
            "{verb} accepted an --assignment flag"
        );
        let usage = String::from_utf8_lossy(&rejected.stderr);
        assert!(
            usage.contains("unexpected argument") || usage.contains("--assignment"),
            "{verb}: {usage}"
        );

        let example = bee(&["sessions", verb, "--example"]);
        assert!(example.status.success(), "{verb} --example");
        let body: serde_json::Value =
            serde_json::from_str(&stdout(&example)).expect("the example is JSON");
        assert!(
            body.get("assignmentRef").is_some(),
            "{verb}'s example body must name assignmentRef: {body}"
        );
    }
}
