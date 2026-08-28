---
name: wire-sources-for-surfaces
description: "The table that makes a surface honest: for every fact a panel shows, the signed event it reads and the exact copy when that event has not arrived."
---

# Wire the sources for a surface

Fill this table **before** you write copy, and put it in the spec. A surface
whose facts are not traced to signed events is a mood board — it will be built,
and it will invent numbers.

**The rule:** *a panel that cannot name its row shows the unknown copy, never a
number.* Zero is a measurement. Absence is not zero.

## The table

| Fact the surface shows | Signed source | Copy when it has not arrived |
|---|---|---|
| **Liveness** — is this seat working right now | kind **44223** metadata `status` (`idle`, `running`, …), demoted by the ephemeral **lease** (`state: live` / `released`, `CodingSessionLeaseState`) and reachability | `live` / `quiet 3d` / `released` / **`no provider answering`** — never "idle" over a provider nobody is answering for |
| **Turn stages** — did what I asked actually run | kind **44224** lifecycle receipts, turn vocabulary: `turn_queued`, `turn_started`, `turn_degraded`, `turn_dropped`, `turn_refused`, `interrupt_delivered` | `no receipt yet` — silence about a turn is not success, and it is not failure either |
| **Dispositions** — what the team says it did | kind **44240** Pulse entries, `pu-type` ∈ `plan` \| `milestone` \| `note` \| `handoff` \| `blocker` | `no pulse yet`. A Pulse states intent, never an observed worktree fact — render it as a claim by its author, not as a verdict |
| **Authority** — who may steer or report | kind **44228** authority transitions (`grant-operator`, chained to the genesis) | `founder only` when no grant has landed; a seated identity with no grant reads **`seated, not yet granted`**, never "ready" |
| **Goal** — what this is for | kind **44227**, `d=sessionRef`, latest revision by `(created_at, id)` | `no goal set` |
| **Founder** — whose authority this rests on | kind **44226** genesis; the **signer** is the founder, and identity is the genesis **event id**, never the `csg-session` tag | never unknown — a surface with no genesis has no session to show |
| **Story** — the stream itself | kind **44225** transcript items, sequenced per (session, generation); gaps permitted | `no turn observed` — an execution with no transcript has not been quiet for zero seconds, it has said nothing at all |
| **Seats** — who is in this team | kind **44223** `agentRef` + `role` (non-null together); the **display name** comes from the identity's kind-0 profile | `unseated` when `agentRef` is null; when the profile has not been read, show the role and runtime, never a pubkey, never a stale name |
| **Hires** — who was asked for and who arrived | kind **44221** `session.hire` command + its 44224 receipt; refusals carry reasons (e.g. `HIRE_MODEL_NOT_OFFERED`, `HIRE_STALE`) | `hire not answered`; a relay with no `session.hire` refuses it as malformed and the surface says *that*, not "the provider declined" |
| **Changes / tests** — what actually landed | **no event kind today.** The builder's `write-report` fields (branch + HEAD sha, files touched, tests with counts and exit codes, red-before-green, deviations, residuals) arrive as prose in a turn | **`no report yet`** is the only honest value until a structured report kind exists. Never scrape a number out of prose and render it as a count |
| **Plan** — the accepted steps | **no artifact today.** The lead's brief carries `Acceptance: <commands with expected counts / exit codes>` as prose | `no plan published` — say the plan is not a signed artifact rather than showing a checklist the wire cannot back |

## How to use it

1. **One row per fact, not per widget.** If two panels show liveness, they
   read the same row and use the same copy. Two voices for one fact is a bug
   even when both are correct.
2. **Name the row in the spec, beside the element.** Every line of a
   `Surfaces` section points at a row here; an element with no row is either
   cut or explicitly marked as a fact the wire cannot supply.
3. **Write the unknown copy verbatim**, like every other string. It is the
   string a person sees most often on a bad day, and it is the one lanes are
   most likely to invent.
4. **Absence, unknown, and empty are three states.** "Nothing has arrived",
   "we read something we cannot vouch for", and "we read it and it is empty"
   read differently to a person. Specify all three or say which two collapse
   and why.
5. **Never mix planes.** A claim (Pulse, a report, a brief) is rendered as
   somebody's statement with an author. An observation (a receipt, metadata, a
   lease) is rendered as a fact. A surface that paints a claim like a fact is
   the beautiful lie this seat does not ship.

## When the wire cannot supply it

Write it down in the spec, in these words:

```
<fact>: no signed source today — <what would have to exist>.
Surface shows: "<unknown copy>". (needs Brian's sign-off)
```

That is a legitimate outcome and often the most valuable line in a spec: it
names the next event kind somebody has to add. What is never legitimate is a
panel that renders a plausible number nothing signed.
