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

Governing plan: [`docs/UNIFIED_WORK_PLAN.md`](../../docs/UNIFIED_WORK_PLAN.md)
(W0 in § 3, Wave 0 in § 4). Design source: Astra's
[unified plan](../../docs/history/2026-09-20-astra-unified-plan.md) § 2.
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
  when the plan has a `git-ref` criterion that is not yet covered.
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

The three sequences:

| sequence | what it pins |
|---|---|
| `happy-path` | one declaration, five criteria, bindings and evidence to complete coverage; `coverageComplete: true` |
| `amendment` | P adopted, P2 supersedes it, then a **late** `work.evidence_bound` arrives naming P. It must not cover P2: that criterion is `stale`, `coverageComplete` is `false` |
| `fork` | two successors of P, neither superseded. Both are `conflict`, `conflicts` names both heads, coverage is refused for both |
| `superseded-observation` | a `git-ref` criterion bound to a 30618 that named the artifact commit, then superseded by a newer relay-signed 30618 naming a different commit: the criterion is `stale`, not `covered` |
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

`node conformance/project-work/check-fixtures.mjs` asserts every fixture
parses and that ids, hex lengths, uuids and byte limits are well-formed, so a
later edit cannot silently break them. It is deliberately **not** a parser or
a fold — those are W1's, and the fixtures are what they bind to. It is wired
into no CI recipe.

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
