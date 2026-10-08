//! `bee preview` — drive this session's local Browser preview (SV-33 S1/S2).
//!
//! Local-only, like `bee session`: nothing here reaches the relay. Each verb is
//! one request to the desktop app's session broker (the owner-only Unix socket
//! at `$BUZZ_SESSION_BROKER_SOCK`, else the production default), carrying the
//! execution's preview grant from `$BEEKEEPER_PREVIEW_GRANT`.
//!
//! There is **no session argument**. The provider mints the grant for exactly
//! one execution and the broker reads the session from it, so an agent can
//! only ever drive its own session's preview; a missing or wrong grant is the
//! broker's refusal, reported with its stable code.
//!
//! Output: stdout carries one JSON object, `{"ok":true,"verb":…,…result}`. A
//! refusal prints `{"ok":false,"code":…,"error":<sentence>}` on stderr with
//! stdout empty. Exit codes: 0 ok; 1 input; 2 no browser on this machine;
//! 3 any grant refusal; 4 everything else. A snapshot's PNG and aria text are
//! written by this process, into its own working tree: the desktop never
//! writes into a seat's tree.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use clap::{Args, Subcommand, ValueEnum};
use serde_json::{json, Map, Value};

use beekeeper_core::preview_grant::{PREVIEW_GRANT_ENV, SESSION_BROKER_SOCK_ENV};

/// The production broker socket, relative to `$HOME` (kept in step with
/// `commands::session` and the desktop's `session_broker::server`).
const DEFAULT_SOCKET_REL: &str = ".local/state/buzz/session-broker.sock";

/// The sentence for "no app to talk to", verbatim from the wire contract.
pub const NO_BROWSER_SENTENCE: &str =
    "No browser on this machine: the Beekeeper app is not running where this agent runs.";

/// Where a snapshot lands when `--out` is not given, relative to the cwd.
const DEFAULT_SNAPSHOT_DIR: &str = ".beekeeper/preview";

/// The broker refuses request lines above this; refusing here first says so
/// without a round trip.
const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// The broker's response line cap; anything longer is not a response.
const MAX_RESPONSE_BYTES: u64 = 6 * 1024 * 1024 + 1024;

/// Driver ops default to 10 s at the broker; the socket outlasts that by 5 s.
const OP_TIMEOUT: Duration = Duration::from_secs(15);

/// `wait_for` may run up to 30 s at the broker.
const MAX_WAIT_MS: u64 = 30_000;

/// The broker's own `wait_for` default, used to size the socket timeout.
const DEFAULT_WAIT_MS: u64 = 5_000;

/// Drive this session's local Browser preview.
#[derive(Debug, Subcommand)]
pub enum PreviewCmd {
    /// Report the preview's state (works with no preview open).
    Status,
    /// Open the preview at a local URL or port, docked in the session's
    /// Browser surface if it is mounted, else popped out. Never headless.
    Open {
        /// A local URL: http(s) on localhost, 127.0.0.1 or [::1].
        #[arg(long, conflicts_with = "port", required_unless_present = "port")]
        url: Option<String>,
        /// A local port; resolves to the listening server's URL.
        #[arg(long)]
        port: Option<u16>,
    },
    /// Navigate the open preview: a local URL, or back, forward or reload.
    Navigate {
        /// A local URL: http(s) on localhost, 127.0.0.1 or [::1].
        #[arg(required_unless_present_any = ["back", "forward", "reload"],
              conflicts_with_all = ["back", "forward", "reload"])]
        url: Option<String>,
        /// Go back one entry.
        #[arg(long, conflicts_with_all = ["forward", "reload"])]
        back: bool,
        /// Go forward one entry.
        #[arg(long, conflicts_with = "reload")]
        forward: bool,
        /// Reload the page.
        #[arg(long)]
        reload: bool,
    },
    /// Capture the page: aria snapshot text and a PNG, written into this
    /// working tree (default `./.beekeeper/preview/`).
    Snapshot {
        /// Where to write the PNG; the aria text goes beside it as
        /// `<name>.aria.yaml`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Capture the aria text only.
        #[arg(long)]
        no_image: bool,
    },
    /// Click an element.
    Click {
        #[command(flatten)]
        target: Target,
        /// Mouse button.
        #[arg(long, value_enum)]
        button: Option<MouseButton>,
        /// Number of clicks (2 for a double click).
        #[arg(long)]
        click_count: Option<u8>,
    },
    /// Type text into an element.
    Type {
        #[command(flatten)]
        target: Target,
        /// The text to type.
        #[arg(id = "input_text", value_name = "TEXT")]
        text: String,
        /// Clear the field first.
        #[arg(long)]
        clear: bool,
    },
    /// Press a key (Playwright key names: `Enter`, `Meta+a`), optionally on
    /// an element.
    Press {
        /// The key.
        key: String,
        #[command(flatten)]
        target: Target,
    },
    /// Scroll the page or an element, by a delta or to an edge.
    Scroll {
        #[command(flatten)]
        target: Target,
        /// Vertical delta in CSS pixels.
        #[arg(
            long,
            allow_hyphen_values = true,
            required_unless_present = "to",
            conflicts_with = "to"
        )]
        dy: Option<i64>,
        /// Horizontal delta in CSS pixels.
        #[arg(long, allow_hyphen_values = true, requires = "dy")]
        dx: Option<i64>,
        /// Scroll to an edge.
        #[arg(long, value_enum)]
        to: Option<ScrollEdge>,
    },
    /// Evaluate a JavaScript expression (awaited) and print its JSON value.
    Eval {
        /// The expression.
        expression: String,
        /// Run in the page's own world, which sees the app's globals
        /// (default: the isolated driver world, which shares only the DOM).
        #[arg(long)]
        page_world: bool,
    },
    /// Wait until an element, text or URL condition holds.
    WaitFor {
        #[command(flatten)]
        target: WaitTarget,
        /// Wait until this text is on the page.
        #[arg(long)]
        text: Option<String>,
        /// Wait until the URL contains this.
        #[arg(long)]
        url_includes: Option<String>,
        /// The element state to wait for.
        #[arg(long, value_enum)]
        state: Option<WaitState>,
        /// Give up after this many ms (at most 30000).
        #[arg(long)]
        timeout_ms: Option<u64>,
    },
    /// List this machine's listening local HTTP servers.
    Servers,
    /// Close the preview.
    Close,
}

/// An element locator: exactly one family, plus an optional `--nth`.
#[derive(Debug, Clone, Default, Args)]
#[group(skip)]
pub struct Target {
    #[command(flatten)]
    family: TargetFamily,
    /// The accessible name, with `--role`.
    #[arg(long, requires = "role")]
    name: Option<String>,
    /// Match `--name` exactly.
    #[arg(long, requires = "name")]
    exact: bool,
    /// Pick the n-th match (0-based) when several match.
    #[arg(long)]
    nth: Option<u32>,
}

#[derive(Debug, Clone, Default, Args)]
#[group(id = "locator", multiple = false)]
struct TargetFamily {
    /// A ref from the last snapshot (`e12@g3`).
    #[arg(long = "ref")]
    reference: Option<String>,
    /// An ARIA role (`button`, `link`, `textbox`, …).
    #[arg(long)]
    role: Option<String>,
    /// A form control's label text.
    #[arg(long)]
    label: Option<String>,
    /// Visible text.
    #[arg(long)]
    text: Option<String>,
    /// A placeholder.
    #[arg(long)]
    placeholder: Option<String>,
    /// A `data-testid`.
    #[arg(long)]
    test_id: Option<String>,
    /// A CSS selector.
    #[arg(long)]
    selector: Option<String>,
}

/// [`Target`] for `wait-for`, whose `--text` is the wait condition rather
/// than a locator family.
#[derive(Debug, Clone, Default, Args)]
#[group(skip)]
pub struct WaitTarget {
    #[command(flatten)]
    family: WaitTargetFamily,
    /// The accessible name, with `--role`.
    #[arg(long, requires = "role")]
    name: Option<String>,
    /// Match `--name` exactly.
    #[arg(long, requires = "name")]
    exact: bool,
    /// Pick the n-th match (0-based) when several match.
    #[arg(long)]
    nth: Option<u32>,
}

#[derive(Debug, Clone, Default, Args)]
#[group(id = "locator", multiple = false)]
struct WaitTargetFamily {
    /// A ref from the last snapshot (`e12@g3`).
    #[arg(long = "ref")]
    reference: Option<String>,
    /// An ARIA role.
    #[arg(long)]
    role: Option<String>,
    /// A form control's label text.
    #[arg(long)]
    label: Option<String>,
    /// A placeholder.
    #[arg(long)]
    placeholder: Option<String>,
    /// A `data-testid`.
    #[arg(long)]
    test_id: Option<String>,
    /// A CSS selector.
    #[arg(long)]
    selector: Option<String>,
}

/// Mouse buttons `click` accepts.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum MouseButton {
    /// Primary.
    Left,
    /// Secondary.
    Right,
    /// Middle.
    Middle,
}

/// Edges `scroll --to` accepts.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ScrollEdge {
    /// The top.
    Top,
    /// The bottom.
    Bottom,
}

/// Element states `wait-for --state` accepts.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum WaitState {
    /// Rendered and visible.
    Visible,
    /// Absent or not visible.
    Hidden,
    /// In the DOM.
    Attached,
    /// Not in the DOM.
    Detached,
}

impl Target {
    /// The locator JSON, or `None` when no family was given.
    fn to_json(&self) -> Option<Value> {
        let f = &self.family;
        locator_json(
            [
                ("ref", &f.reference),
                ("label", &f.label),
                ("text", &f.text),
                ("placeholder", &f.placeholder),
                ("testId", &f.test_id),
                ("selector", &f.selector),
            ],
            f.role.as_deref(),
            self.name.as_deref(),
            self.exact,
            self.nth,
        )
    }
}

impl WaitTarget {
    fn to_json(&self) -> Option<Value> {
        let f = &self.family;
        locator_json(
            [
                ("ref", &f.reference),
                ("label", &f.label),
                ("text", &None),
                ("placeholder", &f.placeholder),
                ("testId", &f.test_id),
                ("selector", &f.selector),
            ],
            f.role.as_deref(),
            self.name.as_deref(),
            self.exact,
            self.nth,
        )
    }
}

fn locator_json(
    families: [(&str, &Option<String>); 6],
    role: Option<&str>,
    name: Option<&str>,
    exact: bool,
    nth: Option<u32>,
) -> Option<Value> {
    let mut out = Map::new();
    if let Some(role) = role {
        out.insert("role".into(), json!(role));
        if let Some(name) = name {
            out.insert("name".into(), json!(name));
            out.insert("exact".into(), json!(exact));
        }
    } else {
        let (key, value) = families
            .iter()
            .find_map(|(key, value)| value.as_ref().map(|value| (*key, value)))?;
        out.insert(key.into(), json!(value));
    }
    if let Some(nth) = nth {
        out.insert("nth".into(), json!(nth));
    }
    Some(Value::Object(out))
}

/// One refusal as `bee preview` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFailure {
    /// The stable wire code.
    pub code: String,
    /// The sentence for the agent.
    pub error: String,
}

impl PreviewFailure {
    fn new(code: &str, error: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            error: error.into(),
        }
    }

    fn no_browser() -> Self {
        Self::new("preview_no_browser", NO_BROWSER_SENTENCE)
    }

    /// The process exit code for this refusal.
    pub fn exit_code(&self) -> i32 {
        exit_code_for(&self.code)
    }

    fn to_json(&self) -> Value {
        json!({ "ok": false, "code": self.code, "error": self.error })
    }
}

/// The exit code for a refusal code: 1 input, 2 no browser, 3 any grant
/// refusal, 4 everything else.
pub fn exit_code_for(code: &str) -> i32 {
    match code {
        "preview_url_refused" | "preview_bad_request" | "preview_too_large" => 1,
        "preview_no_browser" => 2,
        "preview_no_grant"
        | "preview_grant_malformed"
        | "preview_grant_invalid"
        | "preview_wrong_issuer"
        | "preview_grant_bad_signature"
        | "preview_wrong_audience"
        | "preview_grant_expired"
        | "preview_grant_not_yet_valid"
        | "preview_wrong_session" => 3,
        _ => 4,
    }
}

/// What one invocation reads from its environment, passed in so the whole
/// path is testable without touching the process environment.
#[derive(Debug, Clone)]
pub struct PreviewContext {
    /// The broker socket.
    pub socket: PathBuf,
    /// `$BEEKEEPER_PREVIEW_GRANT`, when set and non-empty.
    pub grant: Option<String>,
    /// Who is calling, for the broker's log only.
    pub caller: Option<String>,
    /// The directory a relative snapshot path is resolved against.
    pub cwd: PathBuf,
    /// Unix milliseconds, for the default snapshot file name.
    pub now_ms: u128,
}

impl PreviewContext {
    /// Read the socket, grant and cwd from this process.
    pub fn from_env(caller: Option<String>) -> Result<Self, PreviewFailure> {
        let socket = match std::env::var_os(SESSION_BROKER_SOCK_ENV).filter(|v| !v.is_empty()) {
            Some(explicit) => PathBuf::from(explicit),
            None => std::env::var_os("HOME")
                .map(|home| PathBuf::from(home).join(DEFAULT_SOCKET_REL))
                .ok_or_else(PreviewFailure::no_browser)?,
        };
        let grant = std::env::var(PREVIEW_GRANT_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty());
        let cwd = std::env::current_dir().map_err(|error| {
            PreviewFailure::new(
                "preview_io_error",
                format!("cannot read the working directory: {error}"),
            )
        })?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or_default();
        Ok(Self {
            socket,
            grant,
            caller,
            cwd,
            now_ms,
        })
    }
}

/// Run one `bee preview` verb against this process's environment, print its
/// output, and return the exit code.
pub async fn run(cmd: &PreviewCmd, caller: Option<String>) -> i32 {
    let outcome = match PreviewContext::from_env(caller) {
        Ok(context) => execute(cmd, &context),
        Err(failure) => Err(failure),
    };
    report(outcome)
}

/// Print an outcome in the wire shape and return its exit code.
pub fn report(outcome: Result<Value, PreviewFailure>) -> i32 {
    match outcome {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(failure) => {
            eprintln!("{}", failure.to_json());
            failure.exit_code()
        }
    }
}

/// The verb's wire name and its broker `action`, with the socket timeout it
/// needs. Pure, so the request shape is testable on its own.
pub fn action_for(cmd: &PreviewCmd) -> Result<(&'static str, Value, Duration), PreviewFailure> {
    let need_target = |target: Option<Value>, verb: &str| {
        target.ok_or_else(|| {
            PreviewFailure::new(
                "preview_bad_request",
                format!(
                    "`bee preview {verb}` needs a target: --ref, --role [--name], --label, \
                     --text, --placeholder, --test-id or --selector"
                ),
            )
        })
    };
    let mut action = Map::new();
    let (verb, timeout) =
        match cmd {
            PreviewCmd::Status => ("status", OP_TIMEOUT),
            PreviewCmd::Servers => ("servers", OP_TIMEOUT),
            PreviewCmd::Close => ("close", OP_TIMEOUT),
            PreviewCmd::Open { url, port } => {
                match (url, port) {
                    (Some(url), _) => action.insert("url".into(), json!(url)),
                    (None, Some(port)) => action.insert("port".into(), json!(port)),
                    (None, None) => {
                        return Err(PreviewFailure::new(
                            "preview_bad_request",
                            "`bee preview open` needs --url or --port",
                        ))
                    }
                };
                ("open", OP_TIMEOUT)
            }
            PreviewCmd::Navigate {
                url,
                back,
                forward,
                reload,
            } => {
                match (url, back, forward, reload) {
                    (Some(url), false, false, false) => action.insert("url".into(), json!(url)),
                    (None, true, false, false) => action.insert("back".into(), json!(true)),
                    (None, false, true, false) => action.insert("forward".into(), json!(true)),
                    (None, false, false, true) => action.insert("reload".into(), json!(true)),
                    _ => return Err(PreviewFailure::new(
                        "preview_bad_request",
                        "`bee preview navigate` takes one of <url>, --back, --forward, --reload",
                    )),
                };
                ("navigate", OP_TIMEOUT)
            }
            PreviewCmd::Snapshot { no_image, .. } => {
                action.insert("image".into(), json!(!no_image));
                ("snapshot", OP_TIMEOUT)
            }
            PreviewCmd::Click {
                target,
                button,
                click_count,
            } => {
                action.insert("target".into(), need_target(target.to_json(), "click")?);
                if let Some(button) = button {
                    let name = match button {
                        MouseButton::Left => "left",
                        MouseButton::Right => "right",
                        MouseButton::Middle => "middle",
                    };
                    action.insert("button".into(), json!(name));
                }
                if let Some(count) = click_count {
                    action.insert("clickCount".into(), json!(count));
                }
                ("click", OP_TIMEOUT)
            }
            PreviewCmd::Type {
                target,
                text,
                clear,
            } => {
                action.insert("target".into(), need_target(target.to_json(), "type")?);
                action.insert("text".into(), json!(text));
                action.insert("clear".into(), json!(clear));
                ("type", OP_TIMEOUT)
            }
            PreviewCmd::Press { key, target } => {
                action.insert("key".into(), json!(key));
                if let Some(target) = target.to_json() {
                    action.insert("target".into(), target);
                }
                ("press", OP_TIMEOUT)
            }
            PreviewCmd::Scroll { target, dy, dx, to } => {
                if let Some(target) = target.to_json() {
                    action.insert("target".into(), target);
                }
                match (to, dy) {
                    (Some(edge), _) => {
                        let edge = match edge {
                            ScrollEdge::Top => "top",
                            ScrollEdge::Bottom => "bottom",
                        };
                        action.insert("to".into(), json!(edge));
                    }
                    (None, Some(dy)) => {
                        action.insert("dx".into(), json!(dx.unwrap_or(0)));
                        action.insert("dy".into(), json!(dy));
                    }
                    (None, None) => {
                        return Err(PreviewFailure::new(
                            "preview_bad_request",
                            "`bee preview scroll` needs --dy or --to",
                        ))
                    }
                }
                ("scroll", OP_TIMEOUT)
            }
            PreviewCmd::Eval {
                expression,
                page_world,
            } => {
                action.insert("expression".into(), json!(expression));
                let world = if *page_world { "page" } else { "driver" };
                action.insert("world".into(), json!(world));
                ("eval", OP_TIMEOUT)
            }
            PreviewCmd::WaitFor {
                target,
                text,
                url_includes,
                state,
                timeout_ms,
            } => {
                let target = target.to_json();
                if target.is_none() && text.is_none() && url_includes.is_none() {
                    return Err(PreviewFailure::new(
                        "preview_bad_request",
                        "`bee preview wait-for` needs a target, --text or --url-includes",
                    ));
                }
                if let Some(target) = target {
                    action.insert("target".into(), target);
                }
                if let Some(text) = text {
                    action.insert("text".into(), json!(text));
                }
                if let Some(url) = url_includes {
                    action.insert("urlIncludes".into(), json!(url));
                }
                if let Some(state) = state {
                    let name = match state {
                        WaitState::Visible => "visible",
                        WaitState::Hidden => "hidden",
                        WaitState::Attached => "attached",
                        WaitState::Detached => "detached",
                    };
                    action.insert("state".into(), json!(name));
                }
                if let Some(ms) = timeout_ms {
                    if *ms > MAX_WAIT_MS {
                        return Err(PreviewFailure::new(
                            "preview_bad_request",
                            format!("--timeout-ms is at most {MAX_WAIT_MS}"),
                        ));
                    }
                    action.insert("timeoutMs".into(), json!(ms));
                }
                let wait = timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
                (
                    "wait_for",
                    Duration::from_millis(wait) + Duration::from_secs(5),
                )
            }
        };
    action.insert("verb".into(), json!(verb));
    Ok((verb, Value::Object(action), timeout))
}

/// Run one verb against `context`: build the request, call the broker, and
/// shape the result (writing a snapshot's files).
pub fn execute(cmd: &PreviewCmd, context: &PreviewContext) -> Result<Value, PreviewFailure> {
    let (verb, action, timeout) = action_for(cmd)?;
    let envelope = json!({
        "caller": context.caller,
        "request": { "op": "preview", "grant": context.grant, "action": action },
    });
    let result = broker_call(&context.socket, &envelope, timeout)?;
    let mut result = match result {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => {
            let mut map = Map::new();
            map.insert("result".into(), other);
            map
        }
    };
    if let PreviewCmd::Snapshot { out, .. } = cmd {
        write_snapshot(&mut result, out.as_deref(), context)?;
    }
    // The CLI's display name for the verb: what was typed, not the wire name.
    let shown = if verb == "wait_for" { "wait-for" } else { verb };
    let mut output = Map::new();
    output.insert("ok".into(), json!(true));
    output.insert("verb".into(), json!(shown));
    for (key, value) in result {
        if key != "ok" && key != "verb" {
            output.insert(key, value);
        }
    }
    Ok(Value::Object(output))
}

/// Decode the snapshot's PNG (never printed) and write it and the aria text
/// into the caller's tree, replacing `png.base64` with the paths.
fn write_snapshot(
    result: &mut Map<String, Value>,
    out: Option<&Path>,
    context: &PreviewContext,
) -> Result<(), PreviewFailure> {
    let generation = result
        .get("generation")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let png_path = match out {
        Some(out) if out.is_absolute() => out.to_path_buf(),
        Some(out) => context.cwd.join(out),
        None => context
            .cwd
            .join(DEFAULT_SNAPSHOT_DIR)
            .join(format!("snapshot-{generation}-{}.png", context.now_ms)),
    };
    let aria_path = png_path.with_extension("aria.yaml");
    let io = |path: &Path, error: std::io::Error| {
        PreviewFailure::new(
            "preview_io_error",
            format!("cannot write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = png_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }

    let mut written_png = None;
    if let Some(Value::Object(png)) = result.get_mut("png") {
        if let Some(Value::String(encoded)) = png.remove("base64") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.as_bytes())
                .map_err(|error| {
                    PreviewFailure::new(
                        "preview_bad_response",
                        format!("the snapshot's PNG is not base64: {error}"),
                    )
                })?;
            std::fs::write(&png_path, &bytes).map_err(|error| io(&png_path, error))?;
            written_png = Some(png_path.clone());
        }
    }
    let aria = result.get("aria").and_then(Value::as_str).unwrap_or("");
    std::fs::write(&aria_path, aria).map_err(|error| io(&aria_path, error))?;
    result.insert(
        "pngPath".into(),
        written_png.map_or(Value::Null, |path| json!(path.display().to_string())),
    );
    result.insert("ariaPath".into(), json!(aria_path.display().to_string()));
    Ok(())
}

/// One request/response round trip with the broker.
#[cfg(unix)]
fn broker_call(
    socket: &Path,
    envelope: &Value,
    timeout: Duration,
) -> Result<Value, PreviewFailure> {
    use std::io::ErrorKind;
    use std::io::Read as _;
    use std::os::unix::net::UnixStream;

    let mut line = serde_json::to_string(envelope).map_err(|error| {
        PreviewFailure::new("preview_bad_request", format!("encode request: {error}"))
    })?;
    if line.len() > MAX_REQUEST_BYTES {
        return Err(PreviewFailure::new(
            "preview_too_large",
            "The request is too large.",
        ));
    }
    line.push('\n');

    let stream = UnixStream::connect(socket).map_err(|error| match error.kind() {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused | ErrorKind::PermissionDenied => {
            PreviewFailure::no_browser()
        }
        _ => PreviewFailure::new(
            "preview_no_browser",
            format!("{NO_BROWSER_SENTENCE} ({}: {error})", socket.display()),
        ),
    })?;
    let transport = |what: &str, error: std::io::Error| {
        PreviewFailure::new(
            "preview_broker_unreachable",
            format!("the Beekeeper app's broker stopped answering ({what}: {error})"),
        )
    };
    stream
        .set_read_timeout(Some(timeout))
        .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(8))))
        .map_err(|error| transport("socket setup", error))?;
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|error| transport("send", error))?;

    let mut reader = BufReader::new((&stream).take(MAX_RESPONSE_BYTES));
    let mut response = String::new();
    reader.read_line(&mut response).map_err(|error| {
        if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) {
            PreviewFailure::new("preview_timeout", "The page did not get there in time.")
        } else {
            transport("read", error)
        }
    })?;
    parse_response(&response)
}

#[cfg(not(unix))]
fn broker_call(
    _socket: &Path,
    _envelope: &Value,
    _timeout: Duration,
) -> Result<Value, PreviewFailure> {
    Err(PreviewFailure::no_browser())
}

/// Read one broker response line: `{ok, result}` or `{ok:false, code?, error}`.
pub fn parse_response(line: &str) -> Result<Value, PreviewFailure> {
    let line = line.trim_end();
    if line.is_empty() {
        return Err(PreviewFailure::new(
            "preview_broker_unreachable",
            "the Beekeeper app's broker closed the connection without answering",
        ));
    }
    let response: Value = serde_json::from_str(line).map_err(|error| {
        PreviewFailure::new(
            "preview_bad_response",
            format!("malformed broker response: {error}"),
        )
    })?;
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(response.get("result").cloned().unwrap_or(Value::Null));
    }
    let error = response
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("the broker refused without a reason")
        .to_owned();
    // An app from before previews answers with no code: say it is
    // unavailable rather than inventing a refusal it did not make.
    let code = response
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("preview_unavailable");
    Err(PreviewFailure::new(code, error))
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
