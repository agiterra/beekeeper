//! The hardening every host Git command here carries, measured against the
//! programs a repository's own configuration can name: each case plants
//! them, proves them live with plain Git, then shows the hardened command
//! runs none of them.

/// The automatic repo-sync fetch runs a repository's own configuration
/// outside any project boundary, so none of the programs that
/// configuration can name for a fetch may run: a repo-local credential
/// helper (generic or scoped to the remote's URL) and a repo-local askpass
/// program, both reached when the remote answers a challenge. The control
/// — the same fetch with plain Git — shows every one of them is live.
#[cfg(unix)]
#[test]
fn a_hardened_fetch_runs_no_program_the_repository_configures() {
    use std::io::{Read, Write};
    use std::os::unix::fs::PermissionsExt;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"t\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let root = tempfile::tempdir().expect("tempdir");
    let root = root.path().canonicalize().expect("canonical");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let url = format!("http://127.0.0.1:{port}/project.git");
    let canaries = ["generic-helper", "url-helper", "askpass"].map(|name| root.join(name));
    let askpass = root.join("askpass.sh");
    std::fs::write(
        &askpass,
        format!("#!/bin/sh\ntouch '{}'\n", canaries[2].display()),
    )
    .expect("askpass");
    std::fs::set_permissions(&askpass, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let plain = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_ASKPASS")
            .env_remove("SSH_ASKPASS")
            .output()
            .expect("git")
    };
    plain(&["init", "-q"]);
    plain(&["remote", "add", "origin", &url]);
    for (key, value) in [
        (
            "credential.helper".to_owned(),
            format!("!touch '{}'; true", canaries[0].display()),
        ),
        (
            format!("credential.{url}.helper"),
            format!("!touch '{}'; true", canaries[1].display()),
        ),
        ("core.askPass".to_owned(), askpass.display().to_string()),
    ] {
        assert!(plain(&["config", &key, &value]).status.success(), "{key}");
    }

    // Control: plain Git runs every one of them.
    let _ = plain(&["fetch", "--quiet", "origin"]);
    for canary in &canaries {
        assert!(
            canary.exists(),
            "{} did not run: the fixture proves nothing",
            canary.display()
        );
        std::fs::remove_file(canary).expect("reset");
    }

    let mut auth = super::build_test_git_auth_config().expect("auth");
    auth.credential_helper = None;
    auth.allow_file_transport = false;
    let fetched = super::run_git(&["fetch", "--quiet", "origin"], Some(&repo), &auth);
    assert!(fetched.is_err(), "the challenge was answered: {fetched:?}");
    for canary in &canaries {
        assert!(
            !canary.exists(),
            "the hardened fetch ran {}",
            canary.display()
        );
    }
}

/// The automatic snapshot reads history with `git log`; a repository's own
/// `log.showSignature` would make that run its `gpg.program` on any commit
/// carrying a signature. The control, plain Git, shows the program runs.
#[test]
fn hardened_history_reads_run_no_signature_program_the_repository_configures() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("tempdir");
    let root = root.path().canonicalize().expect("canonical");
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).expect("repo");
    let canary = root.join("SIGNATURE_PROGRAM_RAN");
    let program = root.join("fake-gpg.sh");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\ntouch '{}'\ncat >/dev/null\necho '[GNUPG:] SIG_CREATED D 1 8 00 0 X' >&2\n\
             printf -- '-----BEGIN PGP SIGNATURE-----\\n\\nx\\n-----END PGP SIGNATURE-----\\n'\n",
            canary.display()
        ),
    )
    .expect("program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let plain = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .expect("git")
    };
    plain(&["init", "-q"]);
    for (key, value) in [
        ("gpg.program", program.to_str().expect("program")),
        ("user.signingKey", "X"),
        ("log.showSignature", "true"),
    ] {
        assert!(plain(&["config", key, value]).status.success(), "{key}");
    }
    let signed = plain(&["commit", "-q", "-S", "--allow-empty", "-m", "signed"]);
    assert!(
        signed.status.success(),
        "{}",
        String::from_utf8_lossy(&signed.stderr)
    );
    std::fs::remove_file(&canary).expect("reset after signing");

    // Control: plain Git runs the repository's signature program.
    let _ = plain(&["log", "-1", "--format=%H"]);
    assert!(
        canary.exists(),
        "the fixture's signature program did not run: it proves nothing"
    );
    std::fs::remove_file(&canary).expect("reset");

    let auth = super::build_test_git_auth_config().expect("auth");
    super::run_git(&["log", "-1", "--format=%H%x00%s"], Some(&repo), &auth).expect("log");
    super::run_git(
        &["show", "--no-patch", "--format=%b", "HEAD"],
        Some(&repo),
        &auth,
    )
    .expect("show");
    assert!(
        !canary.exists(),
        "a hardened history read ran the repository's signature program"
    );
}
