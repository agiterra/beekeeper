//! `bee pack` subcommands — local persona pack operations.
//!
//! These commands operate on local pack directories. No relay connection needed.

use std::path::{Path, PathBuf};

use beekeeper_persona::compose::{compose_role, write_staged_pack, ComposeOptions, RoleSource};
use beekeeper_persona::template::{TemplateCatalog, TemplateRange};

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

    let report = beekeeper_persona::validate::validate_pack(pack_dir);

    for diag in &report.diagnostics {
        match diag {
            beekeeper_persona::validate::ValidationDiagnostic::Error(msg) => {
                eprintln!("  ERROR: {msg}");
            }
            beekeeper_persona::validate::ValidationDiagnostic::Warning(msg) => {
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
    let pack = beekeeper_persona::resolve::resolve_pack(pack_dir)
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

/// Where a build ships its role templates, relative to its resource root and
/// to a development checkout — the desktop's `DEFAULT_TEMPLATES_PATH`.
const SHIPPED_TEMPLATES_PATH: &str = "personas/templates";

/// Where this process finds the shipped template catalog, in order:
///
/// 1. `explicit` (`--templates`), then `$BEEKEEPER_TEMPLATES_DIR` — overrides.
/// 2. The templates this `bee`'s app bundle ships ([`app_bundle_contents`]). An
///    executable inside an app bundle uses its bundle's templates or none.
/// 3. The checkout this binary was built from, **only** when the running
///    executable is where that build put it — inside the checkout, or inside
///    the Cargo output directory this build recorded at compile time, however
///    `CARGO_TARGET_DIR` placed it ([`own_build_templates`]). A binary copied
///    or installed anywhere else finds nothing, and a bundled release never
///    falls back to its build machine's tree.
///
/// `None` when none of those is a directory — the caller decides whether that
/// refuses or composes against an empty catalog. The working directory and
/// its ancestors are never consulted: a seat stands in its project's
/// checkout, and a directory above it is another project's or the operator's,
/// never this build's catalog.
pub(crate) fn resolve_templates_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    let env = std::env::var_os("BEEKEEPER_TEMPLATES_DIR").map(PathBuf::from);
    let exe = std::env::current_exe().ok();
    let build = BuildProvenance {
        source_root: Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        output_dir: cargo_output_dir(Path::new(env!("OUT_DIR"))).map(Path::to_path_buf),
    };
    templates_dir_from(explicit, env.as_deref(), exe.as_deref(), Some(&build))
}

/// Where this build came from and where Cargo put it, both fixed at compile
/// time: nothing a running process or its working directory can supply.
pub(crate) struct BuildProvenance {
    /// The checkout the binary was built from.
    pub source_root: PathBuf,
    /// The Cargo profile directory (`<target>/[<triple>/]<profile>`) this
    /// build wrote its binaries into, when it could be read from `OUT_DIR`.
    pub output_dir: Option<PathBuf>,
}

/// The Cargo profile directory an `OUT_DIR` of this package lies in:
/// `<profile>/build/<package>-<hash>/out`, where `<profile>` is `debug`,
/// `release`, … under the target directory (and a target triple, when one
/// was given). That directory holds this build's own `bee` and its test
/// executables (`deps/`). `None` for any other shape: nothing is guessed.
fn cargo_output_dir(out_dir: &Path) -> Option<&Path> {
    if out_dir.file_name()? != "out" {
        return None;
    }
    let build = out_dir.parent()?.parent()?;
    if build.file_name()? != "build" {
        return None;
    }
    build.parent()
}

/// [`resolve_templates_dir`] over explicit inputs, so every branch is tested
/// without touching this process's environment or executable.
pub(crate) fn templates_dir_from(
    explicit: Option<&Path>,
    env: Option<&Path>,
    exe: Option<&Path>,
    build: Option<&BuildProvenance>,
) -> Option<PathBuf> {
    if let Some(dir) = explicit {
        return Some(dir.to_path_buf());
    }
    if let Some(dir) = env.filter(|dir| dir.is_dir()) {
        return Some(dir.to_path_buf());
    }
    let exe = exe.map(|exe| exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf()))?;
    // A bundled executable ships its own catalog or has none: a bundle
    // missing its resources never borrows a tree — not even its build's,
    // which may sit right above it (`target/<profile>/bundle/…`).
    if let Some(contents) = app_bundle_contents(&exe) {
        let dir = contents.join("Resources").join(SHIPPED_TEMPLATES_PATH);
        return dir.is_dir().then_some(dir);
    }
    own_build_templates(&exe, build?)
}

/// The `Contents` directory of the macOS app bundle `exe` runs from
/// (`<name>.app/Contents/MacOS/<exe>`, as `tauri.conf.json` bundles `bee`),
/// or `None` when `exe` is not in one.
fn app_bundle_contents(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && contents.parent()?.extension()? == "app";
    is_bundle.then_some(contents)
}

/// The build checkout's templates, when `exe` is where this build put it:
/// inside that checkout, or inside the Cargo output directory it recorded.
/// Both are this binary's own compile-time facts, verified against the
/// running executable rather than assumed.
fn own_build_templates(exe: &Path, build: &BuildProvenance) -> Option<PathBuf> {
    let root = build.source_root.canonicalize().ok()?;
    let dir = root.join(SHIPPED_TEMPLATES_PATH);
    let output = build
        .output_dir
        .as_deref()
        .and_then(|dir| dir.canonicalize().ok());
    let built_here = exe.starts_with(&root) || output.is_some_and(|output| exe.starts_with(output));
    (built_here && dir.is_dir()).then_some(dir)
}

/// The refusal when [`resolve_templates_dir`] finds no catalog.
pub(crate) fn no_templates_message() -> String {
    let exe = std::env::current_exe()
        .map(|exe| exe.display().to_string())
        .unwrap_or_else(|_| "this bee".to_owned());
    format!(
        "no shipped templates found: {exe} is neither inside an app bundle shipping \
         Contents/Resources/{SHIPPED_TEMPLATES_PATH} nor where the build that made it put it \
         (its checkout or Cargo output directory); pass --templates <dir> or set \
         BEEKEEPER_TEMPLATES_DIR"
    )
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

/// Run `bee pack migrate --from <pack tree> --into <dir> --name <slug>`.
///
/// Turns one directory per role into the flat agents-repository layout,
/// carrying each persona's body and every skill verbatim
/// (`beekeeper_persona::migrate`). Local only: nothing is announced, pushed or
/// re-pointed here. The relay half is two existing commands — `bee packs
/// init --from <dir> --layout flat --expect-source <id>` for a project that
/// already has a source, or plain `bee packs init --from <dir>` for one
/// that does not.
///
/// What it prints is what a reader needs to check the move: the roles, the
/// skills that came with each, and anything the layout change could not
/// carry (today, a persona's frontmatter `skills:` list, which the flat
/// layout derives from the directory instead).
pub fn cmd_migrate(from: &Path, into: &Path, name: &str) -> Result<(), CliError> {
    let report = beekeeper_persona::migrate::convert_pack_tree(from, into, name)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    println!(
        "{}",
        serde_json::json!({
            "from": from.display().to_string(),
            "into": into.display().to_string(),
            "name": name,
            "lead": report.lead,
            "roles": report.roles.iter().map(|role| serde_json::json!({
                "role": role.role,
                "persona": role.persona,
                "skills": role.skills,
                "dropped_keys": role.dropped_keys,
            })).collect::<Vec<_>>(),
            "files": report.files,
        })
    );
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
    let templates =
        resolve_templates_dir(templates).ok_or_else(|| CliError::Usage(no_templates_message()))?;
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

    /// A packaged `bee` finds the templates its bundle ships, from wherever
    /// it is invoked and through a symlink to it; a development build finds
    /// its own checkout only when it runs from inside it; the working
    /// directory, its ancestors and a guessed layout are never a source.
    // The PATH case is a symlink.
    #[cfg(unix)]
    #[test]
    fn templates_resolve_from_the_bundle_or_the_verified_build_root_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let contents = root.join("Beekeeper.app/Contents");
        let exe = contents.join("MacOS/bee");
        write(&exe, "#!/bin/sh\n");
        let shipped = contents.join("Resources/personas/templates");
        std::fs::create_dir_all(&shipped).unwrap();
        let checkout = root.join("checkout");
        let checkout_templates = checkout.join("personas/templates");
        std::fs::create_dir_all(&checkout_templates).unwrap();
        let in_tree = BuildProvenance {
            source_root: checkout.clone(),
            output_dir: None,
        };

        // The bundle wins over a build root it is not inside.
        assert_eq!(
            templates_dir_from(None, None, Some(&exe), Some(&in_tree)),
            Some(shipped.clone())
        );
        // A symlink on PATH resolves to the bundle it points into.
        let link = root.join("bin/bee");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&exe, &link).unwrap();
        assert_eq!(
            templates_dir_from(None, None, Some(&link), None),
            Some(shipped.clone())
        );
        // Explicit and environment overrides win, in that order; an
        // environment value that is not a directory is ignored.
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(
            templates_dir_from(Some(&checkout_templates), Some(&other), Some(&exe), None),
            Some(checkout_templates.clone())
        );
        assert_eq!(
            templates_dir_from(None, Some(&other), Some(&exe), None),
            Some(other)
        );
        assert_eq!(
            templates_dir_from(None, Some(&root.join("missing")), Some(&exe), None),
            Some(shipped)
        );

        // A development build inside its checkout finds that checkout.
        let dev = checkout.join("target/debug/bee");
        write(&dev, "#!/bin/sh\n");
        assert_eq!(
            templates_dir_from(None, None, Some(&dev), Some(&in_tree)),
            Some(checkout_templates)
        );

        // A binary outside every bundle and outside its build root finds
        // nothing — even with `personas/templates` beside it, in a `MacOS`
        // directory that is not a bundle, or in the directories above it.
        let loose = root.join("project/sub/bee");
        write(&loose, "#!/bin/sh\n");
        std::fs::create_dir_all(root.join("personas/templates")).unwrap();
        std::fs::create_dir_all(root.join("project/sub/personas/templates")).unwrap();
        assert_eq!(
            templates_dir_from(None, None, Some(&loose), Some(&in_tree)),
            None
        );
        let fake = root.join("fake/Contents/MacOS/bee");
        write(&fake, "#!/bin/sh\n");
        std::fs::create_dir_all(root.join("fake/Contents/Resources/personas/templates")).unwrap();
        assert_eq!(templates_dir_from(None, None, Some(&fake), None), None);
    }

    /// A build whose Cargo output lives outside its checkout
    /// (`CARGO_TARGET_DIR=/ci-target`, as the gate sets it) finds its
    /// checkout's catalog from exactly that recorded output — its binary and
    /// its test executables — and from nowhere else: not a copy of the same
    /// binary, not another profile's output, not a bundle under the output
    /// missing its resources.
    #[cfg(unix)]
    #[test]
    fn a_build_in_its_recorded_cargo_output_finds_its_checkout_and_a_copy_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let checkout = root.join("woodpecker/repo");
        let checkout_templates = checkout.join("personas/templates");
        std::fs::create_dir_all(&checkout_templates).unwrap();
        let output = root.join("ci-target/debug");
        let out_dir = output.join("build/beekeeper-cli-1a2b3c/out");
        std::fs::create_dir_all(&out_dir).unwrap();
        assert_eq!(cargo_output_dir(&out_dir), Some(output.as_path()));
        let build = BuildProvenance {
            source_root: checkout.clone(),
            output_dir: cargo_output_dir(&out_dir).map(Path::to_path_buf),
        };

        for exe in [
            output.join("bee"),
            output.join("deps/beekeeper_cli-8379c3326b7b4338"),
        ] {
            write(&exe, "#!/bin/sh\n");
            assert_eq!(
                templates_dir_from(None, None, Some(&exe), Some(&build)),
                Some(checkout_templates.clone()),
                "{}",
                exe.display()
            );
        }
        // The same binary copied anywhere else: refused, even beside a
        // `personas/templates` of its own or under a `debug` of another tree.
        for copy in [
            root.join("elsewhere/deps/beekeeper_cli-8379c3326b7b4338"),
            root.join("other-target/debug/deps/beekeeper_cli-8379c3326b7b4338"),
            root.join("ci-target/release/bee"),
        ] {
            write(&copy, "#!/bin/sh\n");
            std::fs::create_dir_all(copy.parent().unwrap().join("personas/templates")).unwrap();
            assert_eq!(
                templates_dir_from(None, None, Some(&copy), Some(&build)),
                None,
                "{}",
                copy.display()
            );
        }
        // A bundle built into the recorded output but missing its resources
        // has no catalog; it never borrows the checkout's.
        let bundled = output.join("bundle/macos/Beekeeper.app/Contents/MacOS/bee");
        write(&bundled, "#!/bin/sh\n");
        assert_eq!(
            templates_dir_from(None, None, Some(&bundled), Some(&build)),
            None
        );
        // A checkout without templates gives nothing, whatever the output.
        let bare = BuildProvenance {
            source_root: root.join("no-templates"),
            output_dir: build.output_dir.clone(),
        };
        std::fs::create_dir_all(&bare.source_root).unwrap();
        assert_eq!(
            templates_dir_from(None, None, Some(&output.join("bee")), Some(&bare)),
            None
        );
    }

    /// Only Cargo's own `OUT_DIR` shape names an output directory, with or
    /// without a target triple; anything else names none.
    #[test]
    fn only_a_cargo_out_dir_names_the_build_output() {
        assert_eq!(
            cargo_output_dir(Path::new("/ci-target/debug/build/beekeeper-cli-1a2b/out")),
            Some(Path::new("/ci-target/debug"))
        );
        assert_eq!(
            cargo_output_dir(Path::new(
                "/ci-target/x86_64-unknown-linux-gnu/release/build/beekeeper-cli-9f/out"
            )),
            Some(Path::new("/ci-target/x86_64-unknown-linux-gnu/release"))
        );
        for other in [
            "/ci-target/debug/build/beekeeper-cli-1a2b",
            "/x/out",
            "/a/notbuild/p/out",
            "out",
        ] {
            assert_eq!(cargo_output_dir(Path::new(other)), None, "{other}");
        }
    }

    /// The production resolver finds a loadable catalog for this build with
    /// no flag and no environment: the checkout it was built from, reached
    /// from wherever Cargo put this test executable.
    #[test]
    fn this_build_resolves_a_loadable_catalog_without_setup() {
        let dir = resolve_templates_dir(None).expect("this build ships templates");
        let catalog = TemplateCatalog::load(&dir, "test").expect("the catalog loads");
        let range = TemplateRange::parse("lead", "^1.0.0").unwrap();
        assert!(catalog.resolve("lead", &range).is_ok());
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
