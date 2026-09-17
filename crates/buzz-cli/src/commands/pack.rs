//! `bee pack` subcommands — local persona pack operations.
//!
//! These commands operate on local pack directories. No relay connection needed.

use std::path::Path;

use buzz_persona::compose::{compose_role, write_staged_pack, ComposeOptions, RoleSource};
use buzz_persona::template::TemplateCatalog;

use crate::error::CliError;

/// Run `bee pack validate <path>`.
///
/// Calls `validate_pack()` from the persona crate, prints diagnostics,
/// and exits with the appropriate code:
/// - 0: valid (may have warnings)
/// - 1: errors found
pub fn cmd_validate(path: &str) -> Result<(), CliError> {
    let pack_dir = Path::new(path);
    if !pack_dir.exists() {
        return Err(CliError::Usage(format!("path does not exist: {path}")));
    }
    if !pack_dir.is_dir() {
        return Err(CliError::Usage(format!("not a directory: {path}")));
    }

    let report = buzz_persona::validate::validate_pack(pack_dir);

    for diag in &report.diagnostics {
        match diag {
            buzz_persona::validate::ValidationDiagnostic::Error(msg) => {
                eprintln!("  ERROR: {msg}");
            }
            buzz_persona::validate::ValidationDiagnostic::Warning(msg) => {
                eprintln!("  WARN:  {msg}");
            }
        }
    }

    if report.has_errors() {
        return Err(CliError::Usage("Validation failed.".into()));
    } else if report.has_warnings() {
        println!("Valid (with warnings).");
    } else {
        println!("Valid.");
    }

    Ok(())
}

/// Run `bee pack inspect <path>`.
///
/// Loads and resolves a pack, then pretty-prints a summary of each persona's
/// effective configuration.
pub fn cmd_inspect(path: &str) -> Result<(), CliError> {
    let pack_dir = Path::new(path);
    if !pack_dir.exists() {
        return Err(CliError::Usage(format!("path does not exist: {path}")));
    }
    if !pack_dir.is_dir() {
        return Err(CliError::Usage(format!("not a directory: {path}")));
    }

    // Resolve the pack — shows fully effective config (post-merge, post-split).
    let pack = buzz_persona::resolve::resolve_pack(pack_dir)
        .map_err(|e| CliError::Other(format!("failed to resolve pack: {e}")))?;

    // Header
    println!("Pack: {} ({})", pack.name, pack.id);
    println!("Version: {}", pack.version);
    println!("Personas: {}", pack.personas.len());
    println!();

    // Per-persona summary (fully resolved effective config)
    for persona in &pack.personas {
        println!("  {}", persona.name);
        println!("    Display: {}", persona.display_name);
        println!("    Description: {}", persona.description);

        if let Some(ref llm_provider) = persona.llm_provider {
            if let Some(ref model) = persona.model {
                println!("    Model: {llm_provider}:{model}");
            } else {
                println!("    Provider: {llm_provider}");
            }
        } else if let Some(ref model) = persona.model {
            println!("    Model: {model}");
        }
        if let Some(temp) = persona.temperature {
            println!("    Temperature: {temp}");
        }
        if let Some(ctx) = persona.max_context_tokens {
            println!("    Max context tokens: {ctx}");
        }

        if !persona.subscribe.is_empty() {
            println!("    Subscribe: {}", persona.subscribe.join(", "));
        }

        let rt = &persona.triggers;
        let mut parts = Vec::new();
        if rt.mentions {
            parts.push("mentions".to_string());
        }
        if !rt.keywords.is_empty() {
            parts.push(format!("keywords {:?}", rt.keywords));
        }
        if rt.all_messages {
            parts.push("all_messages".to_string());
        }
        if !parts.is_empty() {
            println!("    Triggers: {}", parts.join(" + "));
        }

        println!("    Thread replies: {}", persona.thread_replies);
        println!("    Broadcast replies: {}", persona.broadcast_replies);

        if !persona.mcp_servers.is_empty() {
            println!("    MCP servers: {}", persona.mcp_servers.len());
        }

        if !persona.skills.is_empty() {
            println!("    Skills: {}", persona.skills.join(", "));
        }

        if let Some(ref avatar) = persona.avatar {
            println!("    Avatar: {avatar}");
        }

        let prompt_preview = if persona.system_prompt.chars().count() > 80 {
            let truncated: String = persona.system_prompt.chars().take(77).collect();
            format!("{truncated}...")
        } else {
            persona.system_prompt.clone()
        };
        println!(
            "    System prompt: {} chars ({})",
            persona.system_prompt.len(),
            prompt_preview.replace('\n', " ")
        );

        if !persona.runtime_env_vars.is_empty() {
            let env_str: Vec<String> = persona
                .runtime_env_vars
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            println!("    Env vars: {}", env_str.join(", "));
        }
        println!();
    }

    Ok(())
}

/// Run `bee pack compose <path> --role <role> [--templates <dir>] [--out <dir>]`.
///
/// `path` is a pack directory when it holds `.plugin/plugin.json`, otherwise
/// a flat `beekeeper/`-style directory holding `roles/<role>.md`. Without
/// `--out`, the expanded persona file is printed to stdout and the
/// `compose.json` provenance to stderr; with it, the staged pack is written
/// there and its path printed. Warnings always go to stderr. Exit 1 on any
/// refusal, with the composer's reason.
pub fn cmd_compose(
    path: &str,
    role: &str,
    templates: Option<&Path>,
    app_version: &str,
    out: Option<&Path>,
) -> Result<(), CliError> {
    let source_dir = Path::new(path);
    if !source_dir.is_dir() {
        return Err(CliError::Usage(format!("not a directory: {path}")));
    }
    let catalog = match templates {
        Some(dir) => TemplateCatalog::load(dir, app_version)
            .map_err(|e| CliError::Usage(format!("template catalog: {e}")))?,
        None => TemplateCatalog::empty(app_version),
    };
    let (source, source_path) = if source_dir.join(".plugin").join("plugin.json").is_file() {
        (
            RoleSource::Pack {
                dir: source_dir.to_path_buf(),
                role: role.to_owned(),
            },
            format!("{}/{role}", path.trim_end_matches('/')),
        )
    } else {
        (
            RoleSource::Flat {
                root: source_dir.to_path_buf(),
                role: role.to_owned(),
            },
            format!("{}/roles/{role}", path.trim_end_matches('/')),
        )
    };
    let composed = compose_role(&source, &catalog, &ComposeOptions::local(source_path))
        .map_err(|e| CliError::Usage(format!("compose refused: {e}")))?;
    for warning in &composed.provenance.warnings {
        eprintln!("  WARN:  {warning}");
    }
    match out {
        Some(dest) => {
            let written = write_staged_pack(&composed, dest)
                .map_err(|e| CliError::Other(format!("stage failed: {e}")))?;
            println!("{}", written.display());
        }
        None => {
            print!("{}", composed.persona_markdown());
            eprint!("{}", composed.provenance_json());
        }
    }
    Ok(())
}
