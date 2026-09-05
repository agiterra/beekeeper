//! Rules the shipped packs under `personas/roles/` must satisfy.
//!
//! Unlike `integration.rs`, which builds synthetic packs in a temp directory,
//! this suite reads the packs this repository actually ships. A rule that only
//! holds for a fixture is a rule no seat ever obeys.
//!
//! Batch 3 L1.4: a seat that pushes with the full pre-push gate attached hits
//! the 60 s tool timeout (live run 2, Bob 11:01) and, worse, the NIP-98
//! credential git minted at ref discovery expires inside a long hook window
//! (`docs/INTEGRATION.md` § "Landing a batch"). The hook budget is deliberately
//! **not** raised — the hooks are what make a green SHA green — so every seat
//! pack has to carry the rule humans already follow.

use std::path::{Path, PathBuf};

use buzz_persona::pack;

/// The verbatim sentence every seat pack's `push-your-lane` skill must carry.
///
/// Byte-for-byte from `review-2026-09-01/batch3/LANE-L1.md` §L1.4. It is
/// asserted rather than merely documented because four packs drifting apart by
/// one word is exactly how "the rule is in the pack" becomes untrue.
const PUSH_RULE: &str = "Run your own gate first — the one your brief names. Then push normally: git push origin <branch>. The pre-push floor is scoped to what you changed and prints what it skipped; do not pass --no-verify, and never push to GitHub directly: origin is the relay and the bridge mirrors it.";

/// Phrasings the rule must NOT carry any more.
///
/// REVIEW-L1 F7 retired "run your own gate first — the one your brief names",
/// because a brief that named `cargo test -p buzz-core` could stand in for the
/// whole pre-push gate: the seat then pushed **with `--no-verify`** a SHA no
/// clippy, typecheck or file-size gate had ever seen. The danger was the pair,
/// and batch 3's scoped floor removes the second half — the hooks now run on
/// every push, scoped to what changed, and print what they skipped. A brief can
/// only add to that; it cannot shrink it, because it no longer stands in for
/// it. So the phrasing is allowed again and the thing that actually made it
/// unsafe is what is retired instead: telling a seat to skip the hooks.
///
/// Retire a phrasing here rather than deleting it, so the words that once
/// caused a defect cannot quietly come back.
const RETIRED_PHRASINGS: &[&str] = &[
    "push with the hooks skipped",
    "git push --no-verify",
    "Only then push with the hooks skipped",
];

/// The roles that take a brief, do work, and push a lane branch.
///
/// The lead is deliberately absent: it already carries the rule in
/// `personas/roles/lead/skills/beekeeper-project/SKILL.md`, and a seat loads
/// only its own pack.
const SEAT_ROLES: &[&str] = &["builder", "runner", "verifier", "designer"];

/// The verbatim sentence the lead pack must carry about landing `main`.
///
/// Finding 27, live run 2: at 12:00:19 a lead pushed `main` after its own
/// verifier had reported FAIL, and every layer below it said yes. The
/// relay's `require-verdict` gate has read the wire since 2026-09-03, so the
/// sentence no longer says "never push"; it says who admits a landing. Live
/// run 6 and Andy's run (findings 75 and 79, 2026-09-04) are the other half:
/// a lead answered the gate's refusal by telling the founder to push the
/// commit themselves — the remedy Brian ruled out on 2026-09-03 ("humans
/// never gate a landing"). Asserted, not merely written, for the same reason
/// the seat rule is.
const LEAD_MAIN_RULE: &str = "A landing is admitted by the relay's push gate, never by a person. When it refuses, the sentence names the missing fact, and you have two moves: produce that fact, or publish a blocker quoting the sentence verbatim with the row event ids. You never pass a landing to a founder.";

/// The section every pack that runs a gate must carry, by heading.
///
/// Finding 77 (live runs 6 and 7, 2026-09-04): the host's gate observer
/// records a kind 44246 row only for a bare command — a pipe, a redirect, a
/// `$(…)` or a trailing `; echo` of `$?` records nothing, silently — and a
/// lead lost two hours to a gate the push gate could not see. The section is
/// asserted by heading and by its three load-bearing phrases rather than
/// byte-for-byte, because each pack words the example in its own voice.
const GATES_SECTION_HEADING: &str = "## Gates the host can see";

/// Phrases every "Gates the host can see" section must carry.
const GATES_SECTION_PHRASES: &[&str] = &[
    // The hermit-first step, as its own command.
    ". ./bin/activate-hermit",
    // The bare-command rule, and what breaks it.
    "bare command",
    // The no-person rule (finding 79).
    "founder",
];

/// Phrasings no pack may carry anywhere, in any skill or persona.
///
/// The first four hand a landing to a person (findings 75 and 79); the rest
/// are the wrappers finding 77 proved the observer cannot see. Retired here
/// rather than merely edited out, for the same reason as [`RETIRED_PHRASINGS`].
const RETIRED_EVERYWHERE: &[&str] = &[
    "as founder",
    "founder push",
    "land it yourself",
    "hand the landing",
    "| tail",
    "2>&1",
    "echo \"EXIT",
    "echo EXIT",
];

/// Every skill file that instructs a seat how to run a gate.
const GATE_SKILLS: &[(&str, &str)] = &[
    ("lead", "beekeeper-project"),
    ("builder", "push-your-lane"),
    ("runner", "push-your-lane"),
    ("verifier", "push-your-lane"),
    ("designer", "push-your-lane"),
];

/// The four sentences the lead pack must carry, byte-for-byte (§1k).
///
/// Each is a live failure, not a preference. Asserted rather than merely
/// written because a rule that lives only in prose is a rule that drifts by a
/// word at a time until it is a different rule.
///
/// Every entry is `(skill directory, sentence, why)`. A skill named twice
/// carries both sentences.
const LEAD_PACK_SENTENCES: &[(&str, &str, &str)] = &[
    (
        "hire",
        "Publish the assignment before you hire the seat that answers it: bee sessions assign first, its event id in the brief, bee sessions hire second. A hire that arrives with no assignment to cite leaves the seat two bad options — invent a reference, or report nothing — and the live runs produced both.",
        "finding 22: the rule was in write-brief and triage-report, and live run 3 still hired at 12:32:50 and assigned at 12:33:37 — because the skill a lead loads *while hiring* is `hire` and it said nothing",
    ),
    (
        "triage-report",
        "Answer a request with bee sessions decide answer, never in a turn. Prose to the asker leaves the request open on the wire, the mission waiting on you, and the ruling somewhere no fold can read.",
        "finding 21: the lead answered a builder in prose, the request stayed open on the wire, and the founder was asked the same question twice",
    ),
    (
        "ask-for-a-ruling",
        "Answer a request with bee sessions decide answer, never in a turn. Prose to the asker leaves the request open on the wire, the mission waiting on you, and the ruling somewhere no fold can read.",
        "the same rule, in the skill a lead loads while it is holding a ruling",
    ),
    (
        "triage-report",
        "A report whose gate claims are prose is not accepted. Ask for the signed row — bee sessions observe gate — and rule on that. This rule applies to any session where kind 44246 rows are on the wire; where none are, say in the disposition that the claim is unverified rather than accepting it.",
        "finding 26: a report claimed `cargo test -p buzz-cli` green; a verifier reproduced two failures on the same SHA, and the claim turned out to be one test file",
    ),
    (
        "ask-for-a-ruling",
        "If the honest answer would have to be given again for the next commit, ask for a condition rather than a commit: state the class the ruling covers and pass it back with bee sessions decide answer --condition. A per-SHA ruling is a question you have agreed to ask again.",
        "finding 21 again, from the other end: the ruling was given about one SHA, so the next SHA needed the same ruling",
    ),
];
/// The verbatim sentence every shipped pack must carry about which `bee` it
/// runs.
///
/// Handed over by lane L12, which owns the unskippable half (the seat briefing
/// names `$BEE` too). A pack can be skipped and a briefing cannot, so this is
/// belt and braces on the same fact: on 2026-09-01 a seat reached the desktop
/// app's bundled sidecar because that is what its `PATH` found first, and ran a
/// build of the CLI older than the fix it was testing.
const BEE_PATH_RULE: &str = "Run the CLI as `$BEE` — your host chose it and put it on your PATH; never a path someone typed at you, and never a path from a transcript.";

/// What a pack may never point at.
///
/// A pack that cites a source path teaches a seat to read `crates/` to use
/// `bee`, which is the whole cost this lane removes: on 2026-09-01 a lead
/// grepped the fold's Rust source for the word its own tool had printed. The
/// answers live in `bee sessions <verb> --help` and `bee sessions explain
/// <word>`; a pack points there.
const FORBIDDEN_POINTERS: &[&str] = &[
    "crates/",
    // A Windows separator is the same pointer (REVIEW-L13 F4).
    "crates\\",
    "desktop/src",
    // The way a Desktop path is usually written in this repo's own documents.
    "src/features/",
    "mobile/lib/",
    "web/src/",
    "00-BATCH",
];

/// Source-file extensions whose `name.ext:NN` form is a line citation.
///
/// REVIEW-L13 F4: the first version scanned `.rs:NN` only, and this repository's
/// UI is TypeScript and Dart — `codingSessionTeamTransactionFold.ts:412` slipped
/// straight through the guard that exists to catch exactly that.
const CITED_EXTENSIONS: &[&str] = &[".rs:", ".ts:", ".tsx:", ".dart:"];

/// Repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/buzz-persona has a grandparent")
        .to_path_buf()
}

#[test]
fn every_seat_pack_carries_the_push_rule_verbatim() {
    let roles_dir = repo_root().join("personas").join("roles");
    for role in SEAT_ROLES {
        let skill = roles_dir
            .join(role)
            .join("skills")
            .join("push-your-lane")
            .join("SKILL.md");
        assert!(
            skill.is_file(),
            "role {role} carries no push-your-lane skill at {}: a seat loads only its own pack, \
             so the rule has to be in each one",
            skill.display()
        );
        let body = std::fs::read_to_string(&skill)
            .unwrap_or_else(|error| panic!("reading {}: {error}", skill.display()));
        assert!(
            body.contains(PUSH_RULE),
            "role {role}'s push-your-lane skill does not carry the rule byte-for-byte:\n{body}"
        );
        for retired in RETIRED_PHRASINGS {
            assert!(
                !body.contains(retired),
                "role {role}'s push-your-lane skill still carries the retired phrasing \
                 {retired:?}, which let a narrow brief stand in for the whole gate \
                 (REVIEW-L1 F7)"
            );
        }
        assert!(
            body.contains("docs/INTEGRATION.md"),
            "role {role}'s push-your-lane skill must cite docs/INTEGRATION.md for why the \
             credential expires inside a long hook window"
        );
    }
}

#[test]
fn the_lead_pack_says_no_person_gates_a_landing() {
    let skill = repo_root()
        .join("personas")
        .join("roles")
        .join("lead")
        .join("skills")
        .join("beekeeper-project")
        .join("SKILL.md");
    let body = std::fs::read_to_string(&skill)
        .unwrap_or_else(|error| panic!("reading {}: {error}", skill.display()));
    assert!(
        body.contains(LEAD_MAIN_RULE),
        "the lead pack does not carry the landing rule byte-for-byte. The relay's push gate \
         admits a landing or refuses it with a sentence; a lead that answers the refusal by \
         asking a founder to push has handed a person the job the gate exists to take away \
         (findings 27, 75 and 79)."
    );
}

/// Finding 77: a gate the host cannot see is a gate the push gate cannot
/// read, and the only fix is in the seat's hands — so it has to be in the
/// skill the seat loads while running the gate.
#[test]
fn every_gate_skill_carries_the_gates_the_host_can_see_section() {
    let roles_dir = repo_root().join("personas").join("roles");
    for (role, skill) in GATE_SKILLS {
        let path = roles_dir
            .join(role)
            .join("skills")
            .join(skill)
            .join("SKILL.md");
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        assert!(
            body.contains(GATES_SECTION_HEADING),
            "role {role}'s {skill} skill carries no {GATES_SECTION_HEADING:?} section: the \
             host records a kind 44246 row only for a bare command, and nothing else tells \
             the seat so (finding 77)"
        );
        let section = body.split(GATES_SECTION_HEADING).nth(1).unwrap_or_default();
        for phrase in GATES_SECTION_PHRASES {
            assert!(
                section.contains(phrase),
                "role {role}'s {skill} gates section does not carry {phrase:?}"
            );
        }
    }
}

/// Findings 75, 77 and 79: no pack may offer a person as a landing's remedy,
/// and no pack may show a seat a gate shape the observer cannot record.
#[test]
fn no_pack_offers_a_person_or_a_wrapped_gate() {
    let mut offences: Vec<String> = Vec::new();
    for path in every_pack_markdown_file() {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        for (index, line) in body.lines().enumerate() {
            for retired in RETIRED_EVERYWHERE {
                if line.contains(retired) {
                    offences.push(format!(
                        "{}:{}: {retired:?} in {line:?}",
                        path.display(),
                        index + 1
                    ));
                }
            }
        }
    }
    assert!(
        offences.is_empty(),
        "a pack hands a landing to a person or shows a gate the host cannot see:\n{}",
        offences.join("\n")
    );
}

#[test]
fn the_lead_pack_carries_every_sentence_the_live_runs_wrote() {
    let skills = repo_root()
        .join("personas")
        .join("roles")
        .join("lead")
        .join("skills");
    for (skill, sentence, why) in LEAD_PACK_SENTENCES {
        let path = skills.join(skill).join("SKILL.md");
        assert!(
            path.is_file(),
            "the lead pack carries no {skill} skill at {}: {why}",
            path.display()
        );
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        assert!(
            body.contains(sentence),
            "the lead pack's {skill} skill does not carry this sentence byte-for-byte:\n\
             {sentence}\n\
             why it is asserted: {why}"
        );
    }
}

/// The new skill has to be *loaded*, not merely present on disk: a lead reads
/// the skills its persona lists and nothing else.
#[test]
fn the_lead_persona_loads_the_ruling_skill() {
    let persona = repo_root()
        .join("personas")
        .join("roles")
        .join("lead")
        .join("personas")
        .join("lead.persona.md");
    let body = std::fs::read_to_string(&persona)
        .unwrap_or_else(|error| panic!("reading {}: {error}", persona.display()));
    assert!(
        body.contains("./skills/ask-for-a-ruling/"),
        "the lead persona does not list ask-for-a-ruling, so a lead never loads it \
         however carefully the file is written"
    );
}

#[test]
fn every_shipped_role_pack_still_loads() {
    let roles_dir = repo_root().join("personas").join("roles");
    let mut loaded = 0;
    for entry in std::fs::read_dir(&roles_dir).expect("personas/roles is readable") {
        let entry = entry.expect("a readable directory entry");
        if !entry.path().is_dir() {
            continue;
        }
        let pack = pack::load_pack(&entry.path()).unwrap_or_else(|error| {
            panic!("pack {} failed to load: {error}", entry.path().display())
        });
        assert!(
            !pack.personas.is_empty(),
            "pack {} declares no persona",
            entry.path().display()
        );
        loaded += 1;
    }
    assert!(
        loaded >= SEAT_ROLES.len(),
        "expected at least the four seat packs under personas/roles, found {loaded}"
    );
}

/// Every markdown file under `personas/`, in a stable order.
fn every_pack_markdown_file() -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let entries = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("reading {}: {error}", dir.display()));
        for entry in entries {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(&repo_root().join("personas"), &mut found);
    found.sort();
    found
}

/// Why one line of a pack points a seat at the source tree, or `None`.
///
/// Shared by the walker below and by
/// `the_forbidden_pointer_scan_catches_every_neighbouring_spelling`, so the
/// cases the review demonstrated are checked against the same code the packs
/// are.
fn source_pointer_in(line: &str) -> Option<String> {
    for needle in FORBIDDEN_POINTERS {
        if line.contains(needle) {
            return Some(format!("{needle:?}"));
        }
    }
    // A `file.rs:NN` citation is the same failure in a shorter form, in every
    // language this repository writes.
    for extension in CITED_EXTENSIONS {
        let mut rest = line;
        while let Some(position) = rest.find(extension) {
            let after = &rest[position + extension.len()..];
            if after.starts_with(|character: char| character.is_ascii_digit()) {
                return Some(format!("a file{extension}NN citation"));
            }
            rest = &rest[position + extension.len()..];
        }
    }
    None
}

/// Whether one line points at the source tree.
fn line_points_at_the_source_tree(line: &str) -> bool {
    source_pointer_in(line).is_some()
}

/// Batch 3 L13.3. A ratchet: it fails on the first pack that sends a seat to
/// the source tree, whether or not one does today.
#[test]
fn no_pack_points_at_the_source_tree() {
    let mut offences: Vec<String> = Vec::new();
    for path in every_pack_markdown_file() {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
        for (index, line) in body.lines().enumerate() {
            if let Some(reason) = source_pointer_in(line) {
                offences.push(format!(
                    "{}:{}: {reason} in {line:?}",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "a pack points a seat at this repository's source instead of at the tool.\n\
         The answers are in `bee sessions <verb> --help` and `bee sessions explain <word>`; \
         cite those.\n{}",
        offences.join("\n")
    );
}

/// REVIEW-L13 F4. Each of these reached a pack unchallenged by the first
/// version of the guard; the review found all five by appending them to a
/// builder persona in a scratch copy and watching six tests stay green.
#[test]
fn the_forbidden_pointer_scan_catches_every_neighbouring_spelling() {
    let slips = [
        "Read `src/features/coding-sessions/lib/foo.ts` for the shape.",
        "See `crates\\buzz-core\\src\\x.rs` on Windows.",
        "`mobile/lib/shared/relay/nostr_models.dart` holds the kinds.",
        "`web/src/app.tsx` renders it.",
        "see `codingSessionTeamTransactionFold.ts:412` for the rest",
        "`CodingSessionMissionInspector.tsx:88` draws the badge",
        "`lib/features/mission/mission_page.dart:31` on mobile",
        "`crates/buzz-core/src/x.rs` is the fold",
        "`coding_session_team_transaction_fold.rs:99` is the enum",
        "00-BATCH.md \u{a7}1d froze the sentence",
    ];
    for line in slips {
        assert!(
            line_points_at_the_source_tree(line),
            "this pointer would still reach a seat unchallenged: {line:?}"
        );
    }

    // And the guard must not fire on the prose the packs legitimately carry.
    let allowed = [
        "Run `$BEE sessions explain unseated` for the meaning.",
        "`bee sessions operation get --id <operationId>` fetches the record.",
        "Read `docs/SESSION_STATE.md` \u{a7}3 Next and nothing else.",
        "`AGENTS.md` \u{a7} Quality Gates is the contract.",
        "The mission's terminal is a `mission.completed`, never a note.",
    ];
    for line in allowed {
        assert!(
            !line_points_at_the_source_tree(line),
            "the guard fired on prose a pack is allowed to carry: {line:?}"
        );
    }
}

/// Batch 3 L13.3, L12's cross-lane sentence.
#[test]
fn every_shipped_pack_carries_the_bee_path_rule_verbatim() {
    let roles_dir = repo_root().join("personas").join("roles");
    let mut checked = 0;
    for entry in std::fs::read_dir(&roles_dir).expect("personas/roles is readable") {
        let role_dir = entry.expect("a readable directory entry").path();
        if !role_dir.is_dir() {
            continue;
        }
        let role = role_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let persona = role_dir.join("personas").join(format!("{role}.persona.md"));
        let body = std::fs::read_to_string(&persona)
            .unwrap_or_else(|error| panic!("reading {}: {error}", persona.display()));
        assert!(
            body.contains(BEE_PATH_RULE),
            "role {role}'s persona does not carry the $BEE rule byte-for-byte. A seat that \
             runs a `bee` someone typed at it runs whichever build that path holds — on \
             2026-09-01 that was a stale bundled sidecar."
        );
        checked += 1;
    }
    assert!(
        checked >= SEAT_ROLES.len(),
        "expected at least the four seat packs, checked {checked}"
    );
}

/// The packs must send a reader to the tool for the fold's vocabulary.
#[test]
fn the_lead_pack_points_at_the_tool_for_the_words() {
    let skill = repo_root()
        .join("personas")
        .join("roles")
        .join("lead")
        .join("skills")
        .join("triage-report")
        .join("SKILL.md");
    let body = std::fs::read_to_string(&skill)
        .unwrap_or_else(|error| panic!("reading {}: {error}", skill.display()));
    assert!(
        body.contains("sessions explain"),
        "the lead's triage skill must name `$BEE sessions explain <word>`: the words it has \
         to read (unseated, dangling, superseded, an exclusion code) are defined there and \
         nowhere a seat can reach without a checkout"
    );
}
