# Project team activation final adversarial review — 2026-09-13

Reviewed the uncommitted activation snapshot in
`review-project-team-activation-astra` against
`PROJECT_TEAM_ACTIVATION_SLICE.md` and `PROJECT_TEAM_PUBLICATION_IMPL.md`.
This was a bounded code review; I did not rerun the reported native/UI tests,
full CI, smoke, or any live relay operation.

## Blocking findings

1. **An absent-source project cannot use the UI, while the public IPC accepts
   an untrusted destination the UI never offered.** Publication options
   deliberately return no destination when no source exists
   (`project_team_setup_publication.rs:127-130,846-858`), and the UI renders a
   terminal blocked message (`ProjectTeamSetupPublication.tsx:292-297`). That
   blocks the first Tankloop workflow. At the same time, Start accepts
   `destination` and `sourceExpectation` from the webview and, for `IfUnset`,
   checks only that the source is absent; it never binds repo/path/base to a
   host-issued choice (`project_team_setup_publication.rs:703-750`). A direct
   IPC caller can therefore choose any repository and pack path for which the
   owner has write permission. Minimal fix: resolve an endorsed project
   repository and destination in native code for the absent-source case, issue
   an opaque host-bound choice/reservation, and make Start accept only that
   binding (or reject Start whenever options had no destination).

2. **Lead launch does not stage the adopted immutable revision.** Start resolves
   and checks the adopted source (`project_team_setup_activation.rs:537-553`),
   but calls `stage_coding_session_actor_seat` with both `pack_source` and
   `checkout` set to `None` (`:716-724`). The staging command consequently
   falls back to the installed agent's mutable `persona_team_dir`
   (`actor_seats.rs:829-842,150-187`). That path lives in a checkout keyed only
   by repository and `sync_packs_checkout` moves it between revisions
   (`packs_cache.rs:354-355,455-468`). Another install can therefore change the
   bytes this lead receives, and the seat lacks the promised project `packRef`.
   Minimal fix: pass the journal's repo/SHA/path as `ProjectPackSourceInput` and
   refuse unless the returned staged `packRef` exactly equals that source and
   role before submitting the create event.

3. **A lost source-submit response can turn a successful adoption into a false
   conflict.** `adopt_source` persists `SourceUnknown`, but retry immediately
   resubmits the same conditional event (`project_team_setup_publication.rs:658-685`).
   If the first request succeeded, its `IfUnset`/expected predecessor no longer
   matches the live head, so the relay can return `PACK_SOURCE_CONFLICT`; the
   host records Conflict without first checking whether the saved event is
   already effective (`:685-693`). Minimal fix: reconcile the effective source
   before every resend. Return Adopted when it is the saved event, Superseded
   when a later event wins, and resend only when observation proves the old
   expectation still holds.

4. **Installation is not crash-idempotent and can mint a second lead.** The
   command journals only an empty team ID before effects
   (`project_team_setup_activation.rs:480-495`). The installer then mints in
   memory and saves personas, agents, and teams as three independent writes
   (`:297-325`), recording the installed pubkeys in the publication journal
   only afterward (`:326-340,497-503`). A crash after `save_personas` but before
   `save_managed_agents` leaves no agent record for
   `existing_agent_for_team` to find, so retry mints another identity. Minimal
   fix: durably reserve the exact minted installation records before the first
   store mutation and replay them, or use one recoverable transaction covering
   the three stores and journal projection.

5. **The Git commit is not proven to contain the selected snapshot's exact
   bytes.** Verified owned bytes are copied into a worktree, then `git add
   --all` stages the mutable files (`project_team_setup_publication.rs:524-548`).
   Base `.gitattributes` clean/text filters can change the resulting blobs, and
   no post-commit tree/blob equality check exists. Minimal fix: construct pack
   blobs from the owned byte buffers with the hardened stdin Git seam, or at
   minimum disable filters and compare every committed blob byte-for-byte with
   the captured manifest before journaling the commit.

6. **Closing after channel creation can still cause a duplicate channel.** The
   created ID is retained only in React state
   (`ProjectTeamSetupPublication.tsx:86-88,247-265`). If creation succeeds but
   recording fails before the host persists `journal.lead`, closing the
   workbench loses the only retry handle; reopening reports `needs_channel` and
   offers Create again (`:393-421`). Minimal fix: make native activation own
   create-and-record under its durable reservation, or persist/recover the
   created project channel ID before allowing another creation.

7. **Matching Start is not a durable same-request retry after adoption.** Start
   checks the caller's captured expectation against the current source before
   locking and loading the existing journal
   (`project_team_setup_publication.rs:721-751`). Once this operation adopts
   its own source, retrying the identical lost-response request fails
   `source_changed`; the matching-journal return is reached only afterward
   (`:752-758`). Minimal fix: under the publication lock, load and compare the
   existing request first and return it unchanged; perform live expectation
   checks only when reserving a new operation.

8. **Candidate creation has an unrecoverable crash window.** The candidate
   directory is renamed into place before its SHA is returned
   (`project_team_setup_publication.rs:549-562`), while the SHA is journaled
   later (`:809-813`). A crash between those steps leaves status `Checking`, a
   candidate directory, and no recorded commit. Continue does nothing for
   `Checking` (`:863-908`), and rebuilding refuses because the directory exists
   (`:473-477`). Minimal fix: make Continue reconcile an existing candidate by
   verifying its tree and deriving its commit, then journal it; otherwise build
   under a deterministic recoverable marker whose finalized SHA is validated
   on restart.

These are release blockers for the stated first activation path. The reported
focused checks cover the happy-path projection but do not exercise the failure
windows above.

## Corrective re-review

The 2026-09-13 corrective diff closes all eight findings above for the scoped
first-project path:

- Native options now derive the absent-source repository from the canonical
  project and active owner; Start compares the complete caller input with that
  host result after returning an identical saved request first
  (`project_team_setup_publication.rs:389-430,759-802`). The exact 30617 is
  journaled before submission (`project_team_setup_publication_announcement.rs:97-149,187-244`).
- Candidate construction hashes the verified buffers through stdin, builds an
  explicit index, proves every pack blob and every outside-pack base entry, and
  re-verifies an existing candidate before adopting its recovered HEAD
  (`project_team_setup_publication_git.rs:32-47,101-178,181-208,210-329`;
  `project_team_setup_publication.rs:687-751`).
- A retained source event is reconciled before any retry and is not blindly
  resubmitted after an uncertain result
  (`project_team_setup_publication.rs:626-684`).
- Installation encrypts and saves every fresh role identity before the first
  persona/agent/team store write, then recovers those same identities on retry
  (`project_team_setup_activation.rs:192-304,484-601`). Lead staging supplies
  the adopted repo/SHA/path and verifies the resulting full role `PackRef`
  before event submission (`:306-335,1214-1225`).
- Channel creation is now a native activation operation: it journals one UUID
  and signed event before relay submission, replays only those bytes, and
  refuses a stale adopted source before reservation and again before replay
  (`project_team_setup_activation.rs:724-937`). The UI calls only this scoped
  ensure operation (`ProjectTeamSetupPublication.tsx:222-237`).

No remaining release blocker was found in the frozen corrective diff. Evidence
reported by the implementation/finalization lanes: the focused exact-blob and
candidate-recovery native test passed; the activation-focused native tests
passed; 12 focused UI tests and TypeScript typechecking passed; and the rebuilt
single browser flow passed draft publication through install, durable channel,
lead start, close, and reopen. This review did not independently rerun those
commands or expand into full CI, smoke, or live Tankloop changes.
