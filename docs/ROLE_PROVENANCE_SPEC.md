# Report provenance on the Packs tab — founder-commissioned slice (Fable)

Worktree `/Users/brian/Projects/beekeeper/review-role-provenance-fable`, branch
`work/role-provenance-fable`, base `4714bb675`. Continues
`docs/ROLE_ADOPTION_EVIDENCE.md`; audit and Astra's rulings are in
`/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/COMMISSIONING_AUDIT_2026-09-07.md`.

## What this proves, in one sentence
A reported pack revision row can say **"Reported by a commissioned provider"** when the
44223 that reported it is signed by the provider an exact-generation, founder-signed
lifecycle chain commissioned — and otherwise says exactly why not. It never says the pack
bytes ran.

## Constraints (Astra, accepted)
1. Exact generation only: a row's proof is the accepted 44221+44224 pair for **its own**
   generation (`sessionCoordinationFold.ts` `acceptedGenerations`); nothing is inherited
   from another generation. A resumed generation is proven only if every ancestor command
   back to the generation-1 create is itself founder-signed and accepted; otherwise the row
   is `proof-unavailable`.
2. Label is "Reported by a commissioned provider", never "verified execution/adoption".
3. Missing operator projection (a command signed by a non-founder key) is
   `proof-unavailable · operator authority not projected`, not disputed. `disputed` is only
   for contradiction: 44223 signer ≠ the accepted provider for that exact generation; fold
   ambiguity naming that target; create `sessionRef` ≠ genesis `sessionRef` or ≠ the 44223's
   echoed `sessionRef`; genesis `h` ≠ command channel.
4. Founder = signer of the 44226 resolved **by exact event id** from the create's
   `genesisRef`, classified through `classifyCodingSessionGenesisEvent` (signature +
   envelope). No fallback from execution metadata or umbrella projections.
5. Signatures and ids are verified at the ingestion boundary (`hasValidSignature`, the
   existing classifiers); a display cache confers no authority; nothing module-level;
   dispositions are keyed by `channel|targetKey|metadataEventId|signer` so an impostor row
   cannot overwrite a legitimate one; `sourceErrors` are readable product sentences.

## Type contract (lane U codes against these names; lane P exports exactly these)

```ts
// desktop/src/features/roles/lib/rolePackProvenance.ts
export type RolePackProvenanceState = "commissioned" | "proof-unavailable" | "disputed";
export type RolePackProvenanceDisposition = {
  state: RolePackProvenanceState;
  /** Product sentence, null only for `commissioned`. */
  reason: string | null;
  founderPubkey: string | null;      // genesis signer when bound
  commandSignerPubkey: string | null; // signer of this generation's own 44221
  commandEventId: string | null;
  receiptEventId: string | null;
  genesisEventId: string | null;
};
export type RolePackProvenanceRow = {
  channelId: string;
  /** `buildCodingSessionTargetKey(session.commandTarget)`; null when the record has no target. */
  targetKey: string | null;
  metadataEventId: string | null;    // session.statusEventId
  signerPubkey: string | null;       // session.metadataAuthorityPubkey
  sessionRef: string | null;         // session.sessionRef (echoed)
};
export function rolePackProvenanceKey(row: RolePackProvenanceRow): string; // channel|targetKey|metadataEventId|signer (nulls as "")
export type RolePackProvenanceInput = {
  events: readonly RelayEvent[];          // ALREADY signature-verified by the query module
  channelIds: readonly string[];
  now: number;                            // unix seconds
  sourceErrors: readonly { scope: string; message: string }[];
  rows: readonly RolePackProvenanceRow[];
};
export type RolePackProvenanceResult = {
  dispositions: ReadonlyMap<string, RolePackProvenanceDisposition>;
  /** Readable sentences: fold source errors + fold ambiguities, byte-ordered, deduped. */
  notes: string[];
};
export function buildRolePackProvenance(input: RolePackProvenanceInput): RolePackProvenanceResult;

// desktop/src/features/roles/lib/rolePackProvenanceQuery.ts
export const ROLE_PACK_PROVENANCE_QUERY_LIMIT = 1000;
export type RolePackProvenanceEvents = { events: RelayEvent[]; sourceErrors: { scope: string; message: string }[] };
export function rolePackProvenanceQueryKey(projectRef: string | null, channelIds: readonly string[], metadataEventIds: readonly string[]): readonly unknown[];
export async function fetchRolePackProvenanceEvents(channelIds: readonly string[], deps?: { fetchEvents?: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]> }): Promise<RolePackProvenanceEvents>;

// desktop/src/features/roles/lib/useRolePackProvenance.ts
export function useRolePackProvenance(input: { projectRef: string | null; channelIds: readonly string[]; rows: readonly RolePackProvenanceRow[] }): {
  result: RolePackProvenanceResult | null; // null before the first answer
  isLoading: boolean;
  error: string | null;                    // readable
  refetch: () => void;
};
```

## Lane P — model + query + hook (owner: lane P, exclusive)
Files: `desktop/src/features/roles/lib/rolePackProvenance.ts`, `rolePackProvenance.test.mjs`,
`rolePackProvenanceQuery.ts`, `rolePackProvenanceQuery.test.mjs`, `useRolePackProvenance.ts`.

`buildRolePackProvenance`:
- Strip `sig` and call `foldSessionCoordination({ now, events, sourceErrors, commissioners: undefined })` from `@/shared/coordination/sessionCoordinationFold` (note: that module uses relative `.ts` specifiers internally; import it the way `desktop/src/features/project-pulse/lib/pulseQueries.ts` does).
- Index events by id. For each fold session/generation, derive its channel from the 44221 at `lifecycleCommandEventId` (`h` tag) and key `channel|targetKey`.
- Chain walk per generation: command event → `classifyCodingSessionCreateEvent(event, channels)` for creates (gives `genesisRef`, `sessionRef`, `signerPubkey`, `providerAuthorityPubkey`); for resumes read `action.session` (previous target) from the raw event and find the accepted predecessor `channel|targetKey(previous)`; recurse to generation 1. Collect every command signer on the chain.
- Genesis: `classifyCodingSessionGenesisEvent(eventById.get(genesisRef), channels)`; bind `channelId === command channel` and `sessionRef === create.sessionRef === fold session.sessionRef === row.sessionRef` (a null row sessionRef with a non-null bound one is `proof-unavailable`, "the report names no session"; a different non-null one is `disputed`).
- Dispositions, in this order: no accepted generation for the row's key → `disputed` if a fold ambiguity mentions that target's sessionId+generation, else `proof-unavailable` ("no accepted lifecycle proof for this generation" + the readable source errors when any); row signer ≠ generation provider → `disputed`; chain unbound (no genesisRef / genesis absent or unreadable / an ancestor generation not accepted) → `proof-unavailable` with the specific sentence; contradiction per constraint 3 → `disputed`; any chain command signer ≠ founder → `proof-unavailable · operator authority not projected`; else `commissioned`.
- Pure, deterministic, no Date/Math.random, no module state. Reasons are product sentences ("This generation's create names no genesis.", "The genesis this create cites could not be read.", "The command that started generation 2 was signed by a key that is not the founder; operator grants are not projected yet.").

`fetchRolePackProvenanceEvents`: mirror `pulseQueries.ts` `fetchProjectPulseDigest`: chunk channels ≤ `PULSE_CHANNELS_PER_QUERY` (import the constant or copy with attribution), **one read per kind** for 44221, 44224, 44223, 44226 with `#h` and `limit: ROLE_PACK_PROVENANCE_QUERY_LIMIT`, readable truncation ("Lifecycle commands were truncated at 1000 events; older proof may be missing.") and failure sentences ("Lifecycle receipts could not be read: <message>."), then `hasValidSignature` admission with readable exclusions; strip nothing (keep `sig` for the classifiers, which verify again — that is the established boundary). Default `fetchEvents` = `relayClient.fetchEvents`.

`useRolePackProvenance`: `useQuery` keyed by `rolePackProvenanceQueryKey(projectRef, sortedChannelIds, sortedMetadataEventIds)`, `enabled` when channels and rows exist, `staleTime: 30_000`; `queryFn` fetches, then `buildRolePackProvenance` with `now` read after the fetch. No module-level state (React Query only, same as the packs and revisions queries).

Tests (node:test, `.test.mjs`, real `finalizeEvent`-signed fixtures with `nostr-tools` like `desktop/src/features/coding-sessions/lib/codingSessionCreateObservations.test.mjs` does — look at how that file builds signed 44221/44224/44226 and reuse its helpers by copy if they are not exported): the adversarial suite in the audit §4 verbatim — commissioned create (gen 1); founder resume gen 2 commissioned; 44223 for gen 3 with no resume pair → proof-unavailable (no inheritance); impostor X/Y create → proof-unavailable · operator authority not projected; Y signing 44223 for F's target → disputed; operator resume → proof-unavailable · operator authority not projected; genesis `h` mismatch → disputed; sessionRef mismatch → disputed; source error injected → every row proof-unavailable with that sentence; two rows for one target from different signers keep two distinct keys; invalid signature excluded with a readable note; truncation note.

## Lane U — surface (owner: lane U, exclusive)
Files: `desktop/src/features/roles/lib/rolePackSnapshots.ts` (+test), `useProjectPacksView.ts`,
`desktop/src/features/roles/ui/RolePackSnapshots.tsx` (+test), `rolesCopy.ts` (+test),
`ProjectPacksScreen.tsx`, `desktop/tests/e2e/role-packs-project.spec.ts`,
`desktop/src/testing/e2eBridge.ts` (only if the mock relay needs to answer `fetchEvents` for
seeded 44221/44224/44226 — inspect first; report what you found).

- `ReportedRolePackSnapshot.provenance` becomes `RolePackProvenanceState`, plus
  `provenanceReason: string | null`, `founderPubkey: string | null`. `buildRolePackSnapshots`
  takes `provenance: RolePackProvenanceResult | null` and looks each row up by
  `rolePackProvenanceKey({...})`; a missing disposition or a null result is
  `proof-unavailable` with "Provenance has not been checked yet." (loading) or the hook's
  readable error.
- `RolePackSnapshots` gains `provenanceNotes: string[]` (from `result.notes`) rendered once
  under the reported heading as "Proof reads: …" only when non-empty.
- Copy (final, simplified at Astra's review so the screen does not require the reader to
  know the lifecycle chain): `commissioned` → "Reported by the assigned provider";
  `proof-unavailable` → "Unverified · proof unavailable"; `disputed` → "Disputed"; each
  followed by ` · <reason>` when present. Section subtitle: "Versions found on this machine
  and pack revisions reported in this project's channels. Beekeeper checks who sent each
  report. Confirming its source does not prove which role instructions were used." The
  precise meaning (the provider a founder-signed lifecycle chain named for that exact
  generation) stays in code/doc comments and the expandable proof details, not the
  headline. Replace the "Unverified metadata" chip: use the provenance
  label instead, amber for proof-unavailable, red for disputed, plain for commissioned;
  keep the signer pubkey, `data-testid`s, and add `data-provenance`.
- `useProjectPacksView`: build `rows` from `executionCatalog.entries` filtered to this
  project (channelId, targetKey via `buildCodingSessionTargetKey`, statusEventId,
  metadataAuthorityPubkey, sessionRef), call `useRolePackProvenance`, pass result/error into
  the model, include its `refetch` in `refetchPacks`, expose `provenanceError`.
- E2E: extend the ranked-rows test: both existing rows must render "Unverified · proof
  unavailable" (no lifecycle proof seeded) with `data-provenance="proof-unavailable"`; then
  add ONE commissioned row if the mock relay can serve seeded 44226/44221/44224 to
  `fetchEvents` (inspect `e2eBridge.ts` and `__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__`); if it
  cannot, say so and stop at the unit-level proof — do not fake it. **Do not run Playwright
  in this round** (port 4173 is owned by another run); `pnpm build:e2e` only, plus typecheck,
  check, roles+shared node tests, file sizes.

## Finalizer (Fable)
Review against the constraints, run the gates once, run the E2E when port 4173 is released,
commit with signoff on the topic, checkpoint to Astra with logs. No production changes.

## Review round (Sol independent review via Astra) — accepted and applied

1. **Incomplete reads never yield a positive.** Every source error/truncation from the
   per-kind proof reads (44221, 44224, 44226) carries the `channelIds` of the chunk it
   covered; a row in such a channel that would otherwise be `commissioned` reads
   `proof-unavailable · Proof reads for this channel were incomplete…`. `disputed` stays
   disputed. Complete channels keep their positive. A 44223-only read error does not
   suppress (44223 is the report, not the proof).
2. **Cache is bound to the relay.** Query key
   `["role-pack-provenance", relayUrl, projectRef, channels, metadataEventIds]`; the hook
   takes `relayUrl` from the caller (`useCommunities().activeCommunity.relayUrl`) and is
   disabled without one; `resetCommunityState` removes the `["role-pack-provenance"]`
   prefix on a switch (one line in `desktop/src/features/communities/useCommunityInit.ts`).
3. **Late proof arrivals re-rank without polling.** The hook subscribes to the existing
   observed-events fan-out (`codingSessionObservedEvents.ts`, already fed by the
   create-observation and trusted-ingress live subscriptions the Packs tab mounts) and
   invalidates its own prefix when a 44221/44224/44226/44223 for its channel set arrives.
4. Copy simplified: "Reported by the assigned provider"; subtitle "Beekeeper checks who
   sent each report. Confirming its source does not prove which role instructions were
   used." Semantics and `data-provenance` values unchanged.
5. `hireRef` classifier repair lands as its own commit (Astra's ruling).
6. **A positive never outlives its evidence.** The hook reports no result while a
   proof read is in flight or failed (an invalidation from new lifecycle evidence, a
   manual refetch, or a stale re-ask), and the model ignores any disposition that
   arrived beside a provenance error; rows read "proof unavailable" with the read's own
   words until the replacement read confirms. Mounted test: commissioned → conflicting
   receipt arrives → positive withdrawn before the re-read answers → failing re-read
   leaves it unavailable with the failure sentences → a later successful read re-earns
   it. The reported-rows heading is "Reported revisions" (it no longer says
   "unverified" above a row that may be proven).

## Validation record (2026-09-08, early)

Logs: `/Users/brian/Projects/beekeeper/review-role-adoption-fable-logs/final3-*.log`
(lane logs `laneP-*`, `laneP2-*`, `laneU-*` alongside).

| Check | Result | Log |
| --- | --- | --- |
| desktop typecheck | exit 0 | `final5-typecheck.log` |
| desktop `pnpm check` + file sizes | exit 0 | `final5-desktop-check.log` |
| roles + create-observation node tests (final tree) | 157 passed, 0 failed | `final5-roles-tests.log` |
| roles + shared/api + create-observation + umbrella node tests (before the stale-positive fix) | 458 passed, 0 failed | `final3-focused-tests.log` |
| hireRef classifier + provenance model (together) | 42 passed | `final3-hireref-tests.log` |
| provenance model/query/hook incl. 3 mounted tests (lane P) | 37 passed | `laneP2-tests.log` |
| Packs E2E (`role-packs-project.spec.ts`, rebuilt bundle, final tree) | 3 passed; commissioned row proven from signed 44226/44221/44224 seeded through the mock relay's ordinary REQ path | `final5-e2e.log` |

Not run, per Astra's coordination: full desktop unit suite, full smoke, Rust/mobile
(pure TypeScript change; root ran the 1,264-case smoke on the prior slice).
Temporary debug instrumentation a lane added to shared coordination files during
root-cause work was reverted; `git status` shows no drift outside the claimed files
and `grep ZZDEBUG` is empty.

Ledger text for Astra: the Packs tab now labels each reported pack revision
"Reported by the assigned provider" only when its 44223 signer is the provider an
exact-generation, founder-signed lifecycle chain named (genesis by exact event id;
resumes proven per generation, never inherited); otherwise "Unverified · proof
unavailable" with the reason, or "Disputed" on contradictory signer/session/channel
evidence. Incomplete proof reads suppress positives per channel; the cache is bound to
the relay and cleared on community switch; late proof arrivals re-rank through the
existing observed-events fan-out. A separate commit lets the create classifier accept
`hireRef` (attribution only), which had made every founder-fulfilled hire unreadable
to create observations. Operator-granted commissioning (44228 projection) remains a
named follow-up; labels never claim pack execution.
