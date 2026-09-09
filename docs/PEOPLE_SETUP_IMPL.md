# People and agent setup clarity — implementation contract

Brief: `BEEKEEPER-OPUS-PEOPLE-SETUP-BRIEF.md` (Desktop). Branch
`work/people-setup-fable`, base `be85f1714`. Presentation only: no authority,
no grants, no folds, no membership semantics change. Status belongs in
`docs/SESSION_STATE.md` (root's).

## 0. The fact that decides the vocabulary

Every "this key is an agent" signal in this app is **positive and verifiable**:
a NIP-OA owner attestation whose signature the native side checks before
setting `isAgent` (`nostr_convert.rs:64-84`), a `managed_agents` row on this
disk, a relay agent registration, a `"bot"` channel role, a receipt-backed
`grant-seat`, a provider authority signer on an execution.

There is **no positive signal that a key is a person.** `isAgent: false` means
only "this profile carried no attestation this client verified", and three
different populations collapse into it: people, agents whose owner never
attested, and keys with no profile at all.

Therefore, everywhere in this slice:

- The tab is **People**, never "Humans", and it means *no agent evidence we
  hold* — stated in the surface, not just in this document.
- A key with no profile is **unidentified**, never a person. It may appear
  under People (it carries no agent evidence) but it is marked as unidentified
  and never rendered as a confirmed human.
- A row's kind vocabulary is `owner | provider | agent | seated | unidentified`.
  `human` is not a value.

## 1. Lane A — recipient selection

File: `desktop/src/features/agents/ui/PersonaShareRecipients.tsx` (394 lines,
606 to spare) and its test; new pure helper module beside it if it earns one.

**Six callers exist and none may change behaviour**: the session People popover
(the only `allowAgents` caller), `PersonaShareDialog`, `AgentCardViewerDialog`,
`ProjectMembersManager`, `CreateProjectContainerDialog`, `ShellSessionScreen`.

- New optional prop `kind?: "all" | "people" | "agents"`. Its **default is
  derived from today's `allowAgents`** (`allowAgents ? "all" : "people"`), so
  every existing call site is byte-identical without edits. `allowAgents` is
  not removed and not redefined.
- `filterShareRecipientCandidates` stays the exported pure seam. It gains
  `kind` and an `isKnownAgent?: (pubkey: string) => boolean` predicate.
  Agent-ness becomes `isKnownAgent(pk) || user.isAgent === true` — strictly
  wider than today, which is what makes managed and relay-registered agents
  filterable even when their owner never attested. The predicate comes from
  `useKnownAgentPubkeys()` (context-published in `App.tsx`, zero new query
  observers), folded additively with the profile flag exactly as that module's
  own doc prescribes.
- **The segmented control renders only when the caller passes `kind`**, so the
  other five surfaces keep their markup. Session invites pass `kind` and open
  on **People**.
- **An agent-heavy page must never lie about emptiness.** When the People view
  has no rows but the query reports more pages, the surface says so in those
  terms — that no people were found *in the results loaded so far* — and offers
  one bounded "Load more results" (one page per press, never a loop) plus the
  existing search path. `"No people found."` is reserved for a view that has
  actually exhausted its results.
- Identical display names on different keys stay distinct: each row carries its
  short key, and the owner's label when the agent's owner is known. Dedup only
  on normalized identical keys.
- Preserved untouched: direct npub/hex entry and its synthetic row, Enter
  selection against a non-stale ranking, `Backspace` chip removal, scroll
  pagination, `excludedPubkeys`, archived-identity filtering, `limit`, and
  every `testIdPrefix`.
- The synthetic direct-entry row asserts `isAgent: false` today. It must be
  presented as **unidentified**, not as a person.

## 2. Lane B — roster presentation and setup orientation

File: `desktop/src/features/coding-sessions/ui/CodingSessionPeoplePopover.tsx`
(371 lines) and a new test; new small adjacent display components allowed; and
`desktop/src/features/coding-sessions/lib/codingSessionRoster.ts` for one
additive field (below).

**The defect Brian saw, and its cause.** A hire grants `grant-operator` to the
provider authority key itself (`useCodingSessionHire.ts:876-892`); the fold maps
that to `collaborator`; the provider publishes no kind:0, so the row has no
name and no agent badge. A provider therefore renders as a truncated hex string
labelled "Collaborator", indistinguishable from an unresolved human
collaborator.

Three facts already exist and are discarded; all three are read without a new
network call:

1. **Provider** — `execution.signerPubkey`, documented as "the fact-stream
   signer (provider authority) behind this execution". Derive the set inside
   the popover from a read already in the query cache. **Do not edit
   `CodingSessionWorkspace.tsx`** — it is root's file this week and sits two
   lines under the ceiling. If deriving in-popover is not clean, stop and
   report the exact one-line call-site change rather than making it.
2. **Seat** — `foldCodingSessionRoster` already computes
   `activeSeats: Map<pubkey, roleSlug>` and `codingSessionRosterEntries` throws
   it away. Add an optional `seatRole: string | null` to the entry type and
   populate it. Purely additive; the hook's only consumer is this popover.
3. **Agent** — replace the bare `profile?.isAgent` with the same additive merge
   Lane A uses (`useKnownAgentPubkeys()` folded with the profile flag).

Presentation rules:

- Each row states its kind in words from the vocabulary in §0, and capability
  in ordinary language — today the capability sentences exist only inside the
  invite role menu, so an existing row never explains what it may do.
- A provider row names what it is and, when reachable from existing facts, its
  runtime or instance label; it is rendered with the shared `PubKey` component
  rather than a bare truncation.
- Unknown keys stay explicitly unidentified. Ownership is never inferred from a
  display name.
- Badges are gated on the profile batch having settled, so a kind does not pop
  in late and read as a flicker. Existing loading, error, empty and per-row
  pending states are preserved, as is founder-only management.
- Nothing implies project membership changed; a session roster is not a project
  roster.

**Windows setup orientation goes in the report, not the product.** The trace
established that runtime installed-and-signed-in, provider provisioned, provider
running, execution created, and seat granted are five different facts surfaced
on five different screens, and that no document ties them together. Lane B
delivers a read-only walkthrough — entry point, what it does, what its success
proves, what it does not prove — as report prose. Any product UI for it needs a
separate file claim first.

## 3. Lane V — behaviour and browser evidence

Own spec and helpers; no edit to `e2eBridge.ts` (14,934 lines, grandfathered and
frozen) — new fixture logic goes in a sibling module, and any bridge handler
that must change is named to me first.

Adversarial cases, each a behaviour assertion: many agents ahead of one person
on a later page (no false empty, no unbounded autofetch, the bounded load path
works); identical names on different keys stay distinguishable; a key with no
profile is offered as unidentified rather than as a person; direct key entry;
self and already-granted exclusion; an agent whose owner is unknown; keyboard
selection and 250% zoom; a narrow Windows-like viewport. Selection and the
issued grant's recipient bytes are asserted through the mock bridge — no live
grants. Navigating the UI mutates nothing: assert the observed command set
against an allowlist the spec prints. Screenshots scoped and hash-distinct.
Port 4176 and our own dist; 4177 and 4178 are root's.

## 4. Gates and limits

Focused tests, `pnpm tsc --noEmit`, `pnpm check`, the file-size ratchet, the
browser spec on 4176, and the full desktop suite once on the final tree — root's
own smoke has 165 unrelated failures and this slice claims none of them. Mock
coverage is not native Windows proof. Filtering is presentation: every key an
agent could reach before, it can reach now.
