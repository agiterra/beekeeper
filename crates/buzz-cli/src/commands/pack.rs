//! `bee pack` subcommands — local persona pack operations.
//!
//! These commands operate on local pack directories. No relay connection needed.

use std::path::{Path, PathBuf};

use buzz_persona::compose::{compose_role, write_staged_pack, ComposeOptions, RoleSource};
use buzz_persona::template::{TemplateCatalog, TemplateRange};

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

/// Where this process finds the shipped template catalog: `explicit` when
/// given, else `$BUZZ_TEMPLATES_DIR`, else the nearest `personas/templates`
/// at or above the working directory (a Beekeeper checkout). `None` when
/// none of those is a directory — the caller decides whether that refuses
/// or composes against an empty catalog.
pub(crate) fn resolve_templates_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = explicit {
        return Some(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("BUZZ_TEMPLATES_DIR") {
        let dir = PathBuf::from(dir);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    let mut cursor = std::env::current_dir().ok()?;
    loop {
        let candidate = cursor.join("personas").join("templates");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !cursor.pop() {
            return None;
        }
    }
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
    let catalog = match resolve_templates_dir(templates) {
        Some(dir) => TemplateCatalog::load(&dir, app_version)
            .map_err(|e| CliError::Usage(format!("template catalog: {e}")))?,
        None => TemplateCatalog::empty(app_version),
    };
    let (source, source_path) = if source_dir.join(".plugin").join("plugin.json").is_file() {
        (
            RoleSource::Pack {
                dir: source_dir.to_path_buf(),
                role: role.to_owned(),
                persona: None,
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

/// Run `bee pack clone-template <name>@<range> [--templates <dir>] [--into <root>]`.
///
/// Spec § 3.2: a project that wants to own a shipped template's text copies
/// it. The template's body — everything after its frontmatter — lands at
/// `<into>/templates/<name>.md`, so a role includes it as
/// `![[./templates/<name>.md]]`; its skills land whole under
/// `<into>/skills/<skill>/`. The frontmatter is not copied: a project file
/// is inserted verbatim into a prompt, and a `name:`/`version:` block would
/// be inserted with it. The resolved version is printed, because after the
/// copy nothing in the project records it.
///
/// Refuses to overwrite an existing file without `--force`, and refuses a
/// range this catalog cannot satisfy with the composer's own sentence.
pub fn cmd_clone_template(
    spec: &str,
    templates: Option<&Path>,
    into: &Path,
    force: bool,
) -> Result<(), CliError> {
    let (name, range) = spec
        .split_once('@')
        .ok_or_else(|| CliError::Usage(format!("expected <name>@<range>, got {spec:?}")))?;
    let templates = resolve_templates_dir(templates).ok_or_else(|| {
        CliError::Usage(
            "no template catalog: pass --templates <dir>, set BUZZ_TEMPLATES_DIR, or run from a \
             Beekeeper checkout (personas/templates)"
                .to_owned(),
        )
    })?;
    let catalog = TemplateCatalog::load(&templates, "clone")
        .map_err(|e| CliError::Usage(format!("template catalog: {e}")))?;
    let range = TemplateRange::parse(name, range).map_err(|e| CliError::Usage(e.to_string()))?;
    let resolved = catalog
        .resolve(name, &range)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    if let Some(warning) = &resolved.warning {
        eprintln!("  WARN:  {warning}");
    }
    let template = resolved.template;
    let text_target = into.join("templates").join(format!("{name}.md"));
    let mut writes: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    for rel in &template.skills {
        let skill_dir = template
            .dir
            .join(rel.trim_start_matches("./").trim_end_matches('/'));
        let skill_name = skill_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| CliError::Other(format!("skill path {rel:?} has no name")))?
            .to_owned();
        writes.push((skill_dir, into.join("skills").join(skill_name)));
    }
    if !force {
        if text_target.exists() {
            return Err(CliError::Usage(format!(
                "{} exists; pass --force to overwrite it",
                text_target.display()
            )));
        }
        for (_, target) in &writes {
            if target.exists() {
                return Err(CliError::Usage(format!(
                    "{} exists; pass --force to overwrite it",
                    target.display()
                )));
            }
        }
    }
    if let Some(parent) = text_target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::Other(format!("could not create {}: {e}", parent.display())))?;
    }
    std::fs::write(&text_target, template.body.as_bytes())
        .map_err(|e| CliError::Other(format!("could not write {}: {e}", text_target.display())))?;
    for (source, target) in &writes {
        copy_dir(source, target)?;
    }
    println!(
        "cloned beekeeper/{name}@{} -> {}",
        template.version,
        text_target.display()
    );
    for (_, target) in &writes {
        println!("cloned skill -> {}", target.display());
    }
    println!(
        "include it with ![[./templates/{name}.md]]; the project now owns these bytes and \
         nothing records that they came from {name}@{}",
        template.version
    );
    Ok(())
}

/// Copy a directory tree; symlinks are refused, as the composer refuses them.
fn copy_dir(from: &Path, to: &Path) -> Result<(), CliError> {
    std::fs::create_dir_all(to)
        .map_err(|e| CliError::Other(format!("could not create {}: {e}", to.display())))?;
    let entries = std::fs::read_dir(from)
        .map_err(|e| CliError::Other(format!("could not read {}: {e}", from.display())))?;
    for entry in entries {
        let entry = entry
            .map_err(|e| CliError::Other(format!("could not read {}: {e}", from.display())))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| CliError::Other(format!("could not read {}: {e}", path.display())))?;
        if file_type.is_symlink() {
            return Err(CliError::Usage(format!(
                "{} is a symlink; a template's skills must be plain files",
                path.display()
            )));
        }
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|e| CliError::Other(format!("could not copy {}: {e}", path.display())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn clone_template_copies_the_body_and_skills_and_refuses_to_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let templates = tmp.path().join("templates");
        write(
            &templates.join("memory/1.0.0/TEMPLATE.md"),
            "---\nname: memory\nversion: 1.0.0\ndescription: m\nskills:\n  - ./skills/recall/\n---\nRecall first.\n",
        );
        write(
            &templates.join("memory/1.0.0/skills/recall/SKILL.md"),
            "---\nname: recall\ndescription: r\n---\nrecall\n",
        );
        let into = tmp.path().join("beekeeper");
        cmd_clone_template("memory@latest", Some(&templates), &into, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("templates/memory.md")).unwrap(),
            "Recall first.\n",
            "the body only: frontmatter would be inserted into a prompt"
        );
        assert!(into.join("skills/recall/SKILL.md").is_file());
        let again =
            cmd_clone_template("memory@latest", Some(&templates), &into, false).unwrap_err();
        assert!(again.to_string().contains("--force"), "{again}");
        cmd_clone_template("memory@^1.0.0", Some(&templates), &into, true).unwrap();
        let missing = cmd_clone_template("memory@^9", Some(&templates), &into, true).unwrap_err();
        assert!(missing.to_string().contains("matches none"), "{missing}");
        let bad = cmd_clone_template("memory", Some(&templates), &into, true).unwrap_err();
        assert!(bad.to_string().contains("<name>@<range>"), "{bad}");
    }

    #[test]
    fn layout_words_and_defaults() {
        use crate::commands::packs::PackLayout;
        assert_eq!(PackLayout::parse("pack").unwrap(), PackLayout::Pack);
        assert_eq!(PackLayout::parse("flat").unwrap(), PackLayout::Flat);
        assert!(PackLayout::parse("nested").is_err());
        assert_eq!(PackLayout::Pack.default_path(), "personas/roles");
        assert_eq!(PackLayout::Flat.default_path(), ".");
        assert_eq!(PackLayout::Pack.default_suffix(), "-packs");
        assert_eq!(PackLayout::Flat.default_suffix(), "-beekeeper-agents");
        // The suffix survives the 64-byte bound; the slug head is what yields.
        let long = "x".repeat(70);
        let id = crate::commands::packs::default_repo_id(&long, "-beekeeper-agents");
        assert_eq!(id.len(), 64);
        assert!(id.ends_with("-beekeeper-agents"));
        assert_eq!(
            crate::commands::packs::default_repo_id("tank-loop", "-beekeeper-agents"),
            "tank-loop-beekeeper-agents"
        );
        let tmp = tempfile::tempdir().unwrap();
        let flat = tmp.path().join("flat");
        write(&flat.join("roles/lead.md"), "Lead.\n");
        write(&flat.join("roles/builder.md"), "Build.\n");
        assert_eq!(PackLayout::Flat.roles_in(&flat), vec!["builder", "lead"]);
        // The pack lister counts directories by name; a flat root's `roles/`
        // would read as a role, so each layout gets its own fixture.
        let packs = tmp.path().join("packs");
        write(&packs.join("lead/.plugin/plugin.json"), "{}");
        assert_eq!(PackLayout::Pack.roles_in(&packs), vec!["lead"]);
        assert!(PackLayout::Flat.roles_in(&packs).is_empty());
    }
}
