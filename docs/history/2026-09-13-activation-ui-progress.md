# Project team activation — UI progress

Started 2026-09-13 in `review-project-team-activation-astra` on
`work/project-team-activation-astra`.

## Implemented before this continuation

- The project Roles workbench can save a checked snapshot, obtain host-resolved
  publication options, publish the explicit `{ kind: "snapshot", snapshotId }`
  output, display the observed source state, install roles from an adopted
  immutable source, and call the project-lead handoff IPC.
- Component coverage records the scoped publication, installation, and lead
  handoff calls. The browser fixture covers the draft-to-lead path.

## Integrated contract

The initial UI candidate created a missing project session channel through the
existing `create_channel` API, then only wrote the returned ID into React state.
That would lose the channel on reopen and could offer a duplicate. The host now
owns `project_team_setup_record_lead_channel`: it verifies the channel's signed
project binding, records exactly one channel under the publication, accepts a
same-channel replay, and refuses a different channel. The UI calls it
immediately after `create_channel` and only enables lead start from the returned
durable activation. A transient failed record keeps the one created ID in the
open workbench so retry does not create another channel.

The adopted-source display now uses the activation's host-resolved repository,
immutable commit, and pack path. It no longer calls a candidate commit the
adopted source before the device has resolved it.

## Validation

- `cd desktop && tsc --noEmit` passed on 2026-09-13.
- `cd desktop && node --import ./test-loader.mjs --experimental-strip-types
  --test src/features/roles/lib/projectTeamSetupComponent.test.mjs` passed:
  7 tests.
- `cd desktop && pnpm build:e2e`, followed by the one smoke-project browser
  flow `project-team-setup.spec.ts --grep 'checked draft publishes, installs
  the adopted source, and starts one project lead'`, passed on 2026-09-13.
  The flow publishes a checked snapshot, installs the exact adopted source,
  creates and durably records the session channel, starts the lead, closes and
  reopens the workbench, then confirms the same source and lead outcome with no
  second channel action.

The worktree had no `desktop/node_modules`. Verification temporarily linked to
the already-present sibling worktree's matching dependencies and removed that
link afterwards; no package installation or purge was run.

## Remaining

- Native focused compilation and receipt reconciliation are still in the host
  lane. No commit, push, app install, or live Tankloop change has occurred.

## Final review corrections in the active root run

The generic `create_channel` followed by a separate record call left a crash
window. The UI now calls only `project_team_setup_ensure_lead_channel(scope)`;
the host owns the saved channel UUID and signed request. Unknown channel setup
(with no lead session reference) retries that same operation. The updated
component test asserts zero generic channel-create calls. Seven component tests
and TypeScript passed after this wiring; final browser verification follows
native completion.

The browser fixture now represents the first-project case: no existing source,
an offered new repository, and `if_unset` adoption. Native tests separately
must prove that the host actually offers this destination; a mock is not that
proof. Static draft/snapshot copy now defers to publication status below rather
than continuing to say “not published” after adoption.
