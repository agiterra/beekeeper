# Project work — v1 frozen contract

This directory is the normative contract for durable project work: the plan
file that states what success means, the signed records that bind assignments
and evidence to it, the fold that answers "what remains", the CLI that writes
and reads it, and the brief a seat receives.

It is **data and documentation only**. No parser, no fold and no command lives
here. Lane W1 implements the parser and the fold in `buzz-core`, W2 the CLI,
W3 the brief, W5 the surface — all against the fixtures in `fixtures/`. The
one piece of code this lane landed is the kind constant,
`KIND_PROJECT_WORK_RECORD = 44249` in `crates/buzz-core/src/kind.rs`.

**This README is normative until `docs/nips/NIP-PW.md` exists.** Lane W1
writes that NIP from this file when it implements the kind; the NIP must not
diverge from what is written here, and where it would, the amendment is made
here first.

The NIP now exists: [`docs/nips/NIP-PW.md`](../../docs/nips/NIP-PW.md), written
from this file by lane W1 (ledger item 195). This file stays normative.

Governing plan: `plans/UNIFIED_WORK_PLAN.md` (agents repository)
(W0 in § 3, Wave 0 in § 4). Design source: Astra's
the unified plan (`plans/archive/2026-09-20-astra-unified-plan.md`, agents repository) § 2.
Ledger item 194.

**Frozen means frozen.** After Fable and Astra review it, this file changes
only by an amendment recorded in `UNIFIED_WORK_PLAN.md` § 8. A lane that finds
a defect here reports it; it does not edit around it.

---

## (a) `beekeeper-plan/v1` — the plan file

A UTF-8 Markdown file at `plans/<slug>.md` in the project's agents repository
(`<slug>-beekeeper-agents`, spec § 4.11). YAML frontmatter is the contract;
the body is context for people and seats and is never parsed.

### Frontmatter keys

Exactly these keys. An unknown key is a refusal, never an ignored key.

| key | type | required | rule |
|---|---|---|---|
| `schema` | string | yes | exactly `beekeeper-plan/v1` |
| `id` | slug | yes | plan id, unique within the project, slug grammar below, ≤ 64 bytes |
| `status` | enum | yes | `in-force` \| `superseded` |
| `title` | string | yes | 1–200 bytes, single line |
| `code_repository` | string | yes | a repository id (1–64 bytes, `[a-z0-9][a-z0-9-]*`), resolved to a project-associated repository at adoption |
| `delivery_ref` | string | yes | a full git ref, e.g. `refs/heads/main`; 1–256 bytes; no glob |
| `criteria` | list | yes | 1–64 entries, order is presentation only |
| `retired_criteria` | list of slug | yes | may be empty; default `[]` is **written**, not implied |

Each `criteria` entry has exactly three keys:

| key | type | rule |
|---|---|---|
| `id` | slug | unique among `criteria` **and** disjoint from `retired_criteria`; ≤ 64 bytes |
| `accept` | string | non-empty after trimming; 1–1024 bytes; what a person reads to judge |
| `proof` | map | exactly one of the three forms below |

### The three `proof` forms

```yaml
proof: {kind: review}                              # a person or an authorized
                                                   # seat rules on it
proof: {kind: action, name: verify, step: verify}  # a named action + step in
                                                   # the same agents repo's
                                                   # actions.yml
proof: {kind: git-ref}                             # the delivery_ref is
                                                   # observed at the artifact
                                                   # commit
```

- `review` — keys: `kind`. Nothing else.
- `action` — keys: `kind`, `name`, `step`. Both are slugs, ≤ 64 bytes, and
  must resolve in `actions.yml` **at the same agents commit** the plan is
  adopted at (spec § 5.1). A missing action blocks *adoption*, not drafting.
- `git-ref` — keys: `kind`. The ref comes from `delivery_ref`; a criterion
  does not get its own.

`proof` is an evidence *requirement*. It is not a command, not a condition
language and not a dependency graph. All criteria are required; a waiver is an
explicit plan amendment, never a checkbox.

### Slug grammar and id rules

```
slug := [a-z0-9] ( [a-z0-9-]* [a-z0-9] )?      # 1..64 bytes, ASCII only
```

Lowercase, digits and internal hyphens. No leading/trailing hyphen, no
underscore, no dot, no slash, no uppercase.

Ids name obligations, not positions:

1. **Stable across reorder.** Moving a criterion up the list changes nothing.
2. **Stable across a file move or rename.** `plans/kettle.md` →
   `plans/kettle-cli.md` preserves every id; the *declaration* pins the old
   path at the old commit and the amendment pins the new one.
3. **Spelling corrections to `accept` keep the id.** A *substantive* change to
   an obligation may keep its id, but requires new evidence under the newly
   adopted revision — see the amendment rule in (c). This is why every
   evidence binding names a declaration, a criterion id **and** an artifact
   commit at once: matching ids alone cannot carry an old pass forward.
4. **A removed or replaced obligation moves to `retired_criteria`** and is
   **never recycled** in that plan. A retired id reappearing under `criteria`
   is a refusal (`recycled-retired-id`).
5. New obligations get new ids.

### Limits

| limit | value | refusal code |
|---|---|---|
| file size | 65 536 bytes (64 KiB) | `plan-too-large` |
| criteria count | 64 | `too-many-criteria` |
| retired count | 256 | `too-many-retired` |
| `accept` length | 1024 bytes | `accept-too-long` |
| slug length | 64 bytes | `slug-too-long` |

Also refused: unknown `schema`; unknown frontmatter key; unknown key inside a
criterion or a `proof`; duplicate criterion id; active/retired overlap; empty
or whitespace-only `accept`; a `name`/`step` that is not a slug (an absolute
path, a `..`, a shell string); a symlinked plan path; a path escaping `plans/`;
a malformed `code_repository` or `delivery_ref`.

### `status`, and where the bytes are read from

- `status` governs **new adoption only.** `superseded`, and any file under
  `plans/archive/`, refuse a new `adopt` and nothing else. Declarations
  already adopted still resolve their own commit and path and keep running.
- Editing, renaming or archiving a file **does not** cancel, amend or rewrite
  work already adopted. A new agents commit means only "a new revision is
  available".
- The git commit is the version. There is no second version counter.
- **The blob is read with `git show <commit>:<path>`, never the working copy
  and never the fetched tip.** The existing agents-file reader reads the tip
  (`crates/buzz-session-provider/src/agents_checkout.rs:98`) and must not be
  reused unchanged for contracts. A plan the fold cannot read at the pinned
  commit yields `unknown`, never `open` and never `covered`.

The worked example is
[`fixtures/plans/valid/kettle.md`](fixtures/plans/valid/kettle.md).

---

## (b) The sibling work-event kind — 44249

One kind carries all three work records. 44244 stays closed and unchanged.

### The closed envelope

| property | value |
|---|---|
| kind | `44249` (`KIND_PROJECT_WORK_RECORD`) |
| class | regular stored event, append-only; never replaceable, never ephemeral |
| schema string | `buzz-project-work/v1` (in content **and** the `pwk-v` tag) |
| content | strict public JSON, `deny_unknown_fields`, camelCase |
| tags | **exactly six** two-field tags, in this order |

```
["h",           "<channel uuid>"]              # the session's channel: the
                                               # membership gate, as 44244
["d",           "<sessionRef uuid>"]           # == content.sessionRef
["a",           "30621:<64-hex>:<d>"]          # == content.projectRef
["pwk-v",       "buzz-project-work/v1"]
["pwk-genesis", "<64-hex genesis event id>"]   # == content.genesisRef
["pwk-type",    "work.declared" | "work.assignment_bound"
                                 | "work.evidence_bound"]  # == content.type
```

Tag-to-content parity is checked on every read, exactly as 44244 checks it
(`coding_session_team_transaction_decode.rs:178-187`). A tag that disagrees
with content is a refusal, not a preference.

**Why `d` is the sessionRef.** Every sibling kind (44244, 44245, 44246,
44247) sets `d` to the session uuid, so one REQ
`{"kinds":[44249],"#d":["<sessionRef>"]}` returns everything the fold needs
using a query shape every reader already builds. `workId` is in content, where
the fold indexes it; it is stable across amendments and groups a declaration
with its successors without needing its own tag.

**What the `a` tag is and is not.** It is a canonical, singleton project
coordinate so a reader can select one project's work. It does **not** put
44249 in `is_project_a_scoped_kind` (44240 and 44248), and this lane does not
add it there. Those kinds are gated by project membership *alone, with no
channel*; 44249 is gated by channel membership through `h`, like its siblings.
Saying otherwise would claim a relay gate that does not exist.

**Authority is not the relay's question.** The relay validates *structure
only*, as it does for 44244/44245/44246/44247. Whether the signer held
standing is the consuming fold's question against the accepted NIP-CSAT chain.

### `work.declared`

```json
{
  "schema": "buzz-project-work/v1",
  "sessionRef": "<uuid>",
  "genesisRef": "<64-hex>",
  "projectRef": "30621:<64-hex>:<d>",
  "type": "work.declared",
  "body": {
    "workId": "<uuid>",
    "goalRef": "<64-hex>",
    "decisionRef": null,
    "responsibleActor": "<64-hex pubkey>",
    "planRef": {
      "repository": "30617:<64-hex owner>:<repo id>",
      "commit": "<40- or 64-hex>",
      "path": "plans/<slug>.md"
    },
    "supersedes": []
  }
}
```

| key | type | null? | rule |
|---|---|---|---|
| `workId` | uuid string | no | canonical lowercase; **stable across amendments** |
| `goalRef` | 64-hex | no | the session's kind:44227 **goal** event, and only that. Never a decision: `state` compares this with the session's current goal, so a decision id here would make the declaration permanently stale |
| `decisionRef` | 64-hex or `null` | **yes** | the 44244 `decision.answer` (or other decision record) that authorized this adoption, when one did. Always present, written `null` when none |
| `responsibleActor` | 64-hex | no | the actor who owes the outcome; a target, never an authorship claim |
| `planRef.repository` | coordinate | no | the **full** kind:30617 coordinate `30617:<64-hex owner>:<repo id>` of the agents repository — the same shape the seat manifest's `packRef.repo` uses. A bare repo id is refused: a name is community-scoped and would have to be re-resolved by every later reader, which is how two readers end up pinning two repositories |
| `planRef.commit` | hex | no | **full** immutable agents commit, 40 or 64 hex, lowercase |
| `planRef.path` | string | no | relative, ≤ 256 bytes, starts `plans/`, no `..`, no leading `/` |
| `supersedes` | list of 64-hex | no (may be `[]`) | ≤ 8 declaration event ids; empty on first adoption, one for an ordinary amendment, **all** conflicting heads for an explicit conflict resolution |

Criteria text is never copied onto the wire. The plan id is read from the
blob, not restated here.

**Which layer refuses a decision id in `goalRef`.** Both are 64-hex, so the
envelope cannot tell them apart and accepts the record: the refusal belongs to
the **fold**, which is the only layer holding the session's goal set. A
declaration whose `goalRef` is not in that set is excluded with
`goal_ref_not_a_goal` and the remedy is named — `goalRef` the goal,
`decisionRef` the decision. Fixture: `sequences/goal-ref-not-a-goal/`.

### `work.assignment_bound`

```json
{
  "...": "envelope as above",
  "type": "work.assignment_bound",
  "body": {
    "declarationRef": "<64-hex>",
    "criterionIds": ["cli-behaviour"],
    "assignmentRef": "<64-hex>",
    "replacesBinding": null
  }
}
```

| key | type | null? | rule |
|---|---|---|---|
| `declarationRef` | 64-hex | no | the `work.declared` event this binds to |
| `criterionIds` | list of slug | no | 1–64, unique, each ≤ 64 bytes, each must exist in the declaration's plan (a fold check, not a relay check) |
| `assignmentRef` | 64-hex | no | a kind:44244 `assignment` event id |
| `replacesBinding` | 64-hex or `null` | **yes** | the earlier binding this supersedes; the key is always present, written as JSON `null` when there is none |

### `work.evidence_bound`

```json
{
  "...": "envelope as above",
  "type": "work.evidence_bound",
  "body": {
    "declarationRef": "<64-hex>",
    "criterionIds": ["verified-landed-revision"],
    "artifactCommit": "<40- or 64-hex>",
    "evidenceRefs": [{"kind": "verdict", "eventId": "<64-hex>"}],
    "completionRef": null
  }
}
```

| key | type | null? | rule |
|---|---|---|---|
| `declarationRef` | 64-hex | no | as above |
| `criterionIds` | list of slug | no | 1–64, unique |
| `artifactCommit` | hex | no | the **code** commit the evidence is about, 40 or 64 hex, lowercase |
| `evidenceRefs` | list of map | no | 1–32 entries, each exactly `{"kind": …, "eventId": "<64-hex>"}` |
| `evidenceRefs[].kind` | enum | no | `report` \| `verdict` \| `action_result` \| `ref_observation` |
| `completionRef` | 64-hex or `null` | **yes** | the kind:44244 `mission.completed` this coverage was computed for; always present, `null` when none |

Every nullable key is **present** and written as `null`. An absent key is a
refusal — the same rule 44244's bodies carry, and the reason lane 182 could
print a complete example for every body.

### `ref_observation`, and how delivery is judged

A `ref_observation` evidence ref carries **no new record**. Its `eventId` is a
**relay-signed kind:30618 ref-state event** for the plan's `code_repository`:

- the 30618's signer must equal the relay's NIP-11 `self` key — the relay is
  the authoritative source of ref state for repositories it hosts
  (`crates/buzz-relay/src/api/git/manifest_event.rs`), and an owner-signed
  claim about its own branch is not an observation;
- its `d` tag names the plan's `code_repository`;
- its `refs/heads/<branch of delivery_ref>` tag must equal the binding's
  `artifactCommit`.

**Freshness is judged at evaluation time, not at binding time.** The *newest*
30618 for that repository must still name that commit for that ref. An older
matching 30618 that a newer one has superseded with a different commit makes
the criterion `stale` — the branch moved on, and what was delivered is no
longer what is there. Ref state that cannot be read makes it `unknown`. The
fixture pair is `sequences/superseded-observation/`.

### Who may sign

All three records require the **same** authority: the `may_lead` predicate the
44244 fold already implements
(`crates/buzz-core/src/coding_session_team_transaction_fold.rs`
`CodingSessionTeamFoldContext::may_lead`). It admits **three** actors, and
this contract adds no fourth and subtracts none:

1. the session **founder** (`context.founderPubkey`);
2. an actor holding an **active `lead` seat** (`activeSeats`, role `lead`);
3. an actor holding an **active grant with `maySteer`** (`activeGrants`) —
   a steer-grantee needs no lead seat.

The third was missing from this file's first draft, and a narrower contract
here would have split the CLI from the relay, which is exactly the failure a
frozen contract exists to prevent. The scope is still the named project and
session: repository write alone is not sufficient, and a lead seat in a
*different* session is not sufficient. Fixtures: in `sequences/happy-path/`,
`bind6` is signed by a steer-grantee with no seat and is **accepted**, while
`bind9` is signed by a plain worker and is excluded `signer_not_may_lead`.

One rule for three records, on purpose: an association is a **claim the fold
must verify**, never proof and never authority. A worker states what it did in
its 44244 `report`; the lead binds that report to a criterion. Letting a
worker bind its own evidence would let it nominate what its work proves.

A record signed by anyone else is excluded by the fold with a named reason; it
is not a fold-wide error and it never silently becomes coverage.

### Size ceilings

| limit | value |
|---|---|
| content, total | 16 384 bytes (16 KiB) |
| `criterionIds` | 64 entries, ≤ 64 bytes each |
| `evidenceRefs` | 32 entries |
| `supersedes` | 8 entries |
| `planRef.path` | 256 bytes |
| `planRef.repository` | 128 bytes |
| `projectRef` | 256 bytes |

16 KiB, not 44244's 128 KiB: every field here is a pointer or a slug. A record
that needs more prose is carrying something that belongs in the plan blob or
in a 44244 report.

Unknown keys are rejected at every level — payload, body and `planRef` /
`evidenceRefs` entries.

---

## (c) The fold's output

A pure function of the input below. No clock, no network, no ordering
assumption: the same events in any arrival order fold to the same output.

### The input

The fold reads **no events but the 44249 records**. Everything else arrives as
facts the caller established and verified from existing events — the fold does
not re-verify signatures or re-resolve pointers it was not given, and it never
infers a passing test from an event id.

```json
{
  "relaySelfKey": "<64-hex>",
  "authority": {
    "founderPubkey": "<64-hex>",
    "activeSeats":  [{"actorPubkey": "<64-hex>", "role": "lead"}],
    "activeGrants": [{"actorPubkey": "<64-hex>", "grantEventRef": "<64-hex>",
                      "maySteer": true}]
  },
  "currentGoalRef": "<64-hex>",
  "goalEvents": ["<64-hex>"],
  "planBlobs": {"<30617 coordinate>@<commit>:plans/x.md": "<blob bytes>"},
  "actionDefinitions": {
    "<30617 coordinate>@<commit>#verify": {"definitionHash": "<64-hex>",
                                           "steps": ["verify"]}
  },
  "teamProjection": {
    "includedEventIds": ["<64-hex>"],
    "assignments": {"<assignment event id>": {"assigneeActor": "<64-hex>",
                                              "assigneeRole": "builder"}}
  },
  "evidence": {"<event id>": { "…one fact per kind…" }},
  "refStates": [ "<relay-signed kind:30618 events>" ]
}
```

- **`authority`** is the 44244 fold's own context shape, so one predicate
  serves both folds.
- **`goalEvents`** is the session's kind:44227 goal set — what makes
  `goal_ref_not_a_goal` decidable.
- **`actionDefinitions`** is compiled **by the caller** from `actions.yml` at
  the declaration's plan commit, with the publication compiler. The fold
  compares hashes; it does not compile. An action the caller could not compile
  is absent, and every criterion needing it reads `unknown`. It is keyed
  `<30617 coordinate>@<commit>#<action name>` — **never by name alone**. A
  declaration's `action` criteria are evaluated *only* against the definitions
  compiled at **that declaration's own `planRef.commit`**. Collapsing the key
  to the name makes one of two same-named definitions win by sort order, so the
  amended head's correct evidence fails while the superseded plan's old
  evidence passes; the two `same-action-two-commits*` sequences pin both sort
  orders.
- **`teamProjection`** is the existing canonical 44244 projection —
  `fold_coding_session_team_transactions` over this session's team
  transactions. `includedEventIds` is every record that projection **includes**
  (not excluded, not superseded, not corrected away); `assignments` is its
  projected assignment records, each with the `assigneeActor` and
  `assigneeRole` it recorded. Provenance matters: the assembler must read this
  from the team fold, not from raw events, because ingest validates structure
  and never the authorship relationship the team contract requires.
- **`evidence`** is keyed by event id and holds report, verdict and
  action-result facts. **`refStates`** holds every known relay-signed 30618 for
  the code repository, because freshness needs the *newest* one, not only the
  bound one.

Evidence fact shapes:

```json
{"kind": "report",  "eventId": "…", "signer": "…",
 "assignmentRef": "…", "headSha": "<40/64-hex>"}

{"kind": "verdict", "eventId": "…", "signer": "…",
 "subtype": "disposition", "decision": "approve",
 "assignmentRef": "…", "reportRef": "…"}

{"kind": "action_result", "eventId": "<46023 id>", "exitedEventId": "<46014 id>",
 "resultSigner": "<host key>", "echoSigner": "<relay self key>",
 "actionName": "verify", "runId": "<uuid>", "stepId": "verify",
 "definitionHash": "<64-hex>", "disposition": "exited", "exitCode": 0,
 "checkout": {"mode": "commit …", "sha": "<40-hex>", "dirtyBefore": false},
 "dirty": false}
```

### The three proof predicates

A **signed binding is a claim to verify.** A criterion is `covered` only when
its bound evidence satisfies the predicate for its `proof` form:

- **`review`** — a 44244 **verdict** whose `subtype` is `disposition` and
  whose `decision` is `approve` or `approve-with-notes`
  (`CodingSessionTeamDispositionDecision::is_approval`), signed by an actor
  `authority` admits (the `may_lead` predicate above), **included in
  `teamProjection.includedEventIds`**, on a **report** that is itself included
  there, is signed by `teamProjection.assignments[<its assignmentRef>]
  .assigneeActor`, names an assignment bound to that criterion under the
  declaration being projected, and whose `headSha` equals the binding's
  `artifactCommit`.

  **Work coverage never admits what the team contract excludes.** A
  well-formed report published by a channel peer about somebody else's
  assignment is excluded by the team fold (report signer must be the assignee),
  and a lead approving and binding it does not make it evidence. Neither does a
  historical approval that a later ruling replaced: the projection carries the
  replacement, and history stays history.

  Precedence, so two implementations name the same reason for the same fact:
  (1) evidence missing → `evidence_unavailable`; (2) report not canonical (not
  included, wrong signer, or an assignment not bound to this criterion) →
  `report_not_canonical`; (3) disposition signer not `may_lead` →
  `wrong_signer`; (4) disposition not included → `disposition_not_canonical`;
  (5) disposition not an approval → `not_approving`; (6) report `headSha` ≠
  `artifactCommit` → `revision_mismatch`.
- **`action`** — a **host result** (kind:46023, signed by the host, carried by
  the relay's kind:46014 echo whose `echoSigner` is `relaySelfKey`) for a run
  whose `definitionHash` equals
  `actionDefinitions["<the declaration's repository>@<its planRef.commit>#<name>"].definitionHash`,
  whose `stepId` is the criterion's `step`, with `exitCode` 0,
  `checkout.sha` equal to `artifactCommit`, `checkout.dirtyBefore` false and
  `dirty` false.
- **`git-ref`** — the newest relay-signed 30618 for the plan's
  `code_repository` still names `refs/heads/<branch of delivery_ref>` at
  `artifactCommit`, as § (b) rules.

### Reason codes

| `reasonCode` | status | meaning |
|---|---|---|
| `evidence_unavailable` | `unknown` | a bound evidence id is in neither `evidence` nor `refStates` |
| `plan_unreadable` | `unknown` | the plan blob at `planRef.commit` was not supplied |
| `wrong_signer` | `open` | the verdict's signer does not satisfy `may_lead` |
| `not_approving` | `open` | the disposition is present in the team projection and is not an approval |
| `report_not_canonical` | `open` | the team projection excludes the report, or it was signed by somebody who is not the assignment's assignee, or it answers an assignment not bound to this criterion |
| `disposition_not_canonical` | `open` | the team projection excludes the disposition — most often because a later ruling replaced it |
| `revision_mismatch` | `open` | the report is about another revision |
| `wrong_run_or_hash` | `open` | the run executed another definition, or another step |
| `action_failed` | `open` | the run exited non-zero |
| `dirty_revision` | `open` | the tree was dirty before or after the command |
| `bound_to_superseded_declaration` | `stale` | the late-green-for-P case |
| `ref_observation_superseded` | `stale` | a newer ref state names another commit |

### The exact reason strings

**One *branch*, one wording — not one code, one wording.** A reason code is a
class of answer; a branch is a single code path, and a code with four ways to
fail has four rows here. The earlier one-row-per-code table forced four
distinct facts into one sentence and made `wrong_run_or_hash` report a
definition-hash mismatch for a host result whose echo the relay never signed
(A7.4, and the tail of the re-check's R5). A sentence that names the wrong
fact is the kind of comfortable guess this repo treats as a bug.

An implementer reads these here rather than inferring them from a fixture.
Placeholder shapes, which `check-fixtures.mjs` enforces as a regex: `<id>` is
the first 8 hex of an event id followed by `…`; `<sha>` the first 12 of a
commit followed by `…`; `<code>` a signed integer; `<decision>`, `<subtype>`,
`<action>`, `<step>` and `<branch>` slugs; `<repo>` a repository name.

The `branch` column is a stable name for the code path, cited by the fixture
that exercises it. **Every row is exercised by at least one sequence**, and
`check-fixtures.mjs` fails if one is not: a documented sentence nothing
produces is a promise, not a contract.

<!-- check-fixtures: reason-templates -->

| `reasonCode` | branch | `reason` |
|---|---|---|
| `plan_unreadable` | blob-missing | `the plan blob at <sha> was not supplied, so this criterion cannot be judged` |
| `evidence_unavailable` | binding-unresolved | `evidence <id> was not supplied to the fold; nothing here says what it proves` |
| `evidence_unavailable` | report-unresolved | `the report <id> rules on was not supplied to the fold` |
| `evidence_unavailable` | action-not-compiled | `the <action> action was not compiled at this declaration's plan commit <sha>, so nothing here says what its result proves` |
| `evidence_unavailable` | no-relay-self-key | `the relay's self key was not supplied, so a ref observation cannot be judged` |
| `evidence_unavailable` | no-ref-state | `no relay-signed ref state for <repo> was supplied, so delivery cannot be judged` |
| `evidence_unavailable` | ref-state-names-no-branch | `the newest relay-signed ref state for <repo> (<id>) names no refs/heads/<branch>` |
| `report_not_canonical` | signer-not-assignee | `report <id> is signed by <id>, not by assignment <id>'s assignee <id>` |
| `report_not_canonical` | not-included | `report <id> is not in the team projection, so nothing here says it answers assignment <id>` |
| `report_not_canonical` | projection-empty | `report <id> cannot be shown canonical: the team projection includes no records at all` |
| `report_not_canonical` | no-assignment-row | `report <id> answers assignment <id>, which the team projection does not carry, so nothing names its assignee` |
| `report_not_canonical` | criterion-unassigned | `report <id> answers assignment <id>, and no assignment is bound to this criterion under this declaration` |
| `report_not_canonical` | assignment-not-bound | `report <id> answers assignment <id>, which this criterion is not bound to` |
| `wrong_signer` | not-may-lead | `verdict <id> is signed by <id>, who does not satisfy may_lead for this session` |
| `disposition_not_canonical` | superseded | `disposition <id> is not in the team projection: it was superseded by <id>` |
| `disposition_not_canonical` | not-included | `disposition <id> is not in the team projection, so it is not a current ruling` |
| `not_approving` | decision | `disposition <id> decided <decision>; only approve or approve-with-notes satisfy a review criterion` |
| `not_approving` | not-a-disposition | `verdict <id> is a <subtype>; only an approving disposition satisfies a review criterion` |
| `not_approving` | none-bound | `no approving disposition is bound; a review criterion is answered by one` |
| `revision_mismatch` | report-head-sha | `report <id> names headSha <sha>, not the binding's artifactCommit <sha>` |
| `revision_mismatch` | action-checkout | `host result <id> ran on <sha>, not the binding's artifactCommit <sha>` |
| `revision_mismatch` | no-ref-observation | `a git-ref proof is answered by a relay-signed ref state, and none is bound` |
| `wrong_run_or_hash` | echo-signer | `host result <id> was echoed by <id>, not the relay's self key; the <action> definition compiled at this declaration's plan commit <sha> is unproved` |
| `wrong_run_or_hash` | definition-hash | `host result <id> ran definition hash <id>, not the <action> definition compiled at this declaration's plan commit <sha> (<id>)` |
| `wrong_run_or_hash` | step | `host result <id> ran step <step>, not <step> of the <action> definition compiled at this declaration's plan commit <sha>` |
| `wrong_run_or_hash` | none-bound | `no host result for <action>/<step> of the definition compiled at this declaration's plan commit <sha> is bound` |
| `action_failed` | exit-code | `host result <id> exited <code>` |
| `dirty_revision` | before | `host result <id> ran on a tree that was already dirty before the command` |
| `dirty_revision` | after | `host result <id> left the tree dirty after the command` |
| `bound_to_superseded_declaration` | other-declaration | `evidence for this criterion is bound to declaration <id>, which is not the head` |
| `ref_observation_superseded` | newer-state | `the newest relay-signed ref state for <repo> (<id>) names refs/heads/<branch> at <sha>, not the bound artifact commit` |

**`wrong_run_or_hash` always names the declaration's plan commit** — all four
of its branches do, including the two that are not about a hash at all. The
definition that was expected is only meaningful with the plan commit it was
compiled at (the whole of finding 7), so the shorter "compiled at the plan
commit" wording is not permitted anywhere. What A6 does **not** license is the
reverse substitution: naming the plan commit does not entitle a branch to
claim a hash mismatch it did not observe, which is why `echo-signer`, `step`
and `none-bound` each say what actually failed and then name the commit whose
definition stays unproved.

Two more consequences of the same rule, both new here:

- **`report_not_canonical` has six branches, and three of them exist only
  because empty is unproved** (A7.4). A report whose assignment the projection
  does not carry, a projection that includes no records at all, and a criterion
  with no assignment binding are three different facts, and none of them is
  "the team projection excludes it". The `signer-not-assignee` branch lost its
  old ", and the team projection excludes it" tail for the same reason: a wrong
  signer is one fact, and exclusion is another that may or may not hold.
- **`revision_mismatch` and `evidence_unavailable` each span the review, action
  and git-ref predicates.** A report about another revision and a host result
  from another checkout are one code and two sentences; so are an unresolved
  binding, an uncompiled action, a missing relay self key, an absent ref state
  and a ref state that names no delivery branch.

A criterion whose evidence fails a predicate is **`open` with its reason
named**, not `covered` and not silently empty: the binding exists, and saying
so is the difference between "nobody has done this" and "somebody claimed it
and the claim did not hold".

Two criteria of one declaration — one covered, one still owed. This block
is checked by `check-fixtures.mjs` against the same rules as every
sequence fixture, so the text and the fixtures cannot drift apart in key
names or in status and reason spellings.

<!-- check-fixtures: fold-example -->

```json
{
  "schema": "buzz-project-work-coverage/v1",
  "sessionRef": "11111111-2222-4333-8444-555555555555",
  "projectRef": "30621:1ead000000000000000000000000000000000000000000000000000000000000:kettle",
  "declarations": [
    {
      "workId": "9d0f0f0f-1111-4222-8333-444444444444",
      "declarationRef": "dec1000000000000000000000000000000000000000000000000000000000000",
      "planRef": {
        "repository": "30617:1ead000000000000000000000000000000000000000000000000000000000000:pivot-test-beekeeper-agents",
        "commit": "abababababababababababababababababababab",
        "path": "plans/kettle.md"
      },
      "state": "head",
      "supersedes": [],
      "supersededBy": [],
      "stateReasonCode": null,
      "stateReason": null,
      "planResolved": true,
      "planDrift": {
        "declaredCommit": "abababababababababababababababababababab",
        "currentCommit": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "state": "drifted"
      },
      "candidateArtifact": "e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7",
      "artifactCommits": [
        "e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7"
      ],
      "criteria": [
        {
          "criterionId": "cli-behaviour",
          "proof": {
            "kind": "review"
          },
          "status": "covered",
          "assignmentRefs": [
            "a551000000000000000000000000000000000000000000000000000000000000"
          ],
          "evidence": [
            {
              "kind": "report",
              "eventId": "1ea1000000000000000000000000000000000000000000000000000000000000"
            },
            {
              "kind": "verdict",
              "eventId": "bed1000000000000000000000000000000000000000000000000000000000000"
            }
          ],
          "artifactCommit": "e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7",
          "reasonCode": null,
          "reason": null
        },
        {
          "criterionId": "usage-documentation",
          "proof": {
            "kind": "review"
          },
          "status": "open",
          "assignmentRefs": [
            "a552000000000000000000000000000000000000000000000000000000000000"
          ],
          "evidence": [],
          "artifactCommit": null,
          "reasonCode": null,
          "reason": null
        }
      ],
      "coverageComplete": false,
      "coverageReasonCode": "criteria_not_covered",
      "coverageReason": "1 of 2 criteria are not covered under this declaration: usage-documentation is open"
    }
  ],
  "excluded": [],
  "conflicts": []
}```

### Per-declaration `state`

| state | meaning |
|---|---|
| `head` | the current declaration for its `workId`: nothing supersedes it and it is not in a conflict |
| `superseded` | another declaration names it in `supersedes`; `supersededBy` lists them |
| `stale` | still the head, but the session's **current** kind:44227 goal (the fold the product already uses to select the active goal) differs from the declaration's `goalRef`. That mismatch is v1's **only** trigger: a 44244 decision never marks a declaration stale by itself, because a requirement-changing decision is followed by a goal change or by an explicit amendment from the lead. The contract stays pinned; `stateReason` names both goals |
| `conflict` | more than one **head** exists for this `workId`. A **head** is a *maximal* valid declaration: one that no valid declaration of that `workId` names in `supersedes`. Every head is `conflict`, `coverageComplete` is `false` for all, and the resolution is one declaration naming **all** current heads. No timestamp winner |

**`supersedes` is reported exactly as the event recorded it, in every state —
`head`, `superseded`, `stale` and `conflict` alike; `supersededBy` is the
derived inverse over the projected set.** A superseded declaration that itself
superseded an earlier one still says so, because that list is what the record
says and the projection never edits a record.

**`assignmentRefs` lists every valid `work.assignment_bound` naming that
criterion under the declaration being projected, with or without evidence.**
An unfinished criterion is exactly where the question "who owes this" is asked,
so an `open` criterion with an assignment names it; `[]` means nobody has been
assigned it yet, not "no evidence has arrived".

**Conflict is defined over maximal declarations, never over direct siblings.**
Two consequences the fixtures pin:

- P forks to A and B, then A2 supersedes **only** A. A2 and B share no
  immediate predecessor, but both are maximal, so the work is still in
  conflict and a resolution must name both (`sequences/fork-descendant/`).
  A literal same-predecessor rule would clear this conflict while B is still
  unresolved — the reading this amendment closes.
- Two declarations of one `workId` both with empty `supersedes` are two heads
  and therefore a conflict (`sequences/fork-two-roots/`): an empty
  `supersedes` is not a claim to be first.

Scope: only valid declarations of the **same `workId`** in the folded session
count; a `supersedes` entry naming an event outside that set neither creates
nor clears a head.

`superseded` and `conflict` are structural and take precedence over `stale`:
a superseded declaration adopted against an older goal reads `superseded`.

`criteria` is projected for `head` and `stale` declarations only — those are
the two states that *are* a current contract. A `superseded` or `conflict`
declaration carries `criteria: []` and a `coverageReason` saying why. Bindings
made under a conflicted head are retained on the wire and are simply not
projected; nothing is deleted, and resolving the fork projects them.

### Per-declaration `planDrift`

Every declaration row carries `planDrift`. It is **disclosure, never
enforcement** (amendment A10): it changes no declaration `state`, no criterion
`status`, no `coverageComplete` and no reason code, and completion is never
refused for it. A plan committed out from under a live declaration does *not*
make that declaration stale — `stale` is a current-goal mismatch and nothing
else (decision 14) — and the fold still reads the plan at the pinned commit.
Pinning is the contract working. What was missing was the sentence a reader
needs in order to decide whether to re-adopt, and this is that sentence.

```json
"planDrift": {
  "declaredCommit": "abababababababababababababababababababab",
  "currentCommit": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
  "state": "drifted"
}
```

| key | type | null? | rule |
|---|---|---|---|
| `declaredCommit` | hex | no | this declaration's own `planRef.commit`, verbatim |
| `currentCommit` | hex or `null` | **yes** | the commit the agents repository's contract branch names now; `null` when no supplied ref state says |
| `state` | enum | no | `none` \| `drifted` \| `unknown` \| `superseded`, ruled below |

| `state` | when |
|---|---|
| `none` | `currentCommit` is known and equals `declaredCommit` |
| `drifted` | `currentCommit` is known, differs from `declaredCommit`, and no successor of this declaration pins it |
| `superseded` | `currentCommit` is known, differs from `declaredCommit`, and some declaration of the same `workId` that supersedes this one — directly or transitively, over the projected `supersededBy` closure — carries it as its own `planRef.commit`. The drift has already been adopted, so nothing is owed on this row |
| `unknown` | no relay-signed 30618 for the agents repository was supplied, or the newest one names no `refs/heads/main`. `currentCommit` is `null` |

`superseded` is the narrower answer and takes precedence over `drifted`; the
two are mutually exclusive by construction, and the checker enforces both
directions. Equality is exact lowercase-string equality, never a prefix
comparison: a 40-hex and a 64-hex commit are different strings, and one
repository has one object format, so the two widths never meet for one
`planRef.repository`.

**Where `currentCommit` comes from.** The newest relay-signed kind:30618 — by
`(created_at, id)`, the same ordering the `git-ref` predicate uses — whose `d`
tag names the **repository id of `planRef.repository`**, read at its
`refs/heads/main` tag. That is the *agents* repository, not the plan's
`code_repository`: the two are different repositories and a ref state for one
says nothing about the other. The agents repository's contract branch is
`main` (spec § 4.11); `delivery_ref` describes the code repository and is not
consulted here. The row is selected out of the same `refStates` input the
`git-ref` predicate already reads, so this fact needs no new fold input and no
new signer class — an owner-signed claim about its own branch is not ref state
here either.

**What this fact observes, and what it cannot.** Relay ref state names a
**branch tip**, never the last commit to touch a path. So `drifted` says *the
agents repository's `main` is not the commit this declaration pinned* — it does
**not** say the plan file itself was edited, and it cannot, because nothing the
relay publishes is path-scoped. Amendment A10 § 1 says "the plan path's tip
commit"; the input the assembler receives cannot answer at path granularity,
and this file specifies the fact that exists rather than the one that would be
convenient. Every surface's wording stays inside that: *the agents repository
has moved on since this plan commit; the plan at `<declaredCommit>` is still
what this work is judged against*, with the re-adopt command. A surface that
says "your plan changed" is claiming an observation the input did not make. A
path-scoped drift fact needs an input that does not exist yet; it is
deliberately not invented here, and it is the one thing a later amendment
should add if the noise proves to matter.

**The key is always present, and the object is never `null`.** A10 § 1 wrote
"`null` when the ref state is absent"; this file puts the absence one level in,
as `currentCommit: null` with `state: "unknown"`, for three reasons.

1. It is decision 7's house rule — a nullable *key* is present and written
   `null`. A nullable *object* makes every reader write `planDrift?.state`, and
   a strict reader then cannot tell "the fold said unknown" from "this build
   does not emit the key".
2. `unknown` must be disclosed as unknown and never as `none` (A10 § 1). An
   absent object is exactly what a reader defaults to "no drift", which is the
   comfortable guess A10 exists to forbid.
3. The key is additive on a closed record, so every strict reader loads these
   fixtures and must **fail** on an absent key rather than tolerate it (the 204
   rule, A3 § 3).

`planDrift` has no reason code and no row in the reason-template table above:
it is not a criterion refusal and not a coverage refusal. `unknown` and
`superseded` here are facts about ref state, not verdicts about work — a
`superseded` drift on a `head` declaration is perfectly ordinary and means only
that an amendment already adopted the tip.


### Per-criterion `status`

| status | meaning | `reason` |
|---|---|---|
| `open` | no evidence binding under the head declaration names this criterion, **or** its bound evidence resolved and failed its predicate | `null`, or the named failure |
| `covered` | ≥ 1 `work.evidence_bound` under the head declaration names it, every `evidenceRefs` entry resolved, and the required proof form was satisfied | `null` |
| `stale` | either every binding carrying this criterion names a declaration that is **not** the head (the late-green-for-P case), or its `ref_observation` has been superseded by a newer ref state naming a different commit | names the declaration it was bound to, or the newer ref state |
| `unknown` | the fold could not read an input: the plan blob at `planRef.commit` was not supplied, or a bound evidence id is in neither `evidence` nor `refStates` | names the missing input (`plan_unreadable`, `evidence_unavailable`) |

`unknown` is a result, not an error. A fold given no plan blob returns every
criterion `unknown` with `planResolved: false` — never `open`, which would
read as "nothing has been done" when the truth is "we cannot see the list".
When no binding exists yet either, there are **no criterion rows at all**, and
that is the dangerous case: see the gating rule below.

**A consumer gating completion MUST gate on the declaration's `state`,
`planResolved` and `coverageComplete` — never on the presence of criterion
rows.** Two declarations carry none: a head whose plan blob is unavailable
(`coverageReasonCode: "plan_unavailable"`) and any declaration in `conflict`
(`coverageReasonCode: "conflict"`). Both are `coverageComplete: false` with a
declaration-level reason, precisely so that "nothing to show" can never be read
as "nothing outstanding". A gate keyed on a non-empty criteria list lets an
unreadable plan and two competing heads through; that was finding 5.

An amendment does not carry evidence forward. Evidence bound to P stays bound
to P; under P2 those criteria are `stale` until re-bound at P2. This is the
"re-attest all criteria after amendment" rule; selective reuse is deliberately
out of v1.

### `coverageComplete`, and the 44244 terminal

**There is exactly one candidate artifact per declaration.** Coverage is a
statement about *one delivered revision*, not a per-criterion scoreboard.

- `candidateArtifact` is the `artifactCommit` of the valid `git-ref` evidence
  when the plan has a `git-ref` criterion; otherwise it is the single commit
  shared by all covering evidence. It is `null` when nothing is covered, and
  **`null` whenever the plan has a `git-ref` criterion that is not yet
  covered, however many other criteria are** — there is no candidate until
  delivery is observed. `coverageComplete` is unaffected by this: it is false
  in that case for the ordinary reason, the uncovered criterion.
- `artifactCommits` lists the distinct commits the covering evidence names.

`coverageComplete` is `true` for a declaration when **all** of:

1. `state == "head"`,
2. `planResolved == true`,
3. every criterion in the plan's `criteria` (retired ones are not counted) is
   `covered`,
4. its `workId` appears in no entry of `conflicts`,
5. every covering criterion names the **same** `artifactCommit`, equal to
   `candidateArtifact` — `artifactCommits` has exactly one member.

Otherwise it is `false`, `coverageReasonCode` says which clause failed and
`coverageReason` names the criteria or the commits.

**Mixed artifacts.** Tests green at A, the documentation review at B and the
delivery observation at C is five individually-covered criteria and **nothing
verified at the delivered commit**. Those criteria keep their own `covered`
status — they are true statements about the commits they name — but the
declaration reads `coverageComplete: false`,
`coverageReasonCode: "mixed_artifacts"`, and lists the commits.
Fixture: `sequences/mixed-artifacts/`.

**This output contains no mission-terminal field, deliberately.** Whether the
44244 `mission.completed` folded to terminal is a different question with a
different fold (`fold_coding_session_team_transactions`, and lane 183's
pending-completion settlement). A reader that shows both shows **two rows**:

```
coverage : incomplete — usage-documentation open
mission  : terminal   — completed <event id>
```

The two disagreeing is a **disclosure**, not a reconciliation: "a terminal
record with incomplete coverage" is exactly what an older client's completion
looks like, and it must be visible as that. Nothing in this contract merges
them, makes one imply the other, or lets coverage publish or withhold a
completion. The CLI checks coverage *before* publishing the existing signed
`mission.completed`; the completion still settles by re-fold, as 183 built it.

---

## (d) The CLI surface

`bee sessions work <verb>`. Global `--format compact` goes before the
subcommand. All reads return sig-stripped JSON; all writes return
`{event_id, accepted, message}`, and a create adds its entity id.

Exit codes are the repository's: `0` ok, `1` input error, `2` network/relay,
`3` auth, `4` other, `5` write conflict. **Incomplete coverage is exit 0** —
it is a fact the caller asked for, not a failure. A permission or read error
never becomes an empty success.

Every write verb takes `--example [<label>]`, dispatched **before** the key
gate exactly as lane 182 built it
(`crates/buzz-cli/src/lib.rs:5137-5153`), so a seat learning the wire needs
neither an identity nor a relay. Each example is a serialized constructed
value, never a hand-typed string, and each must pass the publication
validator in a test.

### `bee sessions work validate`

```
bee sessions work validate --plan <path>
                           [--repo <id>] [--commit <sha>]   # resolve from git
                           [--example]
```

Reads a plan from disk, or with `--repo`/`--commit` from
`git show <commit>:<path>`. Needs no relay and no key.

```json
{"valid": true, "planId": "kettle-cli", "schema": "beekeeper-plan/v1",
 "status": "in-force", "codeRepository": "pivot-test",
 "deliveryRef": "refs/heads/main",
 "criteria": [{"id": "cli-behaviour", "proof": {"kind": "review"}}],
 "retiredCriteria": [], "bytes": 1462}
```

```json
{"valid": false,
 "errors": [{"code": "duplicate-criterion-id", "path": "criteria[2].id",
             "message": "criterion id \"cli-behaviour\" appears twice"}]}
```

Exit `0` valid, `1` invalid or unreadable.

### `bee sessions work adopt`

```
bee sessions work adopt --session <uuid>
                        --plan-repo <id> --plan-commit <sha> --plan-path <p>
                        --goal <event id> --responsible <pubkey>
                        [--work-id <uuid>]        # required on an amendment
                        [--supersedes <id>]...    # repeatable
                        [--example]
```

Validates the blob at the commit, compiles every `action` proof against
`actions.yml` **at that same commit**, then signs and publishes a
`work.declared`. Refuses `status: superseded` and any `plans/archive/` path.

**Adoption is atomic: a failed compile signs nothing.** Any unresolved plan,
action or repository — a missing `actions.yml` entry, a step the action does
not define, a repository that does not resolve to a project-associated
coordinate, a blob that is not at that commit — refuses with the compile error
and **exit 1**, having published no event. A declaration exists only when
everything it references resolved at that commit; a half-adopted contract
would be a contract nobody can read.
A retry with the same `--work-id` and the same inputs publishes the same
declaration id; it does not mint a second workId.

```json
{"event_id": "<64-hex>", "accepted": true, "message": "adopted kettle-cli at ab…",
 "work_id": "<uuid>", "declaration_ref": "<64-hex>"}
```

### `bee sessions work bind`

```
bee sessions work bind --session <uuid> --declaration <id>
                       --criterion <slug>...            # repeatable, ≥ 1
                       ( --assignment <id> [--replaces <binding id>]
                       | --artifact <sha> --evidence <kind>:<event id>...
                         [--completion <id>] )
                       [--example]
```

`--assignment` writes a `work.assignment_bound`; `--artifact` +
`--evidence` writes a `work.evidence_bound`. The two groups are mutually
exclusive and one is required. `<kind>` is one of the four evidence kinds.
Returns `{event_id, accepted, message}`.

### `bee sessions work status`

```
bee sessions work status --session <uuid>
                         [--work <uuid>]        # one declaration's workId
                         [--plans-from <dir>]   # resolved checkout, else fetch
                         [--offline]            # do not fetch; unresolved
                                                # plans report unknown
```

Prints the (c) object under `coverage`, plus the 44244 fold's terminal state
under `mission`, as two sibling objects:

```json
{"coverage": { "...": "the (c) shape" },
 "mission": {"state": "terminal", "completionRef": "<64-hex>", "pending": null}}
```

Exit `0` whatever the coverage says; `2`/`3` for relay and auth failures.

---

## (e) The work brief

What a seat is handed when its assignment is bound to criteria. Total budget
**8 192 bytes**, allocated per field. Over budget, an excerpt is truncated
with an explicit marker naming the full path and commit — never silently.

| # | field | budget | content |
|---|---|---|---|
| 1 | criterion excerpts | 4 096 B (≤ 512 B each) | for each bound criterion: its `id`, its `accept` **verbatim**, and provenance `<repo>@<commit-12>:<path>` |
| 2 | goal and decision refs | 512 B | the `goalRef` and any decision events that bound the scope, event id + one line each |
| 3 | exact code base | 512 B | code repository id, `delivery_ref`, the full 40-hex base commit, and the absolute worktree path the seat runs in |
| 4 | role obligations | 256 B | the staged role's template name and revision, and the absolute path to its bundle — a **pointer**, never the role text (the seat already has it) |
| 5 | effective permissions | 512 B | plan access (`none` \| `read` \| `write`), whether host-step approval is held (it never is, for a seat), push authority |
| 6 | model source | 256 B | routed or unrouted, and which registry answered (agents repository \| code checkout \| shipped default) |
| 7 | report example | 1 024 B | the exact `bee sessions report --example` command and the required key list |
| 8 | free prose | 1 024 B | the lead's own framing |

Rules:

- **Never the whole repository, never the whole plan body.** Only the bound
  criteria's `accept` text, with provenance.
- Access grants: `none` means no clone and only the excerpt above; `read`
  means inspect the clone and propose an edit through a report; `write` is for
  the lead and Project Setup in this slice. The grant concerns the working
  copy, not relay push permission.
- Provenance is mandatory on every excerpt. A criterion quoted without its
  commit is a quotation a seat cannot re-read.

---

## (f) Compatibility

1. **An old reader must ignore kind 44249 entirely.** It never queries it, so
   it never sees it. Nothing about a session's 44244 history changes: the same
   events fold byte-identically in a build that has never heard of 44249.
   W1 owes a reader test that proves exactly this.
2. **Nothing here adds a key to 44244, 44223 or 44228.** 44244's bodies are
   `deny_unknown_fields` and its fold errors on the *whole set* for one bad
   envelope (the finding-13 cliff) — that is precisely why work records are a
   sibling kind rather than three more 44244 subtypes. 44223 (session
   metadata) and 44228 (authority transitions) are untouched; the `may_lead`
   projection this contract consumes is the one 44228 already produces.
3. **Relay-first landing.** 44249 changes relay ingest, so the finalizer waits
   for hive's NIP-11 `build_time` to pass the landing time before installing a
   desktop that depends on it.
4. **Coverage is not a genesis bump.** An old client publishing a
   `mission.completed` with no coverage is a *discrepancy to disclose*, never
   a claim that the old client enforced this contract.

---

## Fixtures

```
fixtures/
  plans/valid/kettle.md                    the worked example
  plans/invalid/<reason>.md                six refusals, one file each
  records/valid/<variant>.json             one full signed-shape event each
  records/invalid/<reason>.json            one refusal each
  sequences/<name>/events.json             an ordered event array
  sequences/<name>/expected-fold.json      the (c) output for it
```

Every id is a fixed fake of the correct length, every timestamp is fixed, and
nothing reads a clock or a network. `content` is a JSON **string**, as on the
wire and as `conformance/project-todo-fold` writes it. Signatures are omitted:
they are not the fold's concern (the relay verified them at ingest).

File conventions:

- A **valid** record file is the event object itself.
- An **invalid** record file is `{"refusal": "<code>: <why>", "event": {…}}`.
  `check-fixtures.mjs` asserts each of these still fails at least one shape
  check, so a fixture cannot quietly stop refusing.
- An **invalid** plan file carries exactly one `# REFUSED: <code> — <why>`
  YAML comment in its frontmatter, and the checker asserts the defect is still
  present in the file.
- A sequence's `inputs.json` is the fold input of § (c): `relaySelfKey`,
  `authority`, `currentGoalRef`, `goalEvents`, `planBlobs` (keyed
  `<30617 coordinate>@<commit>:<path>`, each pointing at the plan fixture),
  `actionDefinitions`, `evidence` and `refStates`, plus a `note` saying what
  the sequence is for. Both amendment
  commits resolve to the *same* plan fixture on purpose: identical criterion
  ids must not carry evidence forward.

The sequences:

| sequence | what it pins |
|---|---|
| `happy-path` | one declaration, five criteria, bindings and evidence to complete coverage; `coverageComplete: true` |
| `amendment` | P adopted, P2 supersedes it, then a **late** `work.evidence_bound` arrives naming P. It must not cover P2: that criterion is `stale`, `coverageComplete` is `false` |
| `fork` | two successors of P, neither superseded. Both are `conflict`, `conflicts` names both heads, coverage is refused for both |
| `superseded-observation` | a `git-ref` criterion bound to a 30618 that named the artifact commit, then superseded by a newer relay-signed 30618 naming a different commit: the criterion is `stale`, not `covered` |
| `ref-observation-rebound` | `delivered-main` bound twice, the second after main moved: the first binding is immutable and stays, both refs are projected, and the criterion is `covered` at the newest binding's commit because the newest relay-signed 30618 names it (ledger 255) |
| `ref-observation-wrong-ref` | the bound relay-signed 30618 names the artifact commit on another branch, never on the plan's `delivery_ref`: `unknown`, `evidence_unavailable/ref-state-names-no-branch`. A binding cannot choose the ref it is judged against |
| `goal-changed` | the session's current goal differs from the declaration's `goalRef`: the declaration reads `state: "stale"` and stays pinned |
| `goal-ref-not-a-goal` | `goalRef` names the authorizing decision: the envelope accepts it and the fold excludes it `goal_ref_not_a_goal` |
| `evidence-refusals` | four review-side failures in one declaration: `wrong_signer`, `not_approving`, `revision_mismatch`, `evidence_unavailable` |
| `action-hash-mismatch`, `action-failed`, `action-dirty` | one action-side failure each: `wrong_run_or_hash`, `action_failed`, `dirty_revision` |
| `mixed-artifacts` | every criterion covered, at three commits: `coverageComplete: false`, `mixed_artifacts` |
| `fork-descendant` | P→A,B then A2 supersedes only A: heads A2 and B still conflict |
| `fork-two-roots` | two declarations of one `workId`, both with empty `supersedes`: two heads, conflict |
| `wrong-assignee-report` | a channel peer reports somebody else's assignment and the lead approves and binds it: `report_not_canonical` |
| `superseded-disposition` | an approving disposition later replaced by changes-requested: `disposition_not_canonical` |
| `same-action-two-commits`, `same-action-two-commits-reversed` | one action name, two plan commits, both lexicographic orders: the head is covered only by the run of *its* definition, and the old definition's run re-bound under a head reads `wrong_run_or_hash` |
| `plan-unavailable-before-bindings` | a head with no plan blob and no bindings: no criterion rows, and `coverageComplete: false` with `plan_unavailable` |

**Amendment A7.4 — the negative sequences (lane 221).** The twelve below were
authored as the oracle for R5 and for branch-specific reasons, before any
implementation. The first four are the ruling itself; the rest exist because
a branch with no fixture is a wording nobody checked.

| sequence | what it pins |
|---|---|
| `report-absent-assignment` | **R5 counterexample 1.** A report and a lead's approval name an assignment the canonical projection does not carry, so it includes neither record. `report_not_canonical/no-assignment-row` — **not** covered. Empty is unproved |
| `criterion-unassigned-report` | **R5 counterexample 2.** The same canonical report and approval that legitimately cover `cli-behaviour` are re-bound to `usage-documentation`, which has no assignment binding. `report_not_canonical/criterion-unassigned`; the positive control stays `covered`, so the negative is not an artefact of a broken input |
| `projection-empty` | the assignment row exists and its assignee did sign the report, but `includedEventIds` is empty: `report_not_canonical/projection-empty`. An empty inclusion set is never permission to omit the predicate |
| `canonical-exclusions` | the sibling negatives with a **non-empty** projection: an excluded disposition under an included report (`disposition_not_canonical/not-included`, no replacement to name), an excluded report under an included disposition (`report_not_canonical/not-included`), and a canonical report answering an assignment bound to another criterion (`report_not_canonical/assignment-not-bound`) |
| `plan-unreadable-after-bindings` | the sibling of `plan-unavailable-before-bindings`: the plan blob is missing **after** bindings arrived, so the fold reports exactly the criteria they named, every one `unknown`/`plan_unreadable` with `proof: null`, no bindings and no commit. The declaration still reads `plan_unavailable` |
| `review-unresolved-and-unanswered` | five branches in one declaration: a disposition ruling on a report nobody supplied (`evidence_unavailable/report-unresolved`), a refutation offered as a review (`not_approving/not-a-disposition`), a report with no disposition at all (`not_approving/none-bound`), a host result from another checkout (`revision_mismatch/action-checkout`), and a `git-ref` criterion carrying a report (`revision_mismatch/no-ref-observation`) |
| `action-wrong-echo-signer` | a host result whose echo the relay did not sign — `wrong_run_or_hash/echo-signer`, which is not a hash mismatch and still names the plan commit whose definition stays unproved — and a ref state for the right repository naming no delivery branch (`evidence_unavailable/ref-state-names-no-branch`) |
| `action-wrong-step` | the right definition at the right commit, the wrong step (`wrong_run_or_hash/step`), and a bound ref observation describing another repository (`evidence_unavailable/no-ref-state`) |
| `action-no-host-result` | an action criterion bound to a human report and no run at all: `wrong_run_or_hash/none-bound`, naming the action, the step and the plan commit |
| `action-dirty-after` | a clean checkout whose command left the tree dirty: `dirty_revision/after`, the sibling of `action-dirty`'s dirty-before |
| `action-not-compiled` | the caller could not compile `verify` at this declaration's plan commit: `evidence_unavailable/action-not-compiled`, `unknown` — never a hash mismatch, because evidence must not nominate its own expected definition |
| `relay-self-key-absent` | `relaySelfKey: null` with a bound ref observation that resolves: `evidence_unavailable/no-relay-self-key`. An owner-signed claim about its own branch is never promoted to fill the gap |

**Amendment A10 — the drift sequences (lane 234).** Five more, authored as the
oracle for `planDrift` before any implementation. The first three are one base
(`happy-path`) with three different ref-state inputs and **no other
difference**, which is what makes them an oracle rather than three examples:
the only thing that may change between them is `planDrift`.

| sequence | what it pins |
|---|---|
| `plan-drift-none` | `happy-path` plus a relay-signed 30618 for the **agents** repository whose `refs/heads/main` still names the declared plan commit: `planDrift.state: "none"`, and every other value is `happy-path`'s |
| `plan-drift-on-completed` | the same complete coverage with the agents repository's `main` moved past the declared commit: `drifted`, and **nothing else changes** — `state` stays `head`, every criterion stays `covered`, `coverageComplete` stays `true`. Drift is disclosed, never enforced |
| `plan-drift-unknown` | `happy-path`'s inputs exactly — a 30618 for the *code* repository and none for the agents repository: `unknown` with `currentCommit: null`, never `none`. A ref state for another repository says nothing about where the plan's contract branch is |
| `plan-drift-drifted` | `goal-changed`'s declaration — `stale` on the goal, every criterion `open` — with the agents `main` moved past its plan commit: `state: "stale"` for `goal_changed` and `planDrift.state: "drifted"`, two independent facts |
| `plan-drift-superseded` | `amendment`'s P and P2 with the agents `main` naming P2's plan commit: P reads `superseded` (its own successor adopted the drift, so nothing is owed on that row) and P2 reads `none` |

`node conformance/project-work/check-fixtures.mjs` asserts every fixture
parses and that ids, hex lengths, uuids and byte limits are well-formed, so a
later edit cannot silently break them. It is deliberately **not** a parser or
a fold — those are W1's, and the fixtures are what they bind to. It is wired
into no CI recipe.

Since A7.4 it also checks the oracle for holes, which is the part a fixture
corpus cannot do for itself:

- **Every reason code and every branch template in § (c) is exercised by some
  sequence.** Deleting the only sequence that reaches a branch, or restating
  its sentence in another branch's words, fails with the branch named.
- **Every expected `reason` matches exactly one template**, anchored end to
  end with each placeholder's shape enforced. Two templates matching one
  sentence is a table defect and fails too: a branch's sentence must identify
  its branch.
- **Coverage is positive.** No criterion may read `covered` while
  `includedEventIds` is empty, while the report's assignment has no projected
  row, while its signer is not that assignment's assignee, while the report
  answers an assignment not in the criterion's `assignmentRefs`, or while a
  host result's echo is not the relay's self key.

Proven to bite (each perturbation applied to the green corpus, one at a
time): restating the `echo-signer` reason in the `definition-hash` wording
failed as an unexercised branch; marking `report-absent-assignment` covered
failed four ways at once (empty projection, excluded report, signer ≠
assignee, excluded disposition); marking `criterion-unassigned-report`'s
unassigned criterion covered failed on the criterion→assignment relationship;
giving a `plan_unreadable` row a proof form failed; deleting the
`dirty_revision/after` row from the table failed as an unmatched reason; and
deleting `action-no-host-result` failed as an unexercised branch.

### For lane 222

The oracle landed first, so the fold disagrees with it on purpose. What is
expected to go red on `work/lane-221-coverage-oracle`, and why:

1. `project_work_fold::tests::every_sequence_folds_to_exactly_its_expected_output`
   and `project_work_inputs::tests::folding_an_assembled_input_matches_every_expected_fold`
   — the twelve new sequences, and every branch whose sentence changed.
2. `project_work_fold::tests::every_reason_string_matches_the_contracts_table`
   — it parses a two-column table and asserts twelve rows
   (`project_work_fold_tests.rs:565`). The table is three columns and 31 rows
   now. Its "four unexercised codes" allowance (`:635`) must go: every code
   and every branch is exercised, and the checker enforces it.
3. `report_not_canonical` and `disposition_not_canonical` must stop waiving
   their checks when `includedEventIds` or `assignments` is empty
   (`project_work_fold_project.rs:525`, `:594`, `:602`) and must stop waiving
   the assignment relationship when `assignmentRefs` is empty. Each waiver
   becomes one of the new branches.
4. `wrong_run_or_hash` needs its four sentences separated
   (`:698` echo-signer, `:711` definition-hash, `:726` step, `:779`
   none-bound), `dirty_revision` its two (`:757`, `:767`), and the
   `signer-not-assignee` sentence loses its exclusion tail.
5. `unresolved_criteria` (`:245`) is now contract, not an accident: keep the
   rows, `proof: null`, and the declaration's `plan_unavailable`.
6. Any strict reader of the coverage document that requires a non-null
   `proof` on a criterion row.

### For lane 235

The oracle lands first again. What is expected to go red on
`work/lane-234-plan-drift-oracle`, and what must load the new key:

1. `project_work_fold::tests::every_sequence_folds_to_exactly_its_expected_output`
   and `project_work_inputs::tests::folding_an_assembled_input_matches_every_expected_fold`
   — every declaration row now carries `planDrift`, and both `SEQUENCES`
   arrays are `[Sequence; 30]` (`project_work_fold_tests.rs:40`,
   `project_work_inputs_tests.rs:58`) against 35 sequence directories. The
   five new names must be added to both.
2. `WorkDeclarationProjection` (`project_work_fold.rs:205`) gains the field,
   serialized between `planResolved` and `candidateArtifact` — the key order
   the fixtures and `check-fixtures.mjs` fix — and both construction sites in
   `project_work_fold_project.rs` (`:160`, `:180`) must supply it. The `:180`
   site is the unresolved-plan row: a declaration whose plan blob is missing
   still reports drift, because drift is about ref state and not about the
   blob.
3. The fold reads the agents repository's row out of the existing `refStates`
   input. Nothing new is added to `RawWorkInputs` or `assemble_fold_inputs`
   (`project_work_inputs.rs:122`), but the assembler's callers must now
   *supply* the agents repository's 30618 as well as the code repository's, or
   every row honestly reads `unknown`.
4. Strict readers that must load the key, all of which bind these fixtures:
   `bee sessions work status` (`buzz-cli/src/commands/sessions/work.rs`, bound
   in `work_tests.rs:74`, whose `SEQUENCES` is `[Sequence; 18]`); the
   completion result (`operations_completion.rs`, A10 § 2 — the fact travels
   with it, and completion is still never refused for drift); the desktop type
   `ProjectWorkDeclaration` (`desktop/src/shared/api/tauriProjectWork.ts:198`),
   its presentation (`lib/projectWork.ts`) and its surface
   (`ui/ProjectWorkCoverage.tsx`), with the declared-vs-current line and the
   re-adopt command.
5. The provider's work brief (`buzz-session-provider/src/work_brief.rs`) is
   the follow-on A10 § 2 names, after lane 229 lands the crate's ownership. It
   is not this pair's work, and it is not a reason to delay the surfaces.
6. Wording, not merely plumbing: a surface may say the agents repository has
   moved on since the pinned plan commit. It may **not** say the plan file
   changed — see "What this fact observes, and what it cannot" in § (c).

---

## Decisions

Where Astra's prose left a choice, the smaller option was taken.

1. **Kind 44249**, named `KIND_PROJECT_WORK_RECORD`. It is the lowest unused
   and unreserved number in this fork and in vanilla: 44231–44239 are reserved
   by the continuity research, 44240 is Pulse with 44241–44243 reserved by the
   Pulse plan, and 44244–44248 are the team transaction, policy, observation,
   handover and to-do op. `git grep 44249` over this tree matched nothing, and
   `git grep 44249 vanilla/main` (`12201c49b`) matched nothing.
2. **One kind, three record types** — not three kinds. They share an
   envelope, an authority rule and a fold; splitting them would triple the
   query surface for no separation anyone reads.
3. **`d` = `sessionRef`**, matching all four siblings, rather than `workId`.
   One familiar REQ returns a session's whole work set; `workId` lives in
   content where the fold indexes it.
4. **Six tags, `a` required** — but 44249 is **not** added to
   `is_project_a_scoped_kind`. The `a` tag is a selector; `h` is the gate.
   Claiming project-membership gating we did not build would be the kind of
   lie this repo treats as a crash.
5. **All three records need `may_lead`**, not a per-type authority table. An
   association is a claim to verify; a worker nominating what its own work
   proves is the failure mode this closes.
6. **16 KiB content ceiling**, not 44244's 128 KiB. Every field is a pointer
   or a slug.
7. **Nullable keys are present and `null`**, never absent — 44244's house
   rule, and what makes a complete `--example` possible.
8. **`unknown` is a first-class criterion status** with a reason, so an
   unreadable plan blob can never render as `open`.
9. **No mission-terminal field in the coverage output.** Two questions, two
   objects, shown as two rows; disagreement is disclosed.
10. **Evidence never carries across an amendment.** Re-attest at the new
    declaration; selective reuse waits for evidence, not for v1.
11. **`retired_criteria` is required and written, even when empty**, so the
    absence of retirements is a stated fact rather than an inference.
12. **`--example` on the write verbs only.** `validate` and `status` take
    explicit flags and read nothing they could get wrong, exactly as
    `sessions note` and `sessions decide` were left without one in 182.

13. **A `ref_observation` is a relay-signed kind:30618, judged at evaluation
    time** (orchestrator ruling, 2026-09-20). No new record type, no new
    signer class: the relay already signs ref state for repositories it hosts,
    and a superseded observation reads `stale` rather than `covered` because
    delivery is a fact about the branch *now*, not about the moment somebody
    bound it.
14. **A declaration is `stale` on exactly one trigger: a current-goal
    mismatch** (ruling). A 44244 decision never marks one stale by itself.
    One trigger the product already computes beats a second inference path
    that would guess at the semantics of arbitrary chat.
15. **Adoption is atomic; a failed compile signs nothing** (ruling). Exit 1
    with the compile error. A declaration that references something which did
    not resolve is a contract nobody can read.
16. **`planRef.repository` is the full 30617 coordinate** (ruling), matching
    the seat manifest's `packRef.repo`. A bare id is community-scoped and
    would be re-resolved by every later reader, which is how two readers pin
    two repositories.
17. **The NIP is W1's, written from this file** (ruling). This README is
    normative until `docs/nips/NIP-PW.md` exists, and the NIP must not
    diverge from it.

18. **The fold's inputs carry verified evidence facts, and this file defines
    the predicates** (A2 ruling, finding 3). The alternative — trusting a
    lead's binding — makes a signed claim into proof, which is the one thing
    § (b) says it must never be. The caller verifies signatures and resolves
    pointers because it has the events; the fold decides, deterministically,
    from facts. `actionDefinitions` is compiled by the caller with the
    publication compiler at the declaration's plan commit, so evidence can
    never nominate its own expected hash.
19. **One candidate artifact per declaration** (A2 ruling, finding 4).
    Per-criterion coverage at different commits is five true statements and
    one false conclusion: `mixed_artifacts` says so instead of reporting a
    delivery nobody verified.
20. **Authority is the existing `may_lead` predicate, all three arms**
    (A2 ruling, finding 5), including an active `may_steer` grant. A narrower
    contract here would have been settled twice — once in the CLI, once in the
    relay — and differently.
21. **`goalRef` is the goal; a decision goes in the new nullable
    `decisionRef`** (A2 ruling, finding 6). The earlier text permitted a
    decision id in `goalRef`, which decision 14 then made permanently stale.
    Both are 64-hex, so the fold refuses it, not the envelope — and says
    which key the id belongs in.
22. **A conflict is more than one maximal declaration of a `workId`**
    (A2 ruling, finding 7), not two successors of one predecessor. The
    descendant case and the two-roots case each have a fixture, because the
    direct-sibling reading cleared a live conflict.

23. **Evidence facts come from the canonical team projection, never from raw
    events** (A5 ruling, finding 6). The team fold already requires a report's
    signer to be its assignment's assignee, and drops a ruling a later one
    replaced; relay ingest checks structure and cannot. Two answers to one
    question is the defect — so coverage reads the projection the team contract
    produces, and a lead's binding of an excluded record changes nothing.
24. **Action definitions keep their `(repository, commit, name)` provenance
    through evaluation** (A5 ruling, finding 7). Collapsed to the name, one of
    two same-named definitions wins by sort order: the amended head's correct
    evidence fails and the superseded plan's evidence passes. Both sort orders
    have a fixture because the bug is invisible in one of them.
25. **A declaration with no criterion rows still says why it is not complete**
    (A5 ruling, finding 5): `plan_unavailable` for an unreadable plan,
    `conflict` for competing heads. Consumers gate on state and
    `coverageComplete`; an empty list is not an answer.

26. **Plan drift is a fact on every declaration row, and it is disclosure
    only** (A10, lane 234). `planDrift` is always present — `state: "unknown"`
    with a null `currentCommit` when no ref state says — because an absent
    object is what a reader defaults to "no drift", and because a strict
    reader must be able to tell an unknown from a build that does not emit the
    key. It reads a **branch tip**, the newest relay-signed 30618 for the
    agents repository at `refs/heads/main`, so it cannot claim the plan *file*
    was edited; it changes no `state`, no `status` and no `coverageComplete`,
    and completion is never refused for it.

## Owed by other lanes

The five open questions this lane raised, and the five adversarial-review
findings of amendment A2, were ruled on by the orchestrator on 2026-09-20 and
are now decisions 13–22 above. What they leave for other lanes:

- **W1** owns `docs/nips/NIP-PW.md`, written from this file, plus the unit
  tests behind each decision — in particular the relay-`self` signer check and
  the newest-30618 freshness read (13), and the current-goal fold that
  computes a stale declaration (14).
- **W2** owns the atomic `adopt` path (15) and the repository resolution that
  produces the 30617 coordinate at adoption time (16).
- **W1** also owns establishing the fold input of § (c) — verifying the
  evidence events and compiling `actionDefinitions` — outside the fold, and
  the reader test that an old build ignores 44249 entirely.
- **W1/W2** owe the assembler boundary of decision 23: the single place that
  builds `evidence` and `teamProjection` reads the canonical team fold, and
  `docs/nips/NIP-PW.md` (W1's file, not this lane's) needs the same amendment.
- **Every lane that reads authority** uses
  `CodingSessionTeamFoldContext::may_lead`, never a local enumeration (20).
