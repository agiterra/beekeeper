use super::*;
use sha2::{Digest, Sha256};

#[test]
fn vendored_playwright_script_is_byte_for_byte_v1_63_0() {
    // VENDOR.md records this hash; a changed file must change both.
    let digest = Sha256::digest(PLAYWRIGHT_INJECTED.as_bytes());
    assert_eq!(
        hex::encode(digest),
        "94103308b4f5791976b53543f5812be61ffb988574f7a51f412f87ab0ad60a85"
    );
}

#[test]
fn driver_source_wraps_playwright_the_way_playwright_does() {
    let source = driver_source();
    assert!(source.starts_with("(() => {\nif (globalThis.__beekeeperPreviewDriver) return;"));
    assert!(source.contains("new (module.exports.InjectedScript())(globalThis, "));
    assert!(source.contains("\"browserName\":\"webkit\""));
    assert!(source.contains("globalThis.__beekeeperPreviewDriverFactory = (createInjected) =>"));
    assert!(source.trim_end().ends_with("})();"));
    // The injected-script options are valid JSON.
    let options: serde_json::Value = serde_json::from_str(INJECTED_OPTIONS).expect("options");
    assert_eq!(options["testIdAttributeName"], "data-testid");
}

#[test]
fn run_body_returns_the_sentinel_when_the_driver_is_missing() {
    assert!(RUN_BODY.contains(DRIVER_MISSING));
    assert!(RUN_BODY.contains("JSON.parse(payload)"));
}

#[test]
fn driver_results_decode_to_object_or_refusal() {
    let ok = parse_driver_result(Some(r#"{"ok":true,"clicked":true}"#)).expect("ok");
    assert_eq!(ok.get("clicked"), Some(&json!(true)));
    assert!(!ok.contains_key("ok"));

    let refused = parse_driver_result(Some(
        r#"{"ok":false,"code":"preview_target_ambiguous","message":"That target matches 3 elements; add --nth or narrow it.","count":3}"#,
    ))
    .expect_err("refusal");
    assert_eq!(refused.code, "preview_target_ambiguous");
    assert!(refused.message.contains("3 elements"));

    for bad in [None, Some("not json"), Some("[1]")] {
        assert_eq!(
            parse_driver_result(bad).expect_err("bad").code,
            "preview_unavailable"
        );
    }
}

#[test]
fn aria_is_capped_at_a_line_boundary() {
    let short = "- button \"Save\" [ref=e1@g1]\n";
    assert_eq!(cap_aria(short), (short.to_string(), false));
    let line = "- link \"Some link text that repeats\" [ref=e12@g3]\n";
    let long = line.repeat(ARIA_CAP_BYTES / line.len() + 10);
    let (cut, truncated) = cap_aria(&long);
    assert!(truncated);
    assert!(cut.len() <= ARIA_CAP_BYTES);
    assert!(cut.ends_with('\n'));
    assert!(cut.lines().all(|l| format!("{l}\n") == line));
}

#[test]
fn eval_results_are_capped_at_64_kib() {
    let small = bound_eval_result(r#"{"title":"x"}"#);
    assert_eq!(small["value"], json!({"title": "x"}));
    assert_eq!(small["truncated"], json!(false));

    let big = format!("\"{}\"", "a".repeat(EVAL_RESULT_CAP_BYTES + 100));
    let capped = bound_eval_result(&big);
    assert_eq!(capped["truncated"], json!(true));
    assert_eq!(
        capped["value"].as_str().map(str::len),
        Some(EVAL_RESULT_CAP_BYTES)
    );
}

#[test]
fn eval_body_has_an_expression_and_a_statement_form_and_no_eval() {
    let expr = eval_body("document.title", false);
    assert!(expr.contains("(async () => (\ndocument.title\n))()"));
    let statements = eval_body("const t = document.title; return t;", true);
    assert!(statements.contains("(async () => {\nconst t = document.title; return t;\n})()"));
    for body in [expr, statements] {
        assert!(!body.contains("eval("), "page CSP may forbid eval");
        assert!(body.contains("return __bkJson === undefined ? \"null\" : __bkJson;"));
    }
}
