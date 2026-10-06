//! Every refusal [`super::parse_sandbox_yml`] can emit, asserted by code
//! rather than by message, plus the reclaim plan the same declaration drives.
//!
//! These are pure string-to-result tests on purpose: `buzz-core` carries no
//! dev-dependencies, so there is no `tempfile` here and no filesystem to set
//! up. The one test that reads a real file reads this repository's own
//! `sandbox.yml`, which is the point of it.

use super::*;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

/// A manifest with the right schema and whatever body a test needs.
fn manifest(body: &str) -> String {
    format!("schema: {SANDBOX_SCHEMA}\npool: beekeeper-build\n{body}")
}

/// A manifest holding exactly one entry, written as a YAML flow mapping.
fn one(entry: &str) -> String {
    manifest(&format!("entries:\n  - {entry}\n"))
}

/// The code a manifest is refused with.
fn refusal(text: &str) -> &'static str {
    parse_sandbox_yml(text)
        .expect_err("this manifest should have been refused")
        .code
}

/// The plan a manifest parses to.
fn accepted(text: &str) -> SandboxPlan {
    parse_sandbox_yml(text).expect("this manifest should have parsed")
}

fn warning_codes(plan: &SandboxPlan) -> Vec<&'static str> {
    plan.warnings.iter().map(|warning| warning.code).collect()
}

// ── the file this repository actually ships ──────────────────────────────

#[test]
fn the_repositorys_own_manifest_parses() {
    let plan = load_sandbox_manifest(&repo_root())
        .expect("this repository's sandbox.yml must parse")
        .expect("this repository ships a sandbox.yml");
    assert_eq!(plan.pool.as_deref(), Some("beekeeper-build"));
    assert!(
        plan.entries.len() >= 8,
        "the manifest should cover the build state this repository actually has, got {} entries",
        plan.entries.len()
    );
    assert_eq!(plan.manifest_sha256.len(), 64);
}

/// The fallback can never free more than the project declares, and the
/// manifest can never silently drop a directory the old closed list freed.
#[test]
fn the_fallback_list_is_a_subset_of_what_this_repository_declares() {
    let plan = load_sandbox_manifest(&repo_root())
        .expect("sandbox.yml must parse")
        .expect("sandbox.yml must exist");
    let declared = reclaim_plan(Some(&plan));
    for path in crate::worktree_lifecycle::RECLAIMABLE_BUILD_DIRS {
        assert_eq!(
            declared.action_for(path),
            ReclaimAction::DeleteDirectory,
            "{path} is in the no-manifest fallback list, so this repository's sandbox.yml must \
             declare it reclaimable too, or reclaim would quietly free less than it used to"
        );
    }
}

// ── shape and schema ─────────────────────────────────────────────────────

#[test]
fn a_file_that_is_not_a_mapping_is_refused() {
    assert_eq!(refusal("- just a list\n"), SANDBOX_MANIFEST_SHAPE);
}

#[test]
fn a_schema_this_parser_does_not_read_is_refused() {
    assert_eq!(
        refusal("schema: beekeeper-sandbox/v2\nentries: []\n"),
        SANDBOX_MANIFEST_SCHEMA
    );
}

#[test]
fn a_typo_in_a_top_level_key_is_refused_not_defaulted() {
    assert_eq!(
        refusal("schema: beekeeper-sandbox/v1\nentrys: []\n"),
        SANDBOX_MANIFEST_UNKNOWN_KEY
    );
}

#[test]
fn a_typo_in_an_entry_key_is_refused_not_defaulted() {
    assert_eq!(
        refusal(&one("{ path: target, kind: clone, reclaimm: delete }")),
        SANDBOX_MANIFEST_UNKNOWN_KEY
    );
}

#[test]
fn an_entry_with_no_kind_is_refused() {
    assert_eq!(
        refusal(&one("{ path: target }")),
        SANDBOX_MANIFEST_KIND_UNKNOWN
    );
}

#[test]
fn a_kind_this_parser_does_not_know_is_refused() {
    assert_eq!(
        refusal(&one("{ path: target, kind: hardlink }")),
        SANDBOX_MANIFEST_KIND_UNKNOWN
    );
}

#[test]
fn an_empty_entry_list_is_legal() {
    let plan = accepted("schema: beekeeper-sandbox/v1\nentries: []\n");
    assert!(plan.entries.is_empty());
    assert!(plan.pool.is_none());
}

#[test]
fn a_file_with_no_entries_key_is_legal() {
    assert!(accepted("schema: beekeeper-sandbox/v1\n")
        .entries
        .is_empty());
}

#[test]
fn sixty_five_entries_are_refused() {
    let body = (0..=MAX_SANDBOX_ENTRIES)
        .map(|index| format!("  - {{ path: build/d{index}, kind: copy, reclaim: delete }}\n"))
        .collect::<String>();
    assert_eq!(
        refusal(&manifest(&format!("entries:\n{body}"))),
        SANDBOX_MANIFEST_TOO_MANY_ENTRIES
    );
}

// ── the donor is never in the file ───────────────────────────────────────

#[test]
fn an_entry_that_names_a_donor_is_refused_by_name() {
    for key in RESERVED_DONOR_KEYS {
        let text = one(&format!(
            "{{ path: target, kind: clone, {key}: /Users/someone/checkout }}"
        ));
        assert_eq!(
            refusal(&text),
            SANDBOX_MANIFEST_DONOR_NAMED,
            "entry key {key:?} should be refused as naming a donor, not as a typo"
        );
    }
}

#[test]
fn a_top_level_key_that_names_a_donor_is_refused_by_name() {
    assert_eq!(
        refusal("schema: beekeeper-sandbox/v1\ndonor: /Users/someone/checkout\nentries: []\n"),
        SANDBOX_MANIFEST_DONOR_NAMED
    );
}

// ── path rules ───────────────────────────────────────────────────────────

#[test]
fn an_empty_path_is_refused() {
    assert_eq!(
        refusal(&one("{ path: \"   \", kind: clone, reclaim: delete }")),
        SANDBOX_MANIFEST_PATH_EMPTY
    );
}

#[test]
fn an_absolute_path_is_refused() {
    assert_eq!(
        refusal(&one("{ path: /etc, kind: copy, reclaim: delete }")),
        SANDBOX_MANIFEST_PATH_ABSOLUTE
    );
}

#[test]
fn a_path_that_climbs_out_of_the_checkout_is_refused() {
    assert_eq!(
        refusal(&one("{ path: ../sibling, kind: copy, reclaim: delete }")),
        SANDBOX_MANIFEST_PATH_ESCAPES
    );
}

#[test]
fn a_path_that_climbs_out_mid_string_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ path: target/../../etc, kind: copy, reclaim: delete }"
        )),
        SANDBOX_MANIFEST_PATH_ESCAPES
    );
}

#[test]
fn the_whole_checkout_is_not_an_entry() {
    assert_eq!(
        refusal(&one("{ path: \".\", kind: clone, reclaim: delete }")),
        SANDBOX_MANIFEST_PATH_IS_ROOT
    );
}

#[test]
fn nothing_under_dot_git_may_be_seeded_or_reclaimed() {
    assert_eq!(
        refusal(&one("{ path: .git/objects, kind: copy, reclaim: delete }")),
        SANDBOX_MANIFEST_PATH_IS_GIT
    );
}

#[test]
fn a_leading_current_directory_is_tolerated() {
    let plan = accepted(&one("{ path: ./target, kind: clone, reclaim: delete }"));
    assert_eq!(plan.entries.len(), 1);
}

#[test]
fn one_path_declared_twice_is_refused() {
    let text = manifest(
        "entries:\n  - { path: target, kind: clone, reclaim: delete }\n  \
         - { path: target, kind: copy, reclaim: delete }\n",
    );
    assert_eq!(refusal(&text), SANDBOX_MANIFEST_PATH_DUPLICATE);
}

#[test]
fn an_entry_nested_inside_another_is_refused() {
    let text = manifest(
        "entries:\n  - { path: target, kind: clone, reclaim: delete }\n  \
         - { path: target/debug, kind: copy, reclaim: delete }\n",
    );
    assert_eq!(refusal(&text), SANDBOX_MANIFEST_PATH_NESTED);
}

#[test]
fn a_run_that_produces_a_path_inside_another_entry_is_refused() {
    let text = manifest(
        "entries:\n  - { path: target, kind: clone, reclaim: delete }\n  \
         - { kind: run, recipe: stubs, produces: [target/sidecars], reclaim: delete }\n",
    );
    assert_eq!(refusal(&text), SANDBOX_MANIFEST_PATH_NESTED);
}

#[test]
fn sibling_paths_that_merely_share_a_prefix_are_fine() {
    let text = manifest(
        "entries:\n  - { path: target, kind: clone, reclaim: delete }\n  \
         - { path: desktop/src-tauri/target, kind: clone, reclaim: delete }\n  \
         - { path: node_modules, kind: clone, reclaim: delete }\n  \
         - { path: desktop/node_modules, kind: clone, reclaim: delete }\n",
    );
    assert_eq!(accepted(&text).entries.len(), 4);
}

// ── the option matrix, and the two rules that are structural ─────────────

#[test]
fn an_option_on_the_wrong_kind_names_both() {
    let refused = parse_sandbox_yml(&one(
        "{ path: target, kind: copy, on_unsupported: refuse, reclaim: delete }",
    ))
    .expect_err("on_unsupported is a clone option");
    assert_eq!(refused.code, SANDBOX_MANIFEST_OPTION_NOT_LEGAL);
    assert!(
        refused.message.contains("copy") && refused.message.contains("clone"),
        "the message should name the kind that has it and the kind that does not: {}",
        refused.message
    );
}

#[test]
fn a_symlink_entry_may_not_name_reclaim_delete() {
    assert_eq!(
        refusal(&one(
            "{ path: build/shared, kind: symlink, reclaim: delete }"
        )),
        SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK
    );
}

#[test]
fn a_symlink_entry_may_not_name_reclaim_never_either() {
    // Not even the harmless-looking value: the key does not exist on a link,
    // and accepting it would teach a reader that it sometimes does.
    assert_eq!(
        refusal(&one(
            "{ path: build/shared, kind: symlink, reclaim: never }"
        )),
        SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK
    );
}

#[test]
fn a_share_entry_may_not_name_reclaim_delete() {
    assert_eq!(
        refusal(&one(
            "{ path: .hermit/rust, kind: share, id: cargo-home, lock: none, reclaim: delete }"
        )),
        SANDBOX_MANIFEST_RECLAIM_THROUGH_LINK
    );
}

#[test]
fn rewriting_a_shared_pools_file_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ path: .hermit/rust, kind: share, id: cargo-home, lock: none, rewrite: [config] }"
        )),
        SANDBOX_MANIFEST_REWRITE_THROUGH_LINK
    );
}

#[test]
fn rewriting_through_a_whole_directory_link_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ path: build/deps, kind: symlink, link: self, rewrite: [state.json] }"
        )),
        SANDBOX_MANIFEST_REWRITE_THROUGH_LINK
    );
}

#[test]
fn materialize_without_entry_linking_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ path: build/deps, kind: symlink, link: self, materialize: [state.json] }"
        )),
        SANDBOX_MANIFEST_MATERIALIZE_WITHOUT_ENTRIES
    );
}

#[test]
fn a_run_entry_that_claims_a_destination_is_refused() {
    assert_eq!(
        refusal(&one("{ kind: run, recipe: stubs, path: target }")),
        SANDBOX_MANIFEST_RUN_HAS_PATH
    );
}

// ── the link that would eat the pnpm store ───────────────────────────────

#[test]
fn a_naive_symlink_of_node_modules_is_refused_and_says_why() {
    let refused = parse_sandbox_yml(&one("{ path: node_modules, kind: symlink }"))
        .expect_err("a symlinked node_modules purges the shared store");
    assert_eq!(refused.code, SANDBOX_MANIFEST_LINK_UNSAFE);
    assert!(
        refused.message.contains("kind: clone") && refused.message.contains("kind: run"),
        "the refusal must spell the two expressions that work: {}",
        refused.message
    );
}

#[test]
fn an_entry_linked_node_modules_without_the_rewrite_is_refused() {
    assert_eq!(
        refusal(&one("{ path: node_modules, kind: symlink, link: entries }")),
        SANDBOX_MANIFEST_LINK_UNSAFE
    );
}

#[test]
fn an_entry_linked_node_modules_with_the_state_file_rewritten_is_accepted() {
    let plan = accepted(&one("{ path: node_modules, kind: symlink, link: entries, \
         rewrite: [\".pnpm-workspace-state-v1.json\"] }"));
    assert_eq!(plan.entries.len(), 1);
}

#[test]
fn a_shared_node_modules_pool_is_refused_however_it_is_linked() {
    // `share` has no `rewrite`, so there is no shape of it that survives
    // pnpm's absolute-path state. The refusal is therefore unconditional.
    for link in ["self", "entries"] {
        let text = one(&format!(
            "{{ path: node_modules, kind: share, id: modules, lock: shared, link: {link} }}"
        ));
        assert_eq!(refusal(&text), SANDBOX_MANIFEST_LINK_UNSAFE);
    }
}

#[test]
fn a_cloned_node_modules_is_the_accepted_shape() {
    let plan = accepted(&one(
        "{ path: node_modules, kind: clone, on_unsupported: copy, \
         rewrite: [\".pnpm-workspace-state-v1.json\"], reclaim: delete }",
    ));
    assert_eq!(plan.entries.len(), 1);
}

// ── share ────────────────────────────────────────────────────────────────

#[test]
fn a_share_entry_without_a_declared_lock_is_refused() {
    assert_eq!(
        refusal(&one("{ path: .hermit/rust, kind: share, id: cargo-home }")),
        SANDBOX_MANIFEST_SHARE_LOCK_MISSING
    );
}

#[test]
fn a_share_entry_without_an_id_is_refused() {
    assert_eq!(
        refusal(&one("{ path: .hermit/rust, kind: share, lock: none }")),
        SANDBOX_MANIFEST_SHARE_ID
    );
}

#[test]
fn a_share_pool_id_that_is_a_path_is_refused() {
    for id in ["../escape", "a/b", "Upper", "with_underscore", ""] {
        let text = one(&format!(
            "{{ path: .hermit/rust, kind: share, id: \"{id}\", lock: none }}"
        ));
        assert_eq!(
            refusal(&text),
            SANDBOX_MANIFEST_SHARE_ID,
            "pool id {id:?} is not a slug and must be refused"
        );
    }
}

#[test]
fn a_share_entry_with_no_pool_declared_is_refused() {
    let text = "schema: beekeeper-sandbox/v1\nentries:\n  \
                - { path: .hermit/rust, kind: share, id: cargo-home, lock: none }\n";
    assert_eq!(refusal(text), SANDBOX_MANIFEST_POOL_SLUG);
}

#[test]
fn a_pool_that_is_a_path_is_refused() {
    let text = "schema: beekeeper-sandbox/v1\npool: /srv/cache\nentries: []\n";
    assert_eq!(refusal(text), SANDBOX_MANIFEST_POOL_SLUG);
}

// ── run ──────────────────────────────────────────────────────────────────

#[test]
fn a_run_entry_with_neither_recipe_nor_script_is_refused() {
    assert_eq!(
        refusal(&one("{ kind: run, produces: [out] }")),
        SANDBOX_MANIFEST_RUN_SHAPE
    );
}

#[test]
fn a_run_entry_with_both_recipe_and_script_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ kind: run, recipe: stubs, script: scripts/stubs.sh }"
        )),
        SANDBOX_MANIFEST_RUN_SHAPE
    );
}

#[test]
fn a_recipe_name_that_could_read_as_an_option_is_refused() {
    for recipe in ["--version", "-n"] {
        let text = one(&format!("{{ kind: run, recipe: \"{recipe}\" }}"));
        assert_eq!(
            refusal(&text),
            SANDBOX_MANIFEST_RUN_RECIPE_NAME,
            "recipe {recipe:?} must never be able to read as an option"
        );
    }
}

#[test]
fn a_recipe_name_with_a_shell_metacharacter_is_refused() {
    assert_eq!(
        refusal(&one("{ kind: run, recipe: \"ci; rm -rf /\" }")),
        SANDBOX_MANIFEST_RUN_RECIPE_NAME
    );
}

#[test]
fn the_repositorys_own_private_recipe_name_is_accepted() {
    assert_eq!(
        accepted(&one("{ kind: run, recipe: _ensure-sidecar-stubs }"))
            .entries
            .len(),
        1
    );
}

#[test]
fn a_script_outside_the_scripts_directory_is_refused() {
    for script in ["bin/evil.sh", "scripts", "desktop/scripts/x.sh"] {
        let text = one(&format!("{{ kind: run, script: {script} }}"));
        assert_eq!(
            refusal(&text),
            SANDBOX_MANIFEST_RUN_SCRIPT_PATH,
            "script {script:?} is not under a permitted root"
        );
    }
}

#[test]
fn a_script_that_climbs_out_is_refused_by_the_path_rules() {
    assert_eq!(
        refusal(&one("{ kind: run, script: scripts/../../evil.sh }")),
        SANDBOX_MANIFEST_PATH_ESCAPES
    );
}

#[test]
fn a_run_entrys_produces_paths_obey_every_path_rule() {
    for (produced, expected) in [
        ("/etc", SANDBOX_MANIFEST_PATH_ABSOLUTE),
        ("../sibling", SANDBOX_MANIFEST_PATH_ESCAPES),
        (".git/hooks", SANDBOX_MANIFEST_PATH_IS_GIT),
        ("\"   \"", SANDBOX_MANIFEST_PATH_EMPTY),
    ] {
        let text = one(&format!(
            "{{ kind: run, recipe: stubs, produces: [{produced}], reclaim: delete }}"
        ));
        assert_eq!(refusal(&text), expected, "produces {produced:?}");
    }
}

// ── env ──────────────────────────────────────────────────────────────────

#[test]
fn an_env_name_that_is_not_a_variable_name_is_refused() {
    for name in ["lower", "1LEADING", "has-dash", ""] {
        let text = one(&format!(
            "{{ path: target, kind: clone, reclaim: delete, env: {{ \"{name}\": \"x\" }} }}"
        ));
        assert_eq!(
            refusal(&text),
            SANDBOX_MANIFEST_ENV_NAME,
            "env name {name:?} must be refused"
        );
    }
}

#[test]
fn an_env_from_host_name_that_is_not_a_variable_name_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ kind: run, recipe: stubs, env_from_host: [\"lower\"] }"
        )),
        SANDBOX_MANIFEST_ENV_NAME
    );
}

#[test]
fn a_name_in_both_env_and_env_from_host_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ kind: run, recipe: stubs, env: { TOKEN_HOME: \"/tmp\" }, \
             env_from_host: [TOKEN_HOME] }"
        )),
        SANDBOX_MANIFEST_ENV_DUPLICATE
    );
}

#[test]
fn a_name_listed_twice_in_env_from_host_is_refused() {
    assert_eq!(
        refusal(&one(
            "{ kind: run, recipe: stubs, env_from_host: [HOME_DIR, HOME_DIR] }"
        )),
        SANDBOX_MANIFEST_ENV_DUPLICATE
    );
}

// ── an entry that declares nothing ───────────────────────────────────────

#[test]
fn an_entry_that_neither_seeds_nor_reclaims_is_refused() {
    assert_eq!(
        refusal(&one("{ path: mobile/build, kind: copy, seed: never }")),
        SANDBOX_MANIFEST_ENTRY_INERT
    );
    assert_eq!(
        refusal(&one(
            "{ path: mobile/build, kind: clone, seed: never, reclaim: never }"
        )),
        SANDBOX_MANIFEST_ENTRY_INERT
    );
}

#[test]
fn a_reclaim_only_entry_is_legal() {
    let plan = accepted(&one(
        "{ path: mobile/build, kind: copy, seed: never, reclaim: delete }",
    ));
    assert!(!plan.entries[0].seeds());
}

// ── warnings: surfaced, never fatal ──────────────────────────────────────

#[test]
fn a_secret_shaped_env_literal_warns_and_does_not_refuse() {
    let plan = accepted(&one(
        "{ kind: run, recipe: stubs, env: { REGISTRY_TOKEN: \"abc\" } }",
    ));
    assert!(warning_codes(&plan).contains(&WARN_SANDBOX_ENV_SECRET_SHAPED));
}

#[test]
fn a_long_opaque_env_literal_warns_too() {
    let plan = accepted(&one(
        "{ kind: run, recipe: stubs, env: { BUILD_SEED: \"aB3dEf7hIjK2mNoPqR5tUvWxYz01234567\" } }",
    ));
    assert!(warning_codes(&plan).contains(&WARN_SANDBOX_ENV_SECRET_SHAPED));
}

#[test]
fn an_ordinary_env_literal_warns_about_nothing() {
    let plan = accepted(&one("{ path: node_modules, kind: clone, reclaim: delete, \
         env: { PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN: \"false\" } }"));
    assert!(!warning_codes(&plan).contains(&WARN_SANDBOX_ENV_SECRET_SHAPED));
}

#[test]
fn a_lock_free_share_of_a_cargo_target_dir_warns() {
    let plan = accepted(&one(
        "{ path: target, kind: share, id: cargo-target, lock: none }",
    ));
    let warning = plan
        .warnings
        .iter()
        .find(|warning| warning.code == WARN_SANDBOX_SHARE_CARGO_TARGET)
        .expect("sharing a target directory must disclose what it costs");
    assert!(
        warning.message.contains("build lock"),
        "the warning must name the cost, not just flag the entry: {}",
        warning.message
    );
}

#[test]
fn sharing_cargo_home_does_not_warn_about_the_build_lock() {
    // The registry locks per file and is read-mostly, so this is the one
    // path-level share that is the right default rather than a hazard.
    let plan = accepted(&one(
        "{ path: .hermit/rust, kind: share, id: cargo-home, lock: none }",
    ));
    assert!(!warning_codes(&plan).contains(&WARN_SANDBOX_SHARE_CARGO_TARGET));
}

#[test]
fn seeding_an_env_file_always_warns_so_the_receipt_says_so() {
    let plan = accepted(&one("{ path: .env, kind: copy, required: false }"));
    assert!(warning_codes(&plan).contains(&WARN_SANDBOX_SEEDS_ENV));
}

#[test]
fn a_reclaim_only_env_entry_does_not_claim_to_seed_one() {
    let plan = accepted(&one(
        "{ path: .env.local, kind: copy, seed: never, reclaim: delete }",
    ));
    assert!(!warning_codes(&plan).contains(&WARN_SANDBOX_SEEDS_ENV));
}

#[test]
fn a_clone_that_may_fall_back_to_a_copy_warns() {
    let plan = accepted(&one(
        "{ path: node_modules, kind: clone, on_unsupported: copy, reclaim: delete }",
    ));
    assert!(warning_codes(&plan).contains(&WARN_SANDBOX_CLONE_FALLBACK_COPY));
}

#[test]
fn a_clone_that_refuses_instead_does_not_warn() {
    let plan = accepted(&one(
        "{ path: target, kind: clone, on_unsupported: refuse, reclaim: delete }",
    ));
    assert!(!warning_codes(&plan).contains(&WARN_SANDBOX_CLONE_FALLBACK_COPY));
}

// ── loading ──────────────────────────────────────────────────────────────

#[test]
fn a_missing_manifest_reads_as_no_manifest_not_an_error() {
    let loaded = load_sandbox_manifest_with(Path::new("/nowhere"), |_| Ok(None))
        .expect("an absent manifest is not an error");
    assert!(loaded.is_none());
}

#[test]
fn a_manifest_that_is_present_and_wrong_is_refused_never_skipped() {
    let refused = load_sandbox_manifest_with(Path::new("/nowhere"), |_| {
        Ok(Some((32, "schema: nope\nentries: []\n".to_owned())))
    })
    .expect_err("a manifest that is present and wrong must be refused");
    assert_eq!(refused.code, SANDBOX_MANIFEST_SCHEMA);
}

#[test]
fn a_manifest_over_the_ceiling_is_refused_before_it_is_read() {
    let refused = load_sandbox_manifest_with(Path::new("/nowhere"), |_| {
        Ok(Some((MAX_SANDBOX_YML_BYTES + 1, String::new())))
    })
    .expect_err("a manifest over the ceiling must be refused");
    assert_eq!(refused.code, SANDBOX_MANIFEST_TOO_LARGE);
}

#[test]
fn a_manifest_that_cannot_be_read_is_refused_not_treated_as_absent() {
    let refused = load_sandbox_manifest_with(Path::new("/nowhere"), |_| {
        Err(std::io::Error::other("disk is on fire"))
    })
    .expect_err("an unreadable manifest must be refused");
    assert_eq!(refused.code, SANDBOX_MANIFEST_UNREADABLE);
}

#[test]
fn the_manifest_is_looked_for_at_the_root_of_the_checkout() {
    assert_eq!(
        sandbox_manifest_path(Path::new("/checkout")),
        Path::new("/checkout/sandbox.yml")
    );
}

#[test]
fn the_digest_names_which_manifest_ran() {
    let first = accepted(&one("{ path: target, kind: clone, reclaim: delete }"));
    let second = accepted(&one("{ path: target, kind: copy, reclaim: delete }"));
    assert_ne!(first.manifest_sha256, second.manifest_sha256);
    assert_eq!(first.manifest_sha256.len(), 64);
}

// ── reclaim: one declaration, the inverse verb ───────────────────────────

#[test]
fn a_project_with_no_manifest_reclaims_the_closed_list() {
    let plan = reclaim_plan(None);
    assert_eq!(plan.source, ReclaimSource::ClosedListFallback);
    let paths: Vec<&str> = plan.targets.iter().map(|t| t.path.as_str()).collect();
    assert_eq!(paths, crate::worktree_lifecycle::RECLAIMABLE_BUILD_DIRS);
    assert!(plan.targets.iter().all(|t| t.declared.is_none()));
}

#[test]
fn a_symlink_entry_reclaims_as_unlink_only_never_as_a_delete() {
    let parsed = accepted(&one("{ path: build/deps, kind: symlink }"));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(plan.action_for("build/deps"), ReclaimAction::UnlinkOnly);
    assert!(
        plan.targets
            .iter()
            .all(|t| !matches!(t.action, ReclaimAction::DeleteDirectory)),
        "a link must never be reclaimed by deleting the directory it points at"
    );
}

#[test]
fn a_share_entry_is_never_reclaimed_at_all() {
    let parsed = accepted(&one(
        "{ path: .hermit/rust, kind: share, id: cargo-home, lock: none }",
    ));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(plan.action_for(".hermit/rust"), ReclaimAction::Keep);
    assert!(
        plan.actionable().is_empty(),
        "emptying a pool would take it from every other sandbox of the project"
    );
}

#[test]
fn a_clone_entrys_bytes_are_marked_shared_with_the_donor() {
    let parsed = accepted(&one("{ path: target, kind: clone, reclaim: delete }"));
    let plan = reclaim_plan(Some(&parsed));
    let target = plan
        .targets
        .iter()
        .find(|t| t.path == "target")
        .expect("the entry should be in the plan");
    assert!(
        target.bytes_are_shared,
        "a clone shares its blocks with the source, so a size measured here is an upper bound"
    );
    assert_eq!(target.declared, Some(SeedKind::Clone));
}

#[test]
fn a_copy_entrys_bytes_are_its_own() {
    let parsed = accepted(&one("{ path: mobile/build, kind: copy, reclaim: delete }"));
    let plan = reclaim_plan(Some(&parsed));
    assert!(!plan.targets[0].bytes_are_shared);
}

#[test]
fn a_reclaim_never_entry_is_kept() {
    let parsed = accepted(&one("{ path: .env, kind: copy, required: false }"));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(plan.action_for(".env"), ReclaimAction::Keep);
}

#[test]
fn a_runs_produced_paths_are_what_reclaim_acts_on() {
    let parsed = accepted(&one("{ kind: run, recipe: _ensure-sidecar-stubs, \
         produces: [desktop/src-tauri/binaries], reclaim: delete }"));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(
        plan.action_for("desktop/src-tauri/binaries"),
        ReclaimAction::DeleteDirectory
    );
}

#[test]
fn a_path_no_plan_names_is_kept() {
    let parsed = accepted(&one("{ path: target, kind: clone, reclaim: delete }"));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(plan.action_for("src"), ReclaimAction::Keep);
    assert_eq!(reclaim_plan(None).action_for("src"), ReclaimAction::Keep);
}

#[test]
fn the_plan_says_which_manifest_it_came_from() {
    let parsed = accepted(&one("{ path: target, kind: clone, reclaim: delete }"));
    let plan = reclaim_plan(Some(&parsed));
    assert_eq!(
        plan.source,
        ReclaimSource::Manifest {
            sha256: parsed.manifest_sha256.clone()
        }
    );
}
