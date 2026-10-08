use super::*;

fn dev_origin() -> RefusedOrigin {
    let url = Url::parse("http://localhost:1420").expect("dev url");
    RefusedOrigin::from_dev_url(Some(&url)).expect("refused origin")
}

#[test]
fn loopback_http_and_https_are_allowed_on_any_port() {
    for url in [
        "http://localhost:5173/",
        "https://localhost:8443/app?x=1",
        "http://127.0.0.1:8000/",
        "http://[::1]:3000/",
        "http://LOCALHOST:3000",
        "http://localhost/",
        "  http://localhost:5173/  ",
    ] {
        assert!(
            check_preview_url(url, None).is_ok(),
            "{url} should be allowed"
        );
    }
    assert_eq!(
        check_preview_url("about:blank", None).map(|u| u.to_string()),
        Ok("about:blank".to_string())
    );
}

#[test]
fn everything_off_this_machine_or_privileged_is_refused() {
    for url in [
        "https://example.com/",
        "http://example.com:5173/",
        "file:///etc/passwd",
        "tauri://localhost/",
        "ipc://localhost/",
        "buzz-media://localhost/x",
        "data:text/html,<h1>x</h1>",
        "javascript:alert(1)",
        "http://foo.localhost:3000/",
        "http://0.0.0.0:3000/",
        "http://127.0.0.2:3000/",
        "http://192.168.1.10:3000/",
        "http://localhost.:3000/",
        "http://user:pw@localhost:3000/",
        "http://localhost:x@example.com/",
        "ws://localhost:3000/",
        "",
        "localhost:3000",
        "not a url",
    ] {
        let refusal = check_preview_url(url, None).expect_err(url);
        assert_eq!(refusal.code, URL_REFUSED_CODE, "{url}");
        assert!(refusal.message.starts_with(URL_REFUSED_SENTENCE), "{url}");
    }
}

#[test]
fn the_apps_own_dev_origin_is_refused_under_every_loopback_spelling() {
    let refused = dev_origin();
    for url in [
        "http://localhost:1420/",
        "http://127.0.0.1:1420/",
        "http://[::1]:1420/",
    ] {
        let refusal = check_preview_url(url, Some(&refused)).expect_err(url);
        assert_eq!(refusal.code, URL_REFUSED_CODE);
        assert!(refusal.message.contains("its own"), "{}", refusal.message);
        assert!(!navigation_allowed(url, Some(&refused)));
    }
    // Same host, other port: an ordinary dev server.
    assert!(check_preview_url("http://localhost:1421/", Some(&refused)).is_ok());
    // Release builds have no dev origin, so 1420 is an ordinary port there.
    assert!(check_preview_url("http://localhost:1420/", None).is_ok());
    assert_eq!(RefusedOrigin::from_dev_url(None), None);
}

#[test]
fn navigation_handler_also_allows_local_frame_documents() {
    for url in [
        "about:blank",
        "about:srcdoc",
        "data:text/html,x",
        "blob:http://localhost:5173/uuid",
        "http://localhost:5173/next",
    ] {
        assert!(navigation_allowed(url, None), "{url}");
    }
    for url in [
        "https://example.com/",
        "file:///tmp/x.html",
        "tauri://localhost",
    ] {
        assert!(!navigation_allowed(url, None), "{url}");
    }
}

#[test]
fn content_rule_list_blocks_first_then_allows_only_loopback() {
    let json = content_rule_list_json();
    let rules: Vec<serde_json::Value> = serde_json::from_str(&json).expect("rule list is JSON");
    assert_eq!(rules[0]["trigger"]["url-filter"], ".*");
    assert_eq!(rules[0]["action"]["type"], "block");
    let allows: Vec<&str> = rules[1..]
        .iter()
        .map(|rule| {
            assert_eq!(rule["action"]["type"], "ignore-previous-rules");
            rule["trigger"]["url-filter"].as_str().expect("filter")
        })
        .collect();
    // WebKit rule regexes have no disjunction.
    assert!(allows.iter().all(|filter| !filter.contains('|')));
    for expected in [
        "^https?://localhost/",
        "^https?://localhost:[0-9]+/",
        "^https?://127\\.0\\.0\\.1:[0-9]+/",
        "^https?://\\[::1\\]:[0-9]+/",
        "^wss?://localhost:[0-9]+/",
        "^about:",
        "^data:",
        "^blob:",
    ] {
        assert!(
            allows.contains(&expected),
            "missing {expected} in {allows:?}"
        );
    }
    assert_eq!(allows.len(), 15);
}

#[test]
fn content_rule_allow_filters_match_loopback_and_not_userinfo_tricks() {
    // The filters are plain enough to check with the `regex` crate, which
    // agrees with WebKit's subset on these constructs.
    let json = content_rule_list_json();
    let rules: Vec<serde_json::Value> = serde_json::from_str(&json).expect("rule list is JSON");
    let filters: Vec<regex::Regex> = rules[1..]
        .iter()
        .map(|rule| {
            let filter = rule["trigger"]["url-filter"].as_str().expect("filter");
            regex::Regex::new(&format!("(?i){filter}")).expect("regex")
        })
        .collect();
    let allowed = |url: &str| filters.iter().any(|re| re.is_match(url));
    for url in [
        "http://localhost:5173/src/main.ts",
        "ws://localhost:5173/",
        "http://127.0.0.1:8000/a.png",
        "http://[::1]:3000/",
        "https://localhost/",
        "data:image/png;base64,AAAA",
    ] {
        assert!(allowed(url), "{url}");
    }
    for url in [
        "https://cdn.example.com/x.js",
        "http://localhost:x@example.com/",
        "http://localhost:80@example.com/",
        "http://localhost.example.com/",
        "http://127.0.0.1.example.com/",
        "https://fonts.googleapis.com/css",
    ] {
        assert!(!allowed(url), "{url}");
    }
}

#[test]
fn data_store_id_is_stable_and_per_session() {
    let a = data_store_id("proj", "chan-a");
    assert_eq!(a, data_store_id("proj", "chan-a"));
    assert_ne!(a, data_store_id("proj", "chan-b"));
    assert_ne!(a, data_store_id("other", "chan-a"));
    // The separator keeps "ab|c" and "a|bc" apart.
    assert_ne!(data_store_id("ab", "c"), data_store_id("a", "bc"));
}

#[test]
fn cap_text_cuts_on_a_char_boundary() {
    assert_eq!(cap_text("hello", 10), ("hello".to_string(), false));
    assert_eq!(cap_text("hello", 3), ("hel".to_string(), true));
    // "é" is two bytes; a cut inside it backs off.
    assert_eq!(cap_text("aé", 2), ("a".to_string(), true));
}
