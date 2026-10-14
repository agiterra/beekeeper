//! Restricted model authentication read afresh at each adapter start.
//! An explicit handle is authoritative: unavailable custody refuses a start
//! instead of falling back to an ambient account. Values never cross the wire.
use std::collections::BTreeMap;
use std::path::Path;

use beekeeper_core::coding_session_runtime::{
    default_credential_env, RuntimeDescriptor, CLAUDE_CREDENTIAL_ENV, CODEX_CREDENTIAL_ENV,
};
use serde::Deserialize;

use crate::session::CreateFailure;

const MODEL_AUTH_UNAVAILABLE: &str = "MODEL_AUTH_UNAVAILABLE";

#[cfg(test)]
#[path = "runtime_descriptor_plug_tests.rs"]
mod runtime_descriptor_plug_tests;

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

/// The refusal for a runtime word with no built-in variable list and no
/// `credentialEnv` on its descriptor: same code, but the message names the
/// fix rather than blaming the file.
fn undeclared_credential_env(runtime: &str) -> CreateFailure {
    crate::execution_scope::refuse(
        MODEL_AUTH_UNAVAILABLE,
        format!(
            "runtime {runtime:?} has no built-in credential variables and its descriptor names \
             none; add credentialEnv to the runtime descriptor (bee host commission \
             --credential-env VAR) or commission it without a credential file. No adapter \
             was started"
        ),
    )
}

/// The variable names a runtime accepts from the credentials file: the
/// descriptor's own `credentialEnv`, else the built-in list for a known
/// runtime word, else a refusal that says which of the two to supply.
fn permitted_names(runtime: &str, credential_env: &[String]) -> Result<Vec<String>, CreateFailure> {
    if !credential_env.is_empty() {
        return Ok(credential_env.to_vec());
    }
    default_credential_env(runtime)
        .map(|names| names.iter().map(|name| (*name).to_owned()).collect())
        .ok_or_else(|| undeclared_credential_env(runtime))
}

fn read(
    path: &Path,
    instance: &str,
    runtime: &str,
    credential_env: &[String],
) -> Result<Vec<(String, String)>, CreateFailure> {
    let permitted = permitted_names(runtime, credential_env)?;
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
    if values.is_empty()
        || values.iter().any(|(name, value)| {
            !permitted.contains(name) || value.trim().is_empty() || value.contains('\0')
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
    environment_with(
        state_dir,
        &descriptor.instance_ref,
        &descriptor.runtime,
        &descriptor.credential_env,
        descriptor
            .cli_env
            .iter()
            .map(|env| (env.name.clone(), env.value.clone()))
            .collect(),
    )
}

/// Resolve the adapter environment for one runtime: the descriptor's own
/// `cli_env` values, then — when a credential file is configured — every
/// account variable this runtime could inherit blanked and the file's values
/// for `instance` added, restricted to the names `credential_env` (or the
/// runtime word's built-in list) permits.
pub(crate) fn environment_with(
    state_dir: &Path,
    instance: &str,
    runtime: &str,
    credential_env: &[String],
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
        let credentials = read(&path, instance, runtime, credential_env)?;
        // Do not retain a stale descriptor-supplied model account alongside
        // the newly resolved explicit credentials: neither a known word's
        // built-in names nor the names this descriptor declared.
        values.retain(|(name, _)| {
            !CLAUDE_CREDENTIAL_ENV.contains(&name.as_str())
                && !CODEX_CREDENTIAL_ENV.contains(&name.as_str())
                && !credential_env.contains(name)
        });
        // Explicit custody must select one account even if the supervisor
        // inherited a different API key or cloud-routing mode. Empty runtime
        // overrides remove those ambient alternatives before injecting the
        // selected credential values through the existing environment fence.
        // A plugged runtime's alternatives are the names it declared.
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
        for name in credential_env {
            values.push((name.clone(), String::new()));
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
            read(&path, &runtime.instance_ref, &runtime.runtime, &[]).expect("first account")[0].1,
            "first"
        );
        write(r#"{"runtimes":{"claude-primary":{"CLAUDE_CODE_OAUTH_TOKEN":"second"}}}"#);
        assert_eq!(
            read(&path, &runtime.instance_ref, &runtime.runtime, &[]).expect("rotated account")[0]
                .1,
            "second"
        );
        write(r#"{"runtimes":{"claude-primary":{"BEEKEEPER_PRIVATE_KEY":"forbidden"}}}"#);
        assert_eq!(
            read(&path, &runtime.instance_ref, &runtime.runtime, &[])
                .expect_err("namespace refused")
                .code,
            MODEL_AUTH_UNAVAILABLE
        );
        write(r#"{"runtimes":{"codex-primary":{"OPENAI_API_KEY":"other account"}}}"#);
        assert!(read(&path, &runtime.instance_ref, &runtime.runtime, &[]).is_err());
        std::fs::remove_file(path.clone()).expect("withdraw credential");
        assert!(read(&path, &runtime.instance_ref, &runtime.runtime, &[]).is_err());
    }

    /// The plug: the allowed names come from the descriptor. A runtime word
    /// with no built-in list is accepted once it names its variables and
    /// refused — naming the fix — when it names none.
    #[test]
    fn a_plugged_runtime_reads_only_the_variables_its_descriptor_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("credentials.json");
        let write = |body: &str| {
            crate::state::atomic_write(&path, body.as_bytes()).expect("private custody")
        };
        write(r#"{"runtimes":{"acme-primary":{"ACME_TOKEN":"acme-secret"}}}"#);
        let declared = vec!["ACME_TOKEN".to_owned()];
        assert_eq!(
            read(&path, "acme-primary", "acme", &declared).expect("declared name")[0],
            ("ACME_TOKEN".to_owned(), "acme-secret".to_owned())
        );

        // Nothing declared, nothing built in: refused with the same code and
        // a message that says what to add.
        let refused = read(&path, "acme-primary", "acme", &[]).expect_err("undeclared");
        assert_eq!(refused.code, MODEL_AUTH_UNAVAILABLE);
        assert!(
            refused.message.contains("credentialEnv"),
            "{}",
            refused.message
        );
        assert!(
            refused.message.contains("--credential-env"),
            "{}",
            refused.message
        );

        // A declared list is exact: a file value outside it is refused.
        write(r#"{"runtimes":{"acme-primary":{"ACME_TOKEN":"x","OTHER":"y"}}}"#);
        assert!(read(&path, "acme-primary", "acme", &declared).is_err());

        // A known word's descriptor may narrow its built-in list.
        write(r#"{"runtimes":{"claude-primary":{"CLAUDE_CODE_OAUTH_TOKEN":"tok"}}}"#);
        let narrowed = vec!["ANTHROPIC_API_KEY".to_owned()];
        assert!(read(&path, "claude-primary", "claude", &narrowed).is_err());
        assert!(read(&path, "claude-primary", "claude", &[]).is_ok());
    }

    /// With a credential file configured, a plugged runtime's own names are
    /// blanked before the file's values are added, exactly as a known word's
    /// ambient alternatives are — the descriptor cannot smuggle a value in
    /// through `cliEnv` beside the file's.
    #[test]
    fn a_plugged_runtimes_declared_names_are_fenced_then_supplied() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).expect("state dir");
        let path = dir.path().join("credentials.json");
        crate::state::atomic_write(
            &path,
            br#"{"runtimes":{"acme-primary":{"ACME_TOKEN":"from-file"}}}"#,
        )
        .expect("private custody");
        crate::state::atomic_write(
            &state.join("runtime-credentials-source.json"),
            serde_json::json!({ "path": path }).to_string().as_bytes(),
        )
        .expect("pointer");
        let declared = vec!["ACME_TOKEN".to_owned()];
        let env = environment_with(
            &state,
            "acme-primary",
            "acme",
            &declared,
            vec![
                ("ACME_TOKEN".to_owned(), "from-descriptor".to_owned()),
                ("ACME_MODE".to_owned(), "fast".to_owned()),
            ],
        )
        .expect("environment");
        let last = |name: &str| {
            env.iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(last("ACME_TOKEN").as_deref(), Some("from-file"));
        assert_eq!(last("ACME_MODE").as_deref(), Some("fast"));
        assert!(!env.contains(&("ACME_TOKEN".to_owned(), "from-descriptor".to_owned())));
        // The same descriptor with no file configured keeps its own values.
        let plain = environment_with(
            &dir.path().join("no-pointer"),
            "acme-primary",
            "acme",
            &declared,
            vec![("ACME_MODE".to_owned(), "fast".to_owned())],
        )
        .expect("no custody configured");
        assert_eq!(plain, vec![("ACME_MODE".to_owned(), "fast".to_owned())]);
    }
}
