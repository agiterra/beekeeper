NIP-PW
======

Project Work Records
--------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (basic event format), NIP-CSG (the session genesis every record roots at), NIP-CSAT (the authority chain the `may_lead` projection comes from), NIP-CSTX (kind:44244 — the assignments, reports, verdicts and completion this joins to), NIP-MP (the project coordinate), NIP-PK (the agents repository a plan lives in). Interacts with kind:30618 (relay-signed repository ref state) and kind:44227 (the session's current goal).

This document is written from `conformance/project-work/README.md`, which is the frozen contract (ledger item 194) and stays normative where the two could differ. The fixtures under `conformance/project-work/fixtures/` pin every rule stated here, and `crates/buzz-core/src/project_plan.rs`, `project_work.rs` and `project_work_fold.rs` implement them (ledger item 195).

## Abstract

This NIP defines `kind:44249`, a **project work record**: one signed statement joining a committed plan file to the work that delivers it. One kind carries three closed record types — `work.declared` (a lead adopts `plans/<slug>.md` at an exact agents commit as this work's contract), `work.assignment_bound` (which criteria a kind:44244 assignment owes) and `work.evidence_bound` (which evidence answers which criteria, at which artifact commit).

Coverage is not an event. It is what every reader gets by folding a session's work records under one pure, order-independent rule set, against the plan blobs read at the commits the declarations pinned.

## Motivation

A session can say what happened — assignments, reports, verdicts, a completion — and cannot say **what remains**. Acceptance criteria lived in prose in a brief, so nothing could check them, and a `mission.completed` could settle a mission whose stated obligations were unmet with no way to see the discrepancy.

The join is a *reference*, never a copy. Criterion text is read from `plans/<slug>.md` in the project's agents repository with `git show <commit>:<path>`, so the contract is a git object that cannot be edited out from under the work that references it. The commit is the version; there is no second counter, and editing, renaming or archiving the file cancels, amends or rewrites nothing already adopted.

### Why a sibling kind and not three more 44244 subtypes

kind:44244's operation vocabulary is a closed serde enum with no `other` arm, and `fold_coding_session_team_transactions` returns `Err` for the **whole set** on one unrecognised envelope (the finding-13 cliff). A build predating this vocabulary would therefore read a session carrying one work record as a broken mission, while a build that never heard of 44249 simply never queries it.

And work coverage and mission settlement are two questions that must be able to **disagree**. A terminal record with incomplete coverage is exactly what an older client's completion looks like; merging them into one fold would make that unsayable.

## Event

`kind:44249` — regular, stored, append-only. Never replaceable, never ephemeral.

Content is strict public JSON, at most **16 384 bytes**, `schema` = `buzz-project-work/v1`, camelCase, unknown keys rejected at every level (payload, body, `planRef`, each `evidenceRefs` entry). Every nullable key is **present and written as JSON `null`**; an absent key is a refusal.

16 KiB, not 44244's 128 KiB: every field here is a pointer or a slug. A record that needs more prose is carrying something that belongs in the plan blob or in a 44244 report.

### Tags

**Exactly six** two-field tags, in this order:

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

Tag-to-content parity is checked on every read, exactly as 44244 checks it. A tag that disagrees with content is a refusal, not a preference.

`d` is the **sessionRef**, matching all four siblings (44244–44247), so one REQ `{"kinds":[44249],"#d":["<sessionRef>"]}` returns everything the fold needs using a query shape every reader already builds. `workId` lives in content, where the fold indexes it; it is stable across amendments and groups a declaration with its successors without needing its own tag.

The `a` tag is a canonical, singleton **project selector** so a reader can pick one project's work. It does **not** put 44249 in `is_project_a_scoped_kind`: 44240 (Pulse) and 44248 (to-do op) are gated by project membership *alone, with no channel*, while 44249 is gated by channel membership through `h`, like its coding-session siblings. A const assert in `crates/buzz-core/src/kind.rs` pins this, and saying otherwise would claim a relay gate that does not exist.

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
| `workId` | uuid | no | canonical lowercase; **stable across amendments** |
| `goalRef` | 64-hex | no | the kind:44227 goal, or decision record, this work serves |
| `responsibleActor` | 64-hex | no | who owes the outcome; a target, never an authorship claim |
| `planRef.repository` | coordinate | no | the **full** kind:30617 coordinate. A bare repo id is refused: a name is community-scoped and would be re-resolved by every later reader, which is how two readers end up pinning two repositories |
| `planRef.commit` | hex | no | full immutable agents commit, 40 or 64 lowercase hex |
| `planRef.path` | string | no | relative, ≤ 256 bytes, starts `plans/`, no `..`, no leading `/` |
| `supersedes` | list of 64-hex | no (may be `[]`) | ≤ 8 declaration event ids; empty on first adoption, one for an ordinary amendment, **all** conflicting heads for an explicit conflict resolution |

### `work.assignment_bound`

```json
{"...": "envelope as above", "type": "work.assignment_bound",
 "body": {"declarationRef": "<64-hex>", "criterionIds": ["cli-behaviour"],
          "assignmentRef": "<64-hex>", "replacesBinding": null}}
```

`criterionIds` is 1–64 unique slugs, each ≤ 64 bytes, each of which must exist in the declaration's plan — a **fold** check, not a relay check. `replacesBinding` is the earlier binding this supersedes; the key is always present, written `null` when there is none.

### `work.evidence_bound`

```json
{"...": "envelope as above", "type": "work.evidence_bound",
 "body": {"declarationRef": "<64-hex>", "criterionIds": ["verified-landed-revision"],
          "artifactCommit": "<40- or 64-hex>",
          "evidenceRefs": [{"kind": "verdict", "eventId": "<64-hex>"}],
          "completionRef": null}}
```

`evidenceRefs` is 1–32 entries, each exactly `{"kind", "eventId"}`, `kind` one of `report` | `verdict` | `action_result` | `ref_observation`. `artifactCommit` is the **code** commit the evidence is about. `completionRef` is the kind:44244 `mission.completed` this coverage was computed for; present, `null` when none.

### Size ceilings

| limit | value |
|---|---|
| content, total | 16 384 bytes |
| `criterionIds` | 64 entries, ≤ 64 bytes each |
| `evidenceRefs` | 32 entries |
| `supersedes` | 8 entries |
| `planRef.path` | 256 bytes |
| `planRef.repository` | 128 bytes |
| `projectRef` | 256 bytes |

## The plan file — `beekeeper-plan/v1`

A UTF-8 Markdown file at `plans/<slug>.md` in the project's agents repository (`<slug>-beekeeper-agents`). YAML frontmatter is the contract; the body is context for people and seats and is never parsed.

Frontmatter carries **exactly** these keys — an unknown key is a refusal, never an ignored key: `schema` (exactly `beekeeper-plan/v1`), `id` (slug, ≤ 64 bytes), `status` (`in-force` | `superseded`), `title` (1–200 bytes, one line), `code_repository` (a repository id `[a-z0-9][a-z0-9-]*`, 1–64 bytes), `delivery_ref` (a full git ref such as `refs/heads/main`, 1–256 bytes, no glob), `criteria` (1–64 entries), `retired_criteria` (list of slugs, **required and written even when empty**).

Each criterion has exactly `id` (slug), `accept` (1–1024 bytes, non-empty after trimming) and `proof`, which is exactly one of:

```yaml
proof: {kind: review}                              # a person or authorized seat rules
proof: {kind: action, name: verify, step: verify}  # a named action + step in the same
                                                   # agents commit's actions.yml
proof: {kind: git-ref}                             # delivery_ref observed at the
                                                   # artifact commit
```

`proof` is an evidence *requirement*: not a command, not a condition language, not a dependency graph. All criteria are required; a waiver is an explicit plan amendment, never a checkbox.

Slug grammar is `[a-z0-9] ( [a-z0-9-]* [a-z0-9] )?`, 1–64 bytes, ASCII only.

Ids name obligations, not positions. They are stable across reorder and across a file move or rename. A spelling correction to `accept` keeps the id; a substantive change requires new evidence under the newly adopted revision, which is why every evidence binding names a declaration, a criterion id **and** an artifact commit at once. A removed obligation moves to `retired_criteria` and is **never recycled** in that plan.

File limits and refusal codes: `plan-too-large` (65 536 bytes), `too-many-criteria` (64), `too-many-retired` (256), `accept-too-long` (1024), `slug-too-long` (64). Also refused: unknown `schema`, unknown frontmatter/criterion/`proof` key, duplicate criterion id, active/retired overlap, empty or whitespace-only `accept`, an action `name`/`step` that is not a slug, a plan path escaping `plans/` or resolved through a symlink, a malformed `code_repository` or `delivery_ref`.

`status` governs **new adoption only**. `superseded`, and any file under `plans/archive/`, refuse a new `adopt` and nothing else. The blob is read with `git show <commit>:<path>`, **never** the working copy and never the fetched tip; a plan the fold cannot read at the pinned commit yields `unknown`, never `open` and never `covered`.

## Authority

All three records require the **same** authority: the `may_lead` set the 44244 fold already projects — the session founder, any actor holding a live `grant-seat` in the `lead` seat on a whole contiguous kind:44228 chain, and any steer grantee — further scoped to the named project and session. Repository write alone is not sufficient; holding a lead seat in a *different* session is not sufficient.

One rule for three records, on purpose: an association is a **claim the fold must verify**, never proof and never authority. A worker states what it did in its 44244 `report`; the lead binds that report to a criterion. Letting a worker bind its own evidence would let it nominate what its work proves.

**The relay validates structure only.** The schema, the six ordered tags, the closed vocabularies, every bound and the tag-to-content parity are all answerable from one event, so the relay answers them and rejects with the contract's stable code (`record-type`, `schema`, `tag-parity`, `hex`, …). Whether the signer held `may_lead` is **not** answerable there, and is left to the consuming fold — the same division kinds 44244, 44245, 44246 and 44247 draw. A record signed by anyone else is excluded by the fold with a named reason; it is not a fold-wide error and it never silently becomes coverage.

Channel membership is checked before any of this: 44249 is a coding-session kind, so a non-member is refused before its content is parsed.

## The fold

`fold_work(inputs) -> WorkProjection` is a pure function of (the 44249 event set, the resolved plan blobs, the `may_lead` projection, the relay's self key, the session's current goal, relay-signed kind:30618 ref states, and which referenced evidence events the caller established). **No clock, no network, no ordering assumption**: the same events in any arrival order fold to the same output, and a duplicate delivery is harmless. It verifies *references and scope*; it does not re-verify signatures the relay verified at ingest.

```json
{
  "schema": "buzz-project-work-coverage/v1",
  "sessionRef": "…", "projectRef": "30621:…:kettle",
  "declarations": [{
    "workId": "…", "declarationRef": "…", "planRef": {"…": "…"},
    "state": "head", "supersedes": [], "supersededBy": [], "stateReason": null,
    "planResolved": true,
    "criteria": [{
      "criterionId": "cli-behaviour", "proof": {"kind": "review"},
      "status": "covered", "assignmentRefs": ["…"],
      "evidence": [{"kind": "verdict", "eventId": "…"}],
      "artifactCommit": "…", "reason": null
    }],
    "coverageComplete": false,
    "coverageReason": "1 of 5 criteria are not covered under this declaration: usage-documentation is open"
  }],
  "excluded": [{"eventId": "…", "code": "signer-not-may-lead", "message": "…"}],
  "conflicts": [{"workId": "…", "heads": ["…", "…"], "message": "…"}]
}
```

### Per-declaration `state`

| state | meaning |
|---|---|
| `head` | the current declaration for its `workId`: nothing supersedes it and it is not in a conflict |
| `superseded` | another declaration names it in `supersedes`; `supersededBy` lists them |
| `stale` | still the head, but the session's **current** kind:44227 goal differs from the declaration's `goalRef`. That mismatch is v1's **only** trigger; a 44244 decision never marks a declaration stale by itself. The contract stays pinned, and `stateReason` names both goals |
| `conflict` | two or more unsuperseded successors of the same predecessor exist for this `workId`. Every one of them is `conflict`, `coverageComplete` is `false` for all, and a resolution naming **all** competing heads is required. No timestamp winner |

`superseded` and `conflict` are structural and take precedence over `stale`. `criteria` is projected for `head` and `stale` only — the two states that *are* a current contract. A `superseded` or `conflict` declaration carries `criteria: []` and a `coverageReason` saying why; bindings made under a conflicted head are retained on the wire and simply not projected, and resolving the fork projects them.

Declarations are reported grouped by `workId`, current contracts first (`head`, `stale`, `conflict`, `superseded`) and then ascending by `declarationRef`. `excluded` is ascending by event id.

### Per-criterion `status`

| status | meaning | `reason` |
|---|---|---|
| `open` | no evidence binding under the head declaration names this criterion | `null` |
| `covered` | ≥ 1 `work.evidence_bound` under the head declaration names it, every `evidenceRefs` entry resolved, and the required proof form was satisfied | `null` |
| `stale` | every binding carrying this criterion names a declaration that is **not** the head (the late-green-for-P case), or its `ref_observation` has been superseded by a newer ref state naming a different commit | names the declaration, or the newer ref state |
| `unknown` | the fold could not read an input: the plan blob at `planRef.commit` was not supplied, or a referenced evidence event was not established | names the missing input |

`unknown` is a **result, not an error**. A fold given no plan blob returns every criterion it can name `unknown` with `planResolved: false` — never `open`, which would read as "nothing has been done" when the truth is "we cannot see the list".

An amendment does not carry evidence forward. Evidence bound to P stays bound to P; under P2 those criteria are `stale` until re-bound at P2. Selective reuse is deliberately out of v1.

### `ref_observation`, and how delivery is judged

A `ref_observation` carries no new record. Its `eventId` is a **relay-signed kind:30618 ref-state event** for the plan's `code_repository`: the signer must equal the relay's NIP-11 `self` key (an owner-signed claim about its own branch is not an observation), its `d` tag names that repository, and its `refs/heads/<branch of delivery_ref>` tag must equal the binding's `artifactCommit`.

**Freshness is judged at evaluation time, not at binding time.** The *newest* 30618 for that repository must still name that commit for that ref. An older matching 30618 that a newer one superseded with a different commit makes the criterion `stale` — the branch moved on, and what was delivered is no longer what is there. Ref state that cannot be read makes it `unknown`.

### `coverageComplete`, and the 44244 terminal

`coverageComplete` is `true` for a declaration when **all** of: `state == "head"`; `planResolved == true`; every criterion in the plan's `criteria` (retired ones are not counted) is `covered`; and its `workId` appears in no entry of `conflicts`. Otherwise it is `false` and `coverageReason` says which clause failed, naming criteria.

**This output contains no mission-terminal field, deliberately.** Whether the 44244 `mission.completed` folded to terminal is a different question with a different fold. A reader that shows both shows **two rows**:

```
coverage : incomplete — usage-documentation open
mission  : terminal   — completed <event id>
```

The two disagreeing is a **disclosure**, not a reconciliation. Nothing here merges them, makes one imply the other, or lets coverage publish or withhold a completion.

## Compatibility

1. **An old reader ignores kind 44249 entirely.** It never queries it, so it never sees it. Nothing about a session's 44244 history changes: the same events fold identically in a build that has never heard of 44249, and a mixed stream selected by kind produces the same projection. `crates/buzz-core/src/project_work_fold_tests.rs` proves both, and also that the 44244 reader refuses a 44249 envelope outright rather than misreading it.
2. **Nothing here adds a key to 44244, 44223 or 44228.** The `may_lead` projection this contract consumes is the one 44228 already produces.
3. **Relay-first landing.** 44249 changes relay ingest, so a desktop that depends on it is installed only after the relay serving it has been deployed (NIP-11 `build_time` past the landing time).
4. **Coverage is not a genesis bump.** An old client publishing a `mission.completed` with no coverage is a *discrepancy to disclose*, never a claim that the old client enforced this contract.

## Reference implementation

| what | where |
|---|---|
| plan parser and refusal codes | `crates/buzz-core/src/project_plan.rs` |
| closed envelope, records, validators | `crates/buzz-core/src/project_work.rs`, `project_work_decode.rs` |
| the coverage fold | `crates/buzz-core/src/project_work_fold.rs`, `project_work_fold_project.rs` |
| typed builders | `crates/buzz-sdk/src/project_work.rs` |
| relay ingest admission | `crates/buzz-relay/src/handlers/project_work.rs` |
| kind constant and const asserts | `crates/buzz-core/src/kind.rs` |
| fixtures | `conformance/project-work/fixtures/` |
