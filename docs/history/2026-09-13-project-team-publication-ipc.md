# Project team publication IPC correction — September 13

## Live finding

Brian linked his local Tank Loop checkout, ran the project-setup authoring
session, and returned to the saved draft. The installed host validated six
roles: builder, designer, lead, project-setup, runner and verifier. The checked
version was saved as `32510e8086e97504c6a55a8c5e8f7fea284ec4fb474c257735e45f4d57ca7e72`.

Clicking Publish checked version failed at native argument decoding:
`unknown field snapshotId, expected snapshot_id`. This is before the command
body reserves or publishes anything; the saved snapshot is independent of
that command and need not be recreated. Evidence is Brian's September 13
screenshot, attachment `562cdeeb-7232-42db-903d-fcb5603d6cbd.png` (local-only).

## Cause and correction

The TypeScript request correctly follows the specified camelCase IPC shape.
Serde's enum `rename_all = "snake_case"` controls variant names, not the fields
inside struct variants. The native Snapshot field consequently expected
`snapshot_id`; Expected had the same latent issue with `event_id`.

Both fields now have explicit camelCase wire names and accept their former
snake_case spelling as a deserialization alias for existing local journals.
Variant discriminants remain unchanged, and unknown fields remain rejected.
The saved snapshot bytes, IDs, publication authorization and relay logic do
not change. Tests must exercise browser-shaped JSON through native decoding,
not only Rust struct construction or mocked IPC.

## UX observations from this walkthrough

- The draft places Check draft above Author the project team, obscuring the
  intended next action after preparation.
- Returning from authoring leaves the Roles page displaying built-in defaults
  without a visible pointer to the unpublished project draft.
- The published destination and snapshot hash dominate the publication card;
  this live failure appears as a raw IPC error.

These observations are recorded for follow-up; this correction changes only
serialization and its regression coverage. No live publication was retried
by Astra, and no draft or Tank Loop repository files were modified.

## Verification

The new native JSON tests were run against the pre-fix type: one failed with
Brian's exact `unknown field snapshotId, expected snapshot_id` error and one
legacy-input test passed. Restoring the correction made both pass. The broader
focused publication filter passed 11 tests; native format and all-target Tauri
clippy passed. Local red/green logs are under
`../review-2026-09-13-final-ci-sol/publication-ipc-{red,green}.log`. No full
smoke run was needed for this serialization correction.
