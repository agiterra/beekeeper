//! The one-shot ACP exchange behind an auto-title (SV-31).
//!
//! Deliberately not [`buzz_acp::acp::AcpClient`]: that client auto-approves
//! every `session/request_permission` (it is built to drive a working
//! session), and a namer must never be allowed to act. This is the smallest
//! ACP client that can name a session — `initialize`, one `session/new`, an
//! optional model switch, one `session/prompt` — and it **rejects** every
//! permission request and refuses every other agent-to-client request. It
//! advertises no file-system and no terminal capability, opens no MCP server,
//! and collects only the agent's message text.
//!
//! The child is started exactly the way a model-discovery probe is: inside the
//! host-prepared boundary when one exists (an empty host-owned directory, no
//! project, no seat bundle, no context MCP), or — only where no backend exists
//! — with the provider's credential fence applied. Its whole process group is
//! killed when the exchange ends, whether it ended well or not.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use buzz_acp::acp::{reported_model, resolve_model_switch_method, ModelSwitchMethod};

use crate::execution_scope::ExecutionPlan;

/// Longest single JSON-RPC line read from the adapter. A line past it is a
/// protocol failure, not something to buffer without end.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Most message text kept. A title is one short line; everything past this is
/// an adapter that did not follow the instruction.
const MAX_COLLECTED_CHARS: usize = 4_096;
/// How long the child gets to go after its group is signalled.
const REAP_GRACE: Duration = Duration::from_secs(2);
/// The model named when the adapter reported none and no switch was applied:
/// a disclosed non-answer. The payload needs a non-empty string, and naming
/// the *requested* model would attribute the words to a model that may not
/// have written them.
pub(crate) const UNREPORTED_MODEL: &str = "unreported";

/// What one exchange needs to know about the runtime it starts.
#[derive(Debug, Clone)]
pub(crate) struct OneShotRuntime<'a> {
    /// The adapter executable.
    pub agent_command: &'a str,
    /// The adapter's argv after the command.
    pub agent_args: &'a [String],
    /// The descriptor's `cli_env`, for the unbounded fallback spawn only (a
    /// prepared launch already carries it in its resolved environment).
    pub cli_env: &'a [(String, String)],
    /// Whether this is the Claude runtime, which takes a replacement system
    /// prompt as a string in `_meta.systemPrompt`.
    pub claude: bool,
    /// The model to switch to; a value the adapter does not offer leaves the
    /// session on whatever the adapter reports, and that is what is named
    /// ([`UNREPORTED_MODEL`] when it reports nothing).
    pub model: &'a str,
}

/// What one exchange produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OneShotAnswer {
    /// The agent's message text, concatenated, possibly empty.
    pub text: String,
    /// The model the session actually ran on: the switched-to value, else
    /// the adapter's own report, else [`UNREPORTED_MODEL`] — never the bare
    /// request.
    pub model: String,
    /// How many permission requests were rejected.
    pub permissions_rejected: u32,
}

/// Run one titling exchange: start the adapter, ask once, collect the text,
/// and kill the whole process group whatever happened.
pub(crate) async fn run_one_shot(
    runtime: &OneShotRuntime<'_>,
    plan: &ExecutionPlan,
    cwd: &Path,
    instruction: &str,
    message: &str,
) -> Result<OneShotAnswer, String> {
    let mut command = spawn_command(runtime, plan, cwd);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start the adapter: {error}"))?;
    let guard = GroupGuard(child.id());
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "the adapter's stdin was not captured".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "the adapter's stdout was not captured".to_owned())?;
    let mut rpc = Rpc {
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 0,
        text: String::new(),
        permissions_rejected: 0,
    };
    let claude_options = match plan {
        ExecutionPlan::Prepared(prepared) => prepared.claude_options.as_ref(),
        ExecutionPlan::Legacy { .. } => None,
    };
    let result = exchange(&mut rpc, runtime, claude_options, cwd, instruction, message).await;
    drop(rpc);
    reap(&mut child, guard).await;
    result
}

async fn exchange(
    rpc: &mut Rpc,
    runtime: &OneShotRuntime<'_>,
    claude_options: Option<&serde_json::Map<String, Value>>,
    cwd: &Path,
    instruction: &str,
    message: &str,
) -> Result<OneShotAnswer, String> {
    let init = rpc
        .request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false,
                },
                "clientInfo": {
                    "name": "buzz-session-provider-title",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
        .await?;
    let protocol = init["protocolVersion"].as_u64().unwrap_or(1);

    let mut params = json!({
        "cwd": cwd.to_string_lossy(),
        "mcpServers": [],
    });
    // Where the instruction goes. Claude replaces its whole coding preset with
    // a string; a v2 adapter takes a bare `systemPrompt`; anything else gets
    // the instruction ahead of the message in the one prompt.
    let prompt_text = if runtime.claude {
        params["_meta"]["systemPrompt"] = Value::String(instruction.to_owned());
        // The boundary's own options (its pinned settings file, and
        // `strictMcpConfig` so no project or user MCP server loads).
        for (key, value) in claude_options.into_iter().flatten() {
            params["_meta"]["claudeCode"]["options"][key] = value.clone();
        }
        // One model turn: the answer is text, and a tool loop is not a title.
        params["_meta"]["claudeCode"]["options"]["maxTurns"] = json!(1);
        title_prompt(None, message)
    } else if protocol >= 2 {
        params["systemPrompt"] = Value::String(instruction.to_owned());
        title_prompt(None, message)
    } else {
        title_prompt(Some(instruction), message)
    };
    let opened = rpc.request("session/new", params).await?;
    let session_id = opened["sessionId"]
        .as_str()
        .ok_or_else(|| "session/new answered without a sessionId".to_owned())?
        .to_owned();

    let model = apply_model(rpc, &session_id, &opened, runtime.model).await;

    rpc.text.clear();
    rpc.request(
        "session/prompt",
        json!({
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": prompt_text }],
        }),
    )
    .await?;
    Ok(OneShotAnswer {
        text: std::mem::take(&mut rpc.text),
        model,
        permissions_rejected: rpc.permissions_rejected,
    })
}

/// The one prompt: T3's initial-title framing (`TextGenerationPrompts.ts`,
/// "User message:"), with the instruction ahead of it when the adapter had no
/// other place to take one.
fn title_prompt(instruction: Option<&str>, message: &str) -> String {
    match instruction {
        Some(instruction) => format!("{instruction}\n\nUser message:\n{message}"),
        None => format!("User message:\n{message}"),
    }
}

/// Switch the session to `requested`, and say which model it is really on.
///
/// An exact match first, then the first offered value that contains the
/// request (`haiku` → `claude-haiku-4-5`), because an alias is how the
/// default is written. When nothing matches, or the switch is refused, the
/// session stays where the adapter put it and that model is the one named —
/// a requested model is not an applied one, so when the adapter reports no
/// model either, the answer is [`UNREPORTED_MODEL`].
async fn apply_model(rpc: &mut Rpc, session_id: &str, opened: &Value, requested: &str) -> String {
    let current = reported_model(opened);
    let switch = resolve_model_switch_method(opened, requested)
        .or_else(|| alias_switch_method(opened, requested));
    let Some(switch) = switch else {
        return current.unwrap_or_else(|| UNREPORTED_MODEL.to_owned());
    };
    let (method, params, value) = match switch {
        ModelSwitchMethod::ConfigOption {
            config_id,
            option_value,
        } => (
            "session/set_config_option",
            json!({ "sessionId": session_id, "configId": config_id, "value": option_value }),
            option_value,
        ),
        ModelSwitchMethod::SetModel { model_id } => (
            "session/set_model",
            json!({ "sessionId": session_id, "modelId": model_id }),
            model_id,
        ),
    };
    if current.as_deref() == Some(value.as_str()) {
        return value;
    }
    match rpc.request(method, params).await {
        Ok(_) => value,
        Err(error) => {
            tracing::debug!(
                target: "csp::auto_title",
                "the adapter refused the title model {value:?}: {error}"
            );
            current.unwrap_or_else(|| UNREPORTED_MODEL.to_owned())
        }
    }
}

/// The first offered model whose value contains `alias`, case-insensitively.
fn alias_switch_method(opened: &Value, alias: &str) -> Option<ModelSwitchMethod> {
    let needle = alias.trim().to_ascii_lowercase();
    if needle.is_empty() || needle == "default" {
        return None;
    }
    let contains = |value: &str| value.to_ascii_lowercase().contains(&needle);
    for option in buzz_acp::acp::extract_model_config_options(opened) {
        let Some(config_id) = option
            .get("configId")
            .or_else(|| option.get("id"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let hit = option
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.get("value").and_then(Value::as_str))
            .find(|value| contains(value));
        if let Some(value) = hit {
            return Some(ModelSwitchMethod::ConfigOption {
                config_id: config_id.to_owned(),
                option_value: value.to_owned(),
            });
        }
    }
    buzz_acp::acp::extract_model_state(opened)?
        .get("availableModels")?
        .as_array()?
        .iter()
        .filter_map(|model| model.get("modelId").and_then(Value::as_str))
        .find(|value| contains(value))
        .map(|model_id| ModelSwitchMethod::SetModel {
            model_id: model_id.to_owned(),
        })
}

/// The child command: bounded when the host prepared a boundary, fenced
/// otherwise. Never inherits the provider's signing key either way.
fn spawn_command(runtime: &OneShotRuntime<'_>, plan: &ExecutionPlan, cwd: &Path) -> Command {
    let mut command = match plan {
        ExecutionPlan::Prepared(prepared) => {
            let launch = &prepared.launch;
            let (program, argv) = launch
                .boundary()
                .wrap(runtime.agent_command, runtime.agent_args);
            let mut command = Command::new(program);
            command.args(argv).current_dir(launch.cwd());
            launch.env().apply_to(&mut command);
            command
        }
        ExecutionPlan::Legacy { .. } => {
            let mut command = Command::new(runtime.agent_command);
            command.args(runtime.agent_args).current_dir(cwd);
            let fence = &crate::agent_fence::FENCE;
            for key in fence.keys {
                if !fence.exempt.contains(key) {
                    command.env_remove(key);
                }
            }
            for (key, _) in std::env::vars_os() {
                if fence.covers(&key.to_string_lossy()) {
                    command.env_remove(&key);
                }
            }
            for (name, value) in runtime.cli_env {
                command.env(name, value);
            }
            command
        }
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    command
}

/// Kills the child's whole process group when dropped — on every path out of
/// [`run_one_shot`], including a timeout that drops the future mid-exchange.
struct GroupGuard(Option<u32>);

impl GroupGuard {
    fn kill(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.and_then(|pid| i32::try_from(pid).ok()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn reap(child: &mut Child, guard: GroupGuard) {
    guard.kill();
    let _ = child.start_kill();
    let _ = tokio::time::timeout(REAP_GRACE, child.wait()).await;
}

struct Rpc {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    text: String,
    permissions_rejected: u32,
}

impl Rpc {
    async fn write(&mut self, message: &Value) -> Result<(), String> {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| format!("could not write to the adapter: {error}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|error| format!("could not write to the adapter: {error}"))
    }

    /// Send one request and serve the adapter until its answer arrives.
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        loop {
            let line = read_bounded_line(&mut self.stdout)
                .await?
                .ok_or_else(|| format!("the adapter exited before answering {method}"))?;
            let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(agent_method) = message.get("method").and_then(Value::as_str) {
                match message.get("id") {
                    Some(request_id) => {
                        let reply = self.answer_agent_request(agent_method, request_id, &message);
                        self.write(&reply).await?;
                    }
                    None => self.observe(agent_method, &message),
                }
                continue;
            }
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(format!("{method} failed: {error}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// The only answers a namer gives: no to every permission, and "not
    /// offered" to anything else the agent asks of its client.
    fn answer_agent_request(&mut self, method: &str, id: &Value, message: &Value) -> Value {
        if method != "session/request_permission" {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "not offered by a session namer" },
            });
        }
        self.permissions_rejected += 1;
        let options = message["params"]["options"].as_array();
        let reject = ["reject_once", "reject_always"].iter().find_map(|kind| {
            options?
                .iter()
                .find(|option| option.get("kind").and_then(Value::as_str) == Some(kind))
                .and_then(|option| option.get("optionId").cloned())
        });
        let outcome = match reject {
            Some(option_id) => json!({ "outcome": "selected", "optionId": option_id }),
            None => json!({ "outcome": "cancelled" }),
        };
        json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": outcome } })
    }

    fn observe(&mut self, method: &str, message: &Value) {
        if method != "session/update" {
            return;
        }
        let update = &message["params"]["update"];
        if update["sessionUpdate"].as_str() != Some("agent_message_chunk") {
            return;
        }
        if let Some(text) = update["content"]["text"].as_str() {
            let room = MAX_COLLECTED_CHARS.saturating_sub(self.text.chars().count());
            self.text.extend(text.chars().take(room));
        }
    }
}

/// One newline-terminated line, or `None` at end of stream. Errors on a line
/// longer than [`MAX_LINE_BYTES`] rather than buffering it.
async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<String>, String> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|error| format!("could not read from the adapter: {error}"))?;
        if available.is_empty() {
            return Ok((!line.is_empty()).then(|| String::from_utf8_lossy(&line).into_owned()));
        }
        let (chunk, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(end) => (&available[..=end], true),
            None => (available, false),
        };
        if line.len() + chunk.len() > MAX_LINE_BYTES {
            return Err("the adapter wrote a line past the namer's bound".to_owned());
        }
        line.extend_from_slice(chunk);
        let consumed = chunk.len();
        reader.consume(consumed);
        if done {
            return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
        }
    }
}
