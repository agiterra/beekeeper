//! `bee plans example` — a complete, valid, task-neutral `beekeeper-plan/v1`
//! file this binary prints offline.
//!
//! A lead writing its first plan needs the file's shape, not somebody else's
//! plan. Before this command the only complete plans in reach were earlier
//! projects' answers (ledger 270(g): run 10's lead read a sibling project's
//! plan before drafting its own), so the shape was learned by copying content
//! the new goal never asked for.
//!
//! [`EXAMPLE`] is an example, not a template to commit: every value a plan
//! must decide is marked `SUBSTITUTE` in a comment beside it, and the body
//! says so. It carries no project, customer or task. It is parsed by the real
//! [`buzz_core::project_plan::parse_plan`] and judged adoptable by
//! [`buzz_core::project_plan::check_plan_adoptable`] in this module's tests,
//! and the one proof form it only describes (`action`) is parsed there too,
//! so neither can drift from the contract (`conformance/project-work/README.md`
//! § (a)). Like `bee actions example`, it reaches no relay and needs no key,
//! so it is dispatched ahead of the key gate.

use crate::error::CliError;

/// The whole example file, frontmatter and body.
pub const EXAMPLE: &str = "\
---
# beekeeper-plan/v1 — an EXAMPLE of the shape, not a plan to commit as-is.
# Save it as plans/<id>.md in this project's agents repository and replace
# every value marked SUBSTITUTE with what this project's goal asks for.
# The keys are a closed set: an unknown key is refused, and every key below
# is required (retired_criteria is written even when empty).
schema: beekeeper-plan/v1
id: example-plan            # SUBSTITUTE: a slug (a-z, 0-9, inner hyphens), unique in the project
status: in-force            # in-force (adoptable) or superseded (refuses new adoption)
title: Example outcome      # SUBSTITUTE: one line naming the outcome the goal asks for
code_repository: example-code-repository  # SUBSTITUTE: the id of the code repository the work lands in
delivery_ref: refs/heads/main              # the full ref the delivery is judged at
criteria:
  # SUBSTITUTE: one entry per obligation the goal sets. `id` is a stable slug;
  # `accept` is what a reviewer reads to judge it (at most 1024 bytes);
  # `proof` is the evidence that answers it, in one of three forms:
  #   {kind: review}                       a ruling by a person or authorized seat
  #   {kind: git-ref}                      the delivery_ref observed at the delivered commit
  #   {kind: action, name: verify, step: verify}
  #                                        a step of an action in this repository's
  #                                        actions.yml (see `bee actions example`);
  #                                        adoption refuses until that action exists
  - id: example-behaviour
    accept: >-
      SUBSTITUTE: the observable behaviour a reviewer checks to judge that the
      goal's outcome works.
    proof: {kind: review}
  - id: delivered
    accept: The accepted work is on the delivery ref.
    proof: {kind: git-ref}
retired_criteria: []        # ids removed from criteria; never reused
---
# Example outcome

This is an example of the plan format. Replace this body with the goal in
your own words: what the outcome is for, the constraints that matter and what
is out of scope. People and seats read the body; nothing parses it.

Check a working copy with `bee sessions work validate --plan plans/<id>.md
--agents-repo <dir>`, draft it with `bee plans edit <id> --file <file>`, and
land it with `bee agents-repo commit`.
";

/// The `action` proof line [`EXAMPLE`] describes in a comment, exactly as a
/// plan would write it. Held here so the tests can prove it parses.
#[cfg(test)]
pub(crate) const DESCRIBED_ACTION_PROOF: &str = "{kind: action, name: verify, step: verify}";

/// `bee plans example` — print the example and exit 0. No relay, no key.
///
/// # Errors
/// Never; the signature matches the other offline dispatchers.
pub fn cmd_example() -> Result<(), CliError> {
    print!("{EXAMPLE}");
    Ok(())
}

#[cfg(test)]
#[path = "plans_example_tests.rs"]
mod tests;
