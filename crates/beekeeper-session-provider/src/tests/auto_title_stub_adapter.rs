//! A stub ACP adapter for the auto-title tests (SV-31): a bash script that
//! speaks just enough JSON-RPC to be named by `auto_title_acp`.
//!
//! It runs inside the real execution boundary, which lets it write nowhere a
//! test could read afterwards — so it reports what it saw **through its
//! answer**. It checks the client's requests as they arrive (no terminal
//! capability, one `session/new` with no MCP server and the naming
//! instruction, the alias switched to the offered model, a rejected
//! permission) and replies with the configured title only when every check
//! held; otherwise the reply names the checks that failed.

use std::io::Write as _;
use std::path::Path;

/// How the stub behaves on `session/prompt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StubPrompt<'a> {
    /// Ask for a permission, then stream `reply` (or the failed checks) and
    /// end the turn.
    Reply(&'a str),
    /// Never answer.
    Hang,
    /// Exit without answering.
    Exit,
}

/// Write the stub to `dir/name` and return the script path (run it under
/// `bash`, as the session tests do, to avoid ETXTBSY races with other test
/// threads' forks).
pub(crate) fn stub_adapter(dir: &Path, name: &str, prompt: StubPrompt<'_>) -> String {
    stub_adapter_with(dir, name, prompt, true)
}

/// [`stub_adapter`], choosing whether `session/new` offers a model selector.
/// With `offers_models` false it reports no current model and offers nothing
/// to switch to, so no switch is made and the model check holds vacuously.
pub(crate) fn stub_adapter_with(
    dir: &Path,
    name: &str,
    prompt: StubPrompt<'_>,
    offers_models: bool,
) -> String {
    let (model_start, new_result) = if offers_models {
        (
            "no",
            r#"{"sessionId":"title-1","configOptions":[{"id":"model","name":"Model","category":"model","type":"select","currentValue":"default","options":[{"value":"default","name":"Default"},{"value":"claude-haiku-4-5","name":"Haiku"}]}]}"#,
        )
    } else {
        ("ok", r#"{"sessionId":"title-1"}"#)
    };
    let on_prompt = match prompt {
        StubPrompt::Reply(reply) => format!(
            r#"      printf '{{"jsonrpc":"2.0","id":"perm-1","method":"session/request_permission","params":{{"sessionId":"title-1","toolCall":{{"toolCallId":"t1","title":"Write file"}},"options":[{{"optionId":"allow","name":"Allow","kind":"allow_once"}},{{"optionId":"reject","name":"Reject","kind":"reject_once"}}]}}}}\n'
      IFS= read -r answer
      if [[ $answer == *'"optionId":"reject"'* && $answer != *'"optionId":"allow"'* ]]; then PERM=ok; fi
      if [[ $INIT == ok && $NEW == ok && $NEWS == 1 && $MODEL == ok && $PERM == ok ]]; then
        text="{reply}"
      else
        text="Check failed init=$INIT new=$NEW news=$NEWS model=$MODEL perm=$PERM"
      fi
      printf '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"title-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"%s"}}}}}}}}\n' "$text"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;"#
        ),
        StubPrompt::Hang => "      sleep 600 ;;".to_owned(),
        StubPrompt::Exit => "      exit 0 ;;".to_owned(),
    };
    let body = format!(
        r#"INIT=no; NEW=no; NEWS=0; MODEL={model_start}; PERM=no
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      if [[ $line == *'"terminal":false'* ]]; then INIT=ok; fi
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":1,"agentInfo":{{"name":"stub"}}}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      NEWS=$((NEWS + 1))
      if [[ $line == *'"mcpServers":[]'* && $line == *'You name coding sessions'* ]]; then NEW=ok; fi
      printf '{{"jsonrpc":"2.0","id":%s,"result":{new_result}}}\n' "$id" ;;
    *'"method":"session/set_config_option"'*)
      if [[ $line == *'claude-haiku-4-5'* ]]; then MODEL=ok; fi
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
{on_prompt}
  esac
done
"#
    );
    let path = dir.join(name);
    let mut file = std::fs::File::create(&path).expect("create stub adapter");
    write!(file, "#!/bin/bash\n{body}").expect("write stub adapter");
    path.to_string_lossy().into_owned()
}
