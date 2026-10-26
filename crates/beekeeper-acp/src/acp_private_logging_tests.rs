//! Real stdio and tracing capture: the protection must precede initialize and
//! survive the prompt, not merely scrub a translated transcript afterwards.
use super::*;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);
impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const OUTBOUND: &str = "PRIVATE_OUTBOUND_PROMPT_f49a1";
const INCOMING: &str = "PRIVATE_ASSISTANT_TEXT_6a1e2";
const TOOL: &str = "PRIVATE_TOOL_TITLE_4f9e1";
const STDERR: &str = "PRIVATE_PREOPEN_STDERR_12b4a";
const LATE: &str = "PRIVATE_LATE_STDERR_71b3a";
const INIT: &str = "PRIVATE_INITIALIZE_RESPONSE_25b8a";
const ID: &str = "PRIVATE_NATIVE_ID_7e4b1";
const PERMISSION: &str = "PRIVATE_PERMISSION_OPTION_61a4b";

fn exercise(logging: LoggingMode, level: tracing::Level) -> (String, String) {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(level)
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    // One OS thread keeps the subscriber installed for the router and stderr
    // tasks as well as the test future; no process-global subscriber races.
    let _guard = tracing::subscriber::set_default(subscriber);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let tail = runtime.block_on(async {
        // Bash, its builtins and coreutils sleep: the Linux CI image (scripts/ci-image)
        // carries no python3 or jq. Requests arrive as compact serde_json with
        // the numeric `id` key first, so the first `"id":N` is the request's.
        let script = format!(r#"
printf '%s\n' '{STDERR}' >&2
emit() {{ printf '%s\n' "$1"; }}
update() {{ emit '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{ID}","update":'"$1"'}}}}'; }}
while IFS= read -r line; do
    [[ $line =~ \"id\":([0-9]+) ]] || continue
    id=${{BASH_REMATCH[1]}}
    case $line in
        *'"method":"initialize"'*)
            result='{{"protocolVersion":1,"agentInfo":{{"name":"{INIT}"}}}}' ;;
        *'"method":"session/new"'*)
            result='{{"sessionId":"{ID}"}}' ;;
        *'"method":"session/prompt"'*)
            update '{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{INCOMING}"}}}}'
            update '{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"{INCOMING}"}}}}'
            update '{{"sessionUpdate":"tool_call","toolCallId":"{TOOL}","title":"{TOOL}","kind":"read"}}'
            update '{{"sessionUpdate":"tool_call_update","toolCallId":"{TOOL}","status":"completed"}}'
            emit '{{"jsonrpc":"2.0","id":"{PERMISSION}","method":"session/request_permission","params":{{"sessionId":"{ID}","options":[{{"kind":"allow_once","optionId":"{PERMISSION}"}}]}}}}'
            IFS= read -r permission_reply
            result='{{"stopReason":"end_turn"}}' ;;
        *)
            continue ;;
    esac
    emit '{{"jsonrpc":"2.0","id":'"$id"',"result":'"$result"'}}'
    if [[ $line == *'"method":"session/prompt"'* ]]; then
        sleep 0.02
        printf '%s\n' '{LATE}' >&2
    fi
done
"#);
        let mut client = AcpClient::spawn_with_env_fence_and_overrides_and_logging(
            "bash", &["-c".into(), script], &[], false,
            &EnvFence::OPEN, &[], None, logging,
        ).await.unwrap();
        client.initialize().await.unwrap();
        let session = client.session_new("/tmp", vec![], None, None).await.unwrap();
        assert_eq!(session, ID);
        let result = client.session_prompt_with_idle_timeout(
            &session, OUTBOUND, std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(5),
        ).await.unwrap();
        assert_eq!(result, StopReason::EndTurn);
        // Stderr is independently scheduled. Wait for its second entry rather
        // than racing the prompt response against the stderr task.
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while client.stderr_tail().lines().len() < 2 {
                tokio::task::yield_now().await;
            }
        }).await.unwrap();
        let tail = client.stderr_tail().joined().unwrap();
        client.shutdown().await;
        tail
    });
    let bytes = capture.0.lock().unwrap().clone();
    (String::from_utf8(bytes).unwrap(), tail)
}

#[test]
fn private_logging_hides_outbound_debug_and_all_incoming_payloads() {
    let (logs, tail) = exercise(LoggingMode::Private, tracing::Level::DEBUG);
    for canary in [OUTBOUND, INCOMING, TOOL, STDERR, LATE, INIT, ID, PERMISSION] {
        assert!(
            !logs.contains(canary),
            "canary {canary} escaped tracing: {logs}"
        );
        assert!(
            !tail.contains(canary),
            "canary {canary} escaped stderr tail"
        );
    }
}

#[test]
fn private_logging_hides_incoming_info_and_preopen_stderr() {
    let (logs, tail) = exercise(LoggingMode::Private, tracing::Level::INFO);
    for canary in [INCOMING, TOOL, STDERR, LATE, PERMISSION] {
        assert!(
            !logs.contains(canary),
            "canary {canary} escaped INFO tracing: {logs}"
        );
        assert!(!tail.contains(canary));
    }
    assert!(logs.contains("private ACP diagnostic"), "{logs}");
}

#[test]
fn ordinary_logging_keeps_diagnostics_negative_control() {
    let (logs, tail) = exercise(LoggingMode::Ordinary, tracing::Level::DEBUG);
    for canary in [OUTBOUND, INCOMING, TOOL, STDERR, LATE, INIT, ID, PERMISSION] {
        assert!(
            logs.contains(canary),
            "capture missed ordinary diagnostic {canary}: {logs}"
        );
    }
    assert!(tail.contains(STDERR));
    assert!(tail.contains(LATE));
}
