# docs/design/portable-team-loop

One document, and it is here for a reason.

[`POLICY.md`](POLICY.md) is a **contract**, not a plan: it states what a
kind-44245 session policy enforces, ten places in `crates/` cite it by path,
and `crates/beekeeper-cli/tests/policy_enforcement_sentence.rs` reads it off disk to
assert that the sentence a founder is shown is byte-identical to the one the
code implements. A test cannot bind a file in another repository, so this one
stays with the code — the same rule that kept `docs/nips/` and `conformance/`
here when the plans left on 2026-09-22.

Its siblings were plans and went with the rest: `PLAN.md` and
`TEAM_WAKE_DURABILITY.md` are `plans/archive/` in `bee-keeper-beekeeper-agents`.
POLICY.md went with them for one commit by mistake, which broke that test; see
ledger 242.
