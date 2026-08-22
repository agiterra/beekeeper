# Sessions: next-phase design brief

**Status:** input document for a design phase. Not a plan, not a decision record.
**Written:** 2026-08-15
**Worktree:** `integration/glue`

## How to read this document

Claims here carry explicit epistemic markers. They are not interchangeable, and
the distinction is the point of the document:

| Marker | Meaning |
| --- | --- |
| **[VERIFIED]** | Read in source or observed at runtime during this session. File named. |
| **[INTENT]** | Stated in a project document. Describes what is wanted, **not** what is built. |
| **[REQUIREMENT]** | Stated by Brian or Andy as something that must exist. A requirement, not a finding. |
| **[UNVERIFIED]** | Asserted by a person or a model, plausible, **not** confirmed against source. |
| **[OPEN]** | A genuine question. No settled answer exists. |

Anything unmarked is narrative connective tissue and should be treated as
unverified.

Where a **[VERIFIED]** item and an **[INTENT]** item appear to agree, that
agreement may be coincidental. The vision document describes a target state;
several things it describes are not implemented.

---

## 1. What is verified about the current implementation

### 1.1 Two signing keys, two categories of claim

**[VERIFIED]** (`crates/buzz-session-provider/src/lib.rs`, module doc) The
provider consumes operator intent and publishes provider facts across six kinds:

| Kind | Content | Signed by |
| --- | --- | --- |
| 44220 | Turn commands | the human operator |
| 44221 | `session.create` | the human operator |
| 44222 | Provider catalog | the provider |
| 44223 | Per-generation metadata | the provider |
| 44224 | Lifecycle receipts | the provider |
| 44225 | Transcript items | the provider |

**[VERIFIED]** (`crates/buzz-session-provider/src/agent_fence.rs`) The agent
process holds no Buzz key. The fence clears the `BUZZ_*` namespace before
spawning an adapter, while deliberately passing `PATH`, `HOME`, `SSH_AUTH_SOCK`,
language-runtime configuration, and adapter credential caches, because "a coding
agent runs the developer's real toolchain in a real checkout." The stated reason
for the fence: "Anything holding that key can mint transcript history the UI
renders as genuine provider fact."

**[VERIFIED]** The same file records that the fence was added to close an
existing hole: "until this module existed it held all of it, because an ACP
spawn inherits the parent environment wholesale."

### 1.2 Session identity is already provider-independent

**[VERIFIED]** `sessionRef` is a session-scoped UUID distinct from a provider's
own `sessionId`, and `generation` is a positive counter per provider attachment.

**[VERIFIED]** (observed in live provider state on this machine) Two executions
on different runtimes share one `sessionRef`:

```
sessionRef 39149758-b7b0-45bf-a879-f6c2da15c538
  ├── sessionId ab4bc8bf…  providerInstanceRef claude-primary  runtime claude
  └── sessionId dcec7538…  providerInstanceRef codex-primary   runtime codex
```

The multi-provider umbrella is not theoretical; it has run.

### 1.3 Discontinuity already has vocabulary

**[VERIFIED]** (`crates/buzz-core/src/coding_session_payload.rs`)
`ReceiptStatus` includes `Resumed`, `ResumedWithoutContext` ("a new generation
attached with fresh provider context"), and `Stopped`. Error codes include
`PROVIDER_UNAVAILABLE`, `SESSION_ALREADY_ATTACHED`, `CONTEXT_NOT_RECOVERED`,
`PROVIDER_AUTH_REQUIRED`, `SESSION_LIMIT`.

A provider disappearing is a named outcome, not an undefined state.

### 1.4 The host boundary holds in practice

**[VERIFIED]** The provider module doc claims "No host paths in signed content…
A test in this module asserts it." Checked empirically against live state on
this machine: old-Mac paths appear 3 times in local `state.json` (the `cwd`
field of three sessions) and **0 times** in `commands.jsonl` and `outbox.jsonl`.
Host paths stayed local and never reached signed content.

### 1.5 Keys, storage, and fail-closed behavior

**[VERIFIED]** (`desktop/src-tauri/src/session_provider/store.rs`) The provider
nsec lives in the OS keyring, and falls back to an inline `privateKeyNsec` field
in the `0o600` record file "only on builds without a keyring backend (or during
a keyring outage)."

**[VERIFIED]** `hydrate_keys` refuses to mint on a keyring failure; the comment
reads "Silently minting a replacement key would strand every event already
signed by the real one." Observed at runtime on this machine: with the keyring
feature compiled out, the provider logged "has no private key available… Refusing
to start without an identity" and did not provision a replacement.

**[VERIFIED]** A provider record is keyed by relay URL and carries a NIP-OA
`authTag` minted at provisioning time by the owner's key.

### 1.6 The managed-agent tier already exists and is formally specified

**[VERIFIED]** (`docs/remote-agents.md`) A specification for delegating a
*managed agent* to "any compute environment other than the local machine" via a
zero-registration plugin contract: any executable named `buzz-backend-<id>`
offering `info` and `deploy`. `buzz-backend-kubernetes` is the first conforming
provider. Surfaced in the UI as "Run on" (`runOn: "local" | string`).

It states five invariants: **identity fail-closed**, **no secrets in
configuration**, **presence-is-status**, **at-most-one-live-instance**, and
**intentional-termination-is-final**.

**[VERIFIED]** The same document decouples agents from the desktop entirely:
"the desktop is one launcher among many. What makes a process a live Buzz agent
is a keypair, a NIP-OA auth tag, and a relay URL… anything that can set that
environment and exec the harness — a bash script, a systemd unit, a CI job — is
a conforming launcher."

**[VERIFIED]** Coding-session executions are explicitly excluded from this tier.
`SessionMetadata` carries `agent_ref: Option<String>` documented as "Managed-agent
reference. Always `null`: this provider is not one." The field is structurally
required and always null.

### 1.7 Relevant protocol work that already exists

**[VERIFIED]** **NIP-AB** (`crates/buzz-core/src/pairing/NIP-AB.md`, with a
Tamarin model `NIP-AB.spthy`) — QR + SAS device pairing for moving an identity
between devices. Desktop implements a recovery direction: "The fresh desktop
shows the QR and receives the [identity]."

**[VERIFIED]** **NIP-OA** (`docs/nips/NIP-OA.md`) — owner attestation.
`["auth", "<owner-pubkey-hex>", "<conditions>", "<sig-hex>"]`. Explicitly not
impersonation: "An event that includes a valid `auth` tag remains authored by
`event.pubkey`." The `<conditions>` field holds "zero or more clauses separated
by `&`", and a valid tag is described as "a reusable capability."

**[VERIFIED]** **NIP-AE** — agent memory, surfaced as `bee mem` with
`ls/get/hash/set/patch/rm`, tombstones, and `--base-hash` optimistic concurrency
on `patch`. Relay-persisted, not host-local.

**[VERIFIED]** **Mesh compute** (`desktop/src/features/mesh-compute/`) — a
settings toggle described to the user as "Share this machine with members of
this relay so they can run agents here." Backed by MeshLLM; forms a
"Mixture-of-Agents committee when two or more workers are reachable." Currently
resolves to a loopback transport (`http://127.0.0.1:9337/v1`), and deploying a
mesh-provider agent to a remote backend is refused by design.

**[VERIFIED]** `buzz-relay-mesh` is an unrelated inter-relay QUIC mesh for relay
pods. Name collision only.

### 1.8 Command addressing

**[VERIFIED]** (`crates/buzz-session-provider/src/commands.rs`) A command
carries `provider_authority_pubkey`, and the provider drops commands whose value
does not match its own pubkey. This is **addressing** — which provider should
act — and is not by itself a statement about who is permitted to command.

---

## 2. Documented intent (wanted, not necessarily built)

All from `docs/SESSION_VISION.md`.

**[INTENT]** "Provider selection is an execution choice. Session identity is not."

**[INTENT]** "A session is project knowledge, not private state trapped on the
computer of the person who started it."

**[INTENT]** Members should be able to see "why the session exists; who and what
is participating; what each participant is doing; tool activity and results;
decisions and changes of direction; produced artifacts; blockers and requests for
human judgment; the lineage of executions that have contributed."

**[INTENT]** "Authority should remain explicit. Observing, contributing context,
steering agents, approving actions, changing membership, and taking operational
control are different powers. A shared surface does not imply that every viewer
may run commands against somebody else's checkout."

**[INTENT]** "a provider becoming unavailable must leave an intelligible record";
"future executions should be able to continue from the durable state of the
session, subject to access and capability limits."

**[INTENT]** "Exact live-process migration may be a later capability. The product
model must not make it a prerequisite for shared observation or
provider-independent continuity."

**[INTENT]** (`docs/SESSION_HANDOFF_SOL.md`) Economic model: "An execution runs
on the machine and provider login of whoever launched it. A teammate
'continuing' a session means a new execution on *their* subscription. No shared
keys, no metering to build."

---

## 3. Unverified claims in circulation

These are load-bearing in current discussion but were **not** confirmed against
source. Each should be checked before being built on.

**[UNVERIFIED]** That no signer/authorization check exists on turn commands —
i.e. that the provider will act on a well-formed command from any channel member.
A search did not find such a check, but absence of a search result is not
absence of a check. Relay-side enforcement was not examined at all.

**[UNVERIFIED]** That session authority currently derives from "the human who
signed the earliest accepted create," and that this is enforced only as
client-side preflight in React.

**[UNVERIFIED]** That NIP-AE memories are encrypted for the owner.

**[UNVERIFIED]** That portable agent snapshots exclude the nsec, and that
importing a snapshot mints a new agent identity.

**[UNVERIFIED]** That publishing host observability (open ports, running
processes) would violate the no-host-paths boundary. The verified boundary
concerns paths; extending it to ports is an inference, not a finding.

**[UNVERIFIED]** That the workspace referenced by the three orphaned executions
(`/Users/briansweet/agiterra/Hallway`) is re-clonable from the relay's git
service.

---

## 4. Where we want to go

### 4.1 Must-haves, as written by Brian and Andy

**[REQUIREMENT]** Reproduced verbatim. These are requirements, not analysis, and
they are deliberately not restated — an earlier paraphrase introduced framing
(it called one item "the sleeper") that would bias a fresh reading.

- A permanent summary of the goal at the top, i.e. "Add X feature to Buzz" — I
  switch between many sessions and often have to go back through my input
  history to remember what this session was actually doing
- Pills or a list of sub-agents, running processes and open ports
- Yeah. By default, it's just me and you can see what I'm doing. But I can
  invite Brian and you can also add inputs (while I am connected)
- OOH and also a "fork this session" button where you can clone the session to
  your own machine (or just make a copy if it's already your own session).
  [i.e. I don't want to mess up what you're doing, but you're offline and I have
  an idea.]
- Shared visibility for channel members, invite channel members to collaborate

### 4.2 Direction beyond the must-haves

Persistent Buzz agents joining a session, carrying durable identity and memory
across host changes; teammates participating as active operators rather than
observers; and a session that survives the loss of any particular computer.

---

## 5. Open design questions

**[OPEN]** Where does a durable session goal live? The existing `title` field is
on kind 44223, documented as "Immutable facts about one session **generation**"
and authored by the provider — three properties (per-generation lifetime,
immutability, provider authorship) that may or may not suit a goal. What should
its scope, mutability, and signer be?

**[OPEN]** What is the authority model? What roles exist, how are grants
expressed, where are they enforced, and how are they revoked? NIP-OA's
`<conditions>` field exists and is described as a reusable capability; whether it
is the right carrier is unexamined.

**[OPEN]** Is there a single administrator, or succession/multiple? What happens
to a long-lived session when one human identity is unavailable?

**[OPEN]** Should a coding-session execution become a managed agent — filling in
the reserved `agentRef` — and if so what changes about the fence, about what an
agent may sign, and about the trust consumers place in kinds 44222–44225?

**[OPEN]** How is host observability (sub-agents, processes, ports) carried?
Durable signed events, ephemeral signals, capability-gated, or not carried across
the wire at all? Who may see another member's host topology?

**[OPEN]** What does a fork carry? Conversation only, or a code coordinate too?
`SessionMetadata` sets `branch` to null and is documented as not claiming to know
the checkout's branch. What is the lineage relationship between parent and fork,
and how is it represented?

**[OPEN]** What happens when two operators steer simultaneously? The vision
lists this as intentionally open. Provider code notes ACP permits one in-flight
prompt per process and that "an operator who cannot see a queue cannot reason"
about it.

**[OPEN]** When a teammate joins a session, what of an agent's memory becomes
visible? Is there a distinction between session-shared context and owner-private
memory, and where is it enforced?

**[OPEN]** What is the lifecycle of a provider or agent identity — backup,
transfer between hosts, rotation after loss, and fencing against two live copies
of the same key? `at-most-one-live-instance` exists as an invariant for managed
agents; whether and how it should extend to coding-session providers is open.

**[OPEN]** What is the relationship between the three existing compute
mechanisms (mesh compute, backend providers, local execution) and where sessions
should run?

---

## 6. Known concrete state on Brian's machine

Useful as a test case; not a requirement.

**[VERIFIED]** Three coding-session executions exist in restored provider state,
all `generation 1`, all with `cwd = /Users/briansweet/agiterra/Hallway` — a path
that does not exist on the current machine. The provider identity and its
durable state (command watermarks, transcript sequence state in a 34KB
`outbox.jsonl`) survived a machine migration; the workspace those executions ran
in did not.

This is a recovery case worth naming: **key and provider state survive, the
workspace does not.**

**[UNVERIFIED]** That this is the common real-world case, on the reasoning that
people back up application state and git remotes rather than working trees. That
is an untested inference from a single migration, and it should not carry weight
in prioritization without evidence.

---

## Appendix A: positions already taken (non-binding)

Included for transparency, not as constraints. These were formed during
discussion, partly before the verification above, and at least one has already
been reversed once. **Delete this appendix if a cold read is wanted.**

- That fork may be the cheapest collaborative feature, because it requires no
  authority model — a fork runs on the forker's own machine — and because the
  documented economic model already makes continuation a new execution on the
  continuer's own login.
- That the existing `title` field is the wrong home for a permanent session goal.
- That the managed-agent tier in `docs/remote-agents.md` already contains much of
  what a persistent-agent design would need, including a fencing invariant.
- That preserving a provider pubkey suits migrating a retired host, while a
  distinct pubkey per concurrently active host is simpler — a distinction
  proposed by a model, not tested.

Each of these may be wrong.
