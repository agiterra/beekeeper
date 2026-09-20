//! What the shipped role templates owe the people who run them.
//!
//! Two live team runs (ledger 178, 179) showed seats spending a third to a
//! half of their effort operating Beekeeper rather than doing the work:
//! learning wire bodies by failed writes, hiring every available role,
//! polling for results, collecting acknowledgements a local preflight
//! demanded, and calling a stale binary. The software answers landed as
//! ledger 180–191; version 1.1.0 of the templates is the text catching up.
//!
//! These are **string tests over shipped prose**. They are necessary and
//! not sufficient: they prove the instruction is present, spelled the way a
//! seat will read it, and that a published version's bytes never moved.
//! They cannot prove a seat behaves differently — that is the Wave 3 control
//! run's job (`docs/UNIFIED_WORK_PLAN.md` § 4).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::compose::{compose_role, ComposeOptions, RoleSource};
use crate::seed::write_agents_repo_seed;
use crate::template::{TemplateCatalog, TemplateRange, TEMPLATE_MD};

/// The templates this build ships, in the working tree.
fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../personas/templates")
        .canonicalize()
        .expect("the shipped template catalog is two levels up")
}

fn catalog() -> TemplateCatalog {
    TemplateCatalog::load(&templates_dir(), "test").expect("the shipped catalog loads")
}

/// Every template that has a 1.1.0 directory: the roles, the working
/// contract and the Pulse fragment. `memory` is deliberately absent — 1.1.0
/// exists only where the text changed (spec § 3.2).
const UPDATED: [&str; 10] = [
    "architect",
    "builder",
    "designer",
    "lead",
    "poker",
    "project-pulse",
    "project-setup",
    "runner",
    "verifier",
    "working-contract",
];

/// Every file under `<name>/<version>/`, relative path to bytes.
fn version_files(name: &str, version: &str) -> BTreeMap<String, Vec<u8>> {
    let root = templates_dir().join(name).join(version);
    let mut out = BTreeMap::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(&root)
                    .expect("under the version directory")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&path).expect("read"));
            }
        }
    }
    out
}

/// The text of every file under `<name>/1.1.0/`, for the prose rules.
fn updated_text() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for name in UPDATED {
        for (rel, bytes) in version_files(name, "1.1.0") {
            let text = String::from_utf8(bytes)
                .unwrap_or_else(|e| panic!("{name}/1.1.0/{rel} is not UTF-8: {e}"));
            out.push((format!("{name}/1.1.0/{rel}"), text));
        }
    }
    assert!(!out.is_empty(), "no 1.1.0 files found");
    out
}

/// A version directory's identity: one digest over every file's path and
/// bytes, sorted by path.
fn digest(name: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    for (rel, bytes) in version_files(name, version) {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    hex::encode(hasher.finalize())
}

/// Sentences, for the rules that must read a prohibition as a prohibition.
fn sentences(text: &str) -> Vec<String> {
    text.split(['.', '\n'])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// A published version's bytes never change: a project pinned to it, and
/// every seat that staged it, must keep reading exactly what it read.
/// 1.1.0 is a new directory beside 1.0.0, never an edit of it.
#[test]
fn the_1_0_0_templates_are_byte_for_byte_what_they_shipped_as() {
    // Update this table only when a version directory is *added*; a change
    // to a line already here is the bug it exists to catch.
    let pinned: [(&str, &str); 11] = [
        (
            "architect",
            "aa7304a08c456bd7cb685fd0efed0d3f37e28e73a6fe28c08da0bc53fb3c4913",
        ),
        (
            "builder",
            "ff94689d52ef094a46a34ac41e592219b1c239fce1badc149e8044bd59a48f69",
        ),
        (
            "designer",
            "d4773c00251908cdd51dc58e0bf82c2931ff5e5a04aa5fba7c723e6c5d14a8d1",
        ),
        (
            "lead",
            "70562cc826639ca215408ff17b6eeb20496ff7eb48a0e360665ffb7dbb62a36c",
        ),
        (
            "memory",
            "011291a3d04a0860599bb7cea756a028efa081db1890977d9b1df3d2e043b613",
        ),
        (
            "poker",
            "242440984d365ef3c12616fbfd9eb95f6685340e347012d1e09e88eebbce69f6",
        ),
        (
            "project-pulse",
            "0a4da01d8c1f56287fedf222a5d6196fc6664e494898626ab523b23b3cd4ca2a",
        ),
        (
            "project-setup",
            "86bfcdc01589b305ae75f58089a38a0b42635fee6903707a500404687a61a74f",
        ),
        (
            "runner",
            "3241be0dab77afa9808470e715902f096523d46574529dd757d97c686b6783cd",
        ),
        (
            "verifier",
            "5480e6520fb6250fd79ef91225a6f9c38063bdee16e41c024dbd454078fbadce",
        ),
        (
            "working-contract",
            "75f390ca18a52e548bfb512e796ff0429f649b921a23f75ab9ca8ab32b73a9fc",
        ),
    ];
    for (name, expected) in pinned {
        assert_eq!(
            digest(name, "1.0.0"),
            expected,
            "{name}/1.0.0 changed; a published version is immutable — add a new version instead"
        );
    }
}

/// Every 1.1.0 template is loadable, current, and composes into a role a
/// seat can be staged from — including the renamed builder skill.
#[test]
fn every_1_1_0_template_is_current_in_the_catalog_and_composes() {
    let catalog = catalog();
    for name in UPDATED {
        let versions = catalog.versions(name);
        assert!(
            versions.iter().any(|t| t.version.to_string() == "1.0.0"),
            "{name} lost its 1.0.0"
        );
        let resolved = catalog
            .resolve(name, &TemplateRange::parse(name, "^1.0.0").expect("range"))
            .unwrap_or_else(|e| panic!("{name}@^1.0.0: {e}"));
        assert_eq!(
            resolved.template.version.to_string(),
            "1.1.0",
            "{name}: a caret range must carry the minor revision to the next hire"
        );
        assert!(resolved.warning.is_none(), "{name}: {resolved:?}");
        assert!(
            templates_dir()
                .join(name)
                .join("1.1.0")
                .join(TEMPLATE_MD)
                .is_file(),
            "{name}/1.1.0 has no {TEMPLATE_MD}"
        );
    }
    // `memory` did not change, so it ships one version and a caret range
    // still answers with it.
    let memory = catalog
        .resolve(
            "memory",
            &TemplateRange::parse("memory", "^1.0.0").expect("range"),
        )
        .expect("memory@^1.0.0");
    assert_eq!(memory.template.version.to_string(), "1.0.0");

    // Composition: seed a project from this catalog and compose every role.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("demo-beekeeper-agents");
    let report = write_agents_repo_seed(&root, &catalog, "demo").expect("seed");
    for role in &report.roles {
        let composed = compose_role(
            &RoleSource::Flat {
                root: root.clone(),
                role: role.clone(),
            },
            &catalog,
            &ComposeOptions::local(format!("roles/{role}")),
        )
        .unwrap_or_else(|e| panic!("{role} composes: {e}"));
        assert!(composed.provenance.warnings.is_empty(), "{role}");
        assert!(!composed.persona.prompt.trim().is_empty(), "{role}");
    }
    let builder = compose_role(
        &RoleSource::Flat {
            root: root.clone(),
            role: "builder".to_owned(),
        },
        &catalog,
        &ComposeOptions::local("roles/builder"),
    )
    .expect("builder composes");
    let skills: Vec<&str> = builder.skills.iter().map(|s| s.name.as_str()).collect();
    assert!(
        skills.contains(&"implement-the-outcome"),
        "the renamed builder skill is missing: {skills:?}"
    );
    assert!(
        !skills.contains(&"brief-is-law"),
        "1.1.0 still carries the old skill name: {skills:?}"
    );
}

/// A new project seeded by this build, and a project seeded by an earlier
/// one, must both end up reading 1.1.0 — the first because the seed writes
/// the newest version's caret, the second because `^1.0.0` admits it.
#[test]
fn a_new_project_seeds_1_1_0_and_an_existing_projects_caret_resolves_to_it() {
    let catalog = catalog();
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("demo-beekeeper-agents");
    write_agents_repo_seed(&root, &catalog, "demo").expect("seed");

    let lead = std::fs::read_to_string(root.join("roles/lead.md")).expect("seeded lead");
    for expected in [
        "![[beekeeper/lead@^1.1.0]]",
        "![[beekeeper/working-contract@^1.1.0]]",
        "![[beekeeper/memory@^1.0.0]]",
        "![[beekeeper/project-pulse@^1.1.0]]",
    ] {
        assert!(
            lead.contains(expected),
            "seeded lead lacks {expected}:\n{lead}"
        );
    }

    // The existing project: the role file an earlier build wrote, byte for
    // byte, composed against today's catalog.
    let old = tmp.path().join("old-beekeeper-agents");
    std::fs::create_dir_all(old.join("roles")).expect("roles dir");
    std::fs::write(
        old.join("team.yml"),
        "schema: beekeeper-team/v1\nname: old\nversion: 0.1.0\nlead: lead\nroles:\n  lead: {}\n",
    )
    .expect("team.yml");
    std::fs::write(
        old.join("roles/lead.md"),
        "---\ndescription: \"An older seed.\"\n---\n\n![[beekeeper/lead@^1.0.0]]\n\n\
         ![[beekeeper/working-contract@^1.0.0]]\n\n![[beekeeper/memory@^1.0.0]]\n\n\
         ![[beekeeper/project-pulse@^1.0.0]]\n",
    )
    .expect("old role");
    let composed = compose_role(
        &RoleSource::Flat {
            root: old.clone(),
            role: "lead".to_owned(),
        },
        &catalog,
        &ComposeOptions::local("roles/lead"),
    )
    .expect("the older seed still composes");
    let resolved: Vec<(&str, &str)> = composed
        .provenance
        .includes
        .iter()
        .map(|i| {
            (
                i.reference.as_str(),
                i.resolved.as_deref().unwrap_or("unresolved"),
            )
        })
        .collect();
    assert_eq!(
        resolved,
        vec![
            ("beekeeper/lead@^1.0.0", "1.1.0"),
            ("beekeeper/working-contract@^1.0.0", "1.1.0"),
            ("beekeeper/memory@^1.0.0", "1.0.0"),
            ("beekeeper/project-pulse@^1.0.0", "1.1.0"),
        ],
        "an existing project's caret ranges must take the minor revision"
    );
    // An exact pin is the way to stay: nothing forces a project forward.
    let pinned = catalog
        .resolve(
            "lead",
            &TemplateRange::parse("lead", "1.0.0").expect("range"),
        )
        .expect("lead@1.0.0 still resolves");
    assert_eq!(pinned.template.version.to_string(), "1.0.0");
}

/// `bee` on a seat's PATH is somebody else's build (ledger § environment
/// facts). Every example names the host-selected binary.
#[test]
fn no_1_1_0_file_names_a_bare_bee_command() {
    for (path, text) in updated_text() {
        for (index, line) in text.lines().enumerate() {
            let scrubbed = line.replace("$BEE", "«bee»");
            let offender = scrubbed
                .split(|c: char| c.is_whitespace() || c == '`' || c == '(')
                .any(|token| token == "bee");
            assert!(
                !offender,
                "{path}:{}: a bare `bee` command — use $BEE\n{line}",
                index + 1
            );
        }
    }
}

/// The wire's verdict space has three decisions
/// (`CodingSessionTeamRefutationDecision`), and the one a 1.0.0 verifier
/// never learned is the one that matters when its input is missing.
#[test]
fn the_verifier_teaches_all_three_refutation_decisions() {
    let text: String = version_files("verifier", "1.1.0")
        .values()
        .map(|b| String::from_utf8_lossy(b).to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    for decision in ["confirmed", "not-refuted", "blocked"] {
        assert!(
            text.contains(decision),
            "verifier 1.1.0 never says {decision}"
        );
    }
}

/// Collecting acknowledgements cost Andy's run six wakes and a reported
/// $9.33 (ledger 179(a)); 183 made the held completion settle by itself.
/// No 1.1.0 text may instruct a role to gather them — a sentence may only
/// mention it to forbid it.
#[test]
fn no_1_1_0_file_instructs_anyone_to_collect_acknowledgements() {
    const COLLECTING: [&str; 5] = ["collect", "wake", "gather", "chase", "poll"];
    const FORBIDDING: [&str; 5] = ["do not", "never", "rather than", "instead of", "without"];
    for (path, text) in updated_text() {
        for sentence in sentences(&text) {
            if !sentence.contains("acknowledg") {
                continue;
            }
            let collecting = COLLECTING.iter().any(|verb| sentence.contains(verb));
            let forbidding = FORBIDDING.iter().any(|neg| sentence.contains(neg));
            assert!(
                !collecting || forbidding,
                "{path}: teaches acknowledgement collection: {sentence}"
            );
        }
    }
}

/// Model names, providers and prices belong to the project's registry and
/// the host's routing answer, never to a prompt that outlives them.
#[test]
fn no_1_1_0_file_encodes_a_model_id_provider_or_price() {
    const BANNED: [&str; 12] = [
        "claude",
        "anthropic",
        "gpt-",
        "openai",
        "opus",
        "sonnet",
        "haiku",
        "gemini",
        "llama",
        "o3-",
        "usd",
        "per token",
    ];
    for (path, text) in updated_text() {
        let lower = text.to_lowercase();
        for banned in BANNED {
            assert!(
                !lower.contains(banned),
                "{path} names {banned:?}; routing is the project's configuration, not prose"
            );
        }
        // A price: a dollar sign in front of a digit. `$BEE` is fine.
        let bytes: Vec<char> = text.chars().collect();
        for (index, c) in bytes.iter().enumerate() {
            if *c == '$' {
                if let Some(next) = bytes.get(index + 1) {
                    assert!(!next.is_ascii_digit(), "{path} quotes a price");
                }
            }
        }
    }
}

/// The instructions the software landings 180–191 replaced: each role's
/// 1.1.0 text must carry the new procedure in the words a seat will search
/// for.
#[test]
fn each_1_1_0_role_carries_the_procedure_its_software_now_supports() {
    let by_name: BTreeMap<String, String> = UPDATED
        .iter()
        .map(|name| {
            let text = version_files(name, "1.1.0")
                .values()
                .map(|b| String::from_utf8_lossy(b).to_lowercase())
                .collect::<Vec<_>>()
                .join("\n");
            ((*name).to_owned(), text)
        })
        .collect();
    let says = |name: &str, needle: &str| {
        assert!(
            by_name[name].contains(needle),
            "{name} 1.1.0 never says {needle:?}"
        );
    };
    // Read the body before you write one (182), everywhere it is written.
    says("working-contract", "--example");
    says("builder", "--example");
    says("verifier", "--example");
    says("lead", "--example");
    // Lead: bounded hiring, routed first, the verification input, and a
    // completion that settles itself (180, 182, 183, 184, 190).
    says("lead", "not a staffing plan");
    says("lead", "--class");
    says("lead", "modelnotice");
    says("lead", "--verifies");
    says("lead", "--checkout");
    says("lead", "run-status");
    says("lead", "pending");
    // Builder: the tests are part of the work, and red before green.
    says("builder", "part of the change");
    says("builder", "fails against the unfixed state");
    // Architect: a question, and an end to it.
    says("architect", "structural question");
    says("architect", "reconsideration trigger");
    // Designer: the user's decisions, steps and interruptions.
    says("designer", "decisions");
    says("designer", "interruptions");
    says("designer", "command-line output");
    // Runner: judgment, and a reusable action left behind.
    says("runner", "actions.yml");
    says("runner", "no model turn");
    // Poker: a budget and an evidence standard, and no repairs.
    says("poker", "exploration budget");
    says("poker", "evidence standard");
    says("poker", "coverage limits");
    // Setup: readiness that was exercised.
    says("project-setup", "exit status");
    says("project-setup", "is a proposal");
    // Pulse: still re-read at the end of a turn, but only what bears on the
    // task. Nothing here may claim software delivers these entries.
    says("project-pulse", "end of each turn");
    says("project-pulse", "bears on your current");
}
