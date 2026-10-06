# Session Policy — kind 44245, `buzz-coding-session-policy/v1`

Status: CONTRACT — frozen for batch 2 lane B1. Implemented in
`crates/beekeeper-core/src/coding_session_policy.rs`, built by
`crates/beekeeper-sdk/src/coding_session_policy.rs`, structurally validated at
`crates/beekeeper-relay/src/handlers/ingest.rs`. The first consumer landed in batch 2 lane B2: the session
provider reads the newest accepted record for an umbrella into its context
package and enforces exactly one field from it (see §4), and
`bee sessions policy set|get|clear`
(`crates/beekeeper-cli/src/commands/sessions/policy.rs`) writes and reads it. **No UI
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
`crates/beekeeper-core/src/kind.rs` carries the allocation note and a compile-time
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
| `gates.requiredGates` | ≤ 32 names, ≤ 64 B each, unique | **lead pack**, **relay push gate** | Named gates every lane must run — and, under NIP-GS arm (B), the gates that must be observed green before a seat may push |
| `gates.verifierRequired` | `bool` | **lead pack**, **44244 fold**, **relay push gate** | A verifier must rule before the mission may settle — and, on a `require-verdict` ref, before a seat may push |
| `bench.identities` | ≤ 64 lowercase 64-hex pubkeys, unique | **router** | Identities eligible for the bench |
| `bench.providers` | ≤ 16 provider **aliases**, ≤ 256 B each, unique | **router** | Provider instances eligible for the bench |
| `bench.challengerSampleRate` | finite `f64` in `0.0..=1.0` | **router** | Fraction of eligible jobs given to a challenger |
| `irreversible` | non-empty unique subset of `push`, `deploy`, `delete`, `external-message` | **fence** | Acts that need the founder's word |
| `stop.timeBoxSecs` | `u64` ≥ 1 | **lead pack** | Wall-clock seconds after which the lead stops opening work |
| `stop.onMilestone` | 1..=8 KiB, no control characters | **lead pack** | The milestone whose arrival ends the mission |

`bench.providers` names provider **aliases** (`claude-primary`), never instance
ids (`1958c6c448e05eed`). The two are different names for different things and
`crates/beekeeper-core/src/coding_session_identity.rs` now makes confusing them a
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

## 4. Two fields are enforced; everything else is a stated intention

Batch 2 lane B2 wrote the first consumer and batch 3 item G wrote the second,
so the older sentence here — "nothing in this repository refuses a turn because
of a budget in a 44245" — is now false, for `budget.turns` and
`gates.verifierRequired` and no others. This section says which, and repeats
the disclosure the rest of the record still owes its reader.

**Adding a third is a change to this section and to every surface that prints
the sentence in §4.2, in the same landing.** REVIEW-L7 F1 is what happens
otherwise: item G shipped the enforcement and left six places saying only
`budget.turns` counted, so a founder setting `--verifier-required true` was
told nothing counted it and then refused by `bee sessions complete`.
`crates/beekeeper-cli/tests/policy_enforcement_sentence.rs` now fails if they drift.

### 4.1 Enforced: `budget.turns`, at the provider's turn gate

`budget.turns`, when set, is the umbrella's turn ceiling. It **overrides**
`BUZZ_CSP_TURN_BUDGET` for that session rather than tightening it, and it binds
even where the host set no ceiling at all — the common case, since the
environment default is unlimited. A host ceiling and a session ceiling answer
different questions (*how much will this machine spend on anything* versus *how
much was this mission authorized to spend*), and the session's own signed
answer is the more specific one.

- Where: `crates/beekeeper-session-provider/src/commands.rs`
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

### 4.2 Enforced: `gates.verifierRequired` and `gates.requiredGates`

Since 2026-09-02 (batch 3, item G). When the umbrella's newest **accepted**
kind-44245 sets `gates.verifierRequired: true`, the 44244 fold excludes a
`mission.completed` as `CompletionNotVerified` unless every assignment it names
**that the fold settled** carries a verifier's ruling on the report that
settlement governs — a canonical `not-refuted` refutation by an active
`verifier` seat, or that report's own author holding one
(`crates/beekeeper-core/src/coding_session_completion_verification.rs`).
`bee sessions complete` re-folds the signed candidate and refuses to publish
one the fold would exclude.

The flag reaches the fold as one caller-supplied boolean, computed from
`fold_coding_session_policies` — the same policy fold `bee sessions policy get`
runs, so the record that binds is the record the CLI prints. A caller that has
not read the policy set passes `false`, which means **this fold enforces
nothing extra**, never *no verifier is required*; Desktop passes `false` today.

**What it also gates, since 2026-09-03: the push.** *(Lane L22.)* A
`require-verdict` ref's admission rule (NIP-GS appendix) now reads the
mission's newest **founder-signed** kind-44245 and switches on this flag. Unset
or `false`, a seat's push is admitted by **arm (B)** when every required gate
was *observed* green on that exact commit, over a clean worktree, by the
mission's own provider. `true`, arm (B) is off and the push needs **arm (C)**.
A founder's push is admitted under arm (A) either way, whatever this flag says.

**What arm (C) is, since 2026-09-03.** *(Lane L27, the follow-up ruling L22
§6.3 left open.)* Arm (C) is a verifier's `not-refuted` refutation **and** arm
(B)'s gate rows — the same required gates, observed green on the same commit,
over a clean worktree, by the mission's own provider. The flag therefore adds a
requirement and never substitutes one: setting it asks for **more** proof than
the gate-row route, not different proof.

For one day (2026-09-03, lane L22 to lane L27) it did substitute, and that was
backwards: a founder who set `verifierRequired: true` made their mission the
*weaker* of the two arms, since arm (B) demanded three green gates on the pushed
commit and arm (C) demanded none. A control that loosens the thing it is
labelled to tighten is the kind of defect this project ranks with a crash.

The relay reads the policy over one bounded page of founder-signed 44245s; a
mission whose policy falls outside it is judged as setting no flag, which can
only *open* arm (B) and never close it, and every arm-(B) refusal names the
gate list it actually used rather than implying the founder chose it.

For one day (2026-09-02 to 2026-09-03, lane L21) the push rule read no policy at
all and every non-founder push needed arm (C); the launch-form control was
labelled for the completion check alone because promising a landing rule the
switch did not have would have been a control lying about what it enforces.
That sentence is now true and the labels say so.

**And `gates.requiredGates` is enforced with it.** *(Lane L22; under both arms
since L27.)* Under arm (B) — and, since the follow-up ruling, under arm (C) too
— the gate list a founder writes is the list the relay counts, in the row's own
`gate` name. A policy that names none falls back to the relay default —
`cargo fmt`, `cargo clippy`, `cargo test` — which is disclosed in the refusal.
An empty list is treated as naming none, so a policy cannot accidentally admit a
push with no gate green at all.

**Where a founder sets it.** `bee sessions policy set --verifier-required`, and
since L21 the Desktop launch form's "Completing this mission" control
(`NewCodingSessionPolicyField.tsx`). It is three-way — set, cleared, not set —
because the wire field is a nullable boolean and a checkbox would publish
`false` for a founder who never touched it, which is a stated policy nobody
chose. Before L21 the launch form disclosed this field as enforced while
offering no way to set it (finding 39).

The one true sentence, byte-identical in `POLICY_ENFORCEMENT_DISCLOSURE` (CLI
and Tauri) and asserted by `crates/beekeeper-cli/tests/policy_enforcement_sentence.rs`:

> Enforced: budget.turns at the provider's turn gate, gates.verifierRequired at the fold's completion check and at the relay's verdict-gated push, and gates.requiredGates at that push. Every other field is read and shown, never counted.

### 4.3 Not enforced: every other field

`posture`, `budget.tokensPerSeat`, `budget.tokensPerSession`,
`budget.costUsdPerSession`, `budget.contextTier`, `attention`, `gates.redFirst`,
`gates.reviewEveryLane`,
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
(`crates/beekeeper-cli/src/commands/sessions/policy.rs`
`POLICY_ENFORCEMENT_DISCLOSURE`), and Desktop renders
`CODING_SESSION_POLICY_STATED_NOT_ENFORCED`
(`desktop/src/features/coding-sessions/lib/codingSessionPolicy.ts`), whose
companion `CODING_SESSION_POLICY_ENFORCED_FIELDS` holds `budget.turns` and
nothing else — the one list on that side that may claim a field is enforced.

### 4.4 The CLI writes it and reads it, through the same rule

`bee sessions policy set|get|clear`
(`crates/beekeeper-cli/src/commands/sessions/policy.rs`) is the writer and the
reader, and it applies **the rule in §4.1 and no other**: the founder, or a seat
holding an operator grant the relay had accepted at the relevant time.

- `set` and `clear` refuse before signing when this key does not hold it
  (`policy.rs` `require_policy_standing`), evaluated at *now* through
  `beekeeper_core::coding_session_policy::signer_may_steer_at` — the same function
  the provider's fold calls.
- `get` **folds authority** rather than printing whatever the relay returned
  (`policy.rs` `fold_policies` →
  `beekeeper_core::coding_session_policy::fold_coding_session_policies`). It prints
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

### 4.5 Still absent from v1

- **No UI.** The launch form is later work.
- **No mid-session binding.** §4.1's last bullet: a policy published while a
  seat is running does not bind until that umbrella's next create or resume.

### 4.6 Reading a policy requires the app bundle and the provider to move together

Carrying the policy into the context package bumped
`CODING_SESSION_CONTEXT_PACKAGE_VERSION` to **4**
(`crates/beekeeper-core/src/coding_session_context.rs`). The field itself is properly
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
`beekeeper-core` exposes the claim beside the signer —
`CodingSessionLifecycleCommandPayload::hire_requester_matches_signer`, three
answers (`Some(true)` attributed, `Some(false)` disputed, `None` unclaimed) —
and **no consumer calls it yet**.

So, exactly as with a policy that nothing enforces: until a consumer compares
them, `requestedBy` is an **unverified claim, not an attribution**, and any
surface that renders it (a Mission byline, a seat row, an audit table) must say
so rather than printing a lead's name as fact. **B2 owns closing this** at the
CLI and at the founder's host, and disclosing a mismatch rather than dropping
it. The same sentence is carried in
`crates/beekeeper-core/testdata/coding_session_hire_requester/vectors.json`, which
is the file B3's TypeScript decoder is pinned to.
