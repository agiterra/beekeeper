use super::*;

#[test]
fn clean_keeps_a_plain_title() {
    assert_eq!(
        clean_generated_name("Fix the push timeout").as_deref(),
        Some("Fix the push timeout")
    );
}

#[test]
fn clean_strips_quotes_and_terminal_punctuation() {
    assert_eq!(
        clean_generated_name("\"Fix the push timeout.\"").as_deref(),
        Some("Fix the push timeout")
    );
    assert_eq!(
        clean_generated_name("**Rebase the branch**").as_deref(),
        Some("Rebase the branch")
    );
}

#[test]
fn clean_strips_a_label_the_model_added_itself() {
    assert_eq!(
        clean_generated_name("Title: Rename the relay").as_deref(),
        Some("Rename the relay")
    );
}

#[test]
fn clean_takes_only_the_first_nonempty_line() {
    assert_eq!(
        clean_generated_name("\n\nAudit the ACL\n\nThis covers the fork tables.").as_deref(),
        Some("Audit the ACL")
    );
}

#[test]
fn clean_rejects_an_answer_with_no_name_in_it() {
    assert_eq!(clean_generated_name(""), None);
    assert_eq!(clean_generated_name("   \n  "), None);
    assert_eq!(clean_generated_name("\"\""), None);
}

#[test]
fn clean_caps_a_runaway_answer() {
    let long = "word ".repeat(60);
    let cleaned = clean_generated_name(&long).expect("name");
    assert!(cleaned.chars().count() <= MAX_GENERATED_NAME_CHARS);
    assert!(!cleaned.ends_with(' '));
}

#[test]
fn base_url_must_be_an_http_origin() {
    assert!(validate_base_url("http://127.0.0.1:11434/v1").is_ok());
    assert!(validate_base_url("https://api.openai.com/v1").is_ok());
    assert!(validate_base_url("").is_err());
    assert!(validate_base_url("127.0.0.1:11434").is_err());
    assert!(validate_base_url("file:///etc/passwd").is_err());
    assert!(validate_base_url("ftp://example.com").is_err());
}

#[test]
fn the_default_provider_sends_nothing_anywhere() {
    let record = CodingSessionNamingRecord::default();
    assert_eq!(record.provider, CodingSessionNamingProvider::Off);
    assert!(record.api_key.is_empty());
    assert!(record.base_url.is_empty());
}

#[test]
fn settings_never_carry_the_key_itself() {
    let record = CodingSessionNamingRecord {
        provider: CodingSessionNamingProvider::Anthropic,
        base_url: String::new(),
        model: "claude-opus-5".to_string(),
        api_key: "sk-ant-secret".to_string(),
    };
    // No store: the keychain is an interactive dependency, and this property
    // — the settings shape never carries the key — is about the
    // record-to-settings mapping, not about where the key is kept. Reaching
    // the real keychain here blocks on an access prompt nobody is there to
    // answer, and the whole test binary hangs.
    let serialized = serde_json::to_string(&settings_from_in(&record, None)).expect("serialize");
    assert!(!serialized.contains("sk-ant-secret"), "{serialized}");
    assert!(serialized.contains("\"hasApiKey\":true"), "{serialized}");
}

/// Serve one `/chat/completions` answer, and hand back the base URL plus the
/// request body the adapter actually sent.
///
/// A local Ollama is exactly this shape, so this covers the whole
/// OpenAI-compatible path: URL construction, the absent `Authorization`
/// header a keyless local model needs, response parsing, and the cleanup that
/// turns a chatty answer into a title.
async fn stub_openai_server(
    reply: &'static str,
) -> (String, std::sync::Arc<std::sync::Mutex<Option<String>>>) {
    use axum::{routing::post, Router};

    let seen = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
    let recorder = seen.clone();
    let router = Router::new().route(
        "/v1/chat/completions",
        post(move |headers: axum::http::HeaderMap, body: String| {
            let recorder = recorder.clone();
            async move {
                let authorization = headers
                    .get("authorization")
                    .map(|value| value.to_str().unwrap_or("<binary>").to_string())
                    .unwrap_or_else(|| "<absent>".to_string());
                *recorder.lock().expect("record") = Some(format!("{authorization}\n{body}"));
                axum::Json(serde_json::json!({
                    "choices": [{ "message": { "role": "assistant", "content": reply } }]
                }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

#[tokio::test]
async fn an_openai_compatible_endpoint_names_a_session() {
    let (base_url, seen) = stub_openai_server("\"Fix the push timeout.\"\n").await;
    let client = reqwest::Client::new();

    let name = name_via_openai_compatible(
        &client,
        NamingTask::Name,
        &base_url,
        "llama3.2",
        None,
        "The relay rejects a push that takes longer than a minute.",
    )
    .await
    .expect("name");

    // The model's quotes and trailing period are gone by the time this
    // reaches a name field.
    assert_eq!(name, "Fix the push timeout");

    let request = seen.lock().expect("seen").clone().expect("a request");
    // A keyless local model must not be sent an empty bearer token — that is
    // how a server that would have answered returns 401 instead.
    assert!(request.starts_with("<absent>\n"), "{request}");
    // Only the first message travels. Nothing about this machine does.
    assert!(request.contains("longer than a minute"), "{request}");
    assert!(request.contains("\"model\":\"llama3.2\""), "{request}");
}

#[tokio::test]
async fn a_keyed_endpoint_gets_a_bearer_token() {
    let (base_url, seen) = stub_openai_server("Rename the relay").await;
    let client = reqwest::Client::new();

    let name = name_via_openai_compatible(
        &client,
        NamingTask::Name,
        &base_url,
        "gpt-x",
        Some("sk-test-key"),
        "Rename every mention of the old relay host.",
    )
    .await
    .expect("name");
    assert_eq!(name, "Rename the relay");

    let request = seen.lock().expect("seen").clone().expect("a request");
    assert!(request.starts_with("Bearer sk-test-key\n"), "{request}");
}

#[tokio::test]
async fn an_endpoint_that_answers_with_no_name_is_an_error_not_an_empty_title() {
    let (base_url, _) = stub_openai_server("   ").await;
    let client = reqwest::Client::new();

    let failure =
        name_via_openai_compatible(&client, NamingTask::Name, &base_url, "m", None, "something")
            .await
            .expect_err("no name");
    assert!(failure.contains("no name"), "{failure}");
}

#[tokio::test]
async fn an_unreachable_endpoint_says_where_it_could_not_reach() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .expect("client");
    // Port 1 on loopback: nothing listens, and the refusal is immediate.
    let failure = name_via_openai_compatible(
        &client,
        NamingTask::Name,
        "http://127.0.0.1:1/v1",
        "m",
        None,
        "something",
    )
    .await
    .expect_err("unreachable");
    assert!(failure.contains("127.0.0.1:1"), "{failure}");
}

#[tokio::test]
async fn a_test_run_sends_the_fixed_sample_and_nothing_a_person_wrote() {
    let (base_url, seen) = stub_openai_server("Fix the push timeout").await;

    let name = name_with(
        CodingSessionNamingProvider::OpenAiCompatible,
        &base_url,
        "llama3.2",
        None,
        NAMING_TEST_MESSAGE,
    )
    .await
    .expect("name");
    assert_eq!(name, "Fix the push timeout");

    let request = seen.lock().expect("seen").clone().expect("a request");
    // The sample is what travels. This is the promise the settings card makes
    // about pressing Test, and it is worth a test of its own.
    assert!(request.contains("credential helper"), "{request}");
}

#[tokio::test]
async fn an_unconfigured_namer_is_refused_before_any_request_is_built() {
    for (provider, model) in [
        (CodingSessionNamingProvider::Off, "llama3.2"),
        // A provider with no model named is not configured either — reaching
        // an endpoint with an empty model would be a 400 dressed up as a
        // network problem.
        (CodingSessionNamingProvider::OpenAiCompatible, ""),
        (CodingSessionNamingProvider::Anthropic, ""),
    ] {
        let failure = name_with(provider, "http://127.0.0.1:1/v1", model, None, "anything")
            .await
            .expect_err("refused");
        assert_eq!(failure, "no naming model is configured");
    }
}

#[tokio::test]
async fn anthropic_without_a_key_says_so_rather_than_failing_at_the_wire() {
    let failure = name_with(
        CodingSessionNamingProvider::Anthropic,
        "",
        "claude-opus-5",
        None,
        NAMING_TEST_MESSAGE,
    )
    .await
    .expect_err("refused");
    assert!(failure.contains("needs an API key"), "{failure}");
}

#[test]
fn the_provider_names_survive_a_round_trip() {
    for (provider, wire) in [
        (CodingSessionNamingProvider::Off, "\"off\""),
        (CodingSessionNamingProvider::Anthropic, "\"anthropic\""),
        (
            CodingSessionNamingProvider::OpenAiCompatible,
            "\"openai-compatible\"",
        ),
    ] {
        let serialized = serde_json::to_string(&provider).expect("serialize");
        assert_eq!(serialized, wire);
        let parsed: CodingSessionNamingProvider =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(parsed, provider);
    }
}

#[test]
fn a_generated_goal_keeps_its_sentence_and_loses_its_wrapping() {
    assert_eq!(
        clean_generated_goal("Goal: \"Move the create dialog onto the founded page.\"\nMore."),
        Some("Move the create dialog onto the founded page.".to_string())
    );
    assert_eq!(clean_generated_goal("  \n  "), None);
    let long = "x".repeat(400);
    assert_eq!(
        clean_generated_goal(&long).map(|goal| goal.chars().count()),
        Some(200)
    );
}
