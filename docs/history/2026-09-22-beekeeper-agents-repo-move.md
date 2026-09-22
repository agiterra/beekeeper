# Beekeeper's own documents move into its agents repository — the manifest

*2026-09-22, Andy with Opus. Reviewed before anything moves; nothing in this
report has been applied.*

Beekeeper's roles and plans still live in the code repository, which decision 5
(spec § 1 item 5, 2026-09-18) says they should not: an agent working on the code
trips over the plans, and an out-of-date one can steer it. The product half —
moving a pre-pivot project's **roles** into `<slug>-beekeeper-agents` — is built
and pinned (ledger 233). This is the editorial half: which **documents** go, and
where each lands.

Nothing here is a product feature. It is one commit in each repository, and it
cannot be made until `bee-keeper-beekeeper-agents` exists on hive, because the
code repository must not stop naming the map before there is a map to name.

## The constraints that shape it

- **The draft path grammar has no nested directory under `plans/`**: only
  `plans/<name>.md` and `plans/archive/<name>.md`
  (`crates/buzz-core/src/agents_repo_draft.rs`). So `docs/history/` flattens
  into `plans/archive/` with its filenames unchanged — they are already dated,
  so they still sort.
- **A draft carries a whole file, capped at 60,000 bytes of text and 65,536 of
  content.** Anything larger is readable in the Files tab and editable only
  through git. The tables below say which is which.
- **The map's ceiling travels with it.** `scripts/check-current-state-size.mjs`
  guards a file that is leaving; `team.yml` `limits:` replaces it (ledger 233),
  and the seeded manifest must carry:

  ```yaml
  limits:
    - { path: plans/CURRENT_STATE.md, max_lines: 300, max_bytes: 24000 }
  ```

## In force → `plans/`

2,003,615 bytes, of which the ledger is 1,780,438.

| file | bytes | Files tab |
| --- | --- | --- |
| `CURRENT_STATE.md` | 23,915 | editable |
| `SESSION_STATE.md` | 1,780,438 | git only |
| `PROJECT_TEAMS_AND_ACTIONS_SPEC.md` | 79,734 | git only |
| `UNIFIED_WORK_PLAN.md` | 25,956 | editable |
| `PROJECT_TEAM_SETUP_IMPL.md` | 20,880 | editable |
| `PROJECT_TEAM_ACTIVATION_SLICE.md` | 3,638 | editable |
| `PROJECT_AGENT_HIRING_IMPL.md` | 17,625 | editable |
| `COLLABORATIVE_WORKSPACE_PLAN.md` | 28,025 | editable |
| `NATIVE_STEERING_IMPL.md` | 23,404 | editable |

## Retired → `plans/archive/`

43 plans and specs (1,030,478 bytes), plus every dated report
under `docs/history/` (38 files), the eight under `docs/archive/`, and
the three under `docs/design/portable-team-loop/`. `docs/history/README.md`
becomes `plans/archive/README.md` and keeps the convention sentence — including
its rule that reports written before 2026-09-11 stay at the top of the ledger
and are not moved.

<details><summary>The 43 plans and specs</summary>

| file | bytes | Files tab |
| --- | --- | --- |
| `AGENT_PROGRESS_UI_DESIGN_NOTE.md` | 35,808 | editable |
| `AGENT_SIDEBAR_SURFACE_PICKER_DESIGN.md` | 30,825 | editable |
| `CI_COMPLETION_IMPLEMENTATION.md` | 6,316 | editable |
| `CI_COMPLETION_USAGE.md` | 9,808 | editable |
| `CI_CONTINUATION_RECOVERY_SPEC.md` | 15,699 | editable |
| `CI_MANAGED_CONTINUATION_IMPL.md` | 19,195 | editable |
| `CI_MANAGED_CONTINUATION_SPEC.md` | 3,959 | editable |
| `CODING_SESSION_UI_CONVERGENCE_HANDOFF.md` | 16,800 | editable |
| `CONTEXTUAL_SESSIONS_IMPL.md` | 13,544 | editable |
| `CONTEXT_REDACTION_DISPLAY_DESIGN.md` | 22,469 | editable |
| `CREW_SESSIONS_PLAN.md` | 96,448 | git only |
| `DECLARED_WORK_PULSE_IMPL.md` | 16,844 | editable |
| `DUAL_STREAM_THESIS_RESEARCH_2026-08-19.md` | 81,439 | git only |
| `FABLE_SESSION_CONTINUITY_RESEARCH_BRIEF.md` | 9,429 | editable |
| `FABLE_SESSION_CONTINUITY_RESEARCH_REPORT.md` | 74,028 | git only |
| `HANDOVER_IMPL.md` | 25,902 | editable |
| `LIVE-RUN-4.md` | 67 | editable |
| `P1_CONTINUITY_MATRIX.md` | 14,859 | editable |
| `P1_JUDGE_SCRIPT.md` | 4,623 | editable |
| `PEOPLE_SETUP_IMPL.md` | 8,323 | editable |
| `PROJECT_AGENTS_TAB_SPEC.md` | 5,438 | editable |
| `PROJECT_PULSE_SESSION_LEASE_IMPLEMENTATION_PLAN_2026-08-20.md` | 13,366 | editable |
| `PROJECT_PULSE_TRUTH_FIRST_IMPLEMENTATION_PLAN_2026-08-19.md` | 81,189 | git only |
| `PROJECT_TEAM_PUBLICATION_IMPL.md` | 23,473 | editable |
| `REHYDRATION_HARDENING_IMPLEMENTATION_PLAN_2026-08-19.md` | 86,668 | git only |
| `ROLES_DESIGN_SPEC.md` | 11,603 | editable |
| `ROLES_USABILITY_SPEC.md` | 12,677 | editable |
| `ROLE_EVIDENCE_RECOVERY_SPEC.md` | 3,650 | editable |
| `ROLE_OPERATOR_COMMISSIONING_SPEC.md` | 13,917 | editable |
| `ROLE_PROVENANCE_SPEC.md` | 17,201 | editable |
| `SESSION_DESIGN_PHASE_PLAN.md` | 43,717 | editable |
| `SESSION_EXECUTION_PLAN.md` | 41,314 | editable |
| `SESSION_HANDOFF_SOL.md` | 22,579 | editable |
| `SESSION_HANDOFF_SOL_2026-08-14.md` | 12,213 | editable |
| `SESSION_NATIVE_SUBSTRATE.md` | 14,472 | editable |
| `SESSION_NEXT_PHASE_BRIEF.md` | 16,290 | editable |
| `SESSION_PATH.md` | 17,380 | editable |
| `SESSION_PHASE_HANDOFF_2026-08-16.md` | 15,812 | editable |
| `SESSION_PHASE_HANDOFF_2026-08-17.md` | 16,624 | editable |
| `SESSION_PHASE_HANDOFF_2026-08-18.md` | 7,179 | editable |
| `SESSION_STEP4_DESIGN.md` | 29,819 | editable |
| `TANK_LOOP_WALKTHROUGH_IMPL.md` | 6,188 | editable |
| `WORK_COORDINATION_VISIBILITY_SPEC.md` | 11,324 | editable |

</details>

## Staying in the code repository

`docs/nips/` (product contracts, and `NIP-MP.fixtures.json` is `include_str!`'d
by the SDK and the relay), `conformance/`, `VISION*.md`, `SESSION_VISION.md`,
`TESTING.md`, `CONTRIBUTING.md`, `ARCHITECTURE.md`, `RELEASING.md`,
`INTEGRATION.md`, `CONTEXT.md`, `GOVERNANCE.md`, `README.md`,
`personas/templates/` and `personas/roles/` (the neutral shipped foundation),
and `team/registry-bench/`. `team/model-registry.yaml` is copied to the agents
repository's root, which `model_registry_source.rs` already prefers, and the
code repository keeps its copy because the seed embeds it with `include_str!`.

Twenty-five technical notes and runbooks also stay — `multi-tenant-relay.md`,
`git-on-object-storage.md`, `local-desktop-instances.md` and the like. They
describe how the software works, not what the team is doing.

**Three judgment calls worth a second opinion** before the commit, because they
read either way: `PULSE_SLICE1_ACCEPTANCE_RUNBOOK.md`,
`COLLABORATION_TWO_MACHINE_ACCEPTANCE.md` and `ROLE_ADOPTION_EVIDENCE.md` are
acceptance evidence for work that landed, so they could as easily be archive.
This manifest leaves them in the code repository; say the word and they move.

## What the code repository's own commit does

1. `git rm` everything in the two tables above.
2. Rewrite `AGENTS.md`'s top block: the map and the ledger now live in
   `bee-keeper-beekeeper-agents`; a seat reaches them in its `<worktree>-agents`
   clone; a person reaches them with `just agents-repo`, which clones or
   fast-forwards the repository to the sibling `../agiterra-beekeeper-agents`.
   A sibling, not a path inside the checkout, so the code tree keeps the
   property decision 5 bought.
3. Delete `scripts/check-current-state-size.mjs` and its test, the
   `current-state-check` recipe (`justfile`) and its pre-push step
   (`lefthook.yml`) — the ceiling is `team.yml` `limits:` from then on.
4. Fix `docs/CREW_ROLES.md`'s pointer to the retired in-repo `beekeeper-project`
   skill, which has been stale since that pack moved to `agiterra-packs`.

## The order, and why it is this order

The agents repository must exist and hold the documents **before** the code
repository stops naming them. Concretely:

```
# 1. Andy, from an installed build carrying ledger 233:
#    Project settings → Packs → "Move this project's roles into an agents repository"
#    (or, with the CLI, after `bee pack migrate`:)
bee packs init --project 30621:6cbdf445…:bee-keeper --layout flat \
  --from <converted tree> --expect-source 2f400ee7b8f8e3913fd4eebe15c9a3848d1402d59bfd1337fffb5bba271e1c5f

# 2. Clone it, add the documents and the `limits:` block, validate, push:
bee agents-repo check --root . --project 30621:6cbdf445…:bee-keeper

# 3. Then, and only then, the code repository's commit.
```

The live source was re-read on 2026-09-22 and is unchanged: event
`2f400ee7b8f8e3913fd4eebe15c9a3848d1402d59bfd1337fffb5bba271e1c5f`, naming
Brian's `30617:3d3b7169…:agiterra-packs` at `personas/roles`. Step 1 re-points a
project Brian's seats read, which is why it is his and Andy's to run, not a
session's.

