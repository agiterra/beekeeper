//! Driver ops: what an agent does to a preview page, and how the results are
//! bounded on the way back.
//!
//! Each op is a JSON object handed to `driver/driver.js` in the isolated
//! `beekeeper-preview-driver` world through `callAsyncJavaScript` (awaited
//! promises, throws as errors; plain `evaluateJavaScript` returns `""` for
//! both). The driver is installed into that world at document start of
//! every page; a page loaded before it was installed gets it on first use.
//!
//! All input is synthetic. Results say so (`input: "synthetic"`), and carry
//! the URL and navigation generation they were taken at, so an agent never
//! mistakes one page's refs for another's.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tauri::AppHandle;

use super::policy::{cap_text, EVAL_RESULT_CAP_BYTES};
use super::{view, with_record, PreviewError};

/// Playwright's injected script, vendored byte for byte (see VENDOR.md).
const PLAYWRIGHT_INJECTED: &str =
    include_str!("../../../vendor/playwright-injected/injectedScriptSource.js");
/// The driver itself.
const DRIVER_JS: &str = include_str!("driver/driver.js");

/// Largest aria snapshot text returned (WIRE-C4 §2: 64 KiB, truncated at a
/// line boundary).
pub const ARIA_CAP_BYTES: usize = 64 * 1024;
/// Default budget for one driver op.
pub const OP_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest a `wait_for` may wait.
pub const WAIT_FOR_MAX: Duration = Duration::from_secs(30);
/// Default `wait_for` wait.
pub const WAIT_FOR_DEFAULT: Duration = Duration::from_secs(5);
/// How often `wait_for` re-checks.
pub const WAIT_FOR_POLL: Duration = Duration::from_millis(100);

/// The options Playwright's own host passes `InjectedScript` (minus test
/// hooks): WebKit, JavaScript-flavoured locators, `data-testid`.
const INJECTED_OPTIONS: &str = r#"{"isUnderTest":false,"sdkLanguage":"javascript","frameSeq":0,"testIdAttributeName":"data-testid","stableRafCount":1,"browserName":"webkit","shouldPrependErrorPrefix":false,"isUtilityWorld":true,"customEngines":[]}"#;

/// The script installed in the driver world: Playwright's injected script,
/// evaluated the way Playwright evaluates it, then the driver around it.
/// Idempotent, so installing twice is harmless.
pub fn driver_source() -> &'static str {
    static SOURCE: OnceLock<String> = OnceLock::new();
    SOURCE.get_or_init(|| {
        format!(
            "(() => {{\nif (globalThis.__beekeeperPreviewDriver) return;\n\
             const createInjected = () => {{\nconst module = {{}};\n{PLAYWRIGHT_INJECTED}\n\
             return new (module.exports.InjectedScript())(globalThis, {INJECTED_OPTIONS});\n}};\n\
             {DRIVER_JS}\n\
             globalThis.__beekeeperPreviewDriver = globalThis.__beekeeperPreviewDriverFactory(createInjected);\n\
             delete globalThis.__beekeeperPreviewDriverFactory;\n}})();\n"
        )
    })
}

/// Sentinel the call body returns when the driver is not installed yet.
const DRIVER_MISSING: &str = "__beekeeper_driver_missing__";

/// The body of every driver call: run the op and return its JSON.
const RUN_BODY: &str = "const driver = globalThis.__beekeeperPreviewDriver;\n\
     if (!driver) return \"__beekeeper_driver_missing__\";\n\
     return JSON.stringify(await driver.run(JSON.parse(payload)));";

/// Run one op object through the driver. Returns the driver's result object
/// (`ok:true`) or the refusal it reported, as an error.
pub async fn run_op(
    app: &AppHandle,
    channel_id: &str,
    mut op: Map<String, Value>,
    timeout: Duration,
) -> Result<Map<String, Value>, PreviewError> {
    let generation = with_record(channel_id, |record| record.generation);
    op.insert("generation".into(), json!(generation));
    let payload = Value::Object(op).to_string();
    let mut raw = call(app, channel_id, RUN_BODY, &payload, timeout).await?;
    if raw.as_deref() == Some(DRIVER_MISSING) {
        let install = format!("{}\nreturn \"installed\";", driver_source());
        call(app, channel_id, &install, "", timeout).await?;
        raw = call(app, channel_id, RUN_BODY, &payload, timeout).await?;
    }
    parse_driver_result(raw.as_deref())
}

async fn call(
    app: &AppHandle,
    channel_id: &str,
    body: &str,
    payload: &str,
    timeout: Duration,
) -> Result<Option<String>, PreviewError> {
    view::call_js(
        app,
        channel_id,
        body.to_string(),
        payload.to_string(),
        false,
        timeout,
    )
    .await
    .map_err(|error| {
        // A throw inside the driver is a driver bug or a page that broke the
        // DOM APIs it relies on; either way it is not the agent's script.
        if error.code == "preview_eval_error" {
            PreviewError::new("preview_unavailable", error.message)
        } else {
            error
        }
    })
}

/// Decode a driver reply: `{ok:true,…}` → the object, `{ok:false,code,
/// message}` → that refusal.
pub fn parse_driver_result(raw: Option<&str>) -> Result<Map<String, Value>, PreviewError> {
    let raw = raw
        .ok_or_else(|| PreviewError::new("preview_unavailable", "The driver returned nothing."))?;
    let value: Value = serde_json::from_str(raw).map_err(|e| {
        PreviewError::new(
            "preview_unavailable",
            format!("The driver returned invalid JSON: {e}"),
        )
    })?;
    let Value::Object(mut object) = value else {
        return Err(PreviewError::new(
            "preview_unavailable",
            "The driver returned a non-object.",
        ));
    };
    if object.get("ok").and_then(Value::as_bool) == Some(true) {
        object.remove("ok");
        return Ok(object);
    }
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("preview_unavailable");
    let message = object
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("The driver refused the op.");
    Err(PreviewError::new(code, message))
}

/// Cap aria text at a line boundary. Returns the text and whether it was cut.
pub fn cap_aria(text: &str) -> (String, bool) {
    if text.len() <= ARIA_CAP_BYTES {
        return (text.to_string(), false);
    }
    let (cut, _) = cap_text(text, ARIA_CAP_BYTES);
    match cut.rfind('\n') {
        Some(end) => (cut[..end + 1].to_string(), true),
        None => (cut, true),
    }
}

/// The function body for `eval`: the agent's expression, awaited, its value
/// JSON-serialized (a value JSON cannot hold becomes its string form). Two
/// shapes: an expression, and, when that is a syntax error, a function body
/// whose `return` gives the value. No `eval()`, so a page CSP without
/// `'unsafe-eval'` does not get in the way.
pub fn eval_body(expression: &str, as_statements: bool) -> String {
    let inner = if as_statements {
        format!("(async () => {{\n{expression}\n}})()")
    } else {
        format!("(async () => (\n{expression}\n))()")
    };
    format!(
        "const __bkValue = await {inner};\n\
         let __bkJson;\n\
         try {{ __bkJson = JSON.stringify(__bkValue === undefined ? null : __bkValue); }}\n\
         catch (error) {{ __bkJson = JSON.stringify(String(__bkValue)); }}\n\
         return __bkJson === undefined ? \"null\" : __bkJson;"
    )
}

/// Run an agent's `eval` and bound the result (64 KiB of JSON; past that the
/// value is the cut JSON text, as a string, with `truncated: true`).
pub async fn eval(
    app: &AppHandle,
    channel_id: &str,
    expression: &str,
    page_world: bool,
) -> Result<Map<String, Value>, PreviewError> {
    let run = |as_statements: bool| {
        view::call_js(
            app,
            channel_id,
            eval_body(expression, as_statements),
            String::new(),
            page_world,
            OP_TIMEOUT,
        )
    };
    let raw = match run(false).await {
        Err(error)
            if error.code == "preview_eval_error" && error.message.contains("SyntaxError") =>
        {
            run(true).await?
        }
        other => other?,
    };
    Ok(bound_eval_result(raw.as_deref().unwrap_or("null")))
}

/// Apply the eval cap to a JSON result text.
pub fn bound_eval_result(json_text: &str) -> Map<String, Value> {
    let mut out = Map::new();
    if json_text.len() > EVAL_RESULT_CAP_BYTES {
        let (cut, _) = cap_text(json_text, EVAL_RESULT_CAP_BYTES);
        out.insert("value".into(), Value::String(cut));
        out.insert("truncated".into(), json!(true));
    } else {
        let value = serde_json::from_str(json_text).unwrap_or(Value::String(json_text.into()));
        out.insert("value".into(), value);
        out.insert("truncated".into(), json!(false));
    }
    out
}

/// Poll a `check` op until it is satisfied or `timeout` passes. Polled from
/// Rust so the wait survives navigations (each one replaces the world the
/// check runs in).
pub async fn wait_for(
    app: &AppHandle,
    channel_id: &str,
    check: Map<String, Value>,
    timeout: Duration,
) -> Result<Map<String, Value>, PreviewError> {
    let started = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        let attempt = run_op(
            app,
            channel_id,
            check.clone(),
            remaining.max(Duration::from_millis(500)),
        )
        .await;
        match attempt {
            Ok(result) if result.get("satisfied").and_then(Value::as_bool) == Some(true) => {
                let mut out = Map::new();
                out.insert("satisfied".into(), json!(true));
                out.insert(
                    "elapsedMs".into(),
                    json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
                );
                return Ok(out);
            }
            Ok(_) => {}
            // Mid-navigation the world is torn down; keep waiting.
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "preview_unavailable" | "preview_timeout"
                ) => {}
            Err(error) => return Err(error),
        }
        if started.elapsed() >= timeout {
            return Err(PreviewError::timeout());
        }
        tokio::time::sleep(WAIT_FOR_POLL).await;
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
