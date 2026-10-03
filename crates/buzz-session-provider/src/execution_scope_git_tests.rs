use super::*;

#[test]
fn only_plain_branch_names_earn_a_ref_grant() {
    for good in ["seat-branch", "wip/builder/abc", "feature/x"] {
        assert!(valid_branch(good), "{good}");
    }
    for bad in ["", "/abs", "a//b", "a/../b", "..", "a/./b"] {
        assert!(!valid_branch(bad), "{bad}");
    }
}

#[test]
fn a_program_word_is_bare_or_simply_quoted_and_is_written_back_quoted() {
    assert_eq!(
        leading_word("fixture --a b"),
        Some(("fixture".to_owned(), "--a b"))
    );
    assert_eq!(
        leading_word("'/App With Spaces/h' --a"),
        Some(("/App With Spaces/h".to_owned(), " --a"))
    );
    assert_eq!(
        leading_word("\"/App Dir/h\""),
        Some(("/App Dir/h".to_owned(), ""))
    );
    // More than a quoted word is left as written.
    assert_eq!(
        leading_word("'/it'\\''s dir/h' x"),
        Some(("/it's dir/h".to_owned(), " x"))
    );
    for elaborate in ["\"/a/$HOME/h\"", "'/a'b", "a\\ b", "'unclosed", "'/a'\\'"] {
        assert_eq!(leading_word(elaborate), None, "{elaborate}");
    }
    assert_eq!(shell_quoted(Path::new("/a b/it's")), "'/a b/it'\\''s'");
}

/// The operator's selected helper and filter programs, installed under a
/// path with spaces and shell characters (as an app bundle or Application
/// Support is), run from the staged configuration exactly: a bare helper
/// name, an already-quoted shell helper, and a filter with arguments.
#[cfg(unix)]
#[test]
fn staged_helpers_and_filters_run_from_an_install_path_with_spaces() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("temp");
    let root = canonical(root.path()).expect("canonical");
    let install = root.join("Beekeeper Dev's $HOME `x`.app/Contents/MacOS");
    std::fs::create_dir_all(&install).expect("install");
    let write_tool = |name: &str, body: &str| {
        let path = install.join(name);
        std::fs::write(&path, body).expect("tool");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    };
    write_tool(
        "git-credential-fixture",
        "#!/bin/sh\n[ \"$2\" = get ] || exit 0\ncat >/dev/null\necho username=fixture-user\necho password=$1\n",
    );
    let quoted = write_tool(
        "quoted-helper",
        "#!/bin/sh\n[ \"$2\" = get ] || exit 0\ncat >/dev/null\necho username=quoted-user\necho password=$1\n",
    );
    write_tool("demo-filter", "#!/bin/sh\nsed \"s/^/$1:/\"\n");
    let control = root.join("control");
    std::fs::create_dir_all(&control).expect("control");
    let operator = root.join("operator-gitconfig");
    let write_operator = |helper: &str| {
        let _ = std::fs::remove_file(&operator);
        for (key, value) in [
            ("credential.helper", helper),
            ("filter.demo.clean", "demo-filter cleaned"),
            ("filter.demo.smudge", "demo-filter smudged"),
        ] {
            let status = std::process::Command::new("git")
                .args(["config", "--file"])
                .arg(&operator)
                .args(["--add", key, value])
                .status()
                .expect("git config");
            assert!(status.success(), "operator config");
        }
    };
    TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = Some(operator.clone()));
    let git = |config: &Path, dir: &Path, args: &[&str], input: &str| {
        use std::io::Write;
        let mut child = std::process::Command::new("git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", config)
            .env("GIT_TERMINAL_PROMPT", "0")
            .current_dir(dir)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("git");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("input");
        let output = child.wait_with_output().expect("output");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let fill = "protocol=https\nhost=example.invalid\n\n";

    // A bare helper name, found beside the host's tools.
    write_operator("fixture synthetic-secret");
    let staged = stage_git_config(&control, false, false, Some(&install)).expect("staged");
    assert!(
        staged
            .helpers
            .iter()
            .any(|path| path.ends_with("git-credential-fixture")),
        "{:?}",
        staged.helpers
    );
    let answer = git(&staged.config, &root, &["credential", "fill"], fill);
    assert!(answer.contains("username=fixture-user"), "{answer}");

    // A filter with its arguments, clean and smudge.
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    git(&staged.config, &repo, &["init", "-q"], "");
    std::fs::write(repo.join(".gitattributes"), "*.txt filter=demo\n").expect("attributes");
    std::fs::write(repo.join("a.txt"), "line\n").expect("file");
    git(&staged.config, &repo, &["add", "."], "");
    assert_eq!(
        git(&staged.config, &repo, &["cat-file", "-p", ":a.txt"], ""),
        "cleaned:line\n"
    );
    std::fs::remove_file(repo.join("a.txt")).expect("remove");
    git(&staged.config, &repo, &["checkout", "--", "a.txt"], "");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("read"),
        "smudged:cleaned:line\n"
    );

    // An operator's already-quoted shell helper keeps working, and is granted.
    write_operator(&format!("!{} synthetic-secret", shell_quoted(&quoted)));
    let staged = stage_git_config(&control, false, false, None).expect("staged");
    assert!(staged.helpers.contains(&quoted), "{:?}", staged.helpers);
    let answer = git(&staged.config, &root, &["credential", "fill"], fill);
    assert!(answer.contains("username=quoted-user"), "{answer}");
    TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = None);
}

/// With operator Git withheld, an unseated session's staged configuration
/// carries who commits and the filter drivers, and none of the operator's
/// credentials: no `credential.*` entry, no `nostr.keyfile`, and no helper
/// program to grant. The same operator configuration staged without the
/// setting carries all three, so the difference is the setting's.
#[cfg(unix)]
#[test]
fn withheld_operator_git_stages_no_credentials() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().expect("temp");
    let root = canonical(root.path()).expect("canonical");
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).expect("tools");
    for name in ["git-credential-fixture", "demo-filter"] {
        let path = tools.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("tool");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let keyfile = root.join("operator.key");
    std::fs::write(&keyfile, "nsec-fixture-not-a-key\n").expect("key");
    let operator = root.join("operator-gitconfig");
    for (key, value) in [
        ("user.name", "Operator"),
        ("user.email", "operator@example.invalid"),
        ("credential.helper", "fixture"),
        ("credential.https://example.invalid.helper", "fixture"),
        ("nostr.keyfile", keyfile.to_str().expect("utf-8")),
        ("filter.demo.clean", "demo-filter cleaned"),
    ] {
        let status = std::process::Command::new("git")
            .args(["config", "--file"])
            .arg(&operator)
            .args(["--add", key, value])
            .status()
            .expect("git config");
        assert!(status.success(), "operator config");
    }
    TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = Some(operator.clone()));
    let read = |staged: &StagedGit| std::fs::read_to_string(&staged.config).expect("staged file");

    let granted_dir = root.join("granted");
    std::fs::create_dir_all(&granted_dir).expect("control");
    let granted = stage_git_config(&granted_dir, true, false, Some(&tools)).expect("staged");
    let text = read(&granted);
    assert!(text.contains("[credential]"), "{text}");
    assert!(text.contains("keyfile"), "{text}");
    assert_eq!(granted.keyfile.as_deref(), Some(keyfile.as_path()));
    assert!(granted
        .helpers
        .iter()
        .any(|path| path.ends_with("git-credential-fixture")));

    let withheld_dir = root.join("withheld");
    std::fs::create_dir_all(&withheld_dir).expect("control");
    let withheld = stage_git_config(&withheld_dir, true, true, Some(&tools)).expect("staged");
    let text = read(&withheld);
    assert!(!text.contains("credential"), "{text}");
    assert!(!text.contains("nostr"), "{text}");
    assert!(!text.contains("keyfile"), "{text}");
    assert!(text.contains("name = Operator"), "{text}");
    assert!(text.contains("email = operator@example.invalid"), "{text}");
    assert!(text.contains("[filter \"demo\"]"), "{text}");
    assert_eq!(withheld.keyfile, None);
    assert!(
        withheld
            .helpers
            .iter()
            .all(|path| path.ends_with("demo-filter")),
        "only the filter driver is a program to grant: {:?}",
        withheld.helpers
    );
    TEST_OPERATOR_CONFIG.with(|config| *config.borrow_mut() = None);
}
