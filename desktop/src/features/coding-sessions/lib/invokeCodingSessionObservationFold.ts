/**
 * Invoke the native kind-44246 fold and bind its answer to the request.
 *
 * TypeScript checks transport shape, echo and provenance only. Semantic
 * correctness — what supersedes what, what is bounded, what is disclosed —
 * belongs exclusively to `buzz_core::fold_coding_session_observations`, which
 * the native adapter calls and returns unchanged (§0.5, I6).
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  CODING_SESSION_OBSERVATION_FOLD_COMMAND,
  CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA,
  CodingSessionObservationWireError,
  decodeCodingSessionObservationFold,
  type CodingSessionObservationFold,
} from "./codingSessionObservationWire";

/**
 * The signed `created_at` of one folded observation, in unix seconds.
 *
 * The fold reads no clock and returns none — every time on this kind is the
 * author's own measurement, and the fold refuses to order by one. The Route
 * rail nonetheless has to *place* a sign somewhere on a time axis, and the
 * relay-signed `created_at` of the event the row came from is the only time
 * covered by a signature. It is carried here, beside the fold rather than
 * inside it, so nothing about ordering, dedupe or discovery can reach for it
 * by accident (§8 I4).
 */
export type CodingSessionObservationSignedTimes = ReadonlyMap<string, number>;

export type CodingSessionObservationFoldResult = {
  readonly fold: CodingSessionObservationFold;
  /** Signed `created_at` per input event id, in unix seconds. */
  readonly signedAt: CodingSessionObservationSignedTimes;
};

/**
 * Fold one umbrella's observations natively.
 *
 * `events` are the signed kind-44246 events as the relay returned them — in
 * the relay's own page order, **newest first**, and that order is part of the
 * contract: the native adapter folds them as a relay page, reversing before
 * it folds, so a caller that sorted them oldest-first would get the oldest
 * statement per gate crowned instead (finding 79). They are handed over
 * whole, because the fold verifies each signature itself and lists what it
 * cannot read rather than dropping it.
 */
export async function invokeCodingSessionObservationFold(input: {
  sessionRef: string;
  genesisRef: string;
  knownAssignmentRefs: readonly string[];
  /**
   * Pubkeys whose `observed` claim this session honours, or `null` when the
   * caller could not resolve them (REVIEW-L5 F2). `null` is not `[]`.
   */
  providerPubkeys: readonly string[] | null;
  events: readonly RelayEvent[];
  invoke?: (command: string, args: Record<string, unknown>) => Promise<unknown>;
}): Promise<CodingSessionObservationFoldResult> {
  const invoke = input.invoke ?? invokeTauri;
  const response = decodeCodingSessionObservationFold(
    await invoke(CODING_SESSION_OBSERVATION_FOLD_COMMAND, {
      request: {
        schema: CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA,
        sessionRef: input.sessionRef,
        genesisRef: input.genesisRef,
        knownAssignmentRefs: [...input.knownAssignmentRefs],
        providerPubkeys:
          input.providerPubkeys === null ? null : [...input.providerPubkeys],
        events: input.events.map((event) => ({ ...event })),
      },
    }),
  );
  bind(response, input);
  return Object.freeze({
    fold: response,
    signedAt: Object.freeze(
      new Map(input.events.map((event) => [event.id, event.created_at])),
    ) as CodingSessionObservationSignedTimes,
  });
}

/**
 * The response must be about the request that was sent.
 *
 * A fold that answered for another session, or cited an event id nobody handed
 * it, is not this session's record however well-formed it looks.
 */
function bind(
  fold: CodingSessionObservationFold,
  input: {
    sessionRef: string;
    genesisRef: string;
    events: readonly RelayEvent[];
  },
): void {
  if (
    fold.sessionRef !== input.sessionRef ||
    fold.genesisRef !== input.genesisRef
  ) {
    throw new CodingSessionObservationWireError(
      "response does not name the session and genesis it was asked about",
    );
  }
  const supplied = new Set(input.events.map((event) => event.id));
  if (
    fold.inputEventIds.length !== supplied.size ||
    fold.inputEventIds.some((id) => !supplied.has(id))
  ) {
    throw new CodingSessionObservationWireError(
      "response does not echo the exact event ids it was handed",
    );
  }
  const cited = [
    ...fold.checkpoints.map((row) => row.eventId),
    ...fold.gates.flatMap((row) => [...row.eventIds]),
    ...fold.findings.flatMap((row) => [...row.eventIds]),
    ...fold.phases.map((row) => row.eventId),
    ...fold.unresolved.map((row) => row.eventId),
    ...fold.ignored.map((row) => row.eventId),
  ];
  // A finding's `refs` and an `assignmentRef` are deliberately absent here:
  // both are pointers their author supplied, and both may name events outside
  // this fold — which is exactly why the fold calls them pointers and never
  // causal references.
  if (cited.some((id) => !supplied.has(id))) {
    throw new CodingSessionObservationWireError(
      "response cites an event id it was not handed",
    );
  }
}
