//! Restricted model authentication read afresh at each adapter start.
//! An explicit handle is authoritative: unavailable custody refuses a start
//! instead of falling back to an ambient account. Values never cross the wire.
use std::collections::BTreeMap;
use std::path::Path;

use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use serde::Deserialize;

use crate::session::CreateFailure;

const MODEL_AUTH_UNAVAILABLE: &str = "MODEL_AUTH_UNAVAILABLE";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialFile {
    runtimes: BTreeMap<String, BTreeMap<String, String>>,
}

fn refusal() -> CreateFailure {
    crate::execution_scope::refuse(
        MODEL_AUTH_UNAVAILABLE,
        "the configured model credential file is unavailable, invalid, or lacks credentials for this runtime; no adapter was started",
    )
}

fn read(
    path: &Path,
    instance: &str,
    runtime: &str,
) -> Result<Vec<(String, String)>, CreateFailure> {
    if !path.is_absolute() {
        return Err(refusal());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).map_err(|_| refusal())?;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refusal());
        }
    }
    let bytes = std::fs::read(path).map_err(|_| refusal())?;
    let file: CredentialFile = serde_json::from_slice(&bytes).map_err(|_| refusal())?;
    let values = file.runtimes.get(instance).ok_or_else(refusal)?;
    let permitted: &[&str] = match runtime {
        "claude" => &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
        "codex" => &["OPENAI_API_KEY", "CODEX_API_KEY"],
        _ => return Err(refusal()),
    };
    if values.is_empty()
        || values.iter().any(|(name, value)| {
            !permitted.contains(&name.as_str()) || value.trim().is_empty() || value.contains('\0')
        })
    {
        return Err(refusal());
    }
    Ok(values
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect())
}

pub(crate) fn environment(
    state_dir: &Path,
    descriptor: &RuntimeDescriptor,
) -> Result<Vec<(String, String)>, CreateFailure> {
    environment_for(
        state_dir,
        &descriptor.instance_ref,
        &descriptor.runtime,
        descriptor
            .cli_env
            .iter()
            .map(|env| (env.name.clone(), env.value.clone()))
            .collect(),
    )
}

pub(crate) fn environment_for(
    state_dir: &Path,
    instance: &str,
    runtime: &str,
    mut values: Vec<(String, String)>,
) -> Result<Vec<(String, String)>, CreateFailure> {
    let configured = std::env::var_os("BEEKEEPER_CSP_RUNTIME_CREDENTIALS_FILE")
        .map(std::path::PathBuf::from)
        .map(|path| Ok(Some(path)))
        .unwrap_or_else(|| {
            let pointer = state_dir.join("runtime-credentials-source.json");
            match std::fs::read(pointer) {
                Ok(bytes) => {
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Source {
                        path: std::path::PathBuf,
                    }
                    serde_json::from_slice::<Source>(&bytes)
                        .map(|s| Some(s.path))
                        .map_err(|_| refusal())
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(refusal()),
            }
        })?;
    if let Some(path) = configured {
        let credentials = read(&path, instance, runtime)?;
        // Do not retain a stale descriptor-supplied model account alongside
        // the newly resolved explicit credentials.
        values.retain(|(name, _)| {
            !matches!(
                name.as_str(),
                "ANTHROPIC_API_KEY"
                    | "ANTHROPIC_AUTH_TOKEN"
                    | "CLAUDE_CODE_OAUTH_TOKEN"
                    | "OPENAI_API_KEY"
                    | "CODEX_API_KEY"
            )
        });
        // Explicit custody must select one account even if the supervisor
        // inherited a different API key or cloud-routing mode. Empty runtime
        // overrides remove those ambient alternatives before injecting the
        // selected credential values through the existing environment fence.
        let account_names: &[&str] = match runtime {
            "claude" => &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_AUTH_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
                "ANTHROPIC_BASE_URL",
                "ANTHROPIC_CUSTOM_HEADERS",
                "CLAUDE_CODE_USE_BEDROCK",
                "CLAUDE_CODE_USE_VERTEX",
                "ANTHROPIC_VERTEX_PROJECT_ID",
                "CLOUD_ML_REGION",
                "AWS_REGION",
                "AWS_PROFILE",
                "AWS_ACCESS_KEY_ID",
                "AWS_SECRET_ACCESS_KEY",
                "AWS_SESSION_TOKEN",
                "AWS_BEARER_TOKEN_BEDROCK",
            ],
            "codex" => &["OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"],
            _ => &[],
        };
        for name in account_names {
            values.push(((*name).to_owned(), String::new()));
        }
        values.extend(credentials);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_reread_and_missing_or_unrelated_keys_block() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("credentials.json");
        let runtime = crate::tests::claude_runtime_command("claude-agent-acp".into());
        let write = |body: &str| {
            crate::state::atomic_write(&path, body.as_bytes()).expect("private custody")
        };
        write(r#"{"runtimes":{"claude-primary":{"ANTHROPIC_API_KEY":"first"}}}"#);
        assert_eq!(
            read(&path, &runtime.instance_ref, &runtime.runtime).expect("first account")[0].1,
            "first"
        );
        write(r#"{"runtimes":{"claude-primary":{"CLAUDE_CODE_OAUTH_TOKEN":"second"}}}"#);
        assert_eq!(
            read(&path, &runtime.instance_ref, &runtime.runtime).expect("rotated account")[0].1,
            "second"
        );
        write(r#"{"runtimes":{"claude-primary":{"BEEKEEPER_PRIVATE_KEY":"forbidden"}}}"#);
        assert_eq!(
            read(&path, &runtime.instance_ref, &runtime.runtime)
                .expect_err("namespace refused")
                .code,
            MODEL_AUTH_UNAVAILABLE
        );
        write(r#"{"runtimes":{"codex-primary":{"OPENAI_API_KEY":"other account"}}}"#);
        assert!(read(&path, &runtime.instance_ref, &runtime.runtime).is_err());
        std::fs::remove_file(path.clone()).expect("withdraw credential");
        assert!(read(&path, &runtime.instance_ref, &runtime.runtime).is_err());
    }
}
