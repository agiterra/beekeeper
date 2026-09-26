use super::*;

const PROVIDER_PUBKEY: &str = "abababababababababababababababababababababababababababababababab";

fn git(dir: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    command.args(args).current_dir(dir);
    for name in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(name);
    }
    let output = command
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_TEMPLATE_DIR")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    state_dir: PathBuf,
    checkout: PathBuf,
    b_plan: PathBuf,
    commit: String,
}

/// Project A's checkout (a git repository with a verify script and a
/// project-only tool), a sibling project B, and a provider state dir.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical");
    let state_dir = root.join("app/session-provider").join(PROVIDER_PUBKEY);
    std::fs::create_dir_all(&state_dir).expect("state");
    let checkout = root.join("repos/a");
    std::fs::create_dir_all(checkout.join("tools")).expect("tools");
    let b_plan = root.join("repos/b/plans/plan.md");
    std::fs::create_dir_all(b_plan.parent().expect("parent")).expect("b");
    std::fs::write(&b_plan, "B_PLAN_CANARY\n").expect("b plan");
    std::fs::write(
        checkout.join("tools/project-tool"),
        "#!/bin/sh\necho PROJECT_TOOL_RAN\n",
    )
    .expect("tool");
    std::fs::write(
        checkout.join("verify.sh"),
        format!(
            r#"#!/bin/sh
echo "HEAD=$(git rev-parse HEAD)"
echo "PROJECT_VAR=${{PROJECT_VAR:-<absent>}}"
echo "NAMED_FROM_HOST=${{NAMED_FROM_HOST:-<absent>}}"
echo "CARGO_MANIFEST_DIR=${{CARGO_MANIFEST_DIR:+<present>}}"
/usr/bin/python3 -c 'import subprocess; print(subprocess.run(["project-tool"],capture_output=True,text=True).stdout.strip())'
if cat "{b}" >/dev/null 2>&1; then echo SIBLING=READ; else echo SIBLING=DENIED; fi
if echo built > build-output.txt; then echo OWN_WRITE=OK; fi
if git update-ref refs/heads/other HEAD 2>/dev/null; then echo OTHER_BRANCH=MOVED; else echo OTHER_BRANCH=REFUSED; fi
"#,
            b = b_plan.display()
        ),
    )
    .expect("verify");
    for file in ["tools/project-tool", "verify.sh"] {
        let path = checkout.join(file);
        let mut perms = std::fs::metadata(&path).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
    }
    git(&checkout, &["init", "-q", "-b", "main"]);
    std::fs::write(checkout.join(".gitignore"), "build-output.txt\n").expect("ignore");
    git(&checkout, &["add", "."]);
    git(&checkout, &["commit", "-q", "-m", "init"]);
    git(&checkout, &["branch", "other"]);
    let commit = git(&checkout, &["rev-parse", "HEAD"]);
    Fixture {
        _dir: dir,
        root,
        state_dir,
        checkout,
        b_plan,
        commit,
    }
}

fn declared(extra: &[(&str, &str)], checkout: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("PROJECT_VAR".to_owned(), "project-a".to_owned());
    env.insert(
        "PATH".to_owned(),
        format!("{}:/usr/bin:/bin", checkout.join("tools").display()),
    );
    for (name, value) in extra {
        env.insert((*name).to_owned(), (*value).to_owned());
    }
    env
}

const PROJECT: &str = "30621:aa:a";

/// A step's scope: the host's own worktree under its state is host-bound;
/// anything else must prove itself the recorded checkout.
fn scope<'a>(
    fx: &'a Fixture,
    run_dir: &'a Path,
    env: &'a BTreeMap<String, String>,
    from_host: &'a [(String, String)],
) -> HostCommandScope<'a> {
    HostCommandScope {
        state_dir: &fx.state_dir,
        project_ref: Some(PROJECT),
        checkout: Some(&fx.checkout),
        run_dir,
        cwd: run_dir,
        name: "run-1\nverify",
        association: if run_dir.starts_with(&fx.state_dir) {
            WorkspaceAssociation::HostBound
        } else {
            WorkspaceAssociation::Unbound
        },
        declared_env: env,
        from_host,
        hermit_state: None,
        host_branch: None,
        host_read: &[],
        git_transport: false,
    }
}

#[test]
fn a_declaration_naming_a_scope_owned_variable_refuses_the_step() {
    let fx = fixture();
    for owned in ["HOME", "GIT_DIR", "CLAUDE_CONFIG_DIR"] {
        let env = declared(&[(owned, "/elsewhere")], &fx.checkout);
        let refusal =
            prepare_host_command(&scope(&fx, &fx.checkout, &env, &[])).expect_err("refused");
        assert_eq!(
            refusal.code,
            crate::host_command::ACTION_STEP_INVALID,
            "{owned}"
        );
        assert!(refusal.message.contains(owned), "{}", refusal.message);
    }
}

#[test]
fn a_step_outside_its_checkout_or_the_hosts_worktrees_is_refused() {
    let fx = fixture();
    let elsewhere = fx.root.join("repos/b");
    let env = BTreeMap::new();
    let result = prepare_host_command(&scope(&fx, &elsewhere, &env, &[]));
    assert_eq!(
        result.expect_err("refused").code,
        crate::execution_scope::EXECUTION_SCOPE_INVALID
    );
}

mod launched {
    use super::*;

    async fn run_verify(fx: &Fixture, run_dir: &Path) -> String {
        let env = declared(&[], run_dir);
        let from_host = vec![("NAMED_FROM_HOST".to_owned(), "host-value".to_owned())];
        let plan = prepare_host_command(&scope(fx, run_dir, &env, &from_host)).expect("prepared");
        assert!(
            matches!(plan, HostLaunchPlan::Bounded(_)),
            "bounded on macOS"
        );
        let prepared = crate::host_command::PreparedCommand {
            command: vec![run_dir.join("verify.sh").to_string_lossy().into_owned()],
            cwd: run_dir.to_path_buf(),
            env: env.clone(),
            secrets: vec!["host-value".to_owned()],
            timeout_secs: 30,
            tail_bytes: 8192,
            artifact_max_bytes: 65_536,
        };
        let artifacts = fx.state_dir.join("actions/run-1/verify");
        let command = crate::host_command::spawn(prepared, &artifacts, plan.launch())
            .await
            .expect("spawn");
        let outcome = command.wait().await;
        assert_eq!(outcome.exit_code, Some(0), "{outcome:?}");
        std::fs::read_to_string(artifacts.join(crate::host_command::STDOUT_LOG)).expect("stdout")
    }

    #[tokio::test]
    async fn a_verify_step_as_found_has_its_project_and_nothing_else() {
        let fx = fixture();
        let out = run_verify(&fx, &fx.checkout).await;
        for expected in [
            format!("HEAD={}", fx.commit),
            "PROJECT_VAR=project-a".to_owned(),
            "NAMED_FROM_HOST=host-value".to_owned(),
            "CARGO_MANIFEST_DIR=".to_owned(),
            "PROJECT_TOOL_RAN".to_owned(),
            "SIBLING=DENIED".to_owned(),
            "OWN_WRITE=OK".to_owned(),
        ] {
            assert!(out.lines().any(|l| l == expected), "{expected}: {out}");
        }
        assert_eq!(
            std::fs::read_to_string(&fx.b_plan).expect("b"),
            "B_PLAN_CANARY\n"
        );
    }

    fn plan(fx: &Fixture, run_dir: &Path) -> HostLaunchPlan {
        let env = declared(&[], run_dir);
        match prepare_host_command(&scope(fx, run_dir, &env, &[])) {
            Ok(plan @ HostLaunchPlan::Bounded(_)) => plan,
            other => panic!("bounded: {other:?}"),
        }
    }

    fn checkout_plan(fx: &Fixture) -> HostLaunchPlan {
        prepare_host_command(
            &HostCommandScope::git(
                &fx.state_dir,
                Some(PROJECT),
                Some(&fx.checkout),
                &fx.checkout,
                "fetch",
            )
            .fetching(),
        )
        .expect("checkout plan")
    }

    /// Exact-SHA verification in the host's own detached worktree, cut and
    /// materialized the way the provider does it: the step sees exactly the
    /// bound commit, cannot move the project's branches, and cannot read the
    /// sibling.
    #[tokio::test]
    async fn an_exact_sha_verify_runs_in_the_hosts_worktree_inside_the_boundary() {
        let fx = fixture();
        let worktree = fx.state_dir.join("actions/run-1/verify/worktree");
        crate::host_command::cut_worktree_unmaterialized(
            &checkout_plan(&fx),
            &fx.checkout,
            &fx.commit,
            &worktree,
        )
        .await
        .expect("worktree");
        crate::host_command::materialize_worktree(&plan(&fx, &worktree), &worktree, &fx.commit)
            .await
            .expect("materialized inside the boundary");
        let before = git(&fx.checkout, &["rev-parse", "other"]);
        let out = run_verify(&fx, &worktree).await;
        assert!(
            out.lines().any(|l| l == format!("HEAD={}", fx.commit)),
            "{out}"
        );
        assert!(out.lines().any(|l| l == "SIBLING=DENIED"), "{out}");
        assert!(out.lines().any(|l| l == "OTHER_BRANCH=REFUSED"), "{out}");
        assert!(out.lines().any(|l| l == "PROJECT_TOOL_RAN"), "{out}");
        assert_eq!(git(&fx.checkout, &["rev-parse", "other"]), before);
        crate::host_command::remove_worktree(&fx.checkout, &worktree).await;
    }

    fn executable(path: &Path, body: &str) {
        std::fs::write(path, body).expect("write");
        let mut perms = std::fs::metadata(path).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(path, perms).expect("chmod");
    }

    /// The project's own required filters still run — and produce correct
    /// content and correct dirty state — but only inside the boundary: the
    /// same filter's read of a foreign canary, a hook and a planted fsmonitor
    /// get nothing during cut, checkout and status, while the unbounded
    /// positive control shows the filter really does read it.
    #[tokio::test]
    async fn worktree_preparation_runs_the_projects_filters_correctly_and_only_inside_the_boundary()
    {
        let fx = fixture();
        let leaks = fx.root.join("leaks");
        std::fs::create_dir_all(&leaks).expect("leaks");
        let b = fx.b_plan.display();
        let hook_leak = leaks.join("hook.txt");
        let filter_leak = leaks.join("filter.txt");
        let monitor_leak = leaks.join("fsmonitor.txt");
        for hook in ["post-checkout", "reference-transaction"] {
            executable(
                &fx.checkout.join(".git/hooks").join(hook),
                &format!(
                    "#!/bin/sh\ncat '{b}' >> '{}' 2>/dev/null\nexit 0\n",
                    hook_leak.display()
                ),
            );
        }
        // A real content filter: the repository stores lowercase, the tree
        // holds uppercase. It also tries to copy the sibling's plan out.
        let smudge = fx.checkout.join(".git/upper-smudge");
        executable(
            &smudge,
            &format!(
                "#!/bin/sh\ncat '{b}' >> '{}' 2>/dev/null\ntr a-z A-Z\n",
                filter_leak.display()
            ),
        );
        git(
            &fx.checkout,
            &[
                "config",
                "filter.case.smudge",
                &smudge.display().to_string(),
            ],
        );
        git(&fx.checkout, &["config", "filter.case.clean", "tr A-Z a-z"]);
        git(&fx.checkout, &["config", "filter.case.required", "true"]);
        let monitor = fx.checkout.join(".git/planted-fsmonitor");
        executable(
            &monitor,
            &format!(
                "#!/bin/sh\ncat '{b}' >> '{}' 2>/dev/null\nexit 1\n",
                monitor_leak.display()
            ),
        );
        std::fs::write(fx.checkout.join(".gitattributes"), "*.txt filter=case\n").expect("attrs");
        std::fs::write(fx.checkout.join("data.txt"), "own data\n").expect("data");
        git(&fx.checkout, &["add", ".gitattributes", "data.txt"]);
        git(&fx.checkout, &["commit", "-q", "-m", "attrs"]);
        git(
            &fx.checkout,
            &["config", "core.fsmonitor", &monitor.display().to_string()],
        );
        let commit = git(&fx.checkout, &["rev-parse", "HEAD"]);
        // Positive control, unbounded: the filter does reach the sibling.
        let control = fx.root.join("control-tree");
        git(
            &fx.checkout,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &control.display().to_string(),
                &commit,
            ],
        );
        assert!(
            std::fs::read_to_string(&filter_leak)
                .unwrap_or_default()
                .contains("B_PLAN_CANARY"),
            "the fixture's filter must reach the canary unbounded, or this test proves nothing"
        );
        git(
            &fx.checkout,
            &[
                "worktree",
                "remove",
                "--force",
                &control.display().to_string(),
            ],
        );
        for leak in [&hook_leak, &filter_leak, &monitor_leak] {
            let _ = std::fs::remove_file(leak);
        }

        let worktree = fx.state_dir.join("actions/run-2/verify/worktree");
        crate::host_command::cut_worktree_unmaterialized(
            &checkout_plan(&fx),
            &fx.checkout,
            &commit,
            &worktree,
        )
        .await
        .expect("cut");
        assert!(
            !worktree.join("data.txt").exists(),
            "nothing checked out on the host"
        );
        let plan = plan(&fx, &worktree);
        crate::host_command::materialize_worktree(&plan, &worktree, &commit)
            .await
            .expect("materialized with the project's filter, and proved clean");
        assert_eq!(
            std::fs::read_to_string(worktree.join("data.txt")).expect("data"),
            "OWN DATA\n",
            "the required filter produced the project's real content"
        );
        std::fs::write(worktree.join("data.txt"), "OWN DATA, EDITED\n").expect("edit");
        let (_, dirty) = crate::host_command::git_head_and_dirty(&plan, &worktree).await;
        assert_eq!(dirty, Some(true), "a real edit still reads as a change");
        for leak in [&hook_leak, &filter_leak, &monitor_leak] {
            let text = std::fs::read_to_string(leak).unwrap_or_default();
            assert!(
                !text.contains("B_PLAN_CANARY"),
                "{} got the canary",
                leak.display()
            );
        }
        crate::host_command::remove_worktree(&fx.checkout, &worktree).await;
    }

    /// Real Git LFS (the root-supplied 3.8.0 release, copied into the
    /// disposable project only): a linked worktree materialized inside the
    /// boundary holds the real content, not the pointer, using the project's
    /// own LFS store.
    #[tokio::test]
    #[ignore = "integration: set BEEKEEPER_TEST_GIT_LFS to a verified git-lfs release binary"]
    async fn git_lfs_content_materializes_inside_the_boundary() {
        let tool = crate::session::testing::required_tool("BEEKEEPER_TEST_GIT_LFS");
        let fx = fixture();
        let tools = fx.checkout.join(".git/test-tools");
        std::fs::create_dir_all(&tools).expect("tools");
        let lfs = tools.join("git-lfs");
        std::fs::copy(&tool, &lfs).expect("copy tool");
        let lfs_text = lfs.display().to_string();
        for (key, value) in [
            ("filter.lfs.process", format!("{lfs_text} filter-process")),
            ("filter.lfs.clean", format!("{lfs_text} clean -- %f")),
            ("filter.lfs.smudge", format!("{lfs_text} smudge -- %f")),
            ("filter.lfs.required", "true".to_owned()),
        ] {
            git(&fx.checkout, &["config", key, &value]);
        }
        std::fs::write(
            fx.checkout.join(".gitattributes"),
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("attrs");
        let payload: Vec<u8> = (0..4096u32).flat_map(|i| i.to_le_bytes()).collect();
        std::fs::write(fx.checkout.join("large.bin"), &payload).expect("payload");
        git(&fx.checkout, &["add", ".gitattributes", "large.bin"]);
        git(&fx.checkout, &["commit", "-q", "-m", "lfs"]);
        let commit = git(&fx.checkout, &["rev-parse", "HEAD"]);
        let stored = git(
            &fx.checkout,
            &["cat-file", "-p", &format!("{commit}:large.bin")],
        );
        assert!(
            stored.starts_with("version https://git-lfs.github.com/spec/v1"),
            "{stored}"
        );

        let worktree = fx.state_dir.join("actions/run-4/verify/worktree");
        crate::host_command::cut_worktree_unmaterialized(
            &checkout_plan(&fx),
            &fx.checkout,
            &commit,
            &worktree,
        )
        .await
        .expect("cut");
        crate::host_command::materialize_worktree(&plan(&fx, &worktree), &worktree, &commit)
            .await
            .expect("LFS content materialized inside the boundary");
        assert_eq!(
            std::fs::read(worktree.join("large.bin")).expect("content"),
            payload
        );
        crate::host_command::remove_worktree(&fx.checkout, &worktree).await;
    }

    /// A credential helper line naming `helper` the way the host writes one
    /// (`!'<path>'`, so an install path with spaces stays one word), as a
    /// quoted Git config value.
    fn helper_line(helper: &Path) -> String {
        let value = format!("!{}", crate::execution_scope_git::shell_quoted(helper));
        format!(
            "\thelper = \"{}\"\n",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }

    /// The operator's configuration for this test thread only: a complete,
    /// disposable selection, so no helper of the person's own is reached.
    fn operator_config(fx: &Fixture, body: &str) {
        let file = fx.root.join("operator-gitconfig");
        std::fs::write(&file, body).expect("operator config");
        crate::execution_scope_git::TEST_OPERATOR_CONFIG
            .with(|config| *config.borrow_mut() = Some(file));
    }

    /// Host Git that fetches keeps the operator's selected transport: the
    /// exact-SHA cut fetches a commit only an authenticated receiver holds,
    /// through the bounded fetch, with the configured helper and keyfile. The
    /// same fetch without the transport right (a scope that runs project code)
    /// gets no credential and cannot.
    #[tokio::test]
    async fn a_host_fetch_authenticates_with_the_operators_selected_transport() {
        let fx = fixture();
        let receiver_root = fx.root.join("receiver");
        std::fs::create_dir_all(&receiver_root).expect("receiver");
        let remote = receiver_root.join("remote.git");
        git(
            &fx.root,
            &[
                "clone",
                "-q",
                "--bare",
                &fx.checkout.display().to_string(),
                &remote.display().to_string(),
            ],
        );
        let upstream = fx.root.join("upstream");
        git(
            &fx.root,
            &[
                "clone",
                "-q",
                &remote.display().to_string(),
                &upstream.display().to_string(),
            ],
        );
        std::fs::write(upstream.join("new.txt"), "REMOTE_ONLY\n").expect("new");
        git(&upstream, &["add", "new.txt"]);
        git(&upstream, &["commit", "-q", "-m", "remote only"]);
        git(&upstream, &["push", "-q", "origin", "HEAD:main"]);
        let remote_only = git(&upstream, &["rev-parse", "HEAD"]);
        let receiver = crate::session::testing::git_receiver(&receiver_root, "operator-key");
        git(
            &fx.checkout,
            &[
                "remote",
                "add",
                "origin",
                &format!("http://127.0.0.1:{}/remote.git", receiver.port),
            ],
        );
        let helper = fx.root.join("git-credential-fixture");
        executable(
            &helper,
            "#!/bin/sh\n[ \"$1\" = get ] || exit 0\nf=$(git config --get nostr.keyfile) || exit 0\n\
             echo username=t\necho password=$(cat \"$f\")\n",
        );
        let keyfile = fx.root.join("operator.key");
        std::fs::write(&keyfile, "operator-key").expect("key");
        operator_config(
            &fx,
            &format!(
                "[credential]\n{}[nostr]\n\tkeyfile = {}\n",
                helper_line(&helper),
                keyfile.display()
            ),
        );
        // Without the transport right: no credential, so the fetch fails.
        let no_transport = prepare_host_command(&HostCommandScope::git(
            &fx.state_dir,
            Some(PROJECT),
            Some(&fx.checkout),
            &fx.checkout,
            "fetch-without",
        ))
        .expect("plan");
        let worktree = fx.state_dir.join("actions/run-5/verify/worktree");
        let refused = crate::host_command::cut_worktree_unmaterialized(
            &no_transport,
            &fx.checkout,
            &remote_only,
            &worktree,
        )
        .await;
        assert!(refused.is_err(), "fetched without the operator's transport");
        // With it: fetched and cut at the exact commit.
        crate::host_command::cut_worktree_unmaterialized(
            &checkout_plan(&fx),
            &fx.checkout,
            &remote_only,
            &worktree,
        )
        .await
        .expect("the bounded fetch authenticated");
        crate::host_command::materialize_worktree(&plan(&fx, &worktree), &worktree, &remote_only)
            .await
            .expect("materialized");
        assert_eq!(
            std::fs::read_to_string(worktree.join("new.txt")).expect("new"),
            "REMOTE_ONLY\n"
        );
        crate::execution_scope_git::TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = None);
        crate::host_command::remove_worktree(&fx.checkout, &worktree).await;
        drop(receiver);
    }

    /// The real Nostr credential helper, offline: bounded host Git that
    /// fetches produces a credential from the staged keyfile; host Git
    /// without the transport right produces none. Only whether a credential
    /// line appeared is recorded, never its value.
    #[test]
    #[ignore = "integration: set BEEKEEPER_TEST_GIT_CREDENTIAL_NOSTR to a built git-credential-nostr"]
    fn the_real_nostr_helper_answers_bounded_host_git_offline() {
        let helper = crate::session::testing::required_tool("BEEKEEPER_TEST_GIT_CREDENTIAL_NOSTR");
        let fx = fixture();
        let keyfile = fx.root.join("operator.key");
        // Created 0600 from the outset: the helper refuses a keyfile anyone
        // else could read.
        crate::execution_scope::write_host_file(
            &keyfile,
            nostr::ToBech32::to_bech32(nostr::Keys::generate().secret_key())
                .expect("nsec")
                .as_bytes(),
            true,
        )
        .expect("key");
        // As the operator's own selection has it: NIP-98 signs the request
        // path, so Git must pass it to the helper.
        operator_config(
            &fx,
            &format!(
                "[credential]\n{}\tuseHttpPath = true\n[nostr]\n\tkeyfile = {}\n",
                helper_line(&helper),
                keyfile.display()
            ),
        );
        // Whether a credential line came back, with Git's status and its
        // stderr (lines naming a credential dropped) — never stdout's value.
        let fill = |plan: &HostLaunchPlan| -> (bool, String) {
            use std::io::Write;
            let mut command = plan.git_command(&fx.checkout, &["credential", "fill"]);
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let mut child = command.spawn().expect("spawn");
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(b"capability[]=authtype\nprotocol=http\nhost=127.0.0.1:1\npath=x.git\nwwwauth[]=Nostr method=\"GET\"\n\n")
                .expect("input");
            let output = child.wait_with_output().expect("output");
            let answered = String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.starts_with("credential=") || line.starts_with("password="));
            let stderr: Vec<String> = String::from_utf8_lossy(&output.stderr)
                .lines()
                .filter(|line| !line.contains("credential=") && !line.contains("password="))
                .map(str::to_owned)
                .collect();
            (
                answered,
                format!("status {:?}; stderr: {stderr:?}", output.status.code()),
            )
        };
        // Which Git the bounded command resolved: the helper needs the
        // `authtype` capability (Git 2.46+).
        let version = checkout_plan(&fx)
            .git_command(&fx.checkout, &["--version"])
            .output()
            .expect("git --version");
        let exec_path = checkout_plan(&fx)
            .git_command(&fx.checkout, &["--exec-path"])
            .output()
            .expect("git --exec-path");
        let git = format!(
            "{} (exec-path {}; status {:?}, {})",
            String::from_utf8_lossy(&version.stdout).trim(),
            String::from_utf8_lossy(&exec_path.stdout).trim(),
            version.status.code(),
            String::from_utf8_lossy(&version.stderr).trim()
        );
        let (answered, report) = fill(&checkout_plan(&fx));
        assert!(
            answered,
            "the real helper produced no credential inside the boundary: {report}; {git}"
        );
        let no_transport = prepare_host_command(&HostCommandScope::git(
            &fx.state_dir,
            Some(PROJECT),
            Some(&fx.checkout),
            &fx.checkout,
            "fill-without",
        ))
        .expect("plan");
        let (answered, report) = fill(&no_transport);
        assert!(
            !answered,
            "a credential without the transport right: {report}"
        );
        crate::execution_scope_git::TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = None);
    }

    /// A project's argv is an operation inside its rights, never a grant: an
    /// action naming another project's file by absolute path is refused.
    #[tokio::test]
    async fn a_step_naming_a_foreign_file_in_its_argv_gets_no_grant_for_it() {
        let fx = fixture();
        let plan = plan(&fx, &fx.checkout);
        let prepared = crate::host_command::PreparedCommand {
            command: vec![
                "/bin/cat".to_owned(),
                fx.b_plan.to_string_lossy().into_owned(),
            ],
            cwd: fx.checkout.clone(),
            env: BTreeMap::new(),
            secrets: Vec::new(),
            timeout_secs: 30,
            tail_bytes: 8192,
            artifact_max_bytes: 65_536,
        };
        let artifacts = fx.state_dir.join("actions/run-3/cat");
        let outcome = crate::host_command::spawn(prepared, &artifacts, plan.launch())
            .await
            .expect("spawn")
            .wait()
            .await;
        assert_ne!(outcome.exit_code, Some(0));
        assert!(!outcome.stdout_tail.contains("B_PLAN_CANARY"));
        assert!(
            outcome.stderr_tail.contains("Operation not permitted"),
            "{outcome:?}"
        );
    }

    /// A real Hermit project builds with Cargo as a host step: the project's
    /// own `bin/` shims resolve the declared toolchain from the host's
    /// Hermit state and rustup toolchains, Cargo's home and target stay in
    /// the project, and the project's build script cannot read a neighbour.
    #[tokio::test]
    #[ignore = "integration: set BEEKEEPER_TEST_HERMIT_STATE (the host's Hermit state) and BEEKEEPER_TEST_HERMIT_BIN (a Hermit bin/ with rustup)"]
    async fn a_hermit_cargo_build_runs_as_a_host_step_and_its_build_script_is_bounded() {
        let hermit_state = std::env::var_os("BEEKEEPER_TEST_HERMIT_STATE")
            .map(PathBuf::from)
            .expect("BEEKEEPER_TEST_HERMIT_STATE must name the host's Hermit state");
        let hermit_bin = std::env::var_os("BEEKEEPER_TEST_HERMIT_BIN")
            .map(PathBuf::from)
            .expect("BEEKEEPER_TEST_HERMIT_BIN must name a Hermit bin/ holding rustup");
        let fx = fixture();
        let bin = fx.checkout.join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let rustup = std::fs::read_dir(&hermit_bin)
            .expect("hermit bin")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.starts_with(".rustup-") && name.ends_with(".pkg"))
            .expect("the Hermit bin/ holds a rustup package");
        for name in [
            "hermit",
            "activate-hermit",
            "hermit.hcl",
            rustup.as_str(),
            "cargo",
            "rustc",
        ] {
            let status = std::process::Command::new("/bin/cp")
                .arg("-P")
                .arg(hermit_bin.join(name))
                .arg(bin.join(name))
                .status()
                .expect("cp");
            assert!(status.success(), "copy {name}");
        }
        let toolchain = hermit_bin.join("../rust-toolchain.toml");
        if toolchain.is_file() {
            std::fs::copy(&toolchain, fx.checkout.join("rust-toolchain.toml")).expect("toolchain");
        }
        std::fs::create_dir_all(fx.checkout.join("src")).expect("src");
        std::fs::write(
            fx.checkout.join("Cargo.toml"),
            "[package]\nname = \"hello\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .expect("manifest");
        std::fs::write(
            fx.checkout.join("src/main.rs"),
            "fn main() { println!(\"HERMIT_CARGO_OK\"); }\n",
        )
        .expect("main");
        std::fs::write(
            fx.checkout.join("build.rs"),
            format!(
                "fn main() {{\n    match std::fs::read_to_string({:?}) {{\n        Ok(_) => println!(\"cargo:warning=FOREIGN_READ=read\"),\n        Err(_) => println!(\"cargo:warning=FOREIGN_READ=denied\"),\n    }}\n}}\n",
                fx.b_plan.display().to_string()
            ),
        )
        .expect("build script");

        let env = BTreeMap::new();
        let mut scope = scope(&fx, &fx.checkout, &env, &[]);
        scope.hermit_state = Some(&hermit_state);
        let plan = prepare_host_command(&scope).expect("prepared");
        assert!(
            matches!(plan, HostLaunchPlan::Bounded(_)),
            "bounded on macOS"
        );
        let prepared = crate::host_command::PreparedCommand {
            command: vec![
                bin.join("cargo").to_string_lossy().into_owned(),
                "run".to_owned(),
                "--offline".to_owned(),
                "-vv".to_owned(),
            ],
            cwd: fx.checkout.clone(),
            env: env.clone(),
            secrets: Vec::new(),
            timeout_secs: 300,
            tail_bytes: 65_536,
            artifact_max_bytes: 4 << 20,
        };
        let artifacts = fx.state_dir.join("actions/run-hermit/build");
        let command = crate::host_command::spawn(prepared, &artifacts, plan.launch())
            .await
            .expect("spawn");
        let outcome = command.wait().await;
        let stdout = std::fs::read_to_string(artifacts.join(crate::host_command::STDOUT_LOG))
            .unwrap_or_default();
        let stderr = std::fs::read_to_string(artifacts.join(crate::host_command::STDERR_LOG))
            .unwrap_or_default();
        assert_eq!(outcome.exit_code, Some(0), "{outcome:?}\n{stderr}");
        assert!(stdout.contains("HERMIT_CARGO_OK"), "{stdout}\n{stderr}");
        assert!(
            stderr.contains("FOREIGN_READ=denied"),
            "the build script read a neighbour: {stderr}"
        );
        assert!(
            fx.checkout.join("target/debug/hello").is_file(),
            "the build output is the project's own"
        );
        assert!(
            fx.checkout.join(".hermit/rust").is_dir(),
            "Cargo's home is the project's own"
        );
    }
}
