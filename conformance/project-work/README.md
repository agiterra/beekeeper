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
    "responsibleActor": "<64-hex pubkey>",
    "planRef": {
      "repository": "<repo id>",
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
| `goalRef` | 64-hex | no | the kind:44227 goal, or the decision record, this work serves |
| `responsibleActor` | 64-hex | no | the actor who owes the outcome; a target, never an authorship claim |
| `planRef.repository` | string | no | agents repository id, 1–64 bytes, repo-id grammar |
| `planRef.commit` | hex | no | **full** immutable agents commit, 40 or 64 hex, lowercase |
| `planRef.path` | string | no | relative, ≤ 256 bytes, starts `plans/`, no `..`, no leading `/` |
| `supersedes` | list of 64-hex | no (may be `[]`) | ≤ 8 declaration event ids; empty on first adoption, one for an ordinary amendment, **all** conflicting heads for an explicit conflict resolution |

Criteria text is never copied onto the wire. The plan id is read from the
blob, not restated here.

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

### Who may sign

All three records require the **same** authority: the `may_lead` set the
44244 fold already projects — the session founder, and any actor holding a
live `grant-seat` in the `lead` seat on a whole contiguous kind:44228 chain —
further scoped to the named project and session. Repository write alone is
not sufficient; holding a lead seat in a *different* session is not
sufficient.

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
| `planRef.repository` | 64 bytes |
| `projectRef` | 256 bytes |

16 KiB, not 44244's 128 KiB: every field here is a pointer or a slug. A record
that needs more prose is carrying something that belongs in the plan blob or
in a 44244 report.

Unknown keys are rejected at every level — payload, body and `planRef` /
`evidenceRefs` entries.

---

## (c) The fold's output

A pure function of (the 44249 event set, the resolved plan blobs, the signer
authority projection). No clock, no network, no ordering assumption: the same
events in any arrival order fold to the same output.

```json
{
  "schema": "buzz-project-work-coverage/v1",
  "sessionRef": "11111111-2222-4333-8444-555555555555",
  "projectRef": "30621:<64-hex>:kettle",
  "declarations": [
    {
      "workId": "<uuid>",
      "declarationRef": "<64-hex>",
      "planRef": {"repository": "…", "commit": "…", "path": "…"},
      "state": "head",
      "supersedes": [],
      "supersededBy": [],
      "stateReason": null,
      "planResolved": true,
      "criteria": [
        {
          "criterionId": "cli-behaviour",
          "proof": {"kind": "review"},
          "status": "covered",
          "assignmentRefs": ["<64-hex>"],
          "evidence": [{"kind": "verdict", "eventId": "<64-hex>"}],
          "artifactCommit": "<40-hex>",
          "reason": null
        }
      ],
      "coverageComplete": false,
      "coverageReason": "1 of 5 criteria are not covered: usage-documentation is open"
    }
  ],
  "excluded": [
    {"eventId": "<64-hex>", "code": "signer-not-may-lead", "message": "…"}
  ],
  "conflicts": [
    {"workId": "<uuid>", "heads": ["<64-hex>", "<64-hex>"],
     "message": "two successors of <64-hex>; completion refused until an authorized record names both"}
  ]
}
```

### Per-declaration `state`

| state | meaning |
|---|---|
| `head` | the current declaration for its `workId`: nothing supersedes it and it is not in a conflict |
| `superseded` | another declaration names it in `supersedes`; `supersededBy` lists them |
| `stale` | still the head, but the lead has recorded an accepted goal/decision change against it; the contract is pinned and a new adoption is owed. `stateReason` names the decision event |
| `conflict` | two or more unsuperseded successors of the same predecessor exist for this `workId`; every one of them is `conflict`, `coverageComplete` is `false` for all, and a resolution naming **all** competing heads is required. No timestamp winner |

`criteria` is projected for `head` and `stale` declarations only — those are
the two states that *are* a current contract. A `superseded` or `conflict`
declaration carries `criteria: []` and a `coverageReason` saying why. Bindings
made under a conflicted head are retained on the wire and are simply not
projected; nothing is deleted, and resolving the fork projects them.

### Per-criterion `status`

| status | meaning | `reason` |
|---|---|---|
| `open` | no evidence binding under the head declaration names this criterion | `null` |
| `covered` | ≥ 1 `work.evidence_bound` under the head declaration names it, every `evidenceRefs` entry resolved, and the required proof form was satisfied | `null` |
| `stale` | evidence for this criterion exists, but every binding that carries it names a declaration that is **not** the head — the late-green-for-P case | names the declaration it was bound to |
| `unknown` | the fold could not read an input: the plan blob at `planRef.commit` was not supplied, or a referenced evidence event was not supplied | names the missing input |

`unknown` is a result, not an error. A fold given no plan blob returns every
criterion `unknown` with `planResolved: false` — never `open`, which would
read as "nothing has been done" when the truth is "we cannot see the list".

An amendment does not carry evidence forward. Evidence bound to P stays bound
to P; under P2 those criteria are `stale` until re-bound at P2. This is the
"re-attest all criteria after amendment" rule; selective reuse is deliberately
out of v1.

### `coverageComplete`, and the 44244 terminal

`coverageComplete` is `true` for a declaration when **all** of:

1. `state == "head"`,
2. `planResolved == true`,
3. every criterion in the plan's `criteria` (retired ones are not counted) is
   `covered`,
4. its `workId` appears in no entry of `conflicts`.

Otherwise it is `false` and `coverageReason` says which clause failed, naming
criteria.

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
- A sequence's `inputs.json` carries the `mayLead` signer set and the plan
  blobs the fold was given, keyed `<repo>@<commit>:<path>`. Both amendment
  commits resolve to the *same* plan fixture on purpose: identical criterion
  ids must not carry evidence forward.

The three sequences:

| sequence | what it pins |
|---|---|
| `happy-path` | one declaration, five criteria, bindings and evidence to complete coverage; `coverageComplete: true` |
| `amendment` | P adopted, P2 supersedes it, then a **late** `work.evidence_bound` arrives naming P. It must not cover P2: that criterion is `stale`, `coverageComplete` is `false` |
| `fork` | two successors of P, neither superseded. Both are `conflict`, `conflicts` names both heads, coverage is refused for both |

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

## Open questions for the orchestrator

1. **Does a `git-ref` criterion need the observation signed?** (c) counts a
   `ref_observation` evidence ref as satisfying it, but nothing here says who
   may sign a repository observation or how fresh it must be. W1 will need a
   rule; the smallest is "any may_lead actor, and the fold reports the
   observation's own timestamp without judging it".
2. **`stale` on a declaration needs a decision event to point at.** The
   contract says the lead records an accepted goal/decision change and that
   marks the declaration stale. Which record is that — a 44244
   `decision.answer`, a new 44227 goal, or both? W1 cannot compute `stale`
   until this is named.
3. **Does `bee sessions work adopt` publish anything when the action compile
   fails?** Stated as "blocks adoption". Silent refusal with exit 1 is
   assumed; confirm nothing partial is signed.
4. **Repository resolution at adoption.** "Resolve repository names to
   canonical project-associated repository coordinates; refuse ambiguity" —
   the coordinate form (`30617:<pubkey>:<id>`) is not stored in `planRef`,
   only the bare id. Should `planRef.repository` carry the full coordinate
   instead? Kept small here; changing it later is a v2.
5. **Where does this contract live long-term?** The siblings each have a
   `docs/nips/NIP-*.md`. This directory is the normative source today and the
   kind constant points at it. If a NIP file is wanted, it is a separate
   lane's file, not a lane's silent addition.
