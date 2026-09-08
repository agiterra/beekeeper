# CI managed continuation — Fable execution slice

Product contract: VISION_COLLABORATION.md and COLLABORATIVE_WORKSPACE_PLAN step 2a.
Base: 985fca952. Worktree: ../review-ci-managed-continuation-fable.
Root owns Roles/shared-role evidence in parallel. Findings go to SESSION_STATE;
send proposed ledger additions in the coordination mailbox for root integration.

## User outcome

A managed agent registers an exact CI run and an authorized exact session target,
ends its turn, and receives the result with its continuation when CI finishes.
No model polls and no foreground shell has to survive the build. This completes
the missing vertical over existing CI results; do not rebuild the result producer.

## Required mechanism and acceptance

- Reuse signed 44220 intents, truthful 44224 receipts, provider durable queue and
  operation/target fences. Persist registration before acknowledging it.
- Correlate exact project/repository/commit/check/run/attempt/workflow/phase and
  full driver/instance/session/generation target; never use latest/HEAD shorthand.
- Verify relay signer and strict result identity; materialize result, evidence,
  registration identity and requested continuation into the awakened context.
- Recover missed results after provider restart, including result already stored
  when registered. Duplicate results or alternate command IDs cannot spend a
  second turn for the same operation and target. State the crash/dispatch guarantee
  precisely; do not claim arbitrary model side effects execute exactly once.
- Revalidate current steering authority at delivery. Revocation and stale target
  generation have durable visible dispositions, not silent retargeting. Existing
  standing agent grants work without a human click.
- Resolve private-project CI read capability separately from session steering.
  Reuse existing scoped capabilities; no broad membership or visibility expansion,
  no borrowed human credentials, and no inference that a CI signer can steer.
  Implement the narrow authorized path if existing mechanism supports it; expose
  a precise unsupported/refusal outcome otherwise and continue independent work.
- Registration must return and release the turn; no detached generic daemon,
  extending MCP/provider clocks or general-purpose agent solely for waiting.
- Test success/failure/cancelled, stored/live, restart before/after result, duplicate
  registration/result, wrong identity/signer/phase, revoked authority, private read
  refusal, stale generation. Exercise actual CLI/provider/local relay composition
  with local isolated data. Do not change production workflow/deployment settings.

## Ownership

Fable may orchestrate bounded build/review lanes with strict file ownership.
Own new continuation spec, core continuation/command contract, CLI registration
and status, provider state/command/event-loop/discovery/delivery and their tests.
Narrow SDK/relay schema parity is allowed when necessary; claim exact files in
mailbox first and proceed unless root has a conflicting claim. Core producer
semantics of CI result kind46008 stay unchanged. Own no desktop Roles files,
role staging/resolution, role provenance or commissioning projection. Minimal
strict desktop command decoder parity may be required: name exact paths first.
Do not start mobile/UI redesign or generic task registry.

Use isolated worktree, Hermit, separate Cargo target/cache and limited jobs to
avoid memory pressure. Root uses desktop browser port4175; Fable may use4176.
Write/refine your implementation spec before parallel builders. Finalizer alone
commits with signoff. Deliver candidate commit, exact validation logs, remaining
limits and a reproducible acceptance command. Do not push, deploy or replace
Brian's app; root integrates and performs final packaging/landing checks.

Coordination: /Users/brian/Desktop/BEEKEEPER-ASTRA-FABLE-COORDINATION.md.
Existing Fable t3 thread: d5211a94-2560-4b54-b6fc-e2940601b807.
