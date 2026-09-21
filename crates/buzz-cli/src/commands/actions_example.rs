//! `bee actions example` — a complete, valid `actions.yml` this binary can
//! print offline, and the refusals that send an author to it.
//!
//! On 2026-09-20 a team lead spent six probe rounds learning this file's shape
//! from successive parser errors (ledger 206 A). Nothing in the binary could
//! state the shape, and the one error an author is most likely to hit —
//! `missing field `on`` — names a key that is sitting in front of them.
//!
//! Two things are provided here:
//!
//! 1. [`example_file`] prints a whole `actions.yml`, commented, one action per
//!    trigger kind. It reaches no relay and signs nothing, so it is dispatched
//!    ahead of the key gate, the same reasoning as `bee sessions --example`
//!    (ledger 182). Every example is parsed by the real
//!    [`buzz_workflow::parse_actions_yml`] in this module's tests, so it
//!    cannot drift from the types.
//! 2. [`explain_actions_error`] turns a parse failure into an answer: the
//!    YAML-1.1 boolean trap is named when it is actually present, every
//!    `missing field` names all the required fields of the object it belongs
//!    to, and every refusal from this file points at `bee actions example`.

use std::fmt::Write as _;

use crate::error::CliError;

/// Which single action `bee actions example --kind` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ExampleKind {
    /// A manual `verify` action that refuses to test the wrong tree.
    ManualVerify,
    /// A push-triggered action that builds and routes the outcome.
    RefUpdated,
    /// A cron-scheduled action that wakes an agent only on failure.
    Schedule,
    /// An action that fires on a terminal CI result.
    CiResult,
}

/// The header every printed example carries: the schema line, the default
/// timezone, and the two traps that cost the most time.
const HEADER: &str = "\
# actions.yml — this project's actions, at the root of its agents repository
# (`<slug>-beekeeper-agents`). Publish with:
#
#   bee actions publish --project 30621:<owner-hex>:<id> --channel <UUID>
#
# Check first with `bee actions status --project … --channel …`: it prints
# each entry's name, hash and trigger, and whether this key may publish and
# trigger it.
#
# Two things that cost a lead six probe rounds on 2026-09-20:
#
#  * `on` is the trigger's tag, and it lives INSIDE `trigger`. A `trigger`
#    mapping without it fails with \"missing field `on`\" — which reads as if
#    `on` were missing from the file, when it is usually at the wrong level.
#  * This parser (serde_yaml 0.9) resolves only `true`/`false` as booleans, so
#    a bare `on:` key is the string \"on\" and needs no quoting. YAML **1.1**
#    parsers (PyYAML, Ruby's Psych, `yq`) do read bare `on` as the boolean
#    true: if you round-trip this file through one of those, the key comes
#    back as `true:` and this parser then says \"missing field `on`\". Quoting
#    it — `\"on\": manual` — is harmless here and survives that trip.
#
# `command` is an argv sequence, never a shell string: [\"just\", \"ci\"], not
# \"just ci\". Every step needs an `id`, and every action needs both a `name`
# and step `id`s — the step `id` is what `if:` conditions refer to.
schema: buzz-project-actions/v1
timezone: America/New_York   # default zone for every `schedule` below
actions:
";

/// A manual `verify` action bound to the commit it tests (ledger 184).
const MANUAL_VERIFY: &str = "
  # (1) A verify-style action that refuses to test the wrong tree.
  #     `checkout: required` makes the relay refuse a run that names no
  #     commit, so this can only be started as
  #       bee workflows trigger --workflow <id> --checkout <40-hex sha>
  #     and the host runs it in a fresh detached worktree at that commit.
  #     `required` is refused at save time on any trigger that can name no
  #     commit (schedule, webhook, message_posted, reaction_added,
  #     diff_posted): only manual, ref_updated and ci_result can.
  - name: verify
    description: Run the gate against one named commit.
    trigger:
      on: manual              # the tag; `manual` never fires on its own
    steps:
      - id: verify            # step ids are required and must be unique
        action: run_on_host
        command: [\"just\", \"ci\"]   # argv, not a shell string
        working_directory: \".\"     # relative to the checkout; \"..\" is refused
        checkout: required          # current (default) | triggering_commit | required
        timeout: 30m                # at most 1h
";

/// A push-triggered action with the two-way brief sugar.
const REF_UPDATED: &str = "
  # (2) Fires when a push changes a ref, and tells an agent either way.
  #     `brief: {on_success, on_failure}` is sugar: it compiles into two
  #     `wake_agent` steps with opposite `if:` on the preceding host step's
  #     exit code, so it needs a `run_on_host` step before it.
  - name: on-push-main
    trigger:
      on: ref_updated
      ref: \"refs/heads/main\"   # a glob over full ref names
    steps:
      - id: build
        action: run_on_host
        command: [\"cargo\", \"build\", \"--workspace\"]
        checkout: triggering_commit
      - id: review
        action: wake_agent
        to: { agent: Keystone }   # an agent named in this project's team.yml
        brief:
          on_success: \"Review the build log for unexpected warnings.\"
          on_failure: \"Correct the build errors, then report.\"
";

/// A cron-scheduled action that routes only on failure.
const SCHEDULE: &str = "
  # (3) A schedule, in the file's `timezone` unless it names its own.
  #     `checkout: required` is REFUSED here: a schedule can name no commit.
  - name: nightly-build
    trigger:
      on: schedule
      cron: \"0 17 * * FRI\"     # Friday 17:00 in the file's timezone
    steps:
      - id: build
        action: run_on_host
        command: [\"just\", \"ci\"]
        env: { CARGO_TERM_COLOR: never }   # literals only — this file is in git
        env_from_host: [\"NPM_TOKEN\"]      # names only; the host supplies values
        capture: { tail_bytes: 8192, artifact_max_bytes: 10485760 }
      - id: fix
        action: wake_agent
        # Flat variable names, from the engine: steps_<id>_output_exit_code,
        # _timed_out, _stdout_tail.
        if: \"steps_build_output_exit_code != 0\"
        to: { agent: Levain }
        brief: \"The nightly build failed. Fix it, run the gate, report.\"
";

/// An action that fires on a terminal CI result.
const CI_RESULT: &str = "
  # (4) Fires on a terminal CI result the relay recorded for this project.
  #     `conclusion` is a list; at least one of success | failure | cancelled.
  #     A `hire_agent` step seats a fresh agent instead of waking a standing
  #     one — the command lives in the brief and the seat runs it itself.
  - name: after-ci
    trigger:
      on: ci_result
      check: gate
      conclusion: [failure]
    steps:
      - id: triage
        action: hire_agent
        role: runner
        session: { agent: Keystone }
        brief: \"CI failed on {{trigger.commit}}: {{trigger.evidence_url}}. Reproduce it, then report.\"
";

impl ExampleKind {
    fn body(self) -> &'static str {
        match self {
            ExampleKind::ManualVerify => MANUAL_VERIFY,
            ExampleKind::RefUpdated => REF_UPDATED,
            ExampleKind::Schedule => SCHEDULE,
            ExampleKind::CiResult => CI_RESULT,
        }
    }

    /// Every kind, in the order the whole-file example prints them.
    pub const ALL: &'static [ExampleKind] = &[
        ExampleKind::ManualVerify,
        ExampleKind::RefUpdated,
        ExampleKind::Schedule,
        ExampleKind::CiResult,
    ];
}

/// The text `bee actions example` prints: a complete `actions.yml`.
///
/// With no `--kind`, every kind is included in one file; with one, only that
/// action. Both are whole files — schema line included — so the output can be
/// written straight to `actions.yml` and published.
pub fn example_file(kind: Option<ExampleKind>) -> String {
    let mut text = String::from(HEADER);
    match kind {
        Some(kind) => text.push_str(kind.body()),
        None => {
            for kind in ExampleKind::ALL {
                text.push_str(kind.body());
            }
        }
    }
    text
}

/// `bee actions example [--kind …]` — print and exit 0. No relay, no key.
pub fn cmd_example(kind: Option<ExampleKind>) -> Result<(), CliError> {
    print!("{}", example_file(kind));
    Ok(())
}

/// Objects in `actions.yml` and the keys each one requires, in the order a
/// reader meets them.
///
/// Held as data so a refusal can list the whole object rather than the one
/// field serde happened to notice first — serde reports a single missing
/// field per attempt, which is what made learning this file cost one write
/// per key. `every_listed_required_field_is_really_required` keeps the table
/// honest against the types.
const REQUIRED_FIELDS: &[(&str, &[&str])] = &[
    ("the file itself", &["schema", "actions"]),
    ("each action", &["name", "trigger", "steps"]),
    (
        "trigger",
        &["on (message_posted | reaction_added | diff_posted | schedule | ci_result | ref_updated | webhook | manual)"],
    ),
    ("trigger: { on: ref_updated }", &["ref"]),
    ("trigger: { on: ci_result }", &["check", "conclusion"]),
    ("each step", &["id", "action"]),
    ("action: run_on_host", &["command (a sequence, not a string)"]),
    ("action: wake_agent", &["to: { agent: … }", "brief"]),
    ("action: hire_agent", &["role", "session: { agent: … }", "brief"]),
    ("action: send_message", &["text"]),
];

/// Turn a `parse_actions_yml` failure into something an author can act on.
///
/// Three additions over the parser's own words, which are preserved verbatim
/// first:
///
/// 1. The YAML-1.1 boolean trap, named **only when it is actually present** —
///    a `trigger` mapping carrying a `true` key where `on` was expected. This
///    parser never produces that from a bare `on:`; a YAML 1.1 emitter does.
/// 2. Every `missing field` gets the full required-key list of the object
///    that field belongs to.
/// 3. Every refusal from this file names `bee actions example`.
pub fn explain_actions_error(file: &str, text: &str, error: &str) -> CliError {
    let mut message = format!("{file}: {error}");
    if let Some(where_at) = boolean_on_trap(text) {
        let _ = write!(
            message,
            "\n\nThis file has a `true:` key where `on:` belongs ({where_at}). A YAML 1.1 \
             parser or emitter (PyYAML, Ruby's Psych, `yq`) reads a bare `on` as the boolean \
             true and writes it back as `true`. Change it to `on:` — or quote it, `\"on\":`, \
             which survives that round trip. This parser reads a bare `on` as the string it \
             looks like, so quoting is never required here."
        );
    } else if error.contains("missing field `on`") {
        let _ = write!(
            message,
            "\n\n`on` is the trigger's tag and belongs INSIDE `trigger`:\n\
             \n    trigger:\n      on: manual\n\n\
             A `trigger:` mapping with any other shape — `type:`, a bare string, the `on` key \
             one level up beside `name` — reports exactly this."
        );
    }
    if let Some(field) = missing_field(error) {
        if let Some((object, keys)) = object_requiring(field) {
            let _ = write!(
                message,
                "\n\nEvery key of {object} is required: {}. serde names one missing field per \
                 attempt, so fix them together rather than one write at a time.",
                keys.join(", ")
            );
        }
    }
    let _ = write!(
        message,
        "\n\n`bee actions example` prints a complete, valid actions.yml with every trigger \
         kind; `bee actions example --kind manual-verify` prints just the verify one."
    );
    CliError::Usage(message)
}

/// The field name in a serde `missing field \`x\`` message.
fn missing_field(error: &str) -> Option<&str> {
    let rest = error.split("missing field `").nth(1)?;
    rest.split('`').next()
}

/// The object whose required-key list mentions `field`.
fn object_requiring(field: &str) -> Option<(&'static str, Vec<&'static str>)> {
    REQUIRED_FIELDS.iter().find_map(|(object, keys)| {
        keys.iter()
            .any(|key| key.split_whitespace().next() == Some(field))
            .then(|| (*object, keys.to_vec()))
    })
}

/// Where a `true` key sits in the position `on` belongs in, if anywhere.
///
/// Returns the action's name (or its index) so the message points at a line
/// rather than at the file. `None` when the document does not parse at all,
/// or when no trigger carries a boolean key: the trap is named only when it
/// is really there.
fn boolean_on_trap(text: &str) -> Option<String> {
    let document: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
    let actions = document.get("actions")?.as_sequence()?;
    for (index, action) in actions.iter().enumerate() {
        let Some(trigger) = action
            .get("trigger")
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        let has_boolean_key = trigger
            .keys()
            .any(|key| matches!(key, serde_yaml::Value::Bool(_)));
        if has_boolean_key && !trigger.contains_key(serde_yaml::Value::String("on".into())) {
            return Some(
                match action.get("name").and_then(serde_yaml::Value::as_str) {
                    Some(name) => format!("action {name:?}"),
                    None => format!("action #{}", index + 1),
                },
            );
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_workflow::{parse_actions_yml, ActionDef, TriggerDef};

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse";

    /// The whole-file example is a real `actions.yml`: the same function
    /// `bee actions publish` and a host's recompilation call accepts it, and
    /// it carries one action per trigger kind.
    #[test]
    fn the_whole_file_example_parses_as_the_publisher_parses_it() {
        let entries = parse_actions_yml(&example_file(None), PROJECT).expect("example parses");
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(
            names,
            ["verify", "on-push-main", "nightly-build", "after-ci"]
        );
        for entry in &entries {
            assert_eq!(entry.hash.len(), 64, "{} has a definition hash", entry.name);
        }
    }

    /// Each `--kind` prints a whole file on its own, and the trigger it prints
    /// is the one it claims. Constructed from the parsed value, so a renamed
    /// or re-tagged variant fails here rather than in a lead's session.
    #[test]
    fn every_kind_prints_a_whole_valid_file_with_the_trigger_it_names() {
        for kind in ExampleKind::ALL {
            let text = example_file(Some(*kind));
            let entries = parse_actions_yml(&text, PROJECT)
                .unwrap_or_else(|error| panic!("{kind:?} example: {error}"));
            assert_eq!(entries.len(), 1, "{kind:?} prints exactly one action");
            let matches = matches!(
                (*kind, &entries[0].def.trigger),
                (ExampleKind::ManualVerify, TriggerDef::Manual)
                    | (ExampleKind::RefUpdated, TriggerDef::RefUpdated { .. })
                    | (ExampleKind::Schedule, TriggerDef::Schedule { .. })
                    | (ExampleKind::CiResult, TriggerDef::CiResult { .. })
            );
            assert!(matches, "{kind:?} printed {:?}", entries[0].def.trigger);
        }
    }

    /// The verify example is the one ledger 184 exists for: it must declare a
    /// bound checkout, or a run could answer with whatever tree it found.
    #[test]
    fn the_verify_example_requires_a_bound_checkout() {
        let entries =
            parse_actions_yml(&example_file(Some(ExampleKind::ManualVerify)), PROJECT).expect("ok");
        assert_eq!(
            entries[0].def.step_requiring_bound_checkout(),
            Some("verify")
        );
        let ActionDef::RunOnHost { ref command, .. } = entries[0].def.steps[0].action else {
            panic!("the verify step runs on a host");
        };
        assert_eq!(
            command,
            &["just", "ci"],
            "command is argv, not a shell line"
        );
    }

    /// Ledger 206 A: with **this** parser a bare `on:` is the string "on",
    /// in block style and in flow style alike. The boolean trap is a YAML 1.1
    /// artefact, so the detector must stay silent on a file that is merely
    /// wrong in some other way, and speak only when a `true` key is really
    /// sitting where `on` belongs.
    #[test]
    fn the_boolean_on_trap_is_named_only_when_a_true_key_is_really_there() {
        let unquoted = "schema: buzz-project-actions/v1\nactions:\n  - name: verify\n    trigger: { on: manual }\n    steps:\n      - id: s\n        action: send_message\n        text: hi\n";
        parse_actions_yml(unquoted, PROJECT).expect("a bare `on` parses under serde_yaml 0.9");
        assert!(boolean_on_trap(unquoted).is_none());

        let quoted = unquoted.replace("{ on: manual }", "{ \"on\": manual }");
        parse_actions_yml(&quoted, PROJECT).expect("quoting it changes nothing here");

        // What a YAML 1.1 emitter writes back.
        let round_tripped = unquoted.replace("{ on: manual }", "{ true: manual }");
        let error = parse_actions_yml(&round_tripped, PROJECT).expect_err("refused");
        assert!(error.to_string().contains("missing field `on`"), "{error}");
        assert_eq!(
            boolean_on_trap(&round_tripped).as_deref(),
            Some("action \"verify\"")
        );
        let explained =
            explain_actions_error("actions.yml", &round_tripped, &error.to_string()).to_string();
        assert!(
            explained.contains("`true:` key where `on:` belongs"),
            "{explained}"
        );
        assert!(explained.contains("bee actions example"), "{explained}");
    }

    /// A trigger at the wrong level reports `missing field `on`` too, and the
    /// answer for that one is where the key goes — not the 1.1 story.
    #[test]
    fn a_trigger_without_on_is_told_where_the_key_goes() {
        let text = "schema: buzz-project-actions/v1\nactions:\n  - name: verify\n    trigger: { type: manual }\n    steps:\n      - id: s\n        action: send_message\n        text: hi\n";
        let error = parse_actions_yml(text, PROJECT).expect_err("refused");
        let explained = explain_actions_error("actions.yml", text, &error.to_string()).to_string();
        assert!(
            explained.contains("belongs INSIDE `trigger`"),
            "{explained}"
        );
        assert!(!explained.contains("`true:` key"), "{explained}");
        assert!(explained.contains("bee actions example"), "{explained}");
    }

    /// Every `missing field` lists the whole object, so eight keys cost one
    /// read rather than eight writes (ledger 182's rule, applied here).
    #[test]
    fn a_missing_field_lists_every_required_key_of_its_object() {
        let text = "schema: buzz-project-actions/v1\nactions:\n  - trigger: { on: manual }\n    steps:\n      - id: s\n        action: send_message\n        text: hi\n";
        let error = parse_actions_yml(text, PROJECT).expect_err("refused");
        let explained = explain_actions_error("actions.yml", text, &error.to_string()).to_string();
        assert!(explained.contains("name, trigger, steps"), "{explained}");
    }

    /// The required-key table is only worth printing if it is true: drop each
    /// listed key from a minimal file and the parser must complain about that
    /// key. Guards against the table outliving a schema change.
    #[test]
    fn every_listed_required_field_is_really_required() {
        // (the file the key is dropped from, the key, the object's label)
        let file = "schema: buzz-project-actions/v1\nactions:\n  - name: a\n    trigger:\n      on: manual\n    steps:\n      - id: s\n        action: send_message\n        text: hi\n";
        // Each row removes exactly one required key, keeping the document
        // otherwise well-formed (a dropped list-item dash would fail as a
        // syntax error instead, proving nothing).
        for (from, to, key) in [
            ("schema: buzz-project-actions/v1\n", "", "schema"),
            ("  - name: a\n", "  - description: a\n", "name"),
            ("        text: hi\n", "", "text"),
            ("      - id: s\n", "      - name: s\n", "id"),
        ] {
            let broken = file.replacen(from, to, 1);
            let error = parse_actions_yml(&broken, PROJECT)
                .expect_err("dropping a required key must be refused")
                .to_string();
            assert!(
                error.contains(&format!("missing field `{key}`")),
                "dropping {key}: {error}"
            );
            assert!(
                object_requiring(key).is_some(),
                "{key} is missing from REQUIRED_FIELDS"
            );
        }
        // `steps` and `trigger` are the other two of each action's three.
        for key in ["steps", "trigger"] {
            assert!(object_requiring(key).is_some(), "{key}");
        }
    }

    /// The seeded agents repository ships a commented verify action; it must
    /// parse once uncommented, or the example a new project finds is a lie.
    /// The text and the uncommenting rule live in `buzz_persona::seed`; this
    /// is the crate that can call the real parser on the result.
    #[test]
    fn the_seeded_actions_file_example_parses_when_uncommented() {
        let seeded = buzz_persona::seed::seeded_actions_yml();
        let uncommented = buzz_persona::seed::uncomment_seeded_actions_example(&seeded);
        let entries = parse_actions_yml(&uncommented, PROJECT)
            .expect("the seeded commented example parses once uncommented");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "verify");
        assert_eq!(
            entries[0].def.step_requiring_bound_checkout(),
            Some("verify"),
            "the seeded example keeps `checkout: required`"
        );
    }
}
