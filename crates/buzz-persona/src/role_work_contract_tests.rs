//! What the shipped role templates owe the people who run them.
//!
//! Two live team runs (ledger 178, 179) showed seats spending a third to a
//! half of their effort operating Beekeeper rather than doing the work:
//! learning wire bodies by failed writes, hiring every available role,
//! polling for results, collecting acknowledgements a local preflight
//! demanded, and calling a stale binary. The software answers landed as
//! ledger 180–191; version 1.1.0 of the templates is the text catching up.
//!
//! Version 1.2.0 is the second half of the same correction. Since lane 209 the
//! host prepends every seat's first turn with a work brief carrying the
//! objective, the lead's brief, the acceptance steps, file ownership, branch,
//! worktree, the exact base and what this host did about it, the filled-in
//! `sessions report`/`verdict` invocations, the bound criteria quoted from the
//! plan at its pinned commit, the grants and the runtime. Every template
//! sentence that told a seat to go and find one of those is now redundant, and
//! 1.2.0 deletes it: the brief is authoritative and the template says only
//! what the brief cannot know. 1.2.0 also carries lane 210's settlement rule
//! for the lead, which 1.1.0 understated.
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

/// The newest version of every shipped template — what a `@^1.0.0` include
/// resolves to in this build, and therefore the text a seat actually reads.
/// A new version directory is added here in the same change that creates it;
/// the prose rules below all run over this table, never over a fixed version.
const CURRENT: [(&str, &str); 11] = [
    ("architect", "1.1.0"),
    ("builder", "1.2.0"),
    ("designer", "1.1.0"),
    ("lead", "1.2.8"),
    ("memory", "1.1.0"),
    ("poker", "1.1.0"),
    ("project-pulse", "1.1.0"),
    ("project-setup", "1.1.0"),
    ("runner", "1.2.0"),
    ("verifier", "1.2.1"),
    ("working-contract", "1.2.2"),
];

/// The roles 1.2.0 thinned, and the version each was thinned from. `verifier`
/// is here too: its 1.2.0 lost two sentences the brief now supplies.
const THINNED_IN_1_2_0: [&str; 5] = ["builder", "lead", "runner", "verifier", "working-contract"];

/// What 1.2.1 revised, and the ranges a project on the wire can be carrying
/// for it. The commit-identity rule (ledger 239) lives in the contract every
/// shipped role includes, so one new version reaches every seat.
const REVISED_IN_1_2_1: [&str; 1] = ["working-contract"];

/// What 1.2.2 revised (ledger 247): the one decision rule that replaces every
/// "stop and ask a person" sentence, and the lead's agents-repo commit duty.
/// Same single-contract reach as 1.2.1.
const REVISED_IN_1_2_2: [&str; 1] = ["working-contract"];

/// Lead 1.2.1 (ledger 247): `ask-for-a-ruling` routes a ruling to the
/// collaborator the project's decision rights name, human or agent, instead
/// of "a person". Every caret an existing project carries must take it.
const LEAD_1_2_1_CARETS: [&str; 3] = ["^1.0.0", "^1.1.0", "^1.2.0"];

/// Lead 1.2.2 (ledger 255): the role text tells the lead to bind a `git-ref`
/// criterion from the relay's observation (`bee sessions work bind ref`)
/// before it publishes `mission.completed`. Every earlier caret takes it.
const LEAD_1_2_2_CARETS: [&str; 4] = ["^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1"];

/// Lead 1.2.3 (control run 5): the exact work-command shapes in order, the
/// verify-only-on-a-delivered-commit rule and the stale host-result wake rule.
/// Every earlier caret takes it.
const LEAD_1_2_3_CARETS: [&str; 5] = ["^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2"];

/// Lead 1.2.4 (control run 5's orientation cost): the provider's situation
/// card names the ids, paths, plan and roster the lead once looked up, and the
/// verifier is hired alongside the builder. Every earlier caret takes it.
const LEAD_1_2_4_CARETS: [&str; 6] = ["^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2", "^1.2.3"];

/// Lead 1.2.5 (control run 6): the lead binds a hired verifier's assignment
/// to the criteria it judges immediately after publishing it, rather than
/// only after the verdict lands. Every earlier caret takes it.
const LEAD_1_2_5_CARETS: [&str; 7] = [
    "^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2", "^1.2.3", "^1.2.4",
];

/// Lead 1.2.6 (control run 7): a verifier's disposition arrives as a wake —
/// the lead must not poll for it or ask the verifier to message it directly
/// — and a verdict alone never settles the verifier's own assignment. Every
/// earlier caret takes it.
const LEAD_1_2_6_CARETS: [&str; 8] = [
    "^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2", "^1.2.3", "^1.2.4", "^1.2.5",
];

/// Lead 1.2.7 (control run 8, ledger 266): the "do nothing on a used
/// host-result wake" sentence is gone because the machinery drops that turn,
/// and hiring the verifier early is a judgment. Every earlier caret takes it.
const LEAD_1_2_7_CARETS: [&str; 9] = [
    "^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2", "^1.2.3", "^1.2.4", "^1.2.5", "^1.2.6",
];

/// Lead 1.2.8 (ledger 288, the shelfcount run): review evidence covers the
/// revision it names, so a change the lead makes after the review is either
/// made before it or reviewed before evidence is bound. Every earlier caret
/// takes it.
const LEAD_1_2_8_CARETS: [&str; 10] = [
    "^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1", "^1.2.2", "^1.2.3", "^1.2.4", "^1.2.5", "^1.2.6",
    "^1.2.7",
];

/// Verifier 1.2.1 (control run 6): `refuter-pass` states the assignmentRef
/// rule the CLI now enforces — a verdict names the assignmentRef of the
/// report it judges, never the verifier's own assignment. Every earlier
/// caret takes it.
const VERIFIER_1_2_1_CARETS: [&str; 3] = ["^1.0.0", "^1.1.0", "^1.2.0"];

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

/// The text of every file of every template's newest version: what a seat
/// composed by this build actually receives.
fn current_text() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, version) in CURRENT {
        for (rel, bytes) in version_files(name, version) {
            let text = String::from_utf8(bytes)
                .unwrap_or_else(|e| panic!("{name}/{version}/{rel} is not UTF-8: {e}"));
            out.push((format!("{name}/{version}/{rel}"), text));
        }
    }
    assert!(!out.is_empty(), "no current template files found");
    out
}

/// One template's newest version, lowercased, all files joined.
fn current_lower(name: &str) -> String {
    let version = CURRENT
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("{name} is not in CURRENT"));
    version_files(name, version)
        .values()
        .map(|b| String::from_utf8_lossy(b).to_lowercase())
        .collect::<Vec<_>>()
        .join("\n")
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

/// 1.1.0 is published too: it was staged by seats and pinned by projects, so
/// its bytes are as frozen as 1.0.0's. 1.2.0 is a new directory beside it.
#[test]
fn the_1_1_0_templates_are_byte_for_byte_what_they_shipped_as() {
    // Update this table only when a version directory is *added*; a change
    // to a line already here is the bug it exists to catch.
    let pinned: [(&str, &str); 10] = [
        (
            "architect",
            "44a971553bfc6de8cfab3cda851b62dc2d09e37ea906833ac9b4a77f285a2f80",
        ),
        (
            "builder",
            "4c34b01ada490b52de4edf45536d94178c03a399fa050c20f87601a41f448dd7",
        ),
        (
            "designer",
            "735a72f62cde02b52a6fd76530db4775ff0e5dfe8a4f22f6bb04cf6372e88016",
        ),
        (
            "lead",
            "b7a683d66d94dd5f1a00896d2a962960f4dc21b51d488bfb289b2c5e9b42e248",
        ),
        (
            "poker",
            "c269558c05a3c6625d9859fcea324e926f644eca267fd11428e9cbfd7459add6",
        ),
        (
            "project-pulse",
            "a0db365a46647a4fefda631243b7bb75876e5a29de65d29035a9320e7d1fe052",
        ),
        (
            "project-setup",
            "f4676c7265195d279163707dfecca284a0cdf392f55bfc7fa0eba9f4e72d9600",
        ),
        (
            "runner",
            "76eccb8bb3d1dcc9c23042e0021dcc31089ab7f2d575edc6da571de8fb54cf47",
        ),
        (
            "verifier",
            "4c35a1dc788667c57c66cccf8e026f0eaf66de66f7a20aa97a21e088bf02f2f1",
        ),
        (
            "working-contract",
            "3f92a8746f8f1b06970043a55c99b799110e60aa14ab8de8e9edb3ab3e9b40e4",
        ),
    ];
    assert_eq!(pinned.len(), UPDATED.len());
    for (name, expected) in pinned {
        assert_eq!(
            digest(name, "1.1.0"),
            expected,
            "{name}/1.1.0 changed; a published version is immutable — add a new version instead"
        );
    }
}

/// 1.2.0 shipped and was staged by the seats of the control run, so its bytes
/// are frozen exactly as 1.0.0's and 1.1.0's are. 1.2.1 is a new directory
/// beside it, not an edit of it.
#[test]
fn the_1_2_0_templates_are_byte_for_byte_what_they_shipped_as() {
    // Update this table only when a version directory is *added*; a change
    // to a line already here is the bug it exists to catch.
    let pinned: [(&str, &str); 5] = [
        (
            "builder",
            "2cc88a598cd94078aa86516f2c5e828d493f04360644dfc86d1ca27ae6dfd2d6",
        ),
        (
            "lead",
            "f50354854a1cfe8ffe02406659647840529163d8b3b03d5468094a2cb19ebb25",
        ),
        (
            "runner",
            "d093bd0dfe0afb2b734eaa317d44194d7b703f3b0263700126d8d690b0f375dd",
        ),
        (
            "verifier",
            "c90d8fc272e471830914a27349c2142fb840a450bbb65ec9d8c903ff1595528d",
        ),
        (
            "working-contract",
            "f81020db3c1c6c9e173d341612abe3fa8776041a18513c56e907d78e8bc031de",
        ),
    ];
    assert_eq!(pinned.len(), THINNED_IN_1_2_0.len());
    for (name, expected) in pinned {
        assert_eq!(
            digest(name, "1.2.0"),
            expected,
            "{name}/1.2.0 changed; a published version is immutable — add a new version instead"
        );
    }
}

/// A seat spent 49 minutes asking a founder which email to commit as, because
/// its workspace had no `user.name`/`user.email` and the text it was staged
/// with said to stop and ask when that field was empty (ledger 236(a), 239).
/// The host now sets that identity on every tree it cuts; the contract has to
/// say so, and has to say what to do if it is somehow still absent — which is
/// to author as the seat's own key and carry on.
///
/// This is a string test over shipped prose: it proves the instruction is
/// present in the words a seat reads, not that a seat obeys it.
#[test]
fn the_working_contract_settles_a_commit_identity_without_asking_anyone() {
    let text = current_lower("working-contract");
    for needle in [
        "user.name",
        "user.email",
        "@beekeeper.local",
        "signed-off-by",
        "co-authored-by",
    ] {
        assert!(
            text.contains(needle),
            "the working contract never says {needle:?}"
        );
    }
    // Wrapped prose: compare on one line so a line break cannot hide a rule.
    let flowed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        flowed.contains("never hold finished work while you ask about one"),
        "the contract never forbids parking work on a commit identity"
    );
    // The rule it replaces, in every spelling a seat could read as a licence
    // to escalate. No shipped template may send anyone to a person over this.
    for banned in ["stop and ask", "if email is empty, stop"] {
        for (path, body) in current_text() {
            let lower = body.to_lowercase();
            if !lower.contains("user.email") {
                continue;
            }
            assert!(
                !lower.contains(banned),
                "{path} still tells a seat to {banned:?} about a commit identity"
            );
        }
    }
}

/// 1.2.1 shipped to the seats of lane 239's landing, so its bytes are frozen
/// too. 1.2.2 is a new directory beside it, not an edit of it.
#[test]
fn the_1_2_1_templates_are_byte_for_byte_what_they_shipped_as() {
    // Update this table only when a version directory is *added*.
    let pinned: [(&str, &str); 1] = [(
        "working-contract",
        "ffdae7f72555a885e93b84a880b9e8afa03f827cdeef27580072ab591f38b37f",
    )];
    assert_eq!(pinned.len(), REVISED_IN_1_2_1.len());
    for (name, expected) in pinned {
        assert_eq!(
            digest(name, "1.2.1"),
            expected,
            "{name}/1.2.1 changed; a published version is immutable — add a new version instead"
        );
    }
}

/// Lead 1.2.1 shipped to run 4's seats (ledger 247/255), so its bytes are
/// frozen; 1.2.2 is a new directory beside it.
#[test]
fn the_lead_1_2_1_template_is_byte_for_byte_what_it_shipped_as() {
    assert_eq!(
        digest("lead", "1.2.1"),
        "c5d3a148033cf564a77b686a661c99f584e1a69ebba79a7d5a5548fbe85c0383",
        "lead/1.2.1 changed; a published version is immutable — add a new version instead"
    );
}

/// Lead 1.2.2 shipped to run 5's seats (ledger 256), so its bytes are frozen;
/// 1.2.3 is a new directory beside it.
#[test]
fn the_lead_1_2_2_template_is_byte_for_byte_what_it_shipped_as() {
    assert_eq!(
        digest("lead", "1.2.2"),
        "0bc1fb657351393aa072edc983e9c728cedf05aacbac5c7ea2bc2f587ae68a12",
        "lead/1.2.2 changed; a published version is immutable — add a new version instead"
    );
}

/// Lead 1.2.3 is what the finalizer lands for run 6, so its bytes are frozen;
/// 1.2.4 is a new directory beside it.
#[test]
fn the_lead_1_2_3_template_is_byte_for_byte_what_it_shipped_as() {
    assert_eq!(
        digest("lead", "1.2.3"),
        "ca412f4243b92f6dff471cd556cfd1cc7a115f6b23de33e36d3a147cc49672e5",
        "lead/1.2.3 changed; a published version is immutable — add a new version instead"
    );
}

/// Lead 1.2.7 shipped to control runs 8 onwards and the shelfcount run
/// (ledger 266, 285), so its bytes are frozen; 1.2.8 is a new directory
/// beside it.
#[test]
fn the_lead_1_2_7_template_is_byte_for_byte_what_it_shipped_as() {
    assert_eq!(
        digest("lead", "1.2.7"),
        "53261c5b57c6ae9afbc9fdd6cbcc4fc49041dfe59808f1bafdaa1fe564bdd7b9",
        "lead/1.2.7 changed; a published version is immutable — add a new version instead"
    );
}

/// Memory 1.1.0 (ledger 288, durable-work plan § 6): a memory carries its
/// scope and revision, a changed fact supersedes the old memory, and current
/// instructions and fresh observations outrank a recollection.
#[test]
fn memory_1_1_0_teaches_scope_revision_and_supersession() {
    let memory = current_lower("memory");
    for phrase in [
        "scope and its revision",
        "supersede the old memory",
        "outrank a recollection",
        "readable by its owner",
        "not confined to this host",
        "repository or pulse",
    ] {
        assert!(
            memory.contains(phrase),
            "memory lacks {phrase:?}:\n{memory}"
        );
    }
}

/// Lead 1.2.8 (ledger 288): review evidence covers the revision it names.
#[test]
fn the_lead_is_told_review_evidence_covers_only_the_reviewed_revision() {
    let lead = current_lower("lead");
    assert!(
        lead.contains("review evidence covers the revision it names"),
        "lead lacks the review-revision rule"
    );
    assert!(
        !lead.contains("every documentation edit"),
        "the rule must not send every documentation edit back to a builder"
    );
}

/// 1.2.2 is worth shipping only if projects already on the wire take it
/// without editing a file. Every caret a seeded project can be carrying for
/// the working contract — `@^1.0.0`, `@^1.1.0`, `@^1.2.0` and `@^1.2.1` — must
/// resolve to it, and an exact pin on any earlier version must still answer
/// with that version's own bytes.
#[test]
fn a_1_2_2_template_is_picked_up_by_every_earlier_caret_include() {
    let catalog = catalog();
    for name in REVISED_IN_1_2_2 {
        for range in ["^1.0.0", "^1.1.0", "^1.2.0", "^1.2.1"] {
            let resolved = catalog
                .resolve(name, &TemplateRange::parse(name, range).expect("range"))
                .unwrap_or_else(|e| panic!("{name}@{range}: {e}"));
            assert!(resolved.warning.is_none(), "{name}@{range}: {resolved:?}");
            assert_eq!(
                resolved.template.version.to_string(),
                "1.2.2",
                "{name}@{range} must take 1.2.2 without anyone editing a role file"
            );
        }
        for exact in ["1.0.0", "1.1.0", "1.2.0", "1.2.1"] {
            let resolved = catalog
                .resolve(name, &TemplateRange::parse(name, exact).expect("range"))
                .unwrap_or_else(|e| panic!("{name}@{exact}: {e}"));
            assert_eq!(
                resolved.template.version.to_string(),
                exact,
                "{name}@{exact} is a pin and must not move"
            );
        }
    }
    // The lead's 1.2.8 rides every earlier caret; 1.2.0 to 1.2.7 stay pins.
    for range in LEAD_1_2_1_CARETS
        .iter()
        .chain(LEAD_1_2_2_CARETS.iter())
        .chain(LEAD_1_2_3_CARETS.iter())
        .chain(LEAD_1_2_4_CARETS.iter())
        .chain(LEAD_1_2_5_CARETS.iter())
        .chain(LEAD_1_2_6_CARETS.iter())
        .chain(LEAD_1_2_7_CARETS.iter())
        .chain(LEAD_1_2_8_CARETS.iter())
    {
        let resolved = catalog
            .resolve("lead", &TemplateRange::parse("lead", range).expect("range"))
            .unwrap_or_else(|e| panic!("lead@{range}: {e}"));
        assert!(resolved.warning.is_none(), "lead@{range}: {resolved:?}");
        assert_eq!(
            resolved.template.version.to_string(),
            "1.2.8",
            "lead@{range} must take 1.2.8 without anyone editing a role file"
        );
    }
    for exact in [
        "1.0.0", "1.1.0", "1.2.0", "1.2.1", "1.2.2", "1.2.3", "1.2.4", "1.2.5", "1.2.6", "1.2.7",
    ] {
        let resolved = catalog
            .resolve("lead", &TemplateRange::parse("lead", exact).expect("range"))
            .expect("lead pin resolves");
        assert_eq!(resolved.template.version.to_string(), exact);
    }

    // The verifier's 1.2.1 rides every earlier caret; 1.0.0 to 1.2.0 stay pins.
    for range in VERIFIER_1_2_1_CARETS {
        let resolved = catalog
            .resolve(
                "verifier",
                &TemplateRange::parse("verifier", range).expect("range"),
            )
            .unwrap_or_else(|e| panic!("verifier@{range}: {e}"));
        assert!(resolved.warning.is_none(), "verifier@{range}: {resolved:?}");
        assert_eq!(
            resolved.template.version.to_string(),
            "1.2.1",
            "verifier@{range} must take 1.2.1 without anyone editing a role file"
        );
    }
    for exact in ["1.0.0", "1.1.0", "1.2.0"] {
        let resolved = catalog
            .resolve(
                "verifier",
                &TemplateRange::parse("verifier", exact).expect("range"),
            )
            .expect("verifier pin resolves");
        assert_eq!(resolved.template.version.to_string(), exact);
    }

    // And a whole project composes on it: a role file seeded by the 1.2.0-era
    // build, byte for byte, must carry the revised contract into its next hire.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("caret-1-2-0-beekeeper-agents");
    std::fs::create_dir_all(root.join("roles")).expect("roles dir");
    std::fs::write(
        root.join("team.yml"),
        "schema: beekeeper-team/v1\nname: recent\nversion: 0.1.0\nlead: builder\nroles:\n  builder: {}\n",
    )
    .expect("team.yml");
    std::fs::write(
        root.join("roles/builder.md"),
        "---\ndescription: \"A 1.2.0-era seed.\"\n---\n\n![[beekeeper/builder@^1.2.0]]\n\n\
         ![[beekeeper/working-contract@^1.2.0]]\n",
    )
    .expect("recent role");
    let composed = compose_role(
        &RoleSource::Flat {
            root: root.clone(),
            role: "builder".to_owned(),
        },
        &catalog,
        &ComposeOptions::local("roles/builder"),
    )
    .expect("the 1.2.0-era seed still composes");
    assert!(composed.provenance.warnings.is_empty(), "{composed:?}");
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
            ("beekeeper/builder@^1.2.0", "1.2.0"),
            ("beekeeper/working-contract@^1.2.0", "1.2.2"),
        ],
        "a 1.2.0-era caret must carry the revised contract"
    );
    assert!(
        composed
            .persona
            .prompt
            .to_lowercase()
            .contains("@beekeeper.local"),
        "the composed builder never receives the commit-identity rule"
    );
}

/// Ledger 247: the effective instructions of the lead, builder and verifier
/// seats — each role composed with its includes, exactly as a new project
/// seeds it — invent no human gate, and carry the one rule that replaces
/// them plus the lead's agents-repo commit duty.
#[test]
fn no_seated_role_is_told_to_stop_and_ask_a_person() {
    let catalog = catalog();
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("gates-beekeeper-agents");
    write_agents_repo_seed(&root, &catalog, "gates").expect("seed");
    for role in ["lead", "builder", "verifier"] {
        let composed = compose_role(
            &RoleSource::Flat {
                root: root.clone(),
                role: role.to_owned(),
            },
            &catalog,
            &ComposeOptions::local(format!("roles/{role}")),
        )
        .unwrap_or_else(|e| panic!("{role} composes: {e}"));
        let mut text = composed.persona.prompt.clone();
        for skill in &composed.skills {
            text.push('\n');
            let path = skill.dir.join("SKILL.md");
            text.push_str(
                &std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            );
        }
        let flowed = text
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        for banned in [
            "ask brian",
            "ask a human",
            "ask a person",
            "asking a person",
            "a person must",
            "a human must",
            "founder",
            "stop and ask",
            "wait for a person",
            "wait for a human",
        ] {
            assert!(!flowed.contains(banned), "{role} is told {banned:?}");
        }
        for needle in [
            "decide within your responsibility, consult affected collaborators, record, continue",
            "an unnecessary permission wait is a finding",
            "adopt the resulting commit, never uncommitted text",
            "nobody silently overwrites another author's draft",
        ] {
            assert!(flowed.contains(needle), "{role} never reads {needle:?}");
        }
        assert_eq!(
            flowed
                .matches(
                    "decide within your responsibility, consult affected collaborators, record, continue",
                )
                .count(),
            1,
            "{role}: the rule is stated once"
        );
    }
}

/// Every template's newest version is loadable, is what a caret range
/// answers with, and composes into a role a seat can be staged from —
/// including the renamed builder skill.
#[test]
fn every_current_template_is_what_a_caret_range_resolves_to_and_composes() {
    let catalog = catalog();
    for (name, version) in CURRENT {
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
            version,
            "{name}: a caret range must carry the minor revision to the next hire"
        );
        assert!(resolved.warning.is_none(), "{name}: {resolved:?}");
        assert!(
            templates_dir()
                .join(name)
                .join(version)
                .join(TEMPLATE_MD)
                .is_file(),
            "{name}/{version} has no {TEMPLATE_MD}"
        );
    }
    // `memory` 1.1.0 (ledger 288) rides the `^1.0.0` every seeded project
    // carries; an exact pin on 1.0.0 still answers with 1.0.0's bytes.
    let memory = catalog
        .resolve(
            "memory",
            &TemplateRange::parse("memory", "^1.0.0").expect("range"),
        )
        .expect("memory@^1.0.0");
    assert_eq!(memory.template.version.to_string(), "1.1.0");
    let memory_pin = catalog
        .resolve(
            "memory",
            &TemplateRange::parse("memory", "1.0.0").expect("range"),
        )
        .expect("memory@1.0.0");
    assert_eq!(memory_pin.template.version.to_string(), "1.0.0");

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
/// one, must both end up reading the newest version — the first because the
/// seed writes that version's caret, the second because `^1.0.0` admits it.
#[test]
fn a_new_project_seeds_the_current_versions_and_an_existing_projects_caret_resolves_to_them() {
    let catalog = catalog();
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("demo-beekeeper-agents");
    write_agents_repo_seed(&root, &catalog, "demo").expect("seed");

    let lead = std::fs::read_to_string(root.join("roles/lead.md")).expect("seeded lead");
    for expected in [
        "![[beekeeper/lead@^1.2.8]]",
        "![[beekeeper/working-contract@^1.2.2]]",
        "![[beekeeper/memory@^1.1.0]]",
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
            ("beekeeper/lead@^1.0.0", "1.2.8"),
            ("beekeeper/working-contract@^1.0.0", "1.2.2"),
            ("beekeeper/memory@^1.0.0", "1.1.0"),
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

/// 1.2.0 is worth shipping only if the projects already on the wire take it
/// without editing a file. Both include ranges a seeded project can be
/// carrying — `@^1.0.0` from the original seed and `@^1.1.0` from a seed
/// written between lane 196 and now — must resolve to 1.2.0, and an exact pin
/// on either older version must still answer with that version's own bytes.
#[test]
fn a_1_2_0_role_is_picked_up_by_both_the_caret_1_0_0_and_the_caret_1_1_0_includes() {
    let catalog = catalog();
    for name in THINNED_IN_1_2_0 {
        // What this build actually ships for that name: 1.2.0 for the four
        // roles, and 1.2.2 for the working contract, which lanes 239 and 247 revised.
        // Asserting the current version rather than a literal is the point —
        // an older caret must never stall on a version it happens to match.
        let current = CURRENT
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("{name} is not in CURRENT"));
        for range in ["^1.0.0", "^1.1.0"] {
            let resolved = catalog
                .resolve(name, &TemplateRange::parse(name, range).expect("range"))
                .unwrap_or_else(|e| panic!("{name}@{range}: {e}"));
            assert!(resolved.warning.is_none(), "{name}@{range}: {resolved:?}");
            assert_eq!(
                resolved.template.version.to_string(),
                current,
                "{name}@{range} must take {current} without anyone editing a role file"
            );
        }
        for exact in ["1.0.0", "1.1.0"] {
            let resolved = catalog
                .resolve(name, &TemplateRange::parse(name, exact).expect("range"))
                .unwrap_or_else(|e| panic!("{name}@{exact}: {e}"));
            assert_eq!(
                resolved.template.version.to_string(),
                exact,
                "{name}@{exact} is a pin and must not move"
            );
        }
    }

    // And a whole project composes on the new version: the file an earlier
    // build seeded with `@^1.1.0` carets, byte for byte.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("caret-1-1-0-beekeeper-agents");
    std::fs::create_dir_all(root.join("roles")).expect("roles dir");
    std::fs::write(
        root.join("team.yml"),
        "schema: beekeeper-team/v1\nname: mid\nversion: 0.1.0\nlead: lead\nroles:\n  lead: {}\n",
    )
    .expect("team.yml");
    std::fs::write(
        root.join("roles/lead.md"),
        "---\ndescription: \"A 1.1.0-era seed.\"\n---\n\n![[beekeeper/lead@^1.1.0]]\n\n\
         ![[beekeeper/working-contract@^1.1.0]]\n\n![[beekeeper/memory@^1.0.0]]\n\n\
         ![[beekeeper/project-pulse@^1.1.0]]\n",
    )
    .expect("mid-era role");
    let composed = compose_role(
        &RoleSource::Flat {
            root: root.clone(),
            role: "lead".to_owned(),
        },
        &catalog,
        &ComposeOptions::local("roles/lead"),
    )
    .expect("the 1.1.0-era seed still composes");
    assert!(composed.provenance.warnings.is_empty(), "{composed:?}");
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
            ("beekeeper/lead@^1.1.0", "1.2.8"),
            ("beekeeper/working-contract@^1.1.0", "1.2.2"),
            ("beekeeper/memory@^1.0.0", "1.1.0"),
            ("beekeeper/project-pulse@^1.1.0", "1.1.0"),
        ],
        "a 1.1.0-era caret must carry the current lead"
    );
    // The composed body is the 1.2.0 paragraph (1.2.1 changed only a skill),
    // not the 1.1.0 text it replaced.
    let lead_1_2_0 = String::from_utf8(
        version_files("lead", "1.2.0")
            .remove(TEMPLATE_MD)
            .expect("lead/1.2.0 TEMPLATE.md"),
    )
    .expect("utf-8");
    let paragraph = lead_1_2_0
        .rsplit("---\n")
        .next()
        .map(str::trim)
        .expect("the role template's own paragraph");
    assert!(
        composed.persona.prompt.contains(paragraph),
        "the composed lead is not 1.2.0"
    );
}

/// `bee` on a seat's PATH is somebody else's build (ledger § environment
/// facts). Every example names the host-selected binary.
#[test]
fn no_current_template_file_names_a_bare_bee_command() {
    for (path, text) in current_text() {
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
    let text = current_lower("verifier");
    for decision in ["confirmed", "not-refuted", "blocked"] {
        assert!(
            text.contains(decision),
            "the verifier never says {decision}"
        );
    }
}

/// Control run 6 (2026-09-24): a verdict's `assignmentRef` named the
/// verifier's own assignment instead of the assignment the report it judged
/// was written against, and the fold's `validate_causal_types` excluded it as
/// a `WrongTypeReference` — the independent verdict was silently lost. 1.2.1
/// of `refuter-pass` states the rule in one sentence and shows the exact
/// publish command shape.
#[test]
fn the_verifier_states_the_assignment_ref_rule_and_the_verdict_command_shape() {
    let text = current_lower("verifier");
    assert!(
        text.contains("names the report's assignment, never your own"),
        "the verifier's refuter-pass skill lacks the assignmentRef rule"
    );
    assert!(
        text.contains("\"assignmentref\":\"<report's assignmentref>\""),
        "the verifier's refuter-pass skill lacks the exact publish command shape"
    );
}

/// Collecting acknowledgements cost Andy's run six wakes and a reported
/// $9.33 (ledger 179(a)); 183 made the held completion settle by itself.
/// No 1.1.0 text may instruct a role to gather them — a sentence may only
/// mention it to forbid it.
#[test]
fn no_current_template_file_instructs_anyone_to_collect_acknowledgements() {
    const COLLECTING: [&str; 5] = ["collect", "wake", "gather", "chase", "poll"];
    const FORBIDDING: [&str; 5] = ["do not", "never", "rather than", "instead of", "without"];
    for (path, text) in current_text() {
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
fn no_current_template_file_encodes_a_model_id_provider_or_price() {
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
    for (path, text) in current_text() {
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
/// current text must carry the new procedure in the words a seat will search
/// for.
#[test]
fn each_current_role_carries_the_procedure_its_software_now_supports() {
    let by_name: BTreeMap<String, String> = UPDATED
        .iter()
        .map(|name| ((*name).to_owned(), current_lower(name)))
        .collect();
    let says = |name: &str, needle: &str| {
        assert!(
            by_name[name].contains(needle),
            "{name} never says {needle:?}"
        );
    };
    // Read the body before you write one (182). The worker roles no longer
    // repeat it: the work brief prints their `--example` line with the ids
    // already filled in (209), so the rule lives once, in the contract, for
    // the verbs no brief carries.
    says("working-contract", "--example");
    // Lead: bounded hiring, routed first, the verification input, and a
    // completion that settles itself (180, 182, 183, 184, 190).
    says("lead", "not a staffing plan");
    says("lead", "--class");
    says("lead", "modelnotice");
    says("lead", "--verifies");
    says("lead", "--checkout");
    says("lead", "run-status");
    says("lead", "pending");
    // Lead: 210's settlement rule, and where an ask has to go to be read.
    says("lead", "requiredaction");
    says("lead", "settles its assignment");
    says("lead", "new assignment");
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

/// What 1.2.0 deleted, and must stay deleted. Since lane 209 the host
/// prepends a seat's first turn with the objective, acceptance steps, file
/// ownership, branch, worktree, base commit and what this host did about it,
/// the filled-in `sessions report`/`verdict` invocations each preceded by its
/// own `--example` line, the bound criteria quoted from the plan, the grants
/// and the runtime. A template sentence that sends a seat to discover one of
/// those is paid for at full context on every tool call of the turn, and it
/// can disagree with the brief, which is authoritative. Observability is by
/// mechanism too: a published disposition is the notification, so no template
/// asks anyone to send one.
#[test]
fn no_current_template_repeats_a_fact_the_work_brief_supplies() {
    const FORBIDDEN: [(&str, &str, &str); 11] = [
        (
            "builder",
            "--example",
            "the brief prints the report command and its --example line, filled in",
        ),
        (
            "verifier",
            "--example",
            "the brief prints the verdict command and its --example line, filled in",
        ),
        (
            "runner",
            "--example",
            "the brief prints the report command and its --example line, filled in",
        ),
        (
            "lead",
            "--example",
            "the lead no longer has to point a worker at it; the brief does",
        ),
        (
            "builder",
            "acceptance checks before editing",
            "the brief carries the acceptance steps and the bound criteria",
        ),
        (
            "builder",
            "owned files or artifacts, dependencies",
            "the brief carries file ownership",
        ),
        (
            "verifier",
            "establish it in your",
            "the brief says whether this host established the base here",
        ),
        (
            "runner",
            "the named workspace, revision",
            "the brief carries the worktree and the base",
        ),
        (
            "lead",
            "starting revision, dependencies",
            "the assignment's own fields carry these; the host assembles them",
        ),
        (
            "lead",
            "notify affected workers",
            "the published disposition is the notification (no asking agents to report)",
        ),
        (
            "lead",
            "include its returned reference",
            "the brief carries the assignment reference",
        ),
    ];
    for (name, phrase, why) in FORBIDDEN {
        assert!(
            !current_lower(name).contains(phrase),
            "{name} still says {phrase:?}: {why}"
        );
    }
}
