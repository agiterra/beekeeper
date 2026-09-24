//! Lead 1.2.3: what control run 5 (2026-09-24) showed the lead fumbling.
//!
//! The lead's seat recorded four CLI usage errors while binding evidence and
//! completing (3m10s from the green host result to `mission.completed`), and
//! ran the verify action on the seed commit before anything was built. 1.2.3
//! gives the work commands in order with every flag the lead supplies. These
//! tests prove a seat composed from a freshly seeded project receives each
//! shape verbatim; like the rest of the prose tests, they cannot prove the
//! seat then uses them.

use std::path::{Path, PathBuf};

use crate::compose::{compose_role, ComposeOptions, RoleSource};
use crate::seed::write_agents_repo_seed;
use crate::template::TemplateCatalog;

fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../personas/templates")
        .canonicalize()
        .expect("the shipped template catalog is two levels up")
}

/// Each command shape the lead needs, in the order it runs them, exactly as
/// the composed persona must carry it.
const LEAD_COMMAND_SHAPES: [&str; 6] = [
    "`$BEE sessions work adopt --plan <plan path> --commit <agents commit> --agents-repo <dir> \
     --channel <ch> --session-ref <ref>`",
    "`$BEE sessions work bind evidence --channel <ch> --session-ref <ref> --declaration <decl> \
     --criteria <id>[,<id>] --artifact <sha> --evidence verdict:<disposition id> \
     --agents-repo <dir>`",
    "`$BEE sessions work bind evidence --channel <ch> --session-ref <ref> --declaration <decl> \
     --criteria <id> --artifact <sha> --evidence action_result:<46023 id> --agents-repo <dir>`",
    "`$BEE sessions work bind ref --channel <ch> --session-ref <ref> --declaration <decl> \
     --criteria <id> --commit <sha> --observed-by <46023 id> --agents-repo <dir>`",
    "`$BEE sessions work status --channel <ch> --session-ref <ref> --agents-repo <dir>`",
    "`$BEE sessions complete --channel <ch> --session-ref <ref> --genesis <genesis id> \
     --agents-repo <dir> --body '{\"assignmentRefs\":[\"<id>\"],\"landedShas\":[\"<sha>\"],\
     \"followUps\":[],\"summary\":\"<one sentence>\"}'`",
];

const LEAD_PACE_RULES: [&str; 4] = [
    "Run the verify action only on a delivered commit, after the delivery ref has moved to it, \
     never on the seed commit or a branch.",
    "On a host-result wake for a result you have already used, do nothing.",
    "If a host-result wake carries `autoEvidence`, the criteria it lists are already bound; do \
     not bind them again.",
    "Hire the verifier seat when you hire the builder, so it is warm; bind its assignment with \
     `--verifies <report id>` when the builder's report lands.",
];

/// Lead 1.2.4: the facts the lead once spent 42% of its tool calls finding
/// are named by the provider's situation card, and the role text says so
/// instead of telling the lead to look them up.
const LEAD_SITUATION_CARD_RULES: [&str; 2] = [
    "The situation card in your first turn names `<ch>` (the channel), `<ref>` (the session \
     ref), the genesis id, `<dir>` (the agents-repository checkout), the plan path and agents \
     commit, each criterion's proof and the roster;",
    "The situation card in the session's first message names this project's\nroster; choose \
     whom the work needs from it.",
];

#[test]
fn the_composed_lead_carries_every_work_command_shape_in_order() {
    let catalog = TemplateCatalog::load(&templates_dir(), "test").expect("the shipped catalog");
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("pace-beekeeper-agents");
    write_agents_repo_seed(&root, &catalog, "pace").expect("seed");
    let composed = compose_role(
        &RoleSource::Flat {
            root,
            role: "lead".to_owned(),
        },
        &catalog,
        &ComposeOptions::local("roles/lead"),
    )
    .expect("the seeded lead composes");
    assert!(composed.provenance.warnings.is_empty(), "{composed:?}");
    let lead_include = composed
        .provenance
        .includes
        .iter()
        .find(|include| include.reference.starts_with("beekeeper/lead@"))
        .expect("the lead includes its role template");
    assert_eq!(lead_include.resolved.as_deref(), Some("1.2.4"));

    let prompt = &composed.persona.prompt;
    let mut last = 0;
    for shape in LEAD_COMMAND_SHAPES {
        let at = prompt
            .find(shape)
            .unwrap_or_else(|| panic!("the composed lead lacks {shape}"));
        assert!(at >= last, "{shape} is out of order");
        last = at;
    }
    for rule in LEAD_PACE_RULES {
        assert!(prompt.contains(rule), "the composed lead lacks {rule:?}");
    }
    assert!(
        prompt.contains(LEAD_SITUATION_CARD_RULES[0]),
        "the composed lead lacks {:?}",
        LEAD_SITUATION_CARD_RULES[0]
    );
    let hire = composed
        .skills
        .iter()
        .find(|skill| skill.name == "hire")
        .expect("the composed lead carries its hire skill");
    let hire_body =
        std::fs::read_to_string(hire.dir.join("SKILL.md")).expect("the hire skill reads");
    assert!(
        hire_body.contains(LEAD_SITUATION_CARD_RULES[1]),
        "the hire skill lacks {:?}",
        LEAD_SITUATION_CARD_RULES[1]
    );
    assert!(
        !hire_body.contains("Discover this project's agents"),
        "1.2.4's hire skill must not tell the lead to look up what the card names"
    );
}
