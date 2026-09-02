# Session Policy — kind 44245, `buzz-coding-session-policy/v1`

Status: CONTRACT — frozen for batch 2 lane B1. Implemented in
`crates/buzz-core/src/coding_session_policy.rs`, built by
`crates/buzz-sdk/src/coding_session_policy.rs`, structurally validated at
`crates/buzz-relay/src/handlers/ingest.rs`. The first consumer landed in batch 2 lane B2: the session
provider reads the newest accepted record for an umbrella into its context
package and enforces exactly one field from it (see §4), and
`bee sessions policy set|get|clear`
(`crates/buzz-cli/src/commands/sessions/policy.rs`) writes and reads it. **No UI
exists yet** — this document names each field's consumer so that the record and
the thing that reads it cannot drift apart.

The **wire contract** is `docs/nips/NIP-CSP.md`, written at landing as the
companion to `NIP-CSTX.md`: envelope, exact keys, closed vocabularies, bounds,
and the four decoding rules. This document is the design note behind it — who
consumes each field, and why v1 draws each line where it does. Where the two
describe the same rule they say the same thing; where they differ in scope, the
NIP is what another implementation reads.

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
| Signer | the founder, or a seat holding an operator grant |
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

## 4. Exactly one field is enforced; everything else is a stated intention

Batch 2 lane B2 wrote the first consumer, so the older sentence here — "nothing
in this repository refuses a turn because of a budget in a 44245" — is now
false, for one field and no others. This section says which, and repeats the
disclosure the rest of the record still owes its reader.

### 4.1 Enforced: `budget.turns`, at the provider's turn gate

`budget.turns`, when set, is the umbrella's turn ceiling. It **overrides**
`BUZZ_CSP_TURN_BUDGET` for that session rather than tightening it, and it binds
even where the host set no ceiling at all — the common case, since the
environment default is unlimited. A host ceiling and a session ceiling answer
different questions (*how much will this machine spend on anything* versus *how
much was this mission authorized to spend*), and the session's own signed
answer is the more specific one.

- Where: `crates/buzz-session-provider/src/commands.rs`
  `exhausted_umbrella_budget`, reached from `decide_turn`'s D9 gate and from
  the create path's first-turn check, which share the predicate so the two can
  never disagree about who is exempt.
- The exemption is unchanged: the **umbrella's founder** is never refused for a
  budget. A ceiling bounds delegated work and the founder is who it protects.
- The refusal names the ceiling that bound it. A `BUDGET_EXHAUSTED` message
  raised by a policy says so in the word *policy* and points at publishing a
  new 44245; one raised by the environment says "Turns per team session" and
  points at restarting the provider. Two ceilings changed in two completely
  different places must never produce one indistinguishable sentence.
- Who may set it: the umbrella's founder, or an identity holding a verified
  operator grant on it. The relay stores a structurally valid 44245 from any
  channel member, so a consumer that skipped this check would let anyone in the
  room bind — or lift — the crew's allowance. A `lead` that holds no operator
  grant is **not** recognized: a provider can prove a grant and cannot prove a
  role slug it did not mint.
- When it is read: at every create and every resume under the umbrella, from
  the relay, folded newest-accepted-wins by
  `context_projector::select_session_policy` — the same fold that fills the
  context package, so `session_overview` and the gate always name the same
  record. **A policy published while a seat is already running does not bind
  that umbrella until its next create or resume.** That is a real gap, stated
  rather than hidden.

### 4.2 Not enforced: every other field

`posture`, `budget.tokensPerSeat`, `budget.tokensPerSession`,
`budget.costUsdPerSession`, `budget.contextTier`, `attention`, `gates.redFirst`,
`gates.reviewEveryLane`, `gates.requiredGates`, `gates.verifierRequired`,
`bench.identities`, `bench.providers`, `bench.challengerSampleRate`,
`irreversible`, `stop.timeBoxSecs` and `stop.onMilestone` are **read and shown,
and nothing checks them**. Publishing one changes no behaviour anywhere in this
repository today.

For each of them a published policy remains a **stated intention, not an
enforced limit**, and *any surface that displays one must say so* — rather than
showing a budget bar that nothing is counting, or a "red first" badge that no
gate is holding a lane to. The context package's `session_overview` carries
that sentence beside the record (`policySemantics.notEnforced`); a CLI or UI
that renders a policy owes its reader the same one. `bee sessions policy`
prints it as `enforcement` on `set`, `get` and `clear`
(`crates/buzz-cli/src/commands/sessions/policy.rs`
`POLICY_ENFORCEMENT_DISCLOSURE`), and Desktop renders
`CODING_SESSION_POLICY_STATED_NOT_ENFORCED`
(`desktop/src/features/coding-sessions/lib/codingSessionPolicy.ts`), whose
companion `CODING_SESSION_POLICY_ENFORCED_FIELDS` holds `budget.turns` and
nothing else — the one list on that side that may claim a field is enforced.

### 4.3 The CLI writes it and reads it, through the same rule

`bee sessions policy set|get|clear`
(`crates/buzz-cli/src/commands/sessions/policy.rs`) is the writer and the
reader, and it applies **the rule in §4.1 and no other**: the founder, or a seat
holding an operator grant the relay had accepted at the relevant time.

- `set` and `clear` refuse before signing when this key does not hold it
  (`policy.rs` `require_policy_standing`), evaluated at *now* through
  `buzz_core::coding_session_policy::signer_may_steer_at` — the same function
  the provider's fold calls.
- `get` **folds authority** rather than printing whatever the relay returned
  (`policy.rs` `fold_policies` →
  `buzz_core::coding_session_policy::fold_coding_session_policies`). It prints
  the newest record with standing, `null` when there is none, and lists every
  refused record under `excluded` with its author and a reason.

A `lead` role slug is not enough anywhere, and is not an input to the rule: a
provider can prove an operator grant from the accepted NIP-CSAT chain and cannot
prove a role slug it did not mint.

Both halves of that were wrong in the first cut, and the two defects were the
same defect. `set` signed anyway for a lead without a grant and disclosed
`willNotBind` — writing a permanent record onto a public relay that no consumer
would act on, with the warning living only in one terminal. `get` folded no
authority at all, so a stranger who published `budget.turns: 9999` into the
channel had it printed back, with an author and an event id, as "the newest
accepted policy", while the provider correctly ignored it (REVIEW-B2 F1, F2).

### 4.4 Still absent from v1

- **No UI.** The launch form is later work.
- **No mid-session binding.** §4.1's last bullet: a policy published while a
  seat is running does not bind until that umbrella's next create or resume.

### 4.5 Reading a policy requires the app bundle and the provider to move together

Carrying the policy into the context package bumped
`CODING_SESSION_CONTEXT_PACKAGE_VERSION` to **4**
(`crates/buzz-core/src/coding_session_context.rs`). The field itself is properly
additive — a package with no policy is byte-identical to what it was — but the
projector stamps the version **unconditionally**, and a reader compiled at v3
refuses anything outside its `MIN..=CURRENT` window. So a v3 reader rejects
*every* package from a v4 provider, not only the ones carrying a policy.

That matters here more than it would elsewhere: **seats run the app-bundled
`bee`/sidecar** (`desktop/src-tauri/tauri.conf.json`, ledger item 103 finding
1), which reaches them only on an app rebuild. A provider updated ahead of the
bundle breaks context reads for every seat — not the policy read, *every* read.
Batch 2 B already made the rebuild urgent, because a bundled `bee` predating it
reads a session containing a `note` or a `decision` as a failure of the whole
session; this is the second, independent reason. **Land the provider and rebuild
the app bundle in the same step.**

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
