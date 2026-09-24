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

const LEAD_PACE_RULES: [&str; 2] = [
    "Run the verify action only on a delivered commit, after the delivery ref has moved to it, \
     never on the seed commit or a branch.",
    "On a host-result wake for a result you have already used, do nothing.",
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
    assert_eq!(lead_include.resolved.as_deref(), Some("1.2.3"));

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
}
