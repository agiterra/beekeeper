//! Live planning path (ledger 275): the real `bee` draft → commit → adopt,
//! run by a lead inside the boundary the production [`prepare`] renders,
//! against a real relay — not plain Git.
//!
//! `#[ignore]`d; needs a disposable relay with a project whose agents
//! repository the rig created, and no model turn:
//!
//! ```sh
//! BEEKEEPER_LIVE_PLAN_RELAY=ws://localhost:3011 \
//! BEEKEEPER_LIVE_PLAN_BEE=<bee> BEEKEEPER_LIVE_PLAN_KEYFILE=<owner key> \
//! BEEKEEPER_LIVE_PLAN_PROJECT=30621:<owner>:<slug> BEEKEEPER_LIVE_PLAN_CHANNEL=<uuid> \
//! BEEKEEPER_LIVE_PLAN_ROOT=<a directory this run owns> \
//! BEEKEEPER_LIVE_PLAN_GITCONFIG=<a config whose credential entry names git-credential-nostr> \
//!   cargo test -p beekeeper-session-provider --lib live_planning -- --ignored --nocapture
//! ```

use super::*;
use nostr::Keys;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for this live test"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[tokio::test]
#[ignore = "live: needs a disposable relay and project (see the module docs)"]
async fn a_lead_drafts_commits_and_adopts_its_plan_through_bee_inside_the_boundary() {
    let relay = required("BEEKEEPER_LIVE_PLAN_RELAY");
    let bee = PathBuf::from(required("BEEKEEPER_LIVE_PLAN_BEE"));
    let key = std::fs::read_to_string(required("BEEKEEPER_LIVE_PLAN_KEYFILE"))
        .expect("key")
        .trim()
        .to_owned();
    let project = required("BEEKEEPER_LIVE_PLAN_PROJECT");
    let channel: uuid::Uuid = required("BEEKEEPER_LIVE_PLAN_CHANNEL")
        .parse()
        .expect("uuid");
    let root = PathBuf::from(required("BEEKEEPER_LIVE_PLAN_ROOT"))
        .canonicalize()
        .expect("owned root");
    let keys = Keys::parse(&key).expect("key");
    let owner = keys.public_key().to_hex();
    let slug = project.rsplit(':').next().expect("slug").to_owned();
    let http = relay.replacen("ws://", "http://", 1);

    let rest = beekeeper_acp::relay::RestClient {
        http: reqwest::Client::new(),
        base_url: http.clone(),
        keys: keys.clone(),
        auth_tag_json: None,
    };
    // Host setup: the channel is filed under the project (kind:9002 with a
    // `project` tag, as the desktop files a session's channel).
    let filed = nostr::EventBuilder::new(nostr::Kind::Custom(9002), "")
        .tags([
            nostr::Tag::parse(["h", &channel.to_string()]).expect("h"),
            nostr::Tag::parse(["project", &project]).expect("project"),
        ])
        .sign_with_keys(&keys)
        .expect("sign");
    rest.submit_event(&filed)
        .await
        .expect("channel filed under the project");
    // The umbrella the lead plans in: genesis and goal, signed by its founder.
    let session_ref = uuid::Uuid::new_v4().to_string();
    // A plan name of this run's own, so a rerun never meets an earlier draft.
    let plan_name = format!("lapbook-{}", &session_ref[..8]);
    let genesis = beekeeper_sdk::builders::build_coding_session_genesis(
        channel,
        &beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload::new(
            session_ref.clone(),
        ),
    )
    .expect("genesis")
    .sign_with_keys(&keys)
    .expect("sign");
    rest.submit_event(&genesis).await.expect("genesis accepted");
    let goal = beekeeper_sdk::builders::build_coding_session_goal(
        channel,
        &session_ref,
        "Build a lap timer CLI with JSON output",
    )
    .expect("goal")
    .sign_with_keys(&keys)
    .expect("sign");
    rest.submit_event(&goal).await.expect("goal accepted");

    // The lead's code worktree, and the agents clone the host cuts beside it
    // (what `seat_agents_clone::cut_seat_agents_clone` does for a lead whose
    // staged grant is `write` — packs_cache test
    // `a_seeded_lead_stages_with_a_writable_agents_grant_and_an_explicit_none_binds`).
    let state_dir = root.join("state");
    std::fs::create_dir_all(&state_dir).expect("state");
    let repo = root.join("repos").join(&slug);
    std::fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let worktree = root.join("repos").join(format!("{slug}-wt-lead"));
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lead",
            worktree.to_str().expect("utf8"),
        ],
    );
    let clone = root.join("repos").join(format!("{slug}-wt-lead-agents"));
    let agents_url = format!("{http}/git/{owner}/{slug}-beekeeper-agents");
    let status = std::process::Command::new("git")
        .args(["clone", "-q", &agents_url, clone.to_str().expect("utf8")])
        .env("NOSTR_PRIVATE_KEY", &key)
        .status()
        .expect("clone");
    assert!(status.success(), "the host's agents clone");

    // The operator's Git transport, from a config this run owns (the test
    // seam that keeps the person's own configuration out of fixtures):
    // its credential entry names git-credential-nostr for the relay.
    crate::execution_scope_git::TEST_OPERATOR_CONFIG.with(|config| {
        *config.borrow_mut() = Some(PathBuf::from(required("BEEKEEPER_LIVE_PLAN_GITCONFIG")));
    });
    // The lead's boundary, as production prepares it.
    let identity = vec![
        ("BUZZ_RELAY_URL".to_owned(), relay.clone()),
        ("BUZZ_PRIVATE_KEY".to_owned(), key.clone()),
        ("NOSTR_PRIVATE_KEY".to_owned(), key.clone()),
        ("BUZZ_PULSE_PROJECT".to_owned(), project.clone()),
    ];
    let mut inputs = ScopeInputs::new(ScopePurpose::Session, &state_dir, "lead", &worktree);
    inputs.project_ref = Some(&project);
    inputs.project_checkout = Some(&repo);
    inputs.actor = Some(&owner);
    inputs.driver = "claude-agent-acp";
    inputs.runtime = RuntimeProfile::TestDouble;
    inputs.agent_command = "bash";
    inputs.identity_env = &identity;
    inputs.agents_checkout = Some((&clone, true));
    inputs.seat_bee = Some(&bee);
    let ExecutionPlan::Prepared(plan) = prepare(&inputs).expect("prepared") else {
        panic!("the boundary must be enforced");
    };

    let run = |script: &str| {
        let (program, args) = plan
            .launch
            .boundary()
            .wrap("/bin/sh", &["-c".to_owned(), script.to_owned()]);
        let mut command = std::process::Command::new(program);
        command
            .args(args)
            .current_dir(&worktree)
            .env_clear()
            .envs(plan.launch.env().vars());
        let output = command.output().expect("bounded run");
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };

    // Read the project's own requirements, draft a plan through `bee`.
    let (ok, requirements, err) = run(&format!("cat '{}/team.yml'", clone.display()));
    assert!(
        ok && requirements.contains("agents_repo: write"),
        "requirements: {err}\n{requirements}"
    );
    let (ok, _, err) = run(&format!(
        "bee plans example > \"$TMPDIR/plan.md\" && bee plans edit {plan_name} --file \"$TMPDIR/plan.md\" --message 'first plan'"
    ));
    assert!(ok, "draft: {err}");
    let (ok, committed, err) = run(&format!(
        "bee agents-repo commit --all --message 'plan: {plan_name}'"
    ));
    assert!(ok, "commit: {err}\n{committed}");
    let committed: serde_json::Value = serde_json::from_str(committed.trim()).expect("commit json");
    let commit = committed["commit"].as_str().expect("commit sha").to_owned();
    assert_eq!(committed["pushed"], "yes", "{committed}");

    // The prepared clone sees the commit without a scratch clone of its own.
    let (ok, head, err) = run(&format!(
        "git -C '{}' fetch -q origin && git -C '{}' rev-parse origin/main",
        clone.display(),
        clone.display()
    ));
    assert!(ok, "refresh: {err}");
    assert_eq!(head.trim(), commit);

    // Adopt it, from the prepared clone.
    let (ok, adopted, err) = run(&format!(
        "bee sessions work adopt --channel {channel} --session-ref {session_ref} \
         --plan plans/{plan_name}.md --commit {commit} --agents-repo '{}'",
        clone.display()
    ));
    let evidence = serde_json::json!({
        "project": project,
        "session_ref": session_ref,
        "genesis": genesis.id.to_hex(),
        "commit": commit,
        "commit_output": committed,
        "adopt_ok": ok,
        "adopt_stdout": adopted,
        "adopt_stderr": err,
    });
    std::fs::write(
        root.join("planning-evidence.json"),
        serde_json::to_string_pretty(&evidence).expect("json"),
    )
    .expect("evidence");
    eprintln!(
        "evidence: {}",
        root.join("planning-evidence.json").display()
    );
    assert!(ok, "adopt: {err}\n{adopted}");
    let adopted: serde_json::Value = serde_json::from_str(adopted.trim()).expect("adopt json");
    assert_eq!(adopted["accepted"], true, "{adopted}");
}
