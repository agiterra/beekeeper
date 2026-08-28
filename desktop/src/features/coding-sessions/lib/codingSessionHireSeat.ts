/**
 * Turning a granted hire into the seated create that answers it.
 *
 * The contract is that **the seat's create receipts are the hire's receipts**:
 * there is no second receipt surface for hiring, so everything the hire asked
 * for has to be carried by one 44221 create — the chosen identity as `actor`,
 * the hired role, the umbrella's `sessionRef` and `genesisRef`, the umbrella's
 * own title, and the brief as the seat's first turn.
 *
 * The brief becoming `initialTurn` is the point of the whole flow (D14): a
 * hired seat's first turn *is* the brief, so the lead never sends a second
 * "start" message and the seat never begins by asking what it is for.
 *
 * Pure. Every effect — staging custody, cutting the worktree, signing,
 * publishing — belongs to the hook that consumes this plan.
 */
import type { CodingSessionStatus } from "./codingSessionTypes";
import type {
  CodingSessionHireCandidate,
  CodingSessionHireLiveSeat,
} from "./codingSessionHirePolicy";
import { codingSessionWorktreeSlug } from "./codingSessionWorktreeName";

/**
 * What every hired seat's first turn opens with.
 *
 * Stated rather than implied: the seat is reading a brief written by another
 * agent, not by the person whose session this is, and a seat that mistakes the
 * two answers to the wrong authority.
 */
export const CODING_SESSION_HIRE_BRIEF_PREFIX = "[From the lead] ";

/**
 * Statuses that mean an execution is over.
 *
 * Deliberately narrow: `disconnected` and `interrupted` are executions that
 * can still be resumed and whose seat is still held, so counting them as free
 * would let one identity be seated twice in one umbrella.
 */
export const CODING_SESSION_HIRE_ENDED_STATUSES: readonly CodingSessionStatus[] =
  ["completed", "stopped", "failed"];

/** Everything the host needs to publish one hired seat's create. */
export type CodingSessionHireSeatPlan = {
  commandId: string;
  channelId: string;
  sessionRef: string;
  genesisRef: string;
  projectRef: string | null;
  actor: string;
  role: string;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  /** Inherited from the umbrella, so the seat lands under the same name. */
  title: string | null;
  /** The brief, prefixed. Never empty. */
  initialTurn: string;
  /** Display name for the seat, used in membership and failure copy. */
  seatLabel: string;
  /** `<session-slug>-<role>-<n>`; the host re-slugs and disambiguates it. */
  worktreeName: string;
};

/** Build the seated create a granted hire is answered with. */
export function buildCodingSessionHireSeatPlan(input: {
  commandId: string;
  channelId: string;
  sessionRef: string;
  genesisRef: string;
  projectRef: string | null;
  /** The umbrella's title, inherited by the seat. */
  title: string | null;
  brief: string;
  role: string;
  identity: CodingSessionHireCandidate;
  providerInstanceRef: string;
  providerAuthorityPubkey: string;
  model: string | null;
  /** Which seat of this role this is, 1-based; names the worktree. */
  seatOrdinal: number;
}): CodingSessionHireSeatPlan {
  const brief = input.brief.trim();
  return {
    commandId: input.commandId,
    channelId: input.channelId,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
    projectRef: input.projectRef,
    actor: input.identity.pubkey,
    role: input.role,
    providerInstanceRef: input.providerInstanceRef,
    providerAuthorityPubkey: input.providerAuthorityPubkey,
    model: input.model,
    title: input.title,
    // A brief that already opens with the prefix keeps one, not two: the lead
    // writing the sentence itself must not produce "[From the lead] [From the
    // lead] …".
    initialTurn: brief.startsWith(CODING_SESSION_HIRE_BRIEF_PREFIX)
      ? brief
      : `${CODING_SESSION_HIRE_BRIEF_PREFIX}${brief}`,
    seatLabel: input.identity.name,
    worktreeName: codingSessionHireWorktreeName({
      title: input.title ?? "",
      role: input.role,
      ordinal: input.seatOrdinal,
    }),
  };
}

/**
 * The tree a hired seat runs in: `<session-slug>-<role>-<n>`.
 *
 * A prefill, exactly like the founding and joining paths': the host re-slugs
 * it, caps it, and disambiguates a name already taken. The ordinal is what
 * keeps two builders of one session apart — item 80(a)/(b), where three seats
 * shared one checkout and their role packs materialized into one
 * `.agents/skills`.
 */
export function codingSessionHireWorktreeName(input: {
  title: string;
  role: string;
  ordinal: number;
}): string {
  return codingSessionWorktreeSlug(
    `${input.title} ${input.role} ${input.ordinal}`,
  );
}

/** Which seat of this role the next hire will be, 1-based. */
export function codingSessionHireSeatOrdinal(
  liveSeats: readonly CodingSessionHireLiveSeat[],
  role: string,
): number {
  return liveSeats.filter((seat) => seat.role === role).length + 1;
}

/** The minimal umbrella shape the live-seat read needs. */
export type CodingSessionHireUmbrellaLike = {
  executions: ReadonlyArray<{
    activeGeneration: {
      agentRef: string | null;
      role: string | null;
      status: CodingSessionStatus;
    };
  }>;
};

/**
 * The seats an umbrella is holding right now.
 *
 * Only seated executions count — an unseated execution is the person's own,
 * and it holds no agent identity a hire could collide with. This is what the
 * seat ceiling counts and what identity selection skips.
 */
export function listCodingSessionHireLiveSeats(
  umbrella: CodingSessionHireUmbrellaLike,
): CodingSessionHireLiveSeat[] {
  const seats: CodingSessionHireLiveSeat[] = [];
  for (const execution of umbrella.executions) {
    const { agentRef, role, status } = execution.activeGeneration;
    if (!agentRef || !role) continue;
    if (CODING_SESSION_HIRE_ENDED_STATUSES.includes(status)) continue;
    seats.push({ actor: agentRef, role });
  }
  return seats;
}

/** The umbrella's authority, as the host reads it before honouring a hire. */
export type CodingSessionHireAuthority = {
  /** Genesis signer, or null when the anchor has not resolved. */
  founderPubkey: string | null;
  /** Pubkeys holding a live `grant-operator` in this umbrella. */
  grantedOperators: readonly string[];
};

/**
 * May this signer ask for a seat?
 *
 * The same rule steering uses: the umbrella's founder, or somebody the founder
 * granted operator. The relay enforces it on ingest; the host enforces it
 * again because the host is the thing that mints an identity and starts a
 * process, and a host that trusted the relay alone would seat whatever reached
 * it.
 *
 * An umbrella whose founder has not resolved authorises nobody. Elsewhere an
 * unresolved anchor is the permissive fallback — nothing is gated — but here
 * the permissive reading would be "anyone may spend this computer".
 */
export function isCodingSessionHireAuthorized(
  requesterPubkey: string,
  authority: CodingSessionHireAuthority,
): boolean {
  const requester = requesterPubkey.trim().toLowerCase();
  if (requester.length === 0) return false;
  if (authority.founderPubkey === null) return false;
  if (authority.founderPubkey.trim().toLowerCase() === requester) return true;
  return authority.grantedOperators.some(
    (pubkey) => pubkey.trim().toLowerCase() === requester,
  );
}

/**
 * The hires this host has not answered yet, deduped by `commandId`.
 *
 * A hire is observed from history *and* live, and both stores replay on every
 * reconnect — so without this a reconnect would seat the same agent again, on
 * a second worktree, with a second process. Keyed by `commandId` because that
 * is what the create's receipt will be keyed by.
 */
export function selectUnansweredCodingSessionHires<
  T extends { commandId: string },
>(requests: readonly T[], answered: ReadonlySet<string>): T[] {
  const seen = new Set<string>();
  const pending: T[] = [];
  for (const request of requests) {
    if (answered.has(request.commandId) || seen.has(request.commandId)) {
      continue;
    }
    seen.add(request.commandId);
    pending.push(request);
  }
  return pending;
}
