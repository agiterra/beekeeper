# Project team activation — native progress

Started 2026-09-13 in `review-project-team-activation-astra` at
`1669550d2`, on `work/project-team-activation-astra`.

The saved candidate already implements snapshot publication and role
installation in `project_team_setup_publication.rs`. The desktop UI invokes
`project_team_setup_start_lead`, but the native command is absent. This lane
will finish a durable, project-qualified lead handoff using the existing role
installer and seat-launch APIs, without committing, pushing, installing an
application, or changing Tankloop.

Initial source review is in progress. Validation is limited to focused native
tests and formatting, as required by `PROJECT_TEAM_ACTIVATION_SLICE.md`.

## 2026-09-13 update

Native registration now includes `project_team_setup_record_lead_channel` and
`project_team_setup_start_lead`. Channel recording verifies the signed
project/channel relationship, pins one channel to the publication, and
refuses a different channel on replay. Lead start binds the installed `lead`
identity to the adopted repository/SHA, validates owner and provider scope,
persists one session/genesis/create request before provider or relay effects,
then stages that exact installed role seat and submits only the retained event
bytes. Activation reads reconcile a signed provider receipt without writing a
replacement request, so reopening can report `started` from observed evidence.

The activation seam is now isolated in
`project_team_setup_activation.rs` (768 lines); the publication coordinator is
953 lines. `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml`, `cargo
check --manifest-path desktop/src-tauri/Cargo.toml`, and the three focused
`publication::tests` tests passed at 2026-09-13. The tests cover selected
snapshot bytes/pin, request rejection, and reopen projection of the retained
lead/channel identity. No live relay, Tankloop, app installation, Git push, or
source commit was performed.

## Review follow-up — 2026-09-13

The activation journal now reserves every role's exact identity as
owner-encrypted recovery material before the persona, managed-agent, or team
stores change. A retry decrypts that same plan; the publication journal never
contains a plaintext nsec. Lead staging now supplies the adopted repository,
SHA, and pack path to the actor-seat stage and refuses a returned `packRef`
that is not the exact `repo`/`sha`/`lead`/`path` binding.

The setup-specific `project_team_setup_ensure_lead_channel` operation is being
added in the activation seam. It reserves a UUID and exact signed private
transport channel create event before submission, then observes or replays
only those bytes. The browser no longer owns a channel ID. Native focused
compile/test evidence remains pending while the concurrent publication lane
finishes its shared journal test module.
