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

/**
 * How old a hire may be and still be answered, in seconds.
 *
 * Fifteen minutes: long enough that a host restarting, or a relay catching up
 * after a reconnect, still seats what a lead is genuinely waiting for; short
 * enough that nobody is surprised by a seat appearing for a request they made
 * before lunch. The CLI waits sixty seconds for an answer, so anything past
 * this window has already been reported `unconfirmed` to the lead.
 */
export const CODING_SESSION_HIRE_MAX_AGE_SECONDS = 15 * 60;

/** The exact sentence a stale hire is refused with. */
export const CODING_SESSION_HIRE_STALE_REASON =
  "this hire request is older than the host's window; hire again";

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
  /** Each runtime's offered model ids, by instance ref. See the decision. */
  modelCatalogs?: ReadonlyMap<string, readonly string[]>;
  /**
   * This host's clock, Unix seconds. Supplied so the staleness window is a
   * fact of the call rather than of when the module happened to run.
   */
  now?: number;
  /** How old a hire may be and still be answered. Seconds. */
  maxAgeSeconds?: number;
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

  // Age is checked after authority and before policy: a host that has been
  // shut for an hour comes back to a channel full of requests nobody is
  // waiting on any more, and seating them would hand a lead a team it asked
  // for in another context entirely. Refused rather than dropped, because a
  // lead that heard nothing cannot tell this host from a dead one.
  const now = input.now ?? Math.floor(Date.now() / 1000);
  const maxAge = input.maxAgeSeconds ?? CODING_SESSION_HIRE_MAX_AGE_SECONDS;
  if (now - request.createdAt > maxAge) {
    const refusal = {
      code: "HIRE_STALE" as const,
      reason: CODING_SESSION_HIRE_STALE_REASON,
    };
    return {
      kind: "refused",
      ...refusal,
      text: formatCodingSessionHireRefusal(refusal),
    };
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
    ...(input.modelCatalogs ? { modelCatalogs: input.modelCatalogs } : {}),
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
      modelNotice: decision.modelNotice,
      seatOrdinal: codingSessionHireSeatOrdinal(liveSeats, decision.role),
    }),
  };
}

/**
 * The umbrella's line for a hire this host answered with somebody else's
 * model.
 *
 * Same reasoning as the refusal notice below: a substitution recorded only in
 * the seat's create is one nobody reads. The lead asked for a model, the host
 * ran a different one, and both the lead and the person get told in the
 * timeline where the work is.
 */
export function codingSessionHireModelNoticeLine(input: {
  role: string;
  notice: string;
}): string {
  return `Hired a ${input.role} — ${input.notice}`;
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

/**
 * The sentence a seat that was created but never granted is reported with.
 *
 * A hired agent that holds no `grant-operator` is not a degraded seat, it is a
 * mute one: the relay refuses its `sessions send` with "only a session founder
 * or a granted operator may steer", so it can do the whole job and never
 * deliver a word of it. That happened on 2026-08-28 — a builder worked for
 * 1,009 s, committed, and its report bounced (item 83). The lead has to be
 * told in the same breath as "seated", because "seated" alone reads as a seat
 * it can expect an answer from.
 */
export function codingSessionHireGrantFailureText(reason: string): string {
  const said = reason.trim();
  return `seated, but not granted: ${
    said.length > 0 ? said : "the grant did not go out"
  } — it cannot report until granted`;
}

/** The umbrella's line for the same failure, so the person sees it too. */
export function codingSessionHireGrantFailureNotice(input: {
  role: string;
  text: string;
}): string {
  return `Hired a ${input.role} — ${input.text}`;
}
