# Project team setup: host publication contract

2026-09-12. Prepared for Astra; milestone 2 only. This is a proposed build
contract, not implementation or executed acceptance evidence. Source reviewed:
`review-project-team-setup-astra` at `342f16b80`, now combined into
`work/steering-integration-astra`. The governing plan is
[PROJECT_TEAM_SETUP_IMPL.md](PROJECT_TEAM_SETUP_IMPL.md). Source review corrected
Start retry identity, output-attempt binding and maintenance seed provenance.
This tracked proposal is ready for the next implementation slice; no host
publication coordinator has been built from it.

## Outcome and limits

Publish one checked snapshot's exact bytes to an isolated Git candidate ref,
verify the remote commit and conditionally adopt its immutable SHA with 30624 v2.
Reopening/restarting reveals the same recorded run and outcome.

No installation, roster minting, lead launch, automatic role restaging, shared
branch update, baseline merge, production deployment or Solo change belongs in
this slice. Existing-source maintenance uses the same publication transaction,
not init. A role label, model response or repository file grants no authority.

## 1. Scope is captured before effects

Start first takes a setup-scoped reservation/index lock keyed by canonical
relay, owner, project and setup ID. Fingerprint the canonical Start request and
preserved authoring reservation, including predecessor ID/null; exclude minted
operation IDs and later target/stage observations from this versioned hash.
Before authoring, Git or relay effects it durably reserves the input fingerprint,
one operation ID and fixed inputs in an owner-sealed index. Matching Start,
including a lost-response retry, returns that original operation, even if done.
Initial Start requires `predecessorPublicationId: null`. A later explicit New
publication supplies the current terminal predecessor ID and fresh scope.
Under the same index lock, exactly one successor is reserved: matching replay
returns it; another fingerprint conflicts. Unresolved/active predecessors cannot
be bypassed. Preserve the chain/index; never invent a new ID because a caller
lost its response. Lock order is index then operation; release the index before
network work. Reserved-but-uninitialized records can finish journal creation
from sealed inputs; a missing initialized journal refuses, never re-reserves.

The reserved scope captures and owner-seals:

- version and host-minted publication ID; setup ID; normalized project
  coordinate; canonical relay URL; owner public key;
- preserved authoring reservation/create ID, genesis/session reference,
  provider key/instance, setup actor and exact target generation when verified;
- destination repository coordinate and validated clone URL on that relay;
  whether a new announcement is authorized; its exact name/description;
- expected source ID, explicitly null for no source, plus the decoded current
  repository/pin/path and its signed event when a source exists;
- pack path and immutable Git base commit, when preserving an existing tree;
  a host-derived candidate branch under `refs/heads/setup/<publication-id>`;
- permission to publish one validated output from this authoring run and adopt
  its SHA. This explicitly describes any switch from the existing pin/ref.

The destination is host-selected from the project/current source and the
operator's scoped choice, never from agent output. Existing non-pack repository
content is preserved from the captured base. A new repository has no base.
Repository and source authorization remain relay-enforced at each write.

For existing-source maintenance, fetch the captured source commit and seed a
new isolated editable pack subtree from its exact configured path, preserving
custom procedures, files and role identities. Record source event/repo/pin,
resolved commit, tree digest and retained role identities as seed provenance.
Neutral shipped content is a seed only when the captured source is absent.
The current `prepare` always seeds shipped content; it is not this maintenance
path. Never overwrite an existing draft or silently replace project procedures.
Derive an operation workspace under the authoring seat's existing write fence;
the same execution can read its relative path in the host's recorded prompt.

Opening, status refresh and receipt reconciliation are read-only. Closing the
workbench neither succeeds nor cancels the run. Starting includes ordinary
validation/publication retries within the same recorded scope; no per-stage
approval dialogs. Changed destination, expectation, authoring target or selected
output is a different explicit operation, never a retry mutation.

Milestone-1 authoring explicitly did not authorize publication. Preserve old
identities/events/bootstrap and require captured scope, never upgrade the grant
silently. A scoped Start may select an existing checked snapshot as operator
output, not agent-bound completion. New runs receive the contract in their local
brief; existing executions receive it through a recorded scoped prompt.

## 2. Completion binds bytes; it does not authorize publication

Use existing provider-signed kind 44225 transcript evidence, not a new relay
kind, new agent HTTP service or prose substring such as “done”. The producer
already emits a terminal `result` for a turn; the prompt echo carries its
command ID (`buzz-session-provider/src/lib.rs:6929`, transcript item contract
`codingSessionTranscriptItemContract.ts:43,114`).

Before EVERY host authoring/correction prompt, reserve an output attempt under
the operation lock: monotonic attempt ID, exact command ID, same execution,
prompt digest and exact signed command bytes, all durable before publication.
The initial attempt may bind the preserved create command only when its exact
initial prompt names this operation/attempt. Otherwise use an ordinary signed
turn command in the existing session. Repeated calls carry the expected previous
attempt ID: the same prompt fingerprint returns its reserved successor; changed
input conflicts. This is no new model driver or replacement session.

A correction atomically marks the old attempt ineligible before sending the new
one. Only the current attempt's matching prompt echo, readiness and terminal
result may bind output; ordinary composer turns lack this record and cannot
trigger automatic publication. Once a snapshot is accepted, the publication
accepts no further attempt/snapshot; later correction requires explicit New
publication through its predecessor transition. Late old readiness never wins.

Lock the following small authoring output protocol:

1. The local brief names `PROJECT_TEAM_SETUP_RESULT.json` in the editable draft
   root, outside the roles subtree. Its strict, bounded JSON contains a schema
   `schema: "beekeeper-project-team-setup-result/v1"`, `setupId`, `authoringId`,
   `publicationId`, `outputAttemptId`,
   `files: [{path, length, sha256}]` sorted by path, `acceptanceCommands: string[]`
   and `unresolvedFacts: string[]`. Files are relative to the roles root and the
   list is complete. Commands/facts are declarations, not host instructions.
2. After finishing writes, the authoring turn emits one structured readiness
   record in assistant text, delimited by `<project-team-setup-ready>` and its
   closing tag. Its only fields are `schema`, `setupId`, `authoringId`,
   `publicationId`, `outputAttemptId` and `resultSha256` (exact result-file digest).
   Schema is `beekeeper-project-team-setup-ready/v1`. No destination,
   credentials, permissions or source condition can appear in this record.
3. The host assembles the bounded ordered assistant text for that turn and
   accepts exactly one such record. It verifies event signatures, kind/tags,
   channel, provider, target/generation, turn ID and the host-recorded prompt
   command ID. It also requires that turn's later provider-signed terminal
   `result` be successful. A duplicate/contradictory record, cancelled/error
   result, foreign execution, incomplete transcript query or missing binding
   remains unready. Idle metadata and a successful create receipt do not count.
4. The host reads the result file with the existing no-link/bounded rules,
   checks its bytes against the signed readiness digest, captures all actual
   role files and checks the complete declared manifest. It then runs the real
   loader validation and creates the ordinary content-addressed snapshot.
5. Before any publication, the owner-sealed journal binds readiness evidence
   IDs, terminal event ID, result-file digest and the resulting snapshot ID.
   Later draft edits cannot alter the candidate. The journal does not claim the
   host observed who physically wrote each local file or proved suitability.

This readiness record is only a request to examine a particular output. The
pre-existing host grant supplies authority; deterministic verification supplies
the byte binding. A forged local completion file without matching authorized
provider evidence causes zero publication. Matching model declarations still
cannot enlarge the grant or cause host execution of declared test commands.

Bound result JSON and readiness text to 256 KiB each; duplicate/unknown JSON
fields refuse. Reuse snapshot bounds and the existing transcript decoder.
Invalid attempts remain recorded; corrections never overwrite signed prompts.

## 3. Exact snapshot-to-Git boundary

Snapshot `run(..., Some(id))` reverifies manifest/files/loader but returns paths.
Add a bounded internal capture with snapshot metadata, canonical manifest bytes
and owned `(relative_path, bytes)` entries; no arbitrary paths or bytes in IPC.

Construct Git blobs/trees from owned bytes with regular-file modes using a
byte-preserving/stdin extension of the hardened runner. Never `git add` a mutable
source after validation. Prove committed pack paths/blobs equal the manifest and
non-pack entries equal the captured base.

Freeze commit identity/message/parent/tree/timestamps before construction so
recovery recreates the same commit. Persist SHA/local candidate ref before push.
Verify existing objects; never remove/reseed a checkout or expose host keys.

## 4. Journal and irreversible boundaries

Keep a dedicated versioned journal in the setup scope's private sibling
storage, outside the editable draft. Reuse the launch journal's owner seal,
regular-file/size checks, process lock, atomic replace and file/directory fsync.
The setup reservation index maps predecessor/fingerprint to publication ID and
tracks reserved versus initialized; journal directories preserve that history.
Both index and operation marker must agree before any effect or recovery.

The journal schema is `beekeeper-project-team-setup-publication/v1`, with
unknown/duplicate fields refused and a 4 MiB serialized limit. Fixed fields are
immutable after capture; only the initially absent verified target, selected
snapshot and unsigned-yet event slots may be filled once. Stage fields append
or advance observations. Journal contents include:

- reservation fingerprint/predecessor, complete scope and seed provenance;
- append-only output attempts, their signed prompts and accepted-attempt pointer;
- verified manifest/snapshot identity and fixed commit construction inputs;
- exact signed repository announcement, when creating a repository;
- candidate ref, commit SHA, push intent and verified remote-ref observation;
- exact signed conditional source event, source-send intent and acknowledgement;
- last verified effective source ID, reconciliation time and named error.

Persist each uncertainty BEFORE the corresponding network effect. Persist exact
signed event bytes before sending them. Save progress after each verified stage.
Never journal credentials, NIP-98 headers or agent-private key material. Journal
signing uses captured owner keys; each resumed stage rechecks active scope and
current authority. Use one process lock per publication operation.

UI states: awaiting output, checking, prepared, announcement unknown, push
unknown, pushed, source unknown, adopted, superseded, conflict or refused.
Adopted requires this effective source event and repository/SHA/path; an ACK
alone is insufficient. Historical acceptance, structural validity, procedure
suitability and installed teams remain separate facts.

## 5. Git and source transaction

1. Verify current source/base expectations before starting publication work.
   For a new repository, save and submit one exact 30617 announcement; reconcile
   its stored signed identity on uncertain responses. Existing announcements
   are read/verified, not overwritten. Preserve project forward-roster authority
   rules; a repository backlink is not a publication grant.
2. Push only the recorded commit to its new candidate branch, with an explicit
   absent-ref lease (`--force-with-lease=<candidate-ref>:`). Existing code uses
   that create-only pattern in `project_git_branches.rs:93`. On retry, an exact
   remote ref/SHA match reconciles success; any different SHA is a conflict.
   Failed/uncertain push never switches the source or tombstones the repository.
3. Verify the remote candidate SHA. The source names its immutable commit with
   `PackPin::Sha`; the candidate ref keeps that commit reachable. Never move an
   already-adopted branch, including `main`. Candidate cleanup is outside scope.
4. Read the effective live source across authors using verified strict decoding
   and ordering `created_at DESC, id ASC`. Capture complete evidence or fail
   unknown; a truncated query is not proof that no source exists. Recheck the
   captured expectation. Do not silently substitute whatever is current.
5. Build v2 with core `build_conditional_project_pack_source`, expected ID/null,
   exact repository/SHA/path. Choose an allowed timestamp strictly later than
   the expected head before signing, avoiding random same-second ID ordering.
   If clock/admission bounds make that impossible, wait/report clock conflict;
   do not sign repeatedly. Freeze signed bytes in the journal, then submit.
6. HTTP 409 with `PACK_SOURCE_CONFLICT` is a source conflict; authentication or
   authorization refusal stays distinct. Never downgrade to v1 or fresh-sign
   after conflict. Reconcile the effective source after success before describing
   adoption, and preserve a later observed replacement/deletion honestly.

The source transaction protects the recorded source event comparison. The Git
create-only lease protects the candidate ref. There is no claimed atomic
transaction across Git and Nostr. The captured base is an immutable input, not
a promise to lock an existing moving branch. This slice never updates such a
branch; that would require a separate expected-Git-head operation.

This deliberately supersedes plan decision 8's moving-branch default for this
host transaction: adopt an immutable SHA atomically, as the later reviewed
publication contract requires. Future improvements use a new scoped publication
and source CAS; fresh executions resolve that source normally. Preserve an old
pin/ref until its explicit scoped adoption succeeds. No overlay resolver or
silent branch-following pointer is added. Track this decision in the plan when
implementing; the proposal alone changes no product behavior.

## 6. Restart, ambiguity and retries

- Read/status commands may query Git and relay evidence but perform no writes,
  provision no provider and sign no replacement events.
- An explicit retry uses the same grant, snapshot, Git commit/ref and signed
  announcement/source. Reconcile before repeating any uncertain stage.
- If the candidate ref already names the commit, skip push. If the exact source
  is current, report adopted. If the exact event was accepted but another source
  is current, report superseded; a retained-event acknowledgement is not adoption.
- A deleted/superseded event may no longer appear in ordinary visible queries.
  Absence alone does not prove it was never accepted. Preserve unknown when the
  API cannot establish history, and never resurrect it by rebuilding an event.
- Expired admission timestamp, unavailable reconciliation, missing/tampered
  journal or changed identity/community blocks replay with an actionable reason.
  Do not re-sign to evade the timestamp window. A later explicit new operation
  may capture a new expectation; that is not an automatic retry.
- Crash after a push but before source submission leaves an unadopted candidate;
  it does not require another repository or removal of the successful push.

## 7. Minimal implementation seams and ownership

| Owner | Exclusive files |
| --- | --- |
| Host publication | New `managed_agents/project_team_setup_publication.rs`, `_reservation.rs`, `_journal.rs`, `_git.rs`, `_wire.rs`, `_completion.rs` and focused tests; owns setup reservation, operation/attempt transitions and source-seeded workspace provenance |
| Setup UI | New `features/roles/ui/ProjectTeamSetupPublication.tsx`; existing `ProjectTeamSetupAuthoring.tsx`, `ProjectTeamSetupDraftView.tsx`, `ProjectTeamSetupWorkbench.tsx`, `lib/projectTeamSetup.ts`, `lib/projectTeamSetupApi.ts` and their component tests |
| Finalizer | `project_team_setup.rs`, snapshot/launch/authoring internal accessors, `handlers.rs`, narrow `project_git_exec.rs` byte API and `packs_repo.rs` visibility; operation-workspace/fence binding and preserved bootstrap; E2E bridge/registration, integration tests and tracked plan/map |

Lock these IPC names; all return the public journal projection:

- `project_team_setup_start_publication({projectRef, setupId, expectedRelayUrl,
  predecessorPublicationId, destination, sourceExpectation, output})` reserves
  or returns the identical operation under the setup index lock, then advances.
- `project_team_setup_get_publication({projectRef, setupId, expectedRelayUrl,
  publicationId})` only reads/reconciles.
- `project_team_setup_continue_publication({projectRef, setupId,
  expectedRelayUrl, publicationId})` advances the original operation unchanged.
- `project_team_setup_prompt_publication({projectRef, setupId, expectedRelayUrl,
  publicationId, expectedPreviousAttemptId, instruction})` reserves/replays a
  host-bound authoring or correction turn before sending it in the same session.

`destination` is `{repoRef, packPath, baseCommit, createAnnouncement}`:
`baseCommit` is an immutable SHA or explicit null; `createAnnouncement` is null
for existing repositories or `{name, description}` for a new owner repository.
The clone URL and candidate ref are host-derived. `sourceExpectation` is a
required discriminated value `{kind:"if_unset"}` or `{kind:"expected", eventId}`;
this avoids treating an omitted IPC field as permission to create. It maps to
v2's explicit null or expected ID. `output` is `{kind:"authoring"}` or
`{kind:"snapshot", snapshotId}`. The latter is honestly operator-selected;
it cannot masquerade as signed authoring completion. Maintenance snapshots must
carry matching captured source-seed provenance; a shipped-seeded copy cannot pass.
The frontend supplies IDs and bounded choices, never keys, paths or execution
proof. Reopen reads the setup index to recover the operation ID. One Start can
advance ordinary stages; Continue handles interruption, not per-stage approval.
Corrections reuse session UX but invoke the recorded-prompt seam, not an ordinary
unbound composer send. New publication explicitly names its predecessor.

Native preparation must accept a sealed source-seed binding and derive an
operation-specific editable path; snapshot validation uses its captured role
identities, never the shipped list for maintenance. Extend trusted snapshot
accessors for this derived path, not arbitrary IPC paths. Show seed revision,
current attempt and pending/accepted output distinctly. Existing-source load
failure blocks maintenance; no fallback to shipped seed or overwrite of a draft.

Reuse scoped context, snapshot/launch proof, core v2 and hardened Git/relay
helpers; never call init, shipped reseeding or the TS v1 publisher. No generic
ACL/job framework/registry/driver. New files stay under 1,000 lines; lanes do
not commit or edit another lane.

## 8. Concrete acceptance before integration

Deterministic composition tests use fake adapters, local isolated Git servers
and the existing mock relay/IPC boundary. No live Tankloop mutation is needed.

1. Saved snapshot X is published even after draft Y appears; tampered snapshot,
   symlink, extra file, duplicate JSON field, oversized result or wrong manifest
   produces zero announcement/push/source effects.
2. A valid readiness digest + prompt/terminal chain binds the exact snapshot.
   Foreign signer/target/channel/generation/command, cancelled or error terminal,
   contradictory markers and a local forged result file produce zero effects.
   Procedure text requesting another repo/key/command cannot enlarge scope.
3. Read/open/reload makes zero writes. One explicit scoped Start progresses
   without repeated approvals. An older ungranted draft stays ungranted.
4. Commit-tree bytes match the manifest, including non-ASCII/binary bytes;
   mutation of source paths during construction cannot alter the commit.
   Existing repository files outside the authorized pack path are unchanged.
5. Concurrent Starts and lost Start responses return one operation/commit/event
   set. Crash between index reservation/journal initialization recovers that ID;
   missing initialized journal refuses. Changed input conflicts. Concurrent New
   publication against one predecessor produces one successor, never two.
6. Initial and correction attempts preserve exact command IDs on retry. Late
   initial readiness, an ordinary unbound composer turn and foreign attempt IDs
   cannot satisfy the current attempt. Concurrent corrections deduplicate; after
   acceptance no attempt can bind a second snapshot to the publication.
7. Inject crashes before/after every durable/network boundary. Announcement
   timeout, accepted push with lost response and accepted source with lost
   response reconcile without duplicate identities/repos or fresh signed events.
8. Candidate ref absent creates once; same SHA reconciles; different SHA refuses.
   The adopted moving branch is unchanged even when source CAS then conflicts.
9. Competing source publication, including v1, causes named CAS conflict;
   deletion/supersession/exact retained retry never becomes false adoption.
   Same-second expected head produces one valid later timestamp, not nonce
   grinding. Expired signed retry never triggers fresh signing.
10. Community/owner switch, revoked authority, missing journal/marker mismatch
   and failed reconciliation preserve the recorded run and stop effects.
11. Browser workflow shows checked/pushed/adopted/superseded/unknown separately,
    reopens after each partial failure and keeps actionable errors visible at
    narrow/enlarged text. Existing Solo before/after still makes zero setup calls.
12. Maintenance seeds custom roles/procedures from the captured existing source
    SHA/path, preserves unrelated files and never copies shipped defaults over
    them. Missing/inaccessible/moved seed refuses; absent-source setup alone uses
    neutral shipped seed. Reopen preserves seed provenance and edited bytes.

Run targeted tests, then applicable final gates. Real adapter/live publication
acceptance remains owed; this proposal records no implementation or test run.
