# Project team publication blocker closure

2026-09-13. Scope: host publication only in the activation candidate. No live
Tankloop write, Git push, application install, commit, or remote change was
performed.

The first-project path now derives its destination inside the native host from
the canonical project coordinate and active owner. The repository id retains a
readable slug but ends in a 12-hex digest of the full coordinate, avoiding
cross-founder same-slug collisions. Publication options return that exact
destination with a host-derived `30617` announcement. Start loads a matching
saved journal under the publication lock before consulting the current source,
then rejects any webview-supplied destination or expectation that differs from
the fresh host derivation.

Initial repository announcements are retained as exact signed bytes in a
separate scoped journal before submission. Candidate Git trees are assembled
from the reverified snapshot buffers with `hash-object --stdin` and explicit
index entries; no `git add` sees a mutable pack worktree. The final tree is
checked against every owned blob, and non-pack entries must equal the captured
base. Recovery re-verifies an already-finalized candidate directory before it
records its SHA. Source retries reconcile the exact saved event first and keep
an unobservable event unknown rather than resubmitting a stale CAS request.
Options and status reads reconcile a retained source event against the live
source before returning `adopted`; a later winner is `superseded`. A fresh
neutral draft refuses existing-source replacement until source-seeded
maintenance provenance exists, while a saved publication stays visible.

Focused evidence after the activation split settles: `cargo fmt` passed.
`candidate_uses_owned_snapshot_bytes_and_conditional_source_pins_its_commit`,
`candidate_blobs_ignore_gitattributes_and_recovery_reverifies_them`, and
`absent_source_destination_is_project_qualified_not_slug_only` passed before
the final combined compile. After the activation channel/projection visibility
repair, the final focused joined run also passed:
`cargo fmt --manifest-path desktop/src-tauri/Cargo.toml && cargo test
--manifest-path desktop/src-tauri/Cargo.toml --lib
candidate_blobs_ignore_gitattributes_and_recovery_reverifies_them` (one test
passed; 3,278 filtered). Terminal output was not redirected to a persistent
local log file.

Final cached focused coverage after all activation changes:
`cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib
project_team_setup::publication` passed 9 tests (0 failed, 3,270 filtered) on
2026-09-13. It covered byte-preserving candidate construction/recovery,
host-derived first-project naming, matching-journal recovery, source pinning,
adopted immutable lead staging, durable encrypted role-plan recovery, and
reserved channel UUID/signed-event reopen.
