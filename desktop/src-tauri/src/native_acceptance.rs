//! Feature-gated driver for real isolated desktop backend acceptance.
//! Calls the same commands as IPC, without adding a product control endpoint.
use crate::{app_state::AppState, commands, managed_agents::CreateManagedAgentRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Scenario {
    expected_owner: String,
    relay_url: String,
    actions: Vec<Action>,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    ApplyWorkspace,
    WaitForSignal {
        path: PathBuf,
    },
    Create {
        input: Box<CreateManagedAgentRequest>,
    },
    Start {
        pubkey: String,
    },
    Stop {
        pubkey: String,
    },
    Delete {
        pubkey: String,
    },
    Inspect {
        pubkey: String,
    },
}

fn restricted(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || meta.len() > 512 * 1024 {
        return Err("scenario must be a bounded regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("scenario must have owner-only custody".into());
        }
    }
    std::fs::read(path).map_err(|e| e.to_string())
}
fn validate(app: &AppHandle, scenario: &Scenario, path: &Path) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("isolated HOME missing")?;
    let service = beekeeper_host_core::layout::home_dir()?;
    if home != Path::new("/tmp/bee-headless-e-desktop-home")
        || !service.ends_with("slice-e-service-home")
        || !path.starts_with("/tmp/bee-headless-e-native-scenarios")
        || scenario.relay_url != "ws://127.0.0.1:3037"
        || scenario.actions.len() > 16
    {
        return Err("native acceptance requires the named isolated E fixture scope".into());
    }
    if app
        .state::<AppState>()
        .signing_keys()?
        .public_key()
        .to_hex()
        != scenario.expected_owner
    {
        return Err("native acceptance signer does not match isolated fixture owner".into());
    }
    Ok(())
}
async fn execute(app: &AppHandle, scenario: &Scenario, action: Action) -> Result<Value, String> {
    match action {
        Action::WaitForSignal { path } => {
            if path.parent() != Some(Path::new("/tmp/bee-headless-e-native-scenarios")) {
                return Err(
                    "acceptance signal must stay in the named isolated scenario directory".into(),
                );
            }
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(180);
            while !path.exists() {
                if tokio::time::Instant::now() >= deadline {
                    return Err(
                        "bounded acceptance signal wait expired; no lifecycle action performed"
                            .into(),
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            restricted(&path)?;
            Ok(json!({"isolatedSignalObserved":true,"path":path}))
        }
        Action::ApplyWorkspace => {
            crate::apply_workspace(
                scenario.relay_url.clone(),
                None,
                None,
                None,
                None,
                app.clone(),
            )
            .await?;
            Ok(json!({"workspaceApplied":true}))
        }
        Action::Create { input } => {
            let result = commands::agents::create_managed_agent(
                *input,
                app.clone(),
                app.state::<AppState>(),
            )
            .await?;
            // No private environment, keys or backend config in evidence.
            Ok(json!({"created":true,"response":result}))
        }
        Action::Start { pubkey } => {
            commands::agents::start_managed_agent(
                pubkey.clone(),
                Some(scenario.relay_url.clone()),
                Some(scenario.expected_owner.clone()),
                None,
                app.clone(),
                app.state::<AppState>(),
            )
            .await?;
            Ok(json!({"started":true,"pubkey":pubkey}))
        }
        Action::Stop { pubkey } => {
            commands::agents::stop_managed_agent(pubkey.clone(), app.clone()).await?;
            Ok(json!({"stopped":true,"pubkey":pubkey}))
        }
        Action::Delete { pubkey } => {
            commands::agents::delete_managed_agent(pubkey.clone(), None, app.clone()).await?;
            Ok(json!({"deleted":true,"pubkey":pubkey}))
        }
        Action::Inspect { pubkey } => {
            commands::agents::host_transfer::get_managed_agent_host_transfer(
                app.clone(),
                pubkey,
                None,
                app.state::<AppState>(),
            )
            .await
        }
    }
}
fn strip_private(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for key in [
                "envVars",
                "env_vars",
                "private_key_nsec",
                "privateKeyNsec",
                "authTag",
                "auth_tag",
                "backend",
                "backendConfig",
                "backend_config",
            ] {
                object.remove(key);
            }
            for child in object.values_mut() {
                strip_private(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_private(child);
            }
        }
        _ => {}
    }
}
pub(crate) fn start(app: AppHandle) {
    let Some(path) = std::env::var_os("BEEKEEPER_NATIVE_ACCEPTANCE_SCENARIO").map(PathBuf::from)
    else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let scenario: Scenario = serde_json::from_slice(&restricted(&path)?)
                .map_err(|_| "invalid native scenario")?;
            validate(&app, &scenario, &path)?;
            let output = path.with_extension("results.json");
            let mut outcomes = Vec::new();
            for action in scenario.actions {
                let scope = Scenario {
                    expected_owner: scenario.expected_owner.clone(),
                    relay_url: scenario.relay_url.clone(),
                    actions: Vec::new(),
                };
                let mut outcome = match execute(&app, &scope, action).await {
                    Ok(value) => json!({"ok":true,"result":value}),
                    Err(error) => json!({"ok":false,"error":error}),
                };
                strip_private(&mut outcome);
                outcomes.push(outcome);
                beekeeper_host_core::atomic_write::atomic_write_json_restricted(
                    &output,
                    &serde_json::to_vec_pretty(&outcomes)
                        .map_err(|_| "cannot encode native result")?,
                )?;
                if outcomes.last().is_some_and(|v| v["ok"] == false) {
                    return Err("native action failed; protected result retained".into());
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("isolated native acceptance: {error}");
        }
    });
}

pub(crate) fn isolated_scope_requested() -> bool {
    std::env::var_os("BEEKEEPER_NATIVE_ACCEPTANCE_SCENARIO")
        .map(PathBuf::from)
        .is_some_and(|path| {
            path.starts_with("/tmp/bee-headless-e-native-scenarios") && restricted(&path).is_ok()
        })
        && std::env::var_os("HOME").as_deref()
            == Some(std::ffi::OsStr::new("/tmp/bee-headless-e-desktop-home"))
        && beekeeper_host_core::layout::home_dir()
            .is_ok_and(|home| home.ends_with("slice-e-service-home"))
}
