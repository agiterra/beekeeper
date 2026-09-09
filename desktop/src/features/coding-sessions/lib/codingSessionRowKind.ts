import {
  ENTITY_ROLE_DESCRIPTIONS,
  type EntityRole,
} from "@/shared/lib/entityRoles";

/**
 * What a coding-session roster row can honestly be said to be.
 *
 * Every "this key is an agent" signal this app holds is **positive and
 * verifiable**: a NIP-OA owner attestation whose signature the native side
 * checks, a `managed_agents` row on this disk, a relay agent registration, a
 * receipt-backed `grant-seat`, a provider authority signer on an execution.
 *
 * There is **no positive signal that a key is a person**. `isAgent: false`
 * means only "this profile carried no attestation this client verified", and
 * three populations collapse into it: people, agents whose owner never
 * attested, and keys with no profile at all.
 *
 * So `human` is deliberately not a value here. A row states the evidence that
 * reaches it, and `unidentified` is the honest answer for everything else —
 * including a key whose kind:0 resolved to a friendly display name, because a
 * display name is evidence of nothing. Ownership and species are never
 * inferred from a name.
 */
export type CodingSessionRowKind =
  | "owner"
  | "provider"
  | "seated"
  | "agent"
  | "unidentified";

export type CodingSessionRowKindInput = {
  /** This session's founder — the key its whole authority chain is rooted in. */
  isFounder: boolean;
  /**
   * A provider authority key: the fact-stream signer behind an execution in
   * this channel (`CodingSessionExecution.signerPubkey`).
   */
  isProvider: boolean;
  /**
   * Receipt-backed `grant-seat` role slug for this key in this session, else
   * null. From `foldCodingSessionRoster`'s `activeSeats`.
   */
  seatRole: string | null;
  /**
   * Managed ∪ relay-registered agents (`useKnownAgentPubkeys`) folded with a
   * verified profile `isAgent` — additive, never narrower than either source.
   */
  isAgent: boolean;
};

/**
 * Resolve one row's kind. Precedence, and why each step outranks the next:
 *
 * 1. `owner` — the founder. Sessions have exactly one, the chain is rooted in
 *    it, and no later grant can restate it.
 * 2. `provider` — a provider authority key that *also* holds a seat is still
 *    first a provider. The seat says which role a runtime stands in; the
 *    provider fact says whose computer signs the stream. Calling it "seated"
 *    would hide the machine, which is the exact defect this vocabulary exists
 *    to fix: a hire grants `grant-operator` to the provider authority key, the
 *    fold maps that to `collaborator`, and a provider then rendered as a bare
 *    hex string labelled "Collaborator".
 * 3. `seated` — a receipt-backed `grant-seat` is itself one of the positive
 *    agent signals, *and* it names the role. It is strictly more specific than
 *    `agent`, so it outranks it rather than being flattened into it.
 * 4. `agent` — positive agent evidence with no seat in this session.
 * 5. `unidentified` — no evidence. Not "a person": see the type's doc.
 */
export function codingSessionRowKind(
  input: CodingSessionRowKindInput,
): CodingSessionRowKind {
  if (input.isFounder) return "owner";
  if (input.isProvider) return "provider";
  if (input.seatRole !== null && input.seatRole.trim().length > 0) {
    return "seated";
  }
  if (input.isAgent) return "agent";
  return "unidentified";
}

/**
 * True when the kind cannot change once the profile batch resolves.
 *
 * Founder, provider and seat evidence all come from the authority chain and
 * the session catalog, which are already settled when the row paints. Only
 * `agent` and `unidentified` are still waiting on a profile — so only those
 * two need holding back, and the provider fix is visible immediately instead
 * of behind an unrelated query.
 */
export function codingSessionRowKindIsFinal(
  kind: CodingSessionRowKind,
): boolean {
  return kind === "owner" || kind === "provider" || kind === "seated";
}

/**
 * Whether kind badges may paint yet.
 *
 * `useUsersBatchQuery` serves persisted display labels as `placeholderData`
 * before the relay answers, so `isSuccess` and `data` are both true too early
 * and `isAgent` is simply absent from those labels. `dataUpdatedAt` stays 0
 * until a real result lands — the same signal the hook's own profile-seeding
 * effect gates on — so a badge derived from it cannot pop in late and read as
 * a flicker. A failed batch settles too: waiting forever would hide the
 * chain-derived kinds as well.
 */
export function codingSessionRosterBadgesSettled(input: {
  memberCount: number;
  profilesUpdatedAt: number;
  profilesFailed: boolean;
}): boolean {
  if (input.memberCount === 0) return true;
  if (input.profilesFailed) return true;
  return input.profilesUpdatedAt > 0;
}

/** Badge word for a kind. A seat names its role slug; nothing else does. */
export function codingSessionRowKindBadge(input: {
  kind: CodingSessionRowKind;
  seatRole: string | null;
}): string {
  switch (input.kind) {
    case "owner":
      return "Owner";
    case "provider":
      return "Provider";
    case "seated":
      return `Seat · ${input.seatRole?.trim() || "unnamed"}`;
    case "agent":
      return "Agent";
    default:
      return "Unidentified";
  }
}

/**
 * One provider-authority fact as the trusted ingress store publishes it.
 * Structurally typed so a unit test need not build a whole 44223 payload.
 */
export type CodingSessionProviderMetadataFact = {
  channelId: string;
  signerPubkey: string;
  metadata: { provider: string | null; runtime: string | null };
};

/**
 * The provider authority keys that signed session facts in one channel, each
 * with the runtime or instance label those same facts reached — or `null` when
 * they reached none. A key present with a `null` label is still a provider;
 * absence from the map is the only "not known to be a provider" answer.
 *
 * `runtime ?? provider` matches how the execution rail already labels a
 * record (`CodingSessionExecutionRail.tsx:467`). Entries arrive newest-first,
 * so the first label a key reaches wins and a later blank never erases it.
 */
export function deriveCodingSessionProviderRuntimeLabels(
  channelId: string,
  entries: readonly CodingSessionProviderMetadataFact[],
): ReadonlyMap<string, string | null> {
  const labels = new Map<string, string | null>();
  for (const entry of entries) {
    if (entry.channelId !== channelId) continue;
    const pubkey = entry.signerPubkey.trim().toLowerCase();
    if (pubkey.length === 0) continue;
    const label =
      entry.metadata.runtime?.trim() || entry.metadata.provider?.trim() || null;
    const existing = labels.get(pubkey);
    if (existing === undefined || (existing === null && label !== null)) {
      labels.set(pubkey, label);
    }
  }
  return labels;
}

/**
 * What a row says about itself, in ordinary words.
 *
 * `capability` reuses `ENTITY_ROLE_DESCRIPTIONS` — the sentences that until
 * now existed only inside the invite role menu, so an existing row never
 * explained what it may do. `evidence` says why the row carries the kind it
 * carries, and never states more than the evidence reaches.
 */
export function codingSessionRowSentence(input: {
  kind: CodingSessionRowKind | null;
  role: EntityRole;
  seatRole: string | null;
  providerLabel: string | null;
  hasProfile: boolean;
}): { capability: string; evidence: string | null } {
  const capability = ENTITY_ROLE_DESCRIPTIONS[input.role];
  if (input.kind === null) return { capability, evidence: null };
  switch (input.kind) {
    case "owner":
      return { capability, evidence: "Created this session" };
    case "provider":
      return {
        capability,
        evidence: input.providerLabel
          ? `Provider runtime (${input.providerLabel}) signing this session's facts — a computer, not a person`
          : "A provider runtime signing this session's facts — a computer, not a person",
      };
    case "seated":
      return {
        capability,
        evidence: `Holds the ${input.seatRole?.trim() || "unnamed"} seat in this session`,
      };
    case "agent":
      return { capability, evidence: "A known agent identity" };
    default:
      return {
        capability,
        evidence: input.hasProfile
          ? "No agent evidence held for this key"
          : "No profile and no agent evidence held for this key",
      };
  }
}
