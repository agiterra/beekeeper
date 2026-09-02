# Session Policy — kind 44245, `buzz-coding-session-policy/v1`

Status: CONTRACT — frozen for batch 2 lane B1. Implemented in
`crates/buzz-core/src/coding_session_policy.rs`, built by
`crates/buzz-sdk/src/coding_session_policy.rs`, structurally validated at
`crates/buzz-relay/src/handlers/ingest.rs`. **No fold, no CLI, and no UI exist
yet** — this document names each field's future consumer so that the record and
the thing that reads it cannot drift apart before the reader is written.

---

## 0. What this record is

One signed record saying *how a mission is meant to be run*: the posture, what
it may spend, how much the founder wants to be told, what a lane owes before
its work counts, who may be benched against whom, which acts stay the
founder's, and when to stop.

Before it, every one of those was either a sentence in a brief that no machine
reads, or nothing at all. A policy nobody can read is a policy nobody enforces,
and the live runs are the evidence: budgets lived in a launch dialog, "red
first" lived in `AGENTS.md`, and "ask before you push" lived in a person's
memory of having said it once.

| Fact | Value |
|---|---|
| Kind | `44245` |
| Schema | `buzz-coding-session-policy/v1` |
| Addressing | `d = sessionRef`; newest **accepted** record wins |
| Tags (exactly four, ordered, two fields each) | `h`, `d`, `csp-v`, `csp-genesis` |
| Signer | the founder, or a lead |
| Content cap | 32 KiB |

### Why 44245

The lowest unused and unreserved kind in this fork and in vanilla, checked the
same way `44244` was: `44231` (checkpoint), `44232` (native snapshot),
`44233`/`44234` (git transition/check) and `44235`–`44239` (headroom) are
reserved by the continuity research; `44240` is the shipped Pulse entry with
`44241`–`44243` reserved by the Pulse plan; `44244` is the team transaction.
`crates/buzz-core/src/kind.rs` carries the allocation note and a compile-time
assertion that nothing sits between the two.

### Regular, not replaceable

The `d` tag addresses an umbrella so a consumer can fold the newest revision.
It does **not** opt this kind into NIP-33 replacement, and a compile-time
assertion in `kind.rs` says so. A replaceable policy would let one author's
write erase the revision a decision was actually made under — and the first
question anyone asks about a mission that went wrong is *what was the policy at
the time*.

### The relay does not decide authority

The relay validates structure: schema, tags, closed vocabularies, bounds,
tag-to-content parity. Whether the signer held the standing to set policy is
the consuming fold's question, answered against the accepted NIP-CSAT chain —
the same division kind 44244 draws. A relay that adjudicated policy authority
would be asserting standing it cannot verify at ingest time.

---

## 1. Every field, and who consumes it

Only `schema`, `sessionRef` and `genesisRef` are required. Everything else is
**omitted** when unset, never written as an explicit `null`.

| Field | Type | Consumer | What it decides |
|---|---|---|---|
| `posture` | `spike` \| `ship` \| `investigate` \| `overnight` | **router** | The risk tier a class is routed at |
| `budget.turns` | `u32` ≥ 1 | **router** | Ceiling on turns across the umbrella |
| `budget.tokensPerSeat` | `u64` ≥ 1 | **router** | Ceiling on tokens any one seat may spend |
| `budget.tokensPerSession` | `u64` ≥ 1 | **router** | Ceiling on tokens the umbrella may spend |
| `budget.costUsdPerSession` | finite `f64` > 0 | **router** | Ceiling on dollars the umbrella may spend |
| `budget.contextTier` | `standard` \| `long` | **router** | Which context window seats run in |
| `attention` | `decisions` \| `decisions-and-milestones` \| `everything` | **UI** | What a person is shown — never what the machine does |
| `gates.redFirst` | `bool` | **lead pack** | Acceptance tests are written failing first |
| `gates.reviewEveryLane` | `bool` | **lead pack** | Every lane is reviewed by someone who did not write it |
| `gates.requiredGates` | ≤ 32 names, ≤ 64 B each, unique | **lead pack** | Named gates every lane must run |
| `gates.verifierRequired` | `bool` | **lead pack** | A verifier must rule before the mission may settle |
| `bench.identities` | ≤ 64 lowercase 64-hex pubkeys, unique | **router** | Identities eligible for the bench |
| `bench.providers` | ≤ 16 provider **aliases**, ≤ 256 B each, unique | **router** | Provider instances eligible for the bench |
| `bench.challengerSampleRate` | finite `f64` in `0.0..=1.0` | **router** | Fraction of eligible jobs given to a challenger |
| `irreversible` | non-empty unique subset of `push`, `deploy`, `delete`, `external-message` | **fence** | Acts that need the founder's word |
| `stop.timeBoxSecs` | `u64` ≥ 1 | **lead pack** | Wall-clock seconds after which the lead stops opening work |
| `stop.onMilestone` | 1..=8 KiB, no control characters | **lead pack** | The milestone whose arrival ends the mission |

`bench.providers` names provider **aliases** (`claude-primary`), never instance
ids (`1958c6c448e05eed`). The two are different names for different things and
`crates/buzz-core/src/coding_session_identity.rs` now makes confusing them a
compile error; ledger item 102 is what happens when nothing does.

---

## 2. The four rules a reader must implement

### 2.1 Unknown fields are rejected in v1

Not ignored. A consumer that tolerated an unknown key would be claiming to
enforce a policy it cannot read — the founder writes `noPushWithoutReview`,
every gate accepts the record, and nothing enforces anything. The refusal names
the offending key so the author can fix it rather than guess.

The cost of this rule is that a v2 field cannot be introduced by writing it
into a v1 record. That is the intended cost: a new field arrives with a new
schema string, and a v1 reader says plainly that it cannot read a v2 policy.

### 2.2 Absent is not null

An unset field is omitted. An explicit `null` is refused, by name, at the top
level and inside every nested object. A key-set check sees a null as *present*
and serde then decodes it to `None`, so without this rule the same signed bytes
mean "unset" to one reader and "set to nothing" to another — precisely the
divergence the item-102 ingress rules forbid.

### 2.3 A record that sets nothing is the withdrawal, and is valid

`{schema, sessionRef, genesisRef}` decodes, and
`CodingSessionPolicyPayload::sets_any_policy()` answers `false`. Under a
newest-wins fold that is the only way to *take a policy back*, so it is a legal
record rather than an error. A consumer must render it as "no policy", never as
"policy unknown": those are different facts and the first one is a decision
somebody made.

Note the asymmetry that follows: an **empty sub-object** (`"budget": {}`) is
refused. A whole record that sets nothing is a withdrawal; a `budget` that sets
nothing inside a record that sets other things is noise, and the way to say
"no budget" is to omit the key.

**An empty array is an empty sub-object one level down**, and is refused the
same way. `"gates": {"requiredGates": []}`, `"bench": {"identities": []}` and
`"bench": {"providers": []}` are all rejected with the sentence `irreversible`
already used — *"must not be empty: omit the key to …"*. Without that rule a
record whose every collection was empty passed the "must carry at least one
field" guard and answered `sets_any_policy() == true`: a record that set
nothing claiming to set something, and a withdrawal any writer could silently
impersonate. `sets_any_policy()` asks whether a **value** is set, never whether
a key is present.

### 2.4 A limit of zero is refused, not stored

`turns: 0`, `tokensPerSeat: 0`, `tokensPerSession: 0`, `costUsdPerSession: 0`
and `timeBoxSecs: 0` are all refused. Zero and "no limit" would otherwise be
the same record read two ways, and a budget a reader can misread as unlimited
is worse than no budget at all. A `tokensPerSeat` above `tokensPerSession` is
refused for the same reason: a ceiling that cannot bind is not a ceiling.

---

## 3. Wire example

```json
{
  "schema": "buzz-coding-session-policy/v1",
  "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  "genesisRef": "1212121212121212121212121212121212121212121212121212121212121212",
  "posture": "overnight",
  "budget": {
    "turns": 240,
    "tokensPerSeat": 4000000,
    "tokensPerSession": 40000000,
    "costUsdPerSession": 120.5,
    "contextTier": "long"
  },
  "attention": "decisions",
  "gates": {
    "redFirst": true,
    "reviewEveryLane": true,
    "requiredGates": ["just ci", "just test"],
    "verifierRequired": false
  },
  "bench": {
    "identities": [
      "abababababababababababababababababababababababababababababababab",
      "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    ],
    "providers": ["claude-primary", "codex-primary"],
    "challengerSampleRate": 0.25
  },
  "irreversible": ["push", "deploy", "external-message"],
  "stop": {
    "timeBoxSecs": 28800,
    "onMilestone": "the lane lands and CI is green"
  }
}
```

Tags for that record, in order:

```json
[
  ["h", "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86"],
  ["d", "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"],
  ["csp-v", "buzz-coding-session-policy/v1"],
  ["csp-genesis", "1212121212121212121212121212121212121212121212121212121212121212"]
]
```

---

## 4. What is deliberately absent from v1

- **No fold.** Newest-accepted-wins is stated here and implemented by whoever
  writes the first consumer, beside the authority check it needs.
- **No CLI and no UI.** `bee sessions policy` and the launch form are later
  work; this lane freezes the record they will write.
- **No enforcement.** Nothing in this repository refuses a turn because of a
  budget in a 44245 today. Until a consumer exists, a published policy is a
  **stated intention, not an enforced limit** — and any surface that displays
  one must say so, rather than showing a budget bar that nothing is counting.

---

## 5. The same disclosure, owed for `requestedBy` (kind 44221)

Not a 44245 field, recorded here because it is the same honesty rule and the
same owner (B2), and nothing else states it.

A `session.hire` may carry `requestedBy`: the pubkey of the seat that ran `bee
sessions hire`. **The relay does not verify it, and that is a choice, not an
impossibility.** The relay verifies the event signature and therefore holds
`event.pubkey`; comparing it with `action.requestedBy` is one line beside
`hire_authority_verdict`. v1 leaves the comparison to the consumer because
LANE-B1 §B1.2 scoped it there.

The consequence, which nothing currently discloses: **any signer the relay
admits for a hire can attribute that request to a different seat's pubkey.**
`buzz-core` exposes the claim beside the signer —
`CodingSessionLifecycleCommandPayload::hire_requester_matches_signer`, three
answers (`Some(true)` attributed, `Some(false)` disputed, `None` unclaimed) —
and **no consumer calls it yet**.

So, exactly as with a policy that nothing enforces: until a consumer compares
them, `requestedBy` is an **unverified claim, not an attribution**, and any
surface that renders it (a Mission byline, a seat row, an audit table) must say
so rather than printing a lead's name as fact. **B2 owns closing this** at the
CLI and at the founder's host, and disclosing a mismatch rather than dropping
it. The same sentence is carried in
`crates/buzz-core/testdata/coding_session_hire_requester/vectors.json`, which
is the file B3's TypeScript decoder is pinned to.
