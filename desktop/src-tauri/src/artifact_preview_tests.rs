use super::*;

const CAPABILITIES: &str = include_str!("../capabilities/default.json");

/// The third isolation layer, pinned.
///
/// A preview window runs a mockup's own JavaScript. What keeps that from
/// reaching the app is that its label matches no `windows` entry in the
/// capability, so every command — `core:default` included — is denied. Adding
/// a glob that happens to cover `artifact-preview-*` would hand a previewed
/// document the app's whole command surface, silently, and nothing else in CI
/// would notice.
#[test]
fn preview_window_label_matches_no_capability() {
    let capability: serde_json::Value =
        serde_json::from_str(CAPABILITIES).expect("capabilities/default.json is valid JSON");
    let windows = capability["windows"]
        .as_array()
        .expect("the capability names its windows");
    let label = format!("{PREVIEW_LABEL_PREFIX}deadbeef");
    for pattern in windows {
        let pattern = pattern.as_str().expect("a window pattern is a string");
        assert!(
            !glob_matches(pattern, &label),
            "capability window pattern {pattern:?} matches the preview label {label:?}; a \
             previewed document would get the app's commands"
        );
    }
}

/// Tauri's window patterns are globs with `*`; this is enough of one to tell
/// whether a pattern would admit the preview label.
fn glob_matches(pattern: &str, label: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == label,
        Some((prefix, suffix)) => {
            label.len() >= prefix.len() + suffix.len()
                && label.starts_with(prefix)
                && label.ends_with(suffix)
        }
    }
}

/// The preview's CSP is what lets scripts run *and* keeps them from reaching
/// anything. Both halves are the policy, so both are pinned: a tightening that
/// drops `'unsafe-inline'` makes the preview a lie, and a loosening that adds
/// a `connect-src` makes it an exfiltration path.
#[test]
fn the_preview_policy_runs_scripts_and_withholds_the_network() {
    let directives: std::collections::HashMap<&str, Vec<&str>> = PREVIEW_CSP
        .split(';')
        .filter_map(|directive| {
            let mut parts = directive.split_whitespace();
            let name = parts.next()?;
            Some((name, parts.collect()))
        })
        .collect();

    let script = directives.get("script-src").expect("script-src is set");
    assert!(script.contains(&"'unsafe-inline'"), "{script:?}");
    assert!(script.contains(&"'unsafe-eval'"), "{script:?}");
    assert!(
        !script.iter().any(|source| source.contains("://")),
        "a previewed document must not load remote code: {script:?}"
    );

    assert_eq!(
        directives.get("default-src").map(|v| v.as_slice()),
        Some(["'none'"].as_slice())
    );
    assert_eq!(
        directives.get("connect-src").map(|v| v.as_slice()),
        Some(["'none'"].as_slice()),
        "no fetch, no XHR, no WebSocket, no beacon"
    );
    assert_eq!(
        directives.get("form-action").map(|v| v.as_slice()),
        Some(["'none'"].as_slice())
    );
    assert_eq!(
        directives.get("frame-ancestors").map(|v| v.as_slice()),
        Some(["'none'"].as_slice()),
        "the preview is never a frame inside the app"
    );
    assert_eq!(
        directives.get("base-uri").map(|v| v.as_slice()),
        Some(["'none'"].as_slice())
    );
    // Images may come from the snapshot and from data URIs — a diagram inlined
    // as base64 is ordinary — but from nowhere a request could leave for.
    let img = directives.get("img-src").expect("img-src is set");
    assert!(
        img.contains(&"buzz-doc:") && img.contains(&"data:"),
        "{img:?}"
    );
    assert!(
        !img.iter().any(|source| source.starts_with("http")),
        "an http img-src is an exfiltration path: {img:?}"
    );
}

/// The scan that decides what a snapshot carries. Over-collecting costs a byte
/// read; under-collecting is reported as `missing`, so the bias is deliberate.
#[test]
fn only_siblings_are_collected() {
    let html = r##"
        <img src="img/shot.png">
        <img src='img/other.png?v=2'>
        <link href="style.css" rel="stylesheet">
        <img src="/absolute.png">
        <img src="../escape.png">
        <img src="https://example.com/remote.png">
        <img src="data:image/png;base64,AAAA">
        <a href="#anchor">x</a>
    "##;
    assert_eq!(
        referenced_siblings(html),
        vec!["img/other.png", "img/shot.png", "style.css"]
    );
}

#[test]
fn a_folder_is_the_path_up_to_its_last_slash() {
    assert_eq!(folder_of("docs/mockups/login.html"), "docs/mockups/");
    assert_eq!(folder_of("docs/login.html"), "docs/");
    assert_eq!(folder_of("login.html"), "");
}

#[test]
fn percent_escapes_resolve_and_a_bad_one_is_left_alone() {
    assert_eq!(percent_decode("my%20shot.png"), "my shot.png");
    assert_eq!(percent_decode("plain.png"), "plain.png");
    // Not an escape: left verbatim rather than silently eaten, so the lookup
    // misses and the file reports missing instead of resolving to something
    // else.
    assert_eq!(percent_decode("100%sure.png"), "100%sure.png");
}

#[test]
fn a_type_is_decided_by_extension_and_an_unknown_one_is_a_download() {
    assert_eq!(mime_for("login.html"), "text/html; charset=utf-8");
    assert_eq!(mime_for("LOGIN.HTML"), "text/html; charset=utf-8");
    assert_eq!(mime_for("shot.png"), "image/png");
    assert_eq!(mime_for("shot.jpeg"), "image/jpeg");
    assert_eq!(mime_for("diagram.svg"), "image/svg+xml");
    assert_eq!(mime_for("notes"), "application/octet-stream");
    assert_eq!(mime_for("thing.exe"), "application/octet-stream");
}
