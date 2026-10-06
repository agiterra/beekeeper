use super::*;

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "codex-primary".into(),
        session_id: "session-1".into(),
        generation: 1,
    }
}

fn history(seq: u64, kind: &str) -> CodingSessionContextHistoryItem {
    CodingSessionContextHistoryItem {
        event_id: format!("{seq:064x}"),
        created_at: seq,
        author: "ab".repeat(32),
        source_kind: crate::kind::KIND_CODING_SESSION_TRANSCRIPT,
        target: target(),
        event_seq: seq,
        turn_id: Some("turn-1".into()),
        role: coding_session_context_role_for_item_kind(kind).unwrap(),
        item_kind: kind.into(),
        content: serde_json::json!({"kind": kind, "text": "verified relay fact"}),
    }
}

fn package() -> CodingSessionContextPackage {
    CodingSessionContextPackage {
        v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
        session: CodingSessionContextIdentity {
            session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
            genesis_ref: "cd".repeat(32),
            channel_id: Uuid::nil(),
            name: Some("Rehydrated context".into()),
            goal: Some("Continue from verified durable facts".into()),
            project_ref: Some(format!("30621:{}:buzz", "ef".repeat(32))),
        },
        provenance: CodingSessionContextProvenance {
            generated_at: 1,
            complete_as_of: Some(1),
            complete: true,
            truncated: false,
            source_event_count: 4,
            source_event_breakdown: None,
            included_history_items: 2,
            omitted_history_items: 0,
            total_history_items: Some(2),
            notes: vec!["Relay query reached EOSE without a local package truncation".into()],
        },
        history: vec![history(1, "user_prompt"), history(2, "assistant_text")],
        roster: Vec::new(),
        inbox: Vec::new(),
        policy: None,
    }
}

#[test]
fn valid_package_round_trips_as_strict_json() {
    let package = package();
    package.validate().unwrap();
    let encoded = serde_json::to_string(&package).unwrap();
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, package);
    decoded.validate().unwrap();
}

#[test]
fn additive_watermark_keeps_legacy_packages_readable() {
    let mut encoded = serde_json::to_value(package()).unwrap();
    encoded["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("completeAsOf");
    let decoded: CodingSessionContextPackage = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.provenance.complete_as_of, None);
    decoded.validate().unwrap();
}

#[test]
fn completeness_and_truncation_are_independent_but_accounted() {
    let mut context = package();
    context.provenance.complete = false;
    context.provenance.complete_as_of = None;
    context.provenance.total_history_items = None;
    context.validate().unwrap();
    context.provenance.truncated = true;
    context.provenance.omitted_history_items = 3;
    context.validate().unwrap();
    context.provenance.truncated = false;
    assert!(context.validate().is_err());

    let mut unsafe_note = package();
    unsafe_note.provenance.notes = vec!["queried /Users/alice/private/repo".into()];
    assert!(unsafe_note.validate().is_err());
}

#[test]
fn rejects_conflicts_and_semantic_role_substitution() {
    let mut conflicting = package();
    conflicting.history[1].event_seq = 1;
    assert!(conflicting.validate().is_err());

    let mut substituted = package();
    substituted.history[0].role = CodingSessionContextRole::Assistant;
    assert!(substituted.validate().is_err());

    let mut mismatched = package();
    mismatched.history[0].content["kind"] = Value::String("assistant_text".into());
    assert!(mismatched.validate().is_err());
}

#[test]
fn rejects_unknown_fields_and_unrecognized_item_kinds() {
    let encoded = serde_json::to_value(package()).unwrap();
    let mut smuggled = encoded.clone();
    smuggled
        .as_object_mut()
        .unwrap()
        .insert("privateKey".into(), Value::String("secret".into()));
    assert!(serde_json::from_value::<CodingSessionContextPackage>(smuggled).is_err());

    let mut unrecognized = package();
    unrecognized.history[0].item_kind = "native_provider_state".into();
    unrecognized.history[0].content["kind"] = Value::String("native_provider_state".into());
    assert!(unrecognized.validate().is_err());

    let mut host_project = package();
    host_project.session.project_ref = Some(format!(
        "30621:{}:/Users/alice/private/repo",
        "ef".repeat(32)
    ));
    assert!(host_project.validate().is_err());
}

#[test]
fn rejects_an_item_too_large_to_return_in_full() {
    let mut package = package();
    package.history[0].content["text"] =
        Value::String("x".repeat(MAX_CONTEXT_HISTORY_CONTENT_BYTES + 1));
    assert!(package.validate().is_err());
}

#[test]
fn first_turn_brief_indexes_outcomes_without_replaying_sensitive_content() {
    let mut package = package();
    package.history = vec![
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "user_prompt",
                "content": "Inspect /Users/alice/private/repo with password hunter2",
                "operatorPubkey": "ab".repeat(32),
                "steered": false
            }),
            ..history(1, "user_prompt")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "tool_call",
                "tool": {"toolName": "cargo_test", "toolId": "tool-1", "input": {}}
            }),
            ..history(2, "tool_call")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "tool_result",
                "toolName": "cargo_test",
                "toolId": "tool-1",
                "content": "failed at /Users/alice/private/repo",
                "isError": true
            }),
            ..history(3, "tool_result")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "result",
                "subtype": "error",
                "isError": true,
                "durationMs": 10,
                "result": "stopped: token limit reached"
            }),
            ..history(4, "result")
        },
    ];
    package.provenance.included_history_items = 4;
    package.provenance.total_history_items = Some(4);

    let brief = coding_session_first_turn_brief(&package);
    let encoded = serde_json::to_string(&brief).unwrap();
    validate_coding_session_first_turn_brief_json(&encoded).unwrap();
    assert!(!encoded.contains("/Users/alice"));
    assert!(!encoded.contains("hunter2"));
    assert_eq!(brief["recentTurns"][0]["request"], Value::Null);
    assert_eq!(brief["recentTurns"][0]["requestOmittedForSafety"], true);
    assert_eq!(brief["recentTurns"][0]["outcome"], "token_limit");
    assert_eq!(
        brief["recentTurns"][0]["toolAttempts"][0]["outcome"],
        "failed"
    );
    assert_eq!(brief["snapshot"]["completeAsOf"], 1);

    let substituted = encoded.replace(
        "coding-session-first-turn-brief/v1",
        "/Users/alice/private/brief",
    );
    assert!(validate_coding_session_first_turn_brief_json(&substituted).is_err());

    let path_only = sanitize_coding_session_context_text(
        "Inspect /Users/alice/private/repo then crates/beekeeper-core/src/lib.rs",
    );
    assert!(path_only.starts_with("Inspect [elided private context:"));
    assert!(!path_only.contains("/Users/alice"));
    assert!(path_only.contains("crates/beekeeper-core/src/lib.rs"));
    assert!(
        !sanitize_coding_session_context_text("Read(/Users/alice/repo)").contains("/Users/alice")
    );

    let secret_field = serde_json::json!({"kind": "tool_call", "password": "hunter2"});
    let sanitized = sanitize_coding_session_context_content(&secret_field);
    assert_eq!(
        sanitize_coding_session_context_content(&sanitized),
        sanitized,
        "sanitization must be idempotent so strict package validation accepts it"
    );
    let token_field = sanitize_coding_session_context_content(
        &serde_json::json!({"kind": "tool_call", "accessToken": "opaque"}),
    );
    assert!(!token_field.to_string().contains("opaque"));
}

#[test]
fn first_turn_brief_is_byte_bounded_and_reports_omitted_turns() {
    let mut package = package();
    package.history = (1..=40)
        .map(|seq| CodingSessionContextHistoryItem {
            turn_id: Some(format!("turn-{seq}")),
            content: serde_json::json!({
                "kind": "user_prompt",
                "content": "x".repeat(MAX_CONTEXT_BRIEF_TEXT_BYTES)
            }),
            ..history(seq, "user_prompt")
        })
        .collect();
    package.provenance.included_history_items = 40;
    package.provenance.total_history_items = Some(40);

    let brief = coding_session_first_turn_brief(&package);
    let encoded = serde_json::to_vec(&brief).unwrap();

    assert!(encoded.len() <= MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES);
    assert_eq!(brief["snapshot"]["indexedTurnCount"], 40);
    assert!(brief["snapshot"]["omittedTurnCount"].as_u64().unwrap() > 0);
}

/// A package whose `sourceEventCount` is reconciled by an explicit breakdown:
/// one genesis, two authority links (two events each), one name revision, one
/// goal revision, one generation (three bookkeeping events) and the two
/// transcript facts that became history items.
fn reconciled() -> CodingSessionContextPackage {
    let breakdown = CodingSessionContextSourceBreakdown {
        genesis_events: 1,
        authority_link_events: 4,
        name_revision_events: 1,
        goal_revision_events: 1,
        generation_bookkeeping_events: 3,
        transcript_events: 2,
        inbox_events: 0,
    };
    let mut package = package();
    package.provenance.source_event_count = breakdown.total();
    package.provenance.source_event_breakdown = Some(breakdown);
    package
}

#[test]
fn source_breakdown_terms_sum_to_source_event_count() {
    let package = reconciled();
    package.validate().unwrap();

    let breakdown = package.provenance.source_event_breakdown.clone().unwrap();
    assert_eq!(breakdown.total(), 12);
    assert_eq!(breakdown.total(), package.provenance.source_event_count);
    // Only transcript events can become history items — that is the whole
    // reason sourceEventCount exceeds totalHistoryItems.
    assert_eq!(
        breakdown.transcript_events,
        package.provenance.total_history_items.unwrap()
    );

    let encoded = serde_json::to_string(&package).unwrap();
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, package);
    decoded.validate().unwrap();
    for key in [
        "genesisEvents",
        "authorityLinkEvents",
        "nameRevisionEvents",
        "goalRevisionEvents",
        "generationBookkeepingEvents",
        "transcriptEvents",
    ] {
        assert!(encoded.contains(key), "{key} must be on the wire");
    }
}

#[test]
fn a_package_whose_breakdown_does_not_sum_is_rejected() {
    for terms in [2, 4] {
        let mut package = reconciled();
        let breakdown = package.provenance.source_event_breakdown.as_mut().unwrap();
        breakdown.generation_bookkeeping_events = terms;
        let error = package.validate().unwrap_err();
        assert!(
            error.contains("sourceEventBreakdown"),
            "unexpected error: {error}"
        );
    }

    // A term large enough to overflow the sum is a rejection, not a wrap.
    let mut overflowing = reconciled();
    overflowing
        .provenance
        .source_event_breakdown
        .as_mut()
        .unwrap()
        .transcript_events = u64::MAX;
    let error = overflowing.validate().unwrap_err();
    assert!(error.contains("overflows"), "unexpected error: {error}");

    // The count moving without the breakdown moving is the same defect.
    let mut package = reconciled();
    package.provenance.source_event_count += 1;
    assert!(package.validate().is_err());
}

#[test]
fn a_package_whose_transcript_events_are_fewer_than_included_plus_omitted_is_rejected() {
    let mut package = reconciled();
    package.provenance.truncated = true;
    package.provenance.omitted_history_items = 1;
    package.provenance.total_history_items = Some(3);
    let error = package.validate().unwrap_err();
    assert!(
        error.contains("transcriptEvents"),
        "unexpected error: {error}"
    );

    // Accounting for the omitted item in the breakdown makes it reconcile.
    let breakdown = package.provenance.source_event_breakdown.as_mut().unwrap();
    breakdown.transcript_events = 3;
    package.provenance.source_event_count += 1;
    package.validate().unwrap();
}

#[test]
fn a_version_1_package_without_a_breakdown_still_validates() {
    let mut package = package();
    package.v = MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION;
    assert_eq!(package.provenance.source_event_breakdown, None);
    package.validate().unwrap();

    let encoded = serde_json::to_string(&package).unwrap();
    assert!(
        !encoded.contains("sourceEventBreakdown"),
        "an absent breakdown must be omitted, never emitted as null"
    );
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.provenance.source_event_breakdown, None);
    decoded.validate().unwrap();
}

#[test]
fn a_future_package_version_is_rejected_by_name() {
    let mut package = reconciled();
    let future = CODING_SESSION_CONTEXT_PACKAGE_VERSION + 1;
    package.v = future;
    let error = package.validate().unwrap_err();
    assert!(error.contains("unsupported"), "unexpected error: {error}");
    assert!(
        error.contains(&future.to_string()),
        "unexpected error: {error}"
    );
    assert!(
        error.contains(&format!(
            "{MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION}..={CODING_SESSION_CONTEXT_PACKAGE_VERSION}"
        )),
        "unexpected error: {error}"
    );
}

#[test]
fn the_first_turn_brief_carries_the_source_breakdown() {
    let reconciled = reconciled();
    let brief = coding_session_first_turn_brief(&reconciled);
    let encoded = serde_json::to_string(&brief).unwrap();
    validate_coding_session_first_turn_brief_json(&encoded).unwrap();

    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"],
        serde_json::to_value(
            reconciled
                .provenance
                .source_event_breakdown
                .clone()
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"]["genesisEvents"],
        1
    );
    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"]["transcriptEvents"],
        2
    );

    // The brief is injected standalone as the ACP bootstrap prompt, so the
    // number the six terms reconcile has to travel in the brief itself — not
    // only in the surrounding `session_overview` response.
    assert_eq!(
        brief["snapshot"]["sourceEventCount"],
        serde_json::json!(reconciled.provenance.source_event_count)
    );
    let terms = brief["snapshot"]["sourceEventBreakdown"]
        .as_object()
        .unwrap()
        .values()
        .map(|term| term.as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        terms,
        brief["snapshot"]["sourceEventCount"].as_u64().unwrap(),
        "the breakdown terms must sum to the count printed beside them"
    );

    let rules = brief["rules"].as_array().unwrap();
    assert!(
        rules
            .iter()
            .filter_map(Value::as_str)
            .any(|rule| rule
                .contains("sourceEventBreakdown reconciles it against totalHistoryItems")),
        "the brief must say how the two numbers reconcile"
    );

    // A package with no breakdown says so explicitly rather than omitting the
    // key from a rendering a reader scans for it.
    let legacy = coding_session_first_turn_brief(&package());
    assert_eq!(legacy["snapshot"]["sourceEventBreakdown"], Value::Null);
    validate_coding_session_first_turn_brief_json(&serde_json::to_string(&legacy).unwrap())
        .unwrap();
}

/// §2 item 43 — a Codex shell row rendered as
/// `Ran [elided private context: 10 bytes, sha256:…] -lc "sed -n …"`.
/// The ten elided bytes were `/bin/zsh`: public knowledge, redacted at the cost
/// of an unreadable row. codex-acp puts the whole command in the tool name
/// (§2 item 2), so the redactor was working on argv rather than prose.
#[test]
fn a_stock_interpreter_survives_but_the_host_layout_around_it_does_not() {
    let sanitized =
        sanitize_coding_session_context_text("/bin/zsh -lc \"sed -n '1,180p' docs/STATUS.md\"");
    assert_eq!(
        sanitized, "/bin/zsh -lc \"sed -n '1,180p' docs/STATUS.md\"",
        "a stock interpreter and a relative path are both readable and both public"
    );

    for public in ["/bin/sh", "/bin/bash", "/usr/bin/env", "/usr/bin/zsh"] {
        assert_eq!(
            sanitize_coding_session_context_text(public),
            public,
            "{public} is identical on every host"
        );
    }
}

#[test]
fn workspace_paths_become_relative_but_neighboring_host_paths_stay_private() {
    let root = std::path::Path::new("/Users/brian/Projects/beekeeper");
    let sanitized = sanitize_coding_session_context_text_for_workspace(
        "See `/Users/brian/Projects/beekeeper/desktop/src/App.tsx:42` and /Users/brian/Secrets/token.txt",
        root,
    );

    assert!(
        sanitized.contains("`desktop/src/App.tsx:42`"),
        "{sanitized}"
    );
    assert!(!sanitized.contains("/Users/brian/Projects/beekeeper"));
    assert!(!sanitized.contains("/Users/brian/Secrets"));
    assert!(sanitized.contains("[elided private context: "));
}

#[test]
fn workspace_prefix_lookalikes_are_not_relativized() {
    let sanitized = sanitize_coding_session_context_text_for_workspace(
        "/Users/brian/Projects/beekeeper-old/private.txt",
        std::path::Path::new("/Users/brian/Projects/beekeeper"),
    );
    assert!(sanitized.starts_with("[elided private context: "));
    assert!(!sanitized.contains("-old/private.txt"));
}

#[test]
fn the_exemption_is_exact_and_does_not_widen_the_hole() {
    for private in [
        "/Users/brian/Projects/beekeeper",
        "/bin/zsh/../../Users/brian",
        "/usr/local/bin/zsh",
        "/opt/homebrew/bin/bash",
        "/bin/zsh-custom",
        "~/bin/zsh",
    ] {
        let sanitized = sanitize_coding_session_context_text(private);
        assert!(
            sanitized.starts_with("[elided private context: "),
            "{private} is host layout and must stay redacted, got {sanitized}"
        );
    }
}

#[test]
fn an_exempt_interpreter_does_not_rescue_the_host_paths_beside_it() {
    let sanitized =
        sanitize_coding_session_context_text("/bin/zsh -lc \"cat /Users/brian/.ssh/id_ed25519\"");
    assert!(sanitized.starts_with("/bin/zsh -lc"), "{sanitized}");
    assert!(!sanitized.contains("/Users/brian"), "{sanitized}");
    assert!(
        sanitized.contains("[elided private context: "),
        "{sanitized}"
    );
}

/// Reported 2026-08-24: an operator asked a coding session "what was the
/// result?" and received `[elided private context: 1934 bytes, sha256:…]`. The
/// session was working on a git-ACL branch, so its answers contained words like
/// "credential" and "authorization" — and a substring match on those words
/// replaced the *entire* message.
#[test]
fn prose_about_credentials_survives_because_a_topic_is_not_a_secret() {
    for text in [
        "Ran git-credential-nostr and the push authenticated on the retry.",
        "The authorization check passed for every member of the project.",
        "I did not find a secret in the hook config; the ACL is what refused it.",
        "The private key never leaves the keychain, so the provider record holds only a pubkey.",
        "Set up the credentials helper, then re-ran the test.",
    ] {
        assert_eq!(
            sanitize_coding_session_context_text(text),
            text,
            "a sentence about credentials is not a credential"
        );
    }
}

#[test]
fn a_value_beside_a_credential_word_still_goes() {
    let sanitized =
        sanitize_coding_session_context_text("export GITHUB_TOKEN=ghp_0123456789abcdefghij");
    assert!(
        !sanitized.contains("ghp_0123456789abcdefghij"),
        "{sanitized}"
    );
    assert!(
        sanitized.contains("\u{2022}\u{2022}\u{2022}\u{2022}"),
        "{sanitized}"
    );
    assert!(sanitized.starts_with("export"), "{sanitized}");

    let assignment = sanitize_coding_session_context_text("password: hunter2");
    assert!(!assignment.contains("hunter2"), "{assignment}");
    assert!(assignment.starts_with("password:"), "{assignment}");

    let prose = sanitize_coding_session_context_text("the api key is abcd1234efgh");
    assert!(!prose.contains("abcd1234efgh"), "{prose}");
    assert!(prose.starts_with("the api key is"), "{prose}");
}

#[test]
fn every_shape_the_old_rule_caught_is_still_caught() {
    for secret in [
        "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq",
        "sk-abcdefghijklmnopqrstuvwxyz",
        "ghp_0123456789abcdefghijklmnop",
        "github_pat_11ABCDEFG0123456789",
        "xoxb-1234567890-abcdefghij",
        "AKIAIOSFODNN7EXAMPLE",
    ] {
        let sanitized = sanitize_coding_session_context_text(&format!("value {secret} end"));
        assert!(
            !sanitized.contains(secret),
            "{secret} survived: {sanitized}"
        );
        assert!(sanitized.starts_with("value "), "{sanitized}");
        assert!(sanitized.ends_with(" end"), "{sanitized}");
    }
}

#[test]
fn a_key_block_is_redacted_whole_and_takes_nothing_else_with_it() {
    let text = concat!(
        "Here is the config.\n",
        "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        "b3BlbnNzaC1rZXktdjEAAAAABG5vbmU\n",
        "-----END OPENSSH PRIVATE KEY-----\n",
        "That is the whole file.",
    );
    let sanitized = sanitize_coding_session_context_text(text);
    assert!(
        !sanitized.contains("b3BlbnNzaC1rZXktdjEAAAAABG5vbmU"),
        "{sanitized}"
    );
    assert!(sanitized.starts_with("Here is the config."), "{sanitized}");
    assert!(
        sanitized.ends_with("That is the whole file."),
        "{sanitized}"
    );
    assert!(
        sanitized.contains("[elided private context: "),
        "{sanitized}"
    );
}

/// The redaction is per line, so a value cannot swallow the paragraph that
/// explains it — the failure mode this whole change exists to undo.
#[test]
fn an_assignment_redacts_its_own_line_and_no_further() {
    let sanitized = sanitize_coding_session_context_text(
        "Checked the config.\ntoken: abcdef123456\nThe rest of the run was clean.",
    );
    assert!(!sanitized.contains("abcdef123456"), "{sanitized}");
    assert!(sanitized.starts_with("Checked the config."), "{sanitized}");
    assert!(
        sanitized.ends_with("The rest of the run was clean."),
        "{sanitized}"
    );
}

/// The space-separated form has no separator to key on, so the *shape* of the
/// following token decides. Getting this wrong in either direction is the
/// whole difficulty: too eager and prose loses a word, too shy and a password
/// ships to the channel.
#[test]
fn a_space_separated_value_goes_and_the_next_word_of_a_sentence_stays() {
    let leaked = sanitize_coding_session_context_text("with password hunter2 in the config");
    assert!(!leaked.contains("hunter2"), "{leaked}");
    assert!(leaked.starts_with("with password "), "{leaked}");
    assert!(leaked.ends_with(" in the config"), "{leaked}");

    for prose in [
        "the private key never leaves the keychain",
        "the secret in the hook config was fine",
        "credentials helper, then re-ran the test",
        "password protection stays on",
    ] {
        assert_eq!(
            sanitize_coding_session_context_text(prose),
            prose,
            "the next word of a sentence is not a value"
        );
    }
}

/// A repo binary named `git-credential-nostr` contains a credential word and
/// is not one. This is the exact string from the session that reported the bug.
#[test]
fn a_tool_named_after_credentials_is_not_redacted() {
    let text = "Ran git-credential-nostr; the push retried once and succeeded.";
    assert_eq!(sanitize_coding_session_context_text(text), text);
}

/// Live, 2026-08-24: an agent explaining `git-credential-nostr` had a clause
/// elided out of the middle of its answer — `` `get`/`store`/`erase`) `` —
/// because a `/` preceded by a backtick read as the start of a quoted absolute
/// path. Inline code spans are how an agent names commands, so this fired
/// exactly where the answer was most technical.
#[test]
fn inline_code_spans_are_not_host_paths() {
    for text in [
        "reading key-value pairs over stdin/stdout for `get`/`store`/`erase`)",
        "either `--force`/`-f` works",
        "the `a`/`b` split",
    ] {
        assert_eq!(
            sanitize_coding_session_context_text(text),
            text,
            "a slash between code spans names no path"
        );
    }
}

/// A web URL survives the host-path guard, including the markdown wrapper a
/// coding-session turn carries its screenshots in.
///
/// This guard exists to keep *this machine's layout* out of a published
/// transcript. A URL's path is addressing on a server. Eliding it turned every
/// attached screenshot into the literal text
/// `![[elided private context: 105 bytes, sha256:…])`, so the picture never
/// rendered and the prompt read as gibberish (observed live, 2026-09-01).
#[test]
fn a_web_url_is_not_a_host_path() {
    let sha = "9d04652c983d29738a5ef5cccbcda0f8826bb67443de11158c186cc816521082";
    for text in [
        format!("![image](http://localhost:3000/media/{sha}.png)"),
        format!("![image](https://hive.agiterra.org/media/{sha}.png)"),
        "see https://github.com/agiterra/beekeeper/blob/main/README.md".to_owned(),
        "http://localhost:3000/media/x.png".to_owned(),
    ] {
        assert_eq!(
            sanitize_coding_session_context_text(&text),
            text,
            "a web URL names a server, not this machine"
        );
    }
}

/// The exemption is scheme-scoped: `file://` is host layout wearing a scheme,
/// and a bare absolute path beside a URL is still redacted.
#[test]
fn the_web_url_exemption_does_not_rescue_real_host_paths() {
    let elided = sanitize_coding_session_context_text("file:///Users/andy/.ssh/id_ed25519");
    assert!(
        elided.contains("[elided private context: "),
        "a file:// URL is a host path: {elided}"
    );

    let mixed =
        sanitize_coding_session_context_text("fetched https://example.com/a from /Users/andy/keys");
    assert!(mixed.contains("https://example.com/a"), "{mixed}");
    assert!(
        mixed.contains("[elided private context: "),
        "the bare host path beside it still goes: {mixed}"
    );
}

// ── Recording redactions for the host's own operator ─────────────────────────
//
// The vault these feed exists so an operator can read back a path on their own
// machine. Everything here is about the *leak direction*: a secret redacted is
// a secret never recorded, and that must hold structurally rather than by every
// caller remembering to filter.

/// The recording twin must redact byte for byte identically. If these two ever
/// disagree, the published transcript and the host's own record have drifted on
/// what counts as private, which is the failure the shared implementation
/// exists to prevent.
#[test]
fn recording_redacts_exactly_what_the_pure_redactor_redacts() {
    for value in [
        serde_json::json!({ "text": "read /Users/andy/Code/thing.rs then ran it" }),
        serde_json::json!({ "token": "abc", "resumeCursor": "cursor-9" }),
        serde_json::json!({ "text": "export GITHUB_TOKEN=ghp_aaaaaaaaaaaaaaaaaaaa" }),
        serde_json::json!({ "text": "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n" }),
        serde_json::json!({ "text": "password hunter2 is in the config" }),
        serde_json::json!({ "text": "nothing private here at all" }),
    ] {
        let (recorded, _) = sanitize_coding_session_context_content_recording(&value);
        assert_eq!(
            recorded,
            sanitize_coding_session_context_content(&value),
            "{value}"
        );
    }
}

/// …and the quoted-path form it exists for still goes.
#[test]
fn a_quoted_absolute_path_is_still_a_host_path() {
    for text in [
        "CWD=\"/Users/brian/Projects/beekeeper\"",
        "look in (/Users/brian/secrets)",
        "the file at '/etc/shadow' is root-only",
    ] {
        let sanitized = sanitize_coding_session_context_text(text);
        assert!(
            sanitized.contains("[elided private context: "),
            "{text} → {sanitized}"
        );
        assert!(!sanitized.contains("/Users/brian"), "{sanitized}");
        assert!(!sanitized.contains("/etc/shadow"), "{sanitized}");
    }
}

/// Live, 2026-08-24 (second pass): "…signing a challenge with the user's Nostr
/// private key (secp256k1) and having the remote verify…" lost its curve name.
/// The bare-space form has no separator to key on, so it leans on the shape of
/// the next token — and a parenthetical full of digits looks exactly like an
/// opaque value while being the commonest way prose qualifies these nouns.
#[test]
fn a_parenthetical_after_a_credential_word_is_prose() {
    for text in [
        "signing with the user's Nostr private key (secp256k1) and verifying it",
        "the token (JWT) is minted per request",
        "an api key [redacted by the vendor] arrived",
    ] {
        assert_eq!(
            sanitize_coding_session_context_text(text),
            text,
            "an aside is not a value"
        );
    }

    // The separator forms are unaffected: those carry their own evidence.
    let assigned = sanitize_coding_session_context_text("token=(hunter2)");
    assert!(!assigned.contains("hunter2"), "{assigned}");
}

/// What a redaction may say about what it hid.
///
/// The old marker published `sha256:` of the value and its byte count. For a
/// host path that is mostly harmless; for `password: hunter2` it is a
/// dictionary attack and a length hint, signed into a channel. A credential is
/// therefore masked at a fixed width with no digest — and the only thing kept
/// is the *format tag*, which is public documentation and is what tells a
/// person which credential to go and rotate.
#[test]
fn a_masked_credential_states_its_kind_and_nothing_else() {
    let long = sanitize_coding_session_context_text("token=ghp_0123456789abcdefghijklmnopqrs");
    assert!(long.contains("ghp_"), "{long}");
    assert!(long.ends_with("pqrs"), "the last four identify it: {long}");
    assert!(!long.contains("sha256"), "no crackable digest: {long}");
    assert!(!long.contains("bytes"), "no length hint: {long}");
    assert!(!long.contains("0123456789"), "{long}");

    // Shape says nothing, so nothing is revealed — this may be a password, and
    // four characters of a dictionary word is most of the answer.
    let unknown = sanitize_coding_session_context_text("password: hunter2");
    assert!(
        unknown.ends_with("\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"),
        "{unknown}"
    );
    assert!(!unknown.contains("er2"), "{unknown}");

    // A short shaped token gets its tag but no tail: the tail would be too
    // much of it.
    let short = sanitize_coding_session_context_text("token=sk-abcdefghijkl");
    assert!(short.contains("sk-"), "{short}");
    assert!(!short.contains("ijkl"), "{short}");

    // Two secrets of different lengths mask identically: no length leaks.
    assert_eq!(
        sanitize_coding_session_context_text("password: a1b2c3d4"),
        sanitize_coding_session_context_text("password: a1b2c3d4e5f6g7h8"),
    );
}

/// A mask is never masked again: projecting a package twice must not grow it.
#[test]
fn redaction_is_idempotent() {
    let once = sanitize_coding_session_context_text("password: hunter2");
    assert_eq!(sanitize_coding_session_context_text(&once), once);

    let token = sanitize_coding_session_context_text("token=ghp_0123456789abcdefghijklmnopqrs");
    assert_eq!(sanitize_coding_session_context_text(&token), token);
}

#[test]
fn a_host_path_is_recorded_with_the_digest_its_marker_carries() {
    let value = serde_json::json!({ "text": "read /Users/andy/Code/thing.rs then ran it" });
    let (sanitized, log) = sanitize_coding_session_context_content_recording(&value);

    assert_eq!(log.len(), 1, "{log:?}");
    let entry = &log[0];
    assert_eq!(entry.class, RedactionClass::HostPath);
    assert_eq!(entry.plaintext, "/Users/andy/Code/thing.rs");
    // The join is the digest and nothing else: no new wire field, so a reader
    // on this machine can match a published marker to its own record.
    let rendered = sanitized["text"].as_str().unwrap();
    assert!(
        rendered.contains(&format!("sha256:{}]", entry.digest)),
        "{rendered}"
    );
    assert!(
        rendered.contains(&format!("{} bytes", entry.bytes)),
        "{rendered}"
    );
}

/// The whole gate, stated as a test: every class that names a credential is
/// redacted and **never** written down.
#[test]
fn no_secret_class_is_ever_recorded() {
    let cases = [
        // shaped secret
        "my key is ghp_aaaaaaaaaaaaaaaaaaaaaaaa here",
        "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq and more",
        // credential assignment
        "GITHUB_TOKEN=ghp_bbbbbbbbbbbbbbbbbbbbbbbb",
        "password hunter2 is in the config",
        "api key is sk-cccccccccccccccccccc",
        // key block
        "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----\n",
    ];
    for text in cases {
        let value = serde_json::json!({ "text": text });
        let (sanitized, log) = sanitize_coding_session_context_content_recording(&value);
        assert!(
            sanitized != value,
            "the case must actually be redacted: {text}"
        );
        assert!(log.is_empty(), "recorded a secret for {text}: {log:?}");
    }

    // Same for a value caught by its key rather than its shape.
    for key in [
        "privateKey",
        "password",
        "authorization",
        "cookie",
        "token",
        "apiKey",
        "accessToken",
        "buzzAuthTag",
    ] {
        let value = serde_json::json!({ key: "s3cr3t-value-goes-here" });
        let (_, log) = sanitize_coding_session_context_content_recording(&value);
        assert!(log.is_empty(), "recorded a secret under {key}: {log:?}");
    }
}

/// Opaque provider bookkeeping is private, not secret — and it is exactly what
/// an operator needs when a session stalls.
#[test]
fn structural_bookkeeping_is_recoverable_but_still_redacted_on_the_wire() {
    let value = serde_json::json!({ "resumeCursor": "cursor-9", "acpSessionId": "acp-7" });
    let (sanitized, log) = sanitize_coding_session_context_content_recording(&value);

    assert!(sanitized["resumeCursor"]
        .as_str()
        .unwrap()
        .starts_with("[elided private context: "));
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log
        .iter()
        .all(|entry| entry.class == RedactionClass::Structural));
    let plaintexts: Vec<&str> = log.iter().map(|entry| entry.plaintext.as_str()).collect();
    assert!(plaintexts.contains(&"cursor-9"), "{plaintexts:?}");
    assert!(plaintexts.contains(&"acp-7"), "{plaintexts:?}");
}

#[test]
fn only_host_paths_and_structural_bookkeeping_are_recoverable() {
    for class in [RedactionClass::HostPath, RedactionClass::Structural] {
        assert!(class.is_recoverable(), "{class:?}");
    }
    for class in [
        RedactionClass::SecretKey,
        RedactionClass::KeyBlock,
        RedactionClass::ShapedSecret,
        RedactionClass::CredentialAssignment,
    ] {
        assert!(!class.is_recoverable(), "{class:?}");
    }
}

/// A path beside a secret must not rescue the secret, and the secret must not
/// suppress the path: two rules, two independent outcomes, one line.
#[test]
fn a_path_and_a_secret_on_one_line_are_classified_separately() {
    let value = serde_json::json!({
        "text": "wrote /Users/andy/.netrc with token=ghp_dddddddddddddddddddd today"
    });
    let (_, log) = sanitize_coding_session_context_content_recording(&value);

    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].class, RedactionClass::HostPath);
    assert_eq!(log[0].plaintext, "/Users/andy/.netrc");
}

/// Redaction is idempotent, so re-sanitizing a published item must not invent a
/// second record of a value whose plaintext is already gone.
#[test]
fn re_redacting_an_already_redacted_item_records_nothing() {
    let value = serde_json::json!({ "text": "read /Users/andy/Code/thing.rs" });
    let (once, first) = sanitize_coding_session_context_content_recording(&value);
    assert_eq!(first.len(), 1);

    let (twice, second) = sanitize_coding_session_context_content_recording(&once);
    assert_eq!(twice, once);
    assert!(second.is_empty(), "{second:?}");
}

// ── What the guard mistook for host layout (found live 2026-08-26) ───────────
//
// Andy ran a real session and the vault filled with things that are not paths:
// a bare `/`, a sentence's full stop, and a `sed` address. Each cost a
// perfectly readable word and taught the reader nothing.

/// A separator with nothing under it describes no machine.
#[test]
fn a_bare_separator_is_not_a_host_path() {
    for text in [
        "split the arguments on / and re-ran it",
        "the ratio was 3 / 4 across both runs",
    ] {
        assert_eq!(sanitize_coding_session_context_text(text), text, "{text}");
    }
}

/// `/^worktree` is a `sed` address. It was recorded as a host path, and so was
/// the `/{` of a brace expansion.
#[test]
fn a_regex_or_glob_anchor_is_not_a_host_path() {
    for text in [
        "ran sed -n '/^worktree/p' over the list",
        "the pattern /$/ ends every line",
        "expanded /{a,b} into two arguments",
    ] {
        assert_eq!(sanitize_coding_session_context_text(text), text, "{text}");
    }
}

/// Every bracket the shell puts around a path belongs to the sentence, not to
/// the filename. A backtick-quoted `~/…` was recorded with its backtick.
#[test]
fn a_backtick_or_paren_around_a_path_survives_the_redaction() {
    let sanitized = sanitize_coding_session_context_text("see (`~/Library/Logs`) for more");
    assert!(sanitized.starts_with("see (`["), "{sanitized}");
    assert!(sanitized.contains("`) for more"), "{sanitized}");
    assert!(!sanitized.contains("Library"), "{sanitized}");
}

/// The path goes; the sentence's punctuation stays where the author put it.
#[test]
fn redaction_takes_the_path_and_leaves_the_punctuation_around_it() {
    let sanitized = sanitize_coding_session_context_text("it lives under /Users/andy/Code.");
    assert!(sanitized.starts_with("it lives under ["), "{sanitized}");
    assert!(
        sanitized.ends_with("].") && !sanitized.contains("Code"),
        "the full stop belongs to the sentence, not the filename: {sanitized}"
    );

    let quoted = sanitize_coding_session_context_text("opened \"/Users/andy/notes\" twice");
    assert!(quoted.starts_with("opened \"["), "{quoted}");
    assert!(quoted.contains("\" twice"), "{quoted}");
    assert!(!quoted.contains("notes"), "{quoted}");
}

/// The value side of an assignment still carries the path, and the key side
/// still survives — the span must not swallow the variable name.
#[test]
fn an_assignment_keeps_its_key_and_redacts_only_the_value() {
    let sanitized = sanitize_coding_session_context_text("set CARGO_HOME=/Users/andy/.cargo now");
    assert!(sanitized.starts_with("set CARGO_HOME=["), "{sanitized}");
    assert!(!sanitized.contains(".cargo"), "{sanitized}");
}

/// The narrowing must not reopen the hole it was narrowed around: every shape
/// that was redacted before still is, and the recorded plaintext is now the
/// path alone rather than the path plus whatever punctuation touched it.
#[test]
fn real_paths_are_still_redacted_and_recorded_without_their_punctuation() {
    for text in [
        "read /Users/andy/Code/thing.rs",
        "read ~/Code/thing.rs",
        "read /Users/andy/.config/git/credentials",
        "read C:\\Users\\andy\\thing.rs",
        "read file:///Users/andy/thing.rs",
    ] {
        let sanitized = sanitize_coding_session_context_text(text);
        assert!(
            sanitized.contains("[elided private context: "),
            "no longer redacted: {text}"
        );
    }

    let (_, log) = sanitize_coding_session_context_content_recording(&serde_json::json!({
        "text": "wrote \"/Users/andy/Code/thing.rs\", then stopped."
    }));
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0].plaintext, "/Users/andy/Code/thing.rs");
}

/// The stock-interpreter exemption still applies once punctuation is stripped,
/// which it did not have to survive before the span rewrite.
#[test]
fn a_quoted_stock_interpreter_is_still_exempt() {
    for text in [
        "ran \"/bin/zsh\" -lc true",
        "ran /usr/bin/env node.",
        "ran '/bin/bash';",
    ] {
        assert_eq!(sanitize_coding_session_context_text(text), text, "{text}");
    }
}

fn roster_entry() -> CodingSessionContextRosterEntry {
    CodingSessionContextRosterEntry {
        target: target(),
        actor: Some("ab".repeat(32)),
        role: Some("builder".into()),
        status: CodingSessionContextSeatStatus::Active,
        last_signed_seq: Some(2),
        last_signed_at_ms: Some(2_000),
    }
}

fn inbox_item(seq: u64) -> CodingSessionContextInboxItem {
    CodingSessionContextInboxItem {
        event_id: format!("{seq:064x}"),
        created_at: 100 + seq,
        command_id: format!("turn-{seq}"),
        sender: "cd".repeat(32),
        sender_role: Some("lead".into()),
        target: target(),
        delivery: "boundary".into(),
        content: format!("do thing {seq}"),
        stage: Some(ReceiptStatus::TurnQueued),
        stage_at: Some(200 + seq),
        stage_code: None,
    }
}

fn crew_package() -> CodingSessionContextPackage {
    let mut package = package();
    package.roster = vec![roster_entry()];
    package.inbox = vec![inbox_item(1), inbox_item(2)];
    package
}

/// The crew fields are additive: a package written before they existed still
/// validates, and a package that has nothing to say about a crew is
/// byte-identical to one.
#[test]
fn a_package_without_crew_fields_still_validates_and_stays_off_the_wire() {
    let mut legacy = package();
    legacy.v = MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION;
    legacy.validate().unwrap();
    let encoded = serde_json::to_string(&legacy).unwrap();
    assert!(!encoded.contains("roster"), "{encoded}");
    assert!(!encoded.contains("inbox"), "{encoded}");

    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert!(decoded.roster.is_empty());
    assert!(decoded.inbox.is_empty());
    decoded.validate().unwrap();

    // And the current version accepts the same absent-keys shape.
    let mut raw = serde_json::to_value(package()).unwrap();
    raw["v"] = Value::from(CODING_SESSION_CONTEXT_PACKAGE_VERSION);
    let current: CodingSessionContextPackage = serde_json::from_value(raw).unwrap();
    current.validate().unwrap();
}

/// A breakdown written before `inboxEvents` existed still sums exactly as it
/// did, because the missing term defaults to zero.
#[test]
fn a_breakdown_without_the_inbox_term_still_reconciles() {
    let mut raw = serde_json::to_value(reconciled()).unwrap();
    let breakdown = raw["provenance"]["sourceEventBreakdown"]
        .as_object_mut()
        .unwrap();
    assert!(breakdown.remove("inboxEvents").is_some());
    let decoded: CodingSessionContextPackage = serde_json::from_value(raw).unwrap();
    assert_eq!(
        decoded
            .provenance
            .source_event_breakdown
            .as_ref()
            .unwrap()
            .inbox_events,
        0
    );
    decoded.validate().unwrap();
}

#[test]
fn a_valid_crew_package_round_trips_and_names_its_seats() {
    let package = crew_package();
    package.validate().unwrap();
    let encoded = serde_json::to_string(&package).unwrap();
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, package);
    decoded.validate().unwrap();
    for key in ["roster", "inbox", "lastSignedSeq", "senderRole", "stage"] {
        assert!(encoded.contains(key), "{key} must be on the wire");
    }
}

/// Every roster invariant that would let a package lie about a seat.
#[test]
fn the_roster_rejects_a_seat_it_cannot_honestly_describe() {
    let mut repeated = crew_package();
    repeated.roster.push(roster_entry());
    assert!(repeated.validate().is_err(), "a seat appears once");

    let mut roleless = crew_package();
    roleless.roster[0].actor = None;
    assert!(
        roleless.validate().is_err(),
        "a role with no actor names a position nobody holds"
    );

    let mut half_aged = crew_package();
    half_aged.roster[0].last_signed_at_ms = None;
    assert!(
        half_aged.validate().is_err(),
        "a sequence with no timestamp cannot be aged"
    );

    let mut zero = crew_package();
    zero.roster[0].last_signed_seq = Some(0);
    assert!(zero.validate().is_err());

    let mut shouting = crew_package();
    shouting.roster[0].role = Some("Lead".into());
    assert!(shouting.validate().is_err(), "role slugs are lowercase");
}

/// Every inbox invariant a reader's paging or honesty depends on.
#[test]
fn the_inbox_rejects_items_that_would_break_paging_or_lie_about_a_stage() {
    let mut out_of_order = crew_package();
    out_of_order.inbox.swap(0, 1);
    assert!(
        out_of_order.validate().is_err(),
        "a page cursor over an unordered inbox skips commands"
    );

    let mut repeated = crew_package();
    repeated.inbox[1].event_id = repeated.inbox[0].event_id.clone();
    assert!(repeated.validate().is_err());

    let mut unknown_class = crew_package();
    unknown_class.inbox[0].delivery = "whenever".into();
    assert!(unknown_class.validate().is_err());

    let mut lifecycle_stage = crew_package();
    lifecycle_stage.inbox[0].stage = Some(ReceiptStatus::Created);
    assert!(
        lifecycle_stage.validate().is_err(),
        "a create outcome is not a turn stage"
    );

    let mut orphan_detail = crew_package();
    orphan_detail.inbox[0].stage = None;
    orphan_detail.inbox[0].stage_at = None;
    assert!(
        orphan_detail.validate().is_ok(),
        "no stage and no detail is the honest unanswered case"
    );
    orphan_detail.inbox[0].stage_code = Some("NO_LIVE_EXECUTION".into());
    assert!(
        orphan_detail.validate().is_err(),
        "a stage code with no stage claims an answer that was not seen"
    );

    let mut leaked = crew_package();
    leaked.inbox[0].content = "read /Users/alice/private/repo".into();
    assert!(leaked.validate().is_err());

    let mut empty = crew_package();
    empty.inbox[0].content = "   ".into();
    assert!(empty.validate().is_err());
}

/// A clip is only worth shipping if the package validator accepts it, so the
/// clipped text has to be sanitizer-stable and inside the bound.
#[test]
fn clipping_redacted_text_yields_something_the_validator_accepts() {
    let brief = (0..140)
        .map(|index| format!("/Users/brian/Projects/beekeeper/crates/file{index}.rs"))
        .collect::<Vec<_>>()
        .join(" ");
    let redacted = sanitize_coding_session_context_text(&brief);
    assert!(
        redacted.len() > MAX_CONTEXT_INBOX_CONTENT_BYTES,
        "redaction grows this brief past the bound: {}",
        redacted.len()
    );

    let clipped = clip_coding_session_context_text(&redacted, MAX_CONTEXT_INBOX_CONTENT_BYTES)
        .expect("a brief of whole words can always be clipped");
    assert!(clipped.len() <= MAX_CONTEXT_INBOX_CONTENT_BYTES);
    assert!(clipped.ends_with(CODING_SESSION_CONTEXT_CLIP_MARKER));
    assert_eq!(
        sanitize_coding_session_context_text(&clipped),
        clipped,
        "a clip must never cut a redaction into something the sanitizer rewrites"
    );

    let mut package = crew_package();
    package.inbox[0].content = clipped;
    package.validate().expect("a clipped item still validates");
}

/// Text that already fits is returned byte-for-byte, and text with no word
/// boundary to cut on is refused rather than butchered.
#[test]
fn clipping_leaves_fitting_text_alone_and_refuses_what_it_cannot_cut() {
    assert_eq!(
        clip_coding_session_context_text("short enough", MAX_CONTEXT_INBOX_CONTENT_BYTES)
            .as_deref(),
        Some("short enough")
    );
    let one_word = "x".repeat(MAX_CONTEXT_INBOX_CONTENT_BYTES + 1);
    assert_eq!(
        clip_coding_session_context_text(&one_word, MAX_CONTEXT_INBOX_CONTENT_BYTES),
        None,
        "one unbroken word longer than the bound has no honest clip"
    );
    assert_eq!(
        clip_coding_session_context_text("a b c", 4),
        None,
        "a bound the marker cannot fit in yields no clip at all"
    );
}
