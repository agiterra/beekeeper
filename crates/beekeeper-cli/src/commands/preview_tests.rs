use super::*;

use clap::{CommandFactory, Parser};

/// `bee preview …` as its own parser, so the verb tree is tested without the
/// top-level `Cli` (whose registration is the integrator's).
#[derive(Debug, Parser)]
#[command(name = "preview")]
struct TestCli {
    #[command(subcommand)]
    cmd: PreviewCmd,
}

fn parse(args: &[&str]) -> Result<PreviewCmd, clap::Error> {
    let mut argv = vec!["preview"];
    argv.extend_from_slice(args);
    TestCli::try_parse_from(argv).map(|cli| cli.cmd)
}

fn action(args: &[&str]) -> Value {
    let cmd = parse(args).expect("parses");
    action_for(&cmd).expect("builds").1
}

#[test]
fn the_verb_tree_is_well_formed_and_stable() {
    TestCli::command().debug_assert();
    let mut names: Vec<String> = TestCli::command()
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "click", "close", "eval", "navigate", "open", "press", "scroll", "servers", "snapshot",
            "status", "type", "wait-for"
        ]
    );
    // No session argument anywhere: the session comes from the grant.
    for sub in TestCli::command().get_subcommands() {
        assert!(
            sub.get_arguments()
                .all(|arg| !arg.get_id().as_str().contains("session")),
            "{} takes a session argument",
            sub.get_name()
        );
    }
}

#[test]
fn each_verb_builds_the_wire_action() {
    assert_eq!(action(&["status"]), json!({"verb": "status"}));
    assert_eq!(action(&["servers"]), json!({"verb": "servers"}));
    assert_eq!(action(&["close"]), json!({"verb": "close"}));
    assert_eq!(
        action(&["open", "--port", "5173"]),
        json!({"verb": "open", "port": 5173})
    );
    assert_eq!(
        action(&["open", "--url", "http://localhost:5173/"]),
        json!({"verb": "open", "url": "http://localhost:5173/"})
    );
    assert_eq!(
        action(&["navigate", "http://127.0.0.1:3000/a"]),
        json!({"verb": "navigate", "url": "http://127.0.0.1:3000/a"})
    );
    assert_eq!(
        action(&["navigate", "--back"]),
        json!({"verb": "navigate", "back": true})
    );
    assert_eq!(
        action(&["navigate", "--reload"]),
        json!({"verb": "navigate", "reload": true})
    );
    assert_eq!(
        action(&["snapshot"]),
        json!({"verb": "snapshot", "image": true})
    );
    assert_eq!(
        action(&["snapshot", "--no-image"]),
        json!({"verb": "snapshot", "image": false})
    );
    assert_eq!(
        action(&["click", "--role", "button", "--name", "Save"]),
        json!({"verb": "click", "target": {"role": "button", "name": "Save", "exact": false}})
    );
    assert_eq!(
        action(&[
            "click",
            "--ref",
            "e12@g3",
            "--button",
            "right",
            "--click-count",
            "2"
        ]),
        json!({"verb": "click", "target": {"ref": "e12@g3"}, "button": "right", "clickCount": 2})
    );
    assert_eq!(
        action(&["type", "--label", "Email", "a@b"]),
        json!({"verb": "type", "target": {"label": "Email"}, "text": "a@b", "clear": false})
    );
    assert_eq!(
        action(&["type", "--test-id", "q", "--nth", "1", "x", "--clear"]),
        json!({"verb": "type", "target": {"testId": "q", "nth": 1}, "text": "x", "clear": true})
    );
    assert_eq!(
        action(&["press", "Enter"]),
        json!({"verb": "press", "key": "Enter"})
    );
    assert_eq!(
        action(&["press", "Meta+a", "--selector", "#q"]),
        json!({"verb": "press", "key": "Meta+a", "target": {"selector": "#q"}})
    );
    assert_eq!(
        action(&["scroll", "--dy", "-600"]),
        json!({"verb": "scroll", "dx": 0, "dy": -600})
    );
    assert_eq!(
        action(&["scroll", "--text", "Footer", "--to", "bottom"]),
        json!({"verb": "scroll", "target": {"text": "Footer"}, "to": "bottom"})
    );
    assert_eq!(
        action(&["eval", "document.title"]),
        json!({"verb": "eval", "expression": "document.title", "world": "driver"})
    );
    assert_eq!(
        action(&["eval", "window.app", "--page-world"]),
        json!({"verb": "eval", "expression": "window.app", "world": "page"})
    );
    assert_eq!(
        action(&["wait-for", "--text", "Saved", "--timeout-ms", "2000"]),
        json!({"verb": "wait_for", "text": "Saved", "timeoutMs": 2000})
    );
    assert_eq!(
        action(&["wait-for", "--role", "dialog", "--state", "hidden"]),
        json!({"verb": "wait_for", "target": {"role": "dialog"}, "state": "hidden"})
    );
}

#[test]
fn bad_invocations_are_input_errors() {
    for args in [
        &["open"][..],
        &["open", "--url", "http://localhost/", "--port", "1"],
        &["navigate"],
        &["navigate", "http://localhost/", "--back"],
        &["click", "--role", "button", "--label", "x"],
        &["click", "--name", "Save"],
        &["scroll"],
        &["scroll", "--dy", "1", "--to", "top"],
        &["status", "--session", "S"],
    ] {
        assert!(parse(args).is_err(), "{args:?} parsed");
    }
    for args in [&["click"][..], &["type", "x"], &["wait-for"]] {
        let failure = action_for(&parse(args).expect("parses")).expect_err("refused");
        assert_eq!(failure.code, "preview_bad_request", "{args:?}");
        assert_eq!(failure.exit_code(), 1);
    }
    let failure =
        action_for(&parse(&["wait-for", "--text", "x", "--timeout-ms", "30001"]).expect("parses"))
            .expect_err("refused");
    assert_eq!(failure.exit_code(), 1);
}

#[test]
fn exit_codes_follow_the_wire_contract() {
    for (code, exit) in [
        ("preview_url_refused", 1),
        ("preview_bad_request", 1),
        ("preview_too_large", 1),
        ("preview_no_browser", 2),
        ("preview_no_grant", 3),
        ("preview_grant_malformed", 3),
        ("preview_grant_invalid", 3),
        ("preview_wrong_issuer", 3),
        ("preview_grant_bad_signature", 3),
        ("preview_wrong_audience", 3),
        ("preview_grant_expired", 3),
        ("preview_grant_not_yet_valid", 3),
        ("preview_wrong_session", 3),
        ("preview_not_open", 4),
        ("preview_closed_by_person", 4),
        ("preview_target_not_found", 4),
        ("preview_target_ambiguous", 4),
        ("preview_stale_ref", 4),
        ("preview_timeout", 4),
        ("preview_eval_error", 4),
        ("preview_unavailable", 4),
    ] {
        assert_eq!(exit_code_for(code), exit, "{code}");
    }
}

#[test]
fn responses_parse_into_results_and_coded_refusals() {
    assert_eq!(
        parse_response("{\"ok\":true,\"result\":{\"closed\":true}}\n"),
        Ok(json!({"closed": true}))
    );
    assert_eq!(
        parse_response(
            "{\"ok\":false,\"code\":\"preview_wrong_session\",\"error\":\"preview grant belongs to a different session\"}"
        ),
        Err(PreviewFailure {
            code: "preview_wrong_session".into(),
            error: "preview grant belongs to a different session".into()
        })
    );
    // An app from before previews: no code, so unavailable, with its words.
    let old = parse_response("{\"ok\":false,\"error\":\"unknown variant `preview`\"}")
        .expect_err("refused");
    assert_eq!(old.code, "preview_unavailable");
    assert!(old.error.contains("unknown variant"));
    assert_eq!(
        parse_response("").map_err(|f| f.code),
        Err("preview_broker_unreachable".into())
    );
    assert_eq!(
        parse_response("not json").map_err(|f| f.code),
        Err("preview_bad_response".into())
    );
}

#[cfg(unix)]
mod live {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A one-shot broker: accepts one connection, records the request line,
    /// answers `response`.
    fn broker(socket: &Path, response: Value) -> std::thread::JoinHandle<Value> {
        let listener = UnixListener::bind(socket).expect("bind");
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("read");
            let mut writer = &stream;
            writeln!(writer, "{response}").expect("write");
            serde_json::from_str(&line).expect("request json")
        })
    }

    fn context(dir: &Path, grant: Option<&str>) -> PreviewContext {
        PreviewContext {
            socket: dir.join("b.sock"),
            grant: grant.map(str::to_owned),
            caller: None,
            cwd: dir.to_path_buf(),
            now_ms: 1_790_000_000_123,
        }
    }

    #[test]
    fn no_app_is_exit_2_with_the_sentence() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), Some("bkpg1.x"));
        let failure = execute(&parse(&["status"]).expect("parses"), &ctx).expect_err("no app");
        assert_eq!(failure.code, "preview_no_browser");
        assert_eq!(failure.error, NO_BROWSER_SENTENCE);
        assert_eq!(failure.exit_code(), 2);
        assert_eq!(
            failure.to_json(),
            json!({"ok": false, "code": "preview_no_browser", "error": NO_BROWSER_SENTENCE})
        );
        // A socket file with nobody listening is the same answer.
        let stale = UnixListener::bind(&ctx.socket).expect("bind");
        drop(stale);
        let failure = execute(&parse(&["status"]).expect("parses"), &ctx).expect_err("refused");
        assert_eq!(failure.exit_code(), 2);
    }

    #[test]
    fn the_grant_travels_in_the_envelope_and_the_result_is_printed_flat() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), Some("bkpg1.token"));
        let seen = broker(
            &ctx.socket,
            json!({"ok": true, "result": {"clicked": true, "url": "http://localhost:5173/",
                   "generation": 3, "input": "synthetic"}}),
        );
        let out = execute(
            &parse(&["click", "--role", "button", "--name", "Save"]).expect("parses"),
            &ctx,
        )
        .expect("ok");
        assert_eq!(
            out,
            json!({"ok": true, "verb": "click", "clicked": true,
                   "url": "http://localhost:5173/", "generation": 3, "input": "synthetic"})
        );
        let request = seen.join().expect("broker");
        assert_eq!(request["request"]["op"], "preview");
        assert_eq!(request["request"]["grant"], "bkpg1.token");
        assert_eq!(request["request"]["action"]["verb"], "click");
    }

    #[test]
    fn a_missing_grant_is_the_brokers_refusal_exit_3() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), None);
        let seen = broker(
            &ctx.socket,
            json!({"ok": false, "code": "preview_no_grant",
                   "error": "no preview grant was presented"}),
        );
        let failure = execute(&parse(&["status"]).expect("parses"), &ctx).expect_err("refused");
        assert_eq!(failure.code, "preview_no_grant");
        assert_eq!(failure.exit_code(), 3);
        let request = seen.join().expect("broker");
        assert_eq!(request["request"]["grant"], Value::Null);
    }

    #[test]
    fn a_snapshot_lands_in_the_callers_tree_and_never_prints_base64() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), Some("bkpg1.token"));
        let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3];
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let seen = broker(
            &ctx.socket,
            json!({"ok": true, "result": {"url": "http://localhost:5173/", "title": "App",
                   "generation": 4, "aria": "- button \"Save\"\n", "ariaBytes": 17,
                   "ariaTruncated": false, "input": "synthetic",
                   "png": {"base64": encoded, "width": 2, "height": 1, "bytes": 7}}}),
        );
        let out = execute(&parse(&["snapshot"]).expect("parses"), &ctx).expect("ok");
        seen.join().expect("broker");
        let png_path = dir
            .path()
            .join(".beekeeper/preview/snapshot-4-1790000000123.png");
        let aria_path = dir
            .path()
            .join(".beekeeper/preview/snapshot-4-1790000000123.aria.yaml");
        assert_eq!(std::fs::read(&png_path).expect("png"), png);
        assert_eq!(
            std::fs::read_to_string(&aria_path).expect("aria"),
            "- button \"Save\"\n"
        );
        assert_eq!(out["pngPath"], json!(png_path.display().to_string()));
        assert_eq!(out["ariaPath"], json!(aria_path.display().to_string()));
        assert_eq!(out["png"], json!({"width": 2, "height": 1, "bytes": 7}));
        assert!(!out.to_string().contains(&encoded));
    }

    #[test]
    fn no_image_with_out_writes_only_the_aria_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), Some("bkpg1.token"));
        let seen = broker(
            &ctx.socket,
            json!({"ok": true, "result": {"generation": 5, "aria": "- main\n", "png": null}}),
        );
        let out = execute(
            &parse(&["snapshot", "--no-image", "--out", "shots/home.png"]).expect("parses"),
            &ctx,
        )
        .expect("ok");
        let request = seen.join().expect("broker");
        assert_eq!(request["request"]["action"]["image"], false);
        assert_eq!(out["pngPath"], Value::Null);
        let aria = dir.path().join("shots/home.aria.yaml");
        assert_eq!(out["ariaPath"], json!(aria.display().to_string()));
        assert_eq!(std::fs::read_to_string(aria).expect("aria"), "- main\n");
        assert!(!dir.path().join("shots/home.png").exists());
    }

    #[test]
    fn an_oversized_request_is_refused_before_the_socket() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path(), Some("bkpg1.token"));
        let huge = "x".repeat(MAX_REQUEST_BYTES + 1);
        let failure =
            execute(&parse(&["eval", &huge]).expect("parses"), &ctx).expect_err("refused");
        assert_eq!(failure.code, "preview_too_large");
        assert_eq!(failure.exit_code(), 1);
    }
}
