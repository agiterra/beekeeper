/**
 * One hire request, answered: authority, then policy, then the seat.
 *
 * This is the whole decision the founder's desktop makes when a lead asks for
 * a seat, as a pure function — so what the host will do is testable without a
 * relay, a keystore, or a provider, and so the hook that performs it holds no
 * rules of its own.
 *
 * The order is the point:
 *
 * 1. **Authority first, and silently.** A request from somebody who is neither
 *    the founder nor a granted operator is not refused with a code — it is
 *    ignored. The relay already refuses it on ingest; a host that answered it
 *    with a published refusal would be telling a stranger that this computer
 *    is listening, and would let anyone in the channel make this machine sign
 *    events on demand.
 * 2. **Policy next**, producing one of the five contract codes, which is
 *    published back to the requesting seat as a turn.
 * 3. **The seat last** — a create carrying the brief as its first turn.
 */
import {
  decideCodingSessionHire,
  formatCodingSessionHireRefusal,
  type CodingSessionHireCandidate,
  type CodingSessionHirePolicy,
  type CodingSessionHireRefusalCode,
} from "./codingSessionHirePolicy";
import {
  buildCodingSessionHireSeatPlan,
  codingSessionHireSeatOrdinal,
  isCodingSessionHireAuthorized,
  listCodingSessionHireLiveSeats,
  type CodingSessionHireAuthority,
  type CodingSessionHireSeatPlan,
  type CodingSessionHireUmbrellaLike,
} from "./codingSessionHireSeat";
import type { CodingSessionHireRequest } from "./codingSessionHireWire";

export type CodingSessionHireAnswer =
  | { kind: "ignored"; why: "unauthorized" | "unknown-umbrella" }
  | {
      kind: "refused";
      code: CodingSessionHireRefusalCode;
      reason: string;
      /** The exact 44220 text published back to the requesting seat. */
      text: string;
    }
  | { kind: "seat"; plan: CodingSessionHireSeatPlan };

export type CodingSessionHireAnswerInput = {
  request: CodingSessionHireRequest;
  /** The umbrella the hire names, or null when this host has never seen it. */
  umbrella:
    | (CodingSessionHireUmbrellaLike & {
        sessionRef: string | null;
        title: string;
        genesisRef: string | null;
        projectRef?: string | null;
      })
    | null;
  authority: CodingSessionHireAuthority;
  policy: CodingSessionHirePolicy;
  candidates: readonly CodingSessionHireCandidate[];
  availableProviderInstanceRefs: readonly string[];
  /** Pubkey of the provider that will answer the create this host publishes. */
  providerAuthorityPubkey: string;
  /** Fresh 44221 command id for the seat's create. */
  commandId: string;
};

/** Decide, without doing anything, what this host owes one hire request. */
export function planCodingSessionHireAnswer(
  input: CodingSessionHireAnswerInput,
): CodingSessionHireAnswer {
  const { request, umbrella } = input;
  // An umbrella this host has no facts about is not one it can seat into: it
  // cannot count the live seats the ceiling is about, and it cannot read the
  // title the seat inherits. Ignored rather than refused, because "I have not
  // observed it yet" is a statement about this host's loading, not about the
  // request.
  if (
    umbrella === null ||
    umbrella.sessionRef !== request.action.sessionRef ||
    umbrella.genesisRef !== request.action.genesisRef
  ) {
    return { kind: "ignored", why: "unknown-umbrella" };
  }
  if (
    !isCodingSessionHireAuthorized(request.requesterPubkey, input.authority)
  ) {
    return { kind: "ignored", why: "unauthorized" };
  }

  const liveSeats = listCodingSessionHireLiveSeats(umbrella);
  const decision = decideCodingSessionHire({
    request: {
      role: request.action.role,
      providerInstanceRef: request.action.providerInstanceRef,
      model: request.action.model,
    },
    policy: input.policy,
    candidates: input.candidates,
    liveSeats,
    availableProviderInstanceRefs: input.availableProviderInstanceRefs,
  });
  if (!decision.ok) {
    return {
      kind: "refused",
      code: decision.code,
      reason: decision.reason,
      text: formatCodingSessionHireRefusal(decision),
    };
  }

  return {
    kind: "seat",
    plan: buildCodingSessionHireSeatPlan({
      commandId: input.commandId,
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      genesisRef: request.action.genesisRef,
      projectRef: umbrella.projectRef ?? null,
      // Inherited, so the hired seat lands under the session's own name rather
      // than starting a second one beside it.
      title: umbrella.title.trim().length > 0 ? umbrella.title : null,
      brief: request.action.brief,
      role: decision.role,
      identity: decision.identity,
      providerInstanceRef: decision.providerInstanceRef,
      providerAuthorityPubkey: input.providerAuthorityPubkey,
      model: decision.model,
      seatOrdinal: codingSessionHireSeatOrdinal(liveSeats, decision.role),
    }),
  };
}

/**
 * The line the umbrella shows for a refusal.
 *
 * Published as an ordinary session-lane message from the operator, so it lands
 * in the umbrella's own timeline where the work is, not only in the private
 * turn the requesting seat receives. A refusal only the refused agent can see
 * is a refusal the person never learns about — and the person is the one who
 * set the policy.
 */
export function codingSessionHireRefusalNotice(input: {
  role: string;
  requesterLabel: string;
  text: string;
}): string {
  return `${input.requesterLabel} asked to hire a ${input.role} — ${input.text}`;
}
