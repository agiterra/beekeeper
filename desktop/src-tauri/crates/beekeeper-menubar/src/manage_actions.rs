//! What the Manage Host window may ask for, and how an answer is read.
//!
//! Pure on purpose, like [`crate::model`]. The window names one of a closed set
//! of actions; it can never hand this process an argv, so a webview bug cannot
//! become an arbitrary `bee` command. A key is only ever a *path*, the choices
//! travel on stdin, and an answer is `bee`'s own JSON. Everything with an
//! opinion about a setup lives behind `bee host manage`, where a terminal
//! reaches the same behavior.

use serde::{Deserialize, Serialize};

/// One thing the window can ask for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    Signer {
        key_file: Option<String>,
    },
    Roles {
        project: String,
    },
    Channels {
        key_file: Option<String>,
    },
    CreateChannel {
        name: String,
        key_file: Option<String>,
    },
    Trust {
        project: String,
        checkout: String,
        agents_repo: String,
        key_file: Option<String>,
    },
    Preflight {
        spec: serde_json::Value,
        key_file: Option<String>,
    },
    Plan {
        spec: serde_json::Value,
        key_file: Option<String>,
    },
    Apply {
        setup: String,
        resume_cancelled: bool,
        key_file: Option<String>,
    },
    Cancel {
        setup: String,
    },
    Start {
        setup: String,
        key_file: Option<String>,
    },
    Stop {
        setup: String,
        key_file: Option<String>,
    },
    Inspect {
        setup: String,
        key_file: Option<String>,
    },
}

/// A command to run: the arguments after `bee`, and what goes on its stdin.
#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// The longest any action may run, comfortably above `bee`'s own receipt waits.
pub const ACTION_TIMEOUT_SECS: u64 = 240;

fn with_key(mut args: Vec<String>, key_file: &Option<String>) -> Vec<String> {
    if let Some(path) = key_file.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        args.push("--owner-key-file".to_string());
        args.push(expand_home(path));
    }
    args
}

/// `~/` is what a person types and what no spawned process understands.
fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::Path::new(&home).join(rest).display().to_string(),
        _ => path.to_string(),
    }
}

fn base(verb: &str) -> Vec<String> {
    ["host", "manage", verb].map(str::to_string).to_vec()
}

fn with(mut args: Vec<String>, pairs: &[(&str, &str)]) -> Vec<String> {
    for (flag, value) in pairs {
        args.push(format!("--{flag}"));
        args.push((*value).to_string());
    }
    args
}

impl Action {
    /// The `bee` invocation for this action.
    pub fn invocation(&self) -> Result<Invocation, String> {
        let spec_stdin = |spec: &serde_json::Value| {
            serde_json::to_string(spec)
                .map(Some)
                .map_err(|error| error.to_string())
        };
        Ok(match self {
            Action::Signer { key_file } => Invocation {
                args: with_key(base("signer"), key_file),
                stdin: None,
            },
            Action::Roles { project } => Invocation {
                args: with(base("roles"), &[("project", project)]),
                stdin: None,
            },
            Action::Channels { key_file } => Invocation {
                args: with_key(base("channels"), key_file),
                stdin: None,
            },
            Action::CreateChannel { name, key_file } => Invocation {
                args: with_key(with(base("create-channel"), &[("name", name)]), key_file),
                stdin: None,
            },
            Action::Trust {
                project,
                checkout,
                agents_repo,
                key_file,
            } => Invocation {
                args: with_key(
                    with(
                        base("trust"),
                        &[
                            ("project", project),
                            ("checkout", &expand_home(checkout)),
                            ("agents-repo", &expand_home(agents_repo)),
                        ],
                    ),
                    key_file,
                ),
                stdin: None,
            },
            Action::Preflight { spec, key_file } => Invocation {
                args: with_key(with(base("preflight"), &[("spec-file", "-")]), key_file),
                stdin: spec_stdin(spec)?,
            },
            Action::Plan { spec, key_file } => Invocation {
                args: with_key(with(base("plan"), &[("spec-file", "-")]), key_file),
                stdin: spec_stdin(spec)?,
            },
            Action::Apply {
                setup,
                resume_cancelled,
                key_file,
            } => {
                let mut args = with(base("apply"), &[("setup", setup)]);
                if *resume_cancelled {
                    args.push("--resume-cancelled".to_string());
                }
                Invocation {
                    args: with_key(args, key_file),
                    stdin: None,
                }
            }
            Action::Cancel { setup } => Invocation {
                args: with(base("cancel"), &[("setup", setup)]),
                stdin: None,
            },
            Action::Start { setup, key_file } => Invocation {
                args: with_key(with(base("start"), &[("setup", setup)]), key_file),
                stdin: None,
            },
            Action::Stop { setup, key_file } => Invocation {
                args: with_key(with(base("stop"), &[("setup", setup)]), key_file),
                stdin: None,
            },
            Action::Inspect { setup, key_file } => Invocation {
                args: with_key(with(base("inspect"), &[("setup", setup)]), key_file),
                stdin: None,
            },
        })
    }
}

/// What `bee` must not inherit from this process: the Host's own list of inherited
/// credentials, which names both spellings of every attestation and the pointer to
/// one (`bee` adopts `BUZZ_<X>` as `BEEKEEPER_<X>`), plus the two owner keys that
/// list leaves to its own processes. Names only: nothing here opens a file.
#[cfg(unix)]
pub fn scrubbed_env() -> Vec<&'static str> {
    let mut names = beekeeper_host_core::env::INHERITED_KEYS_TO_CLEAR.to_vec();
    names.extend([
        "BEEKEEPER_PRIVATE_KEY",
        "BUZZ_PRIVATE_KEY",
        "NOSTR_PRIVATE_KEY",
    ]);
    names
}

/// What the window receives back.
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub ok: bool,
    pub value: Option<serde_json::Value>,
    pub code: Option<String>,
    pub message: Option<String>,
}

impl Outcome {
    /// A failure this process produced itself, before `bee` could answer.
    pub fn refused(code: &str, message: impl Into<String>) -> Self {
        Self {
            ok: false,
            value: None,
            code: Some(code.to_string()),
            message: Some(message.into()),
        }
    }

    /// Read a finished `bee`. A refused apply still prints its journal, so both
    /// the value and the refusal can be present.
    pub fn from_output(success: bool, stdout: &str, stderr: &str) -> Self {
        let value = serde_json::from_str(stdout.trim()).ok();
        if success {
            return Self {
                ok: true,
                value,
                code: None,
                message: None,
            };
        }
        // `bee` prints `{"error":…,"message":…}` as its last stderr line.
        let reported = stderr
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .and_then(|json| json["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| {
                let tail = stderr.trim();
                if tail.is_empty() {
                    "bee stopped without saying why".to_string()
                } else {
                    tail.chars().take(600).collect()
                }
            });
        let (code, message) =
            split_code(&reported).unwrap_or_else(|| ("BEE_REFUSED".to_string(), reported.clone()));
        Self {
            ok: false,
            value,
            code: Some(code),
            message: Some(message),
        }
    }
}

/// `CODE: sentence`, allowing for the category word `bee` prints before a key
/// error (`key error: CODE: sentence`).
fn split_code(reported: &str) -> Option<(String, String)> {
    let is_code = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
    };
    let (first, rest) = reported.split_once(':')?;
    if is_code(first) {
        return Some((first.to_string(), rest.trim().to_string()));
    }
    let (second, rest) = rest.trim_start().split_once(':')?;
    is_code(second).then(|| (second.to_string(), rest.trim().to_string()))
}

/// Where the `bee` that belongs to this app is. Never `PATH`: on this machine
/// the one there is routinely a stale build.
pub fn resolve_bee(
    current_exe: &std::path::Path,
    env_override: Option<std::path::PathBuf>,
    exists: impl Fn(&std::path::Path) -> bool,
) -> Option<std::path::PathBuf> {
    if let Some(path) = env_override {
        return exists(&path).then_some(path);
    }
    let dir = current_exe.parent()?;
    let sibling = dir.join("bee");
    if exists(&sibling) {
        return Some(sibling);
    }
    // Nested in `Beekeeper.app/Contents/Library/LoginItems/…`: the app's own
    // `Contents/MacOS` is the first ancestor that holds one.
    dir.ancestors()
        .filter(|ancestor| ancestor.file_name().is_some_and(|name| name == "Contents"))
        .map(|contents| contents.join("MacOS/bee"))
        .find(|candidate| exists(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn args(action: Action) -> Vec<String> {
        action.invocation().unwrap().args
    }

    #[test]
    fn the_window_names_actions_and_cannot_pass_an_argv() {
        assert!(serde_json::from_str::<Action>(r#"{"kind":"shell","args":["rm"]}"#).is_err());
        assert!(serde_json::from_str::<Action>(r#"{"args":["host","stop"]}"#).is_err());
        let action: Action =
            serde_json::from_str(r#"{"kind":"inspect","setup":"abc","keyFile":"/k"}"#).unwrap();
        assert_eq!(
            args(action),
            [
                "host",
                "manage",
                "inspect",
                "--setup",
                "abc",
                "--owner-key-file",
                "/k"
            ]
        );
    }

    #[test]
    fn a_blank_key_path_leaves_the_choice_to_bee() {
        let action = Action::Channels {
            key_file: Some("   ".into()),
        };
        assert_eq!(args(action), ["host", "manage", "channels"]);
    }

    #[test]
    fn choices_travel_on_stdin_and_never_on_the_command_line() {
        let spec = serde_json::json!({"model": "m"});
        let invocation = Action::Plan {
            spec: spec.clone(),
            key_file: None,
        }
        .invocation()
        .unwrap();
        assert_eq!(
            invocation.args,
            ["host", "manage", "plan", "--spec-file", "-"]
        );
        assert_eq!(invocation.stdin.as_deref(), Some(r#"{"model":"m"}"#));
    }

    #[test]
    fn resuming_a_cancelled_setup_is_asked_for_explicitly() {
        let plain = args(Action::Apply {
            setup: "s".into(),
            resume_cancelled: false,
            key_file: None,
        });
        assert!(!plain.iter().any(|arg| arg == "--resume-cancelled"));
        let resumed = args(Action::Apply {
            setup: "s".into(),
            resume_cancelled: true,
            key_file: None,
        });
        assert!(resumed.iter().any(|arg| arg == "--resume-cancelled"));
    }

    #[cfg(unix)]
    #[test]
    fn neither_spelling_of_an_owner_key_or_an_attestation_reaches_bee() {
        let names = scrubbed_env();
        for name in [
            "BEEKEEPER_PRIVATE_KEY",
            "BUZZ_PRIVATE_KEY",
            "NOSTR_PRIVATE_KEY",
            "BEEKEEPER_AUTH_TAG",
            "BUZZ_AUTH_TAG",
            "BEEKEEPER_AUTH_TAG_FILE",
            "BUZZ_AUTH_TAG_FILE",
        ] {
            assert!(names.contains(&name), "{name}");
        }
    }

    #[test]
    fn a_refusal_keeps_its_code_and_its_journal() {
        let stderr =
            r#"{"error":"refused","message":"CHANNEL_NOT_OWNED: not yours","retryable":false}"#;
        let outcome = Outcome::from_output(false, r#"{"state":"needs_attention"}"#, stderr);
        assert!(!outcome.ok);
        assert_eq!(outcome.code.as_deref(), Some("CHANNEL_NOT_OWNED"));
        assert_eq!(outcome.message.as_deref(), Some("not yours"));
        assert_eq!(outcome.value.unwrap()["state"], "needs_attention");
    }

    #[test]
    fn a_key_error_keeps_its_code_behind_the_category_word() {
        let stderr = r#"{"error":"key","message":"key error: OWNER_KEY_UNAVAILABLE: no key file"}"#;
        let outcome = Outcome::from_output(false, "", stderr);
        assert_eq!(outcome.code.as_deref(), Some("OWNER_KEY_UNAVAILABLE"));
        assert_eq!(outcome.message.as_deref(), Some("no key file"));
        let sentence = Outcome::from_output(false, "", r#"{"message":"relay error: try: again"}"#);
        assert_eq!(sentence.code.as_deref(), Some("BEE_REFUSED"));
    }

    #[test]
    fn an_answer_with_no_code_or_no_json_is_still_an_answer() {
        let outcome = Outcome::from_output(false, "", "panicked: Something");
        assert_eq!(outcome.code.as_deref(), Some("BEE_REFUSED"));
        assert_eq!(outcome.message.as_deref(), Some("panicked: Something"));
        let silent = Outcome::from_output(false, "", "");
        assert!(silent.message.unwrap().contains("without saying why"));
        let fine = Outcome::from_output(true, "[1]\n", "a warning");
        assert!(fine.ok);
        assert_eq!(fine.value.unwrap()[0], 1);
    }

    #[test]
    fn the_app_finds_its_own_bee_from_the_nested_menu_bar_bundle() {
        let nested = Path::new(
            "/A/Beekeeper.app/Contents/Library/LoginItems/Beekeeper Menu Bar.app/Contents/MacOS/beekeeper-menubar",
        );
        let found = resolve_bee(nested, None, |path| {
            path == Path::new("/A/Beekeeper.app/Contents/MacOS/bee")
        });
        assert_eq!(
            found,
            Some(PathBuf::from("/A/Beekeeper.app/Contents/MacOS/bee"))
        );
        assert_eq!(resolve_bee(nested, None, |_| false), None);
    }

    #[test]
    fn an_override_is_believed_only_if_it_exists() {
        let exe = Path::new("/target/debug/beekeeper-menubar");
        let wanted = PathBuf::from("/other/bee");
        assert_eq!(
            resolve_bee(exe, Some(wanted.clone()), |path| path == wanted.as_path()),
            Some(wanted.clone())
        );
        assert_eq!(resolve_bee(exe, Some(wanted), |_| false), None);
    }
}
