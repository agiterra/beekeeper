import { invokeTauri } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";
import type { CodingSessionMissionAuthorityProjection } from "./codingSessionMissionAuthority";
import {
  decodeVerifiedCodingSessionTeamTransaction,
  type VerifiedCodingSessionTeamTransaction,
} from "./codingSessionTeamTransactionWire";

export const CODING_SESSION_TEAM_FOLD_COMMAND =
  "fold_coding_session_team_transactions";
export const CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA =
  "buzz-coding-session-team-fold-request/v1";
export const CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA =
  "buzz-coding-session-team-fold-adapter/v1";

const HEX64 = /^[0-9a-f]{64}$/;
const nativeFoldBrand = Symbol("native-coding-session-team-fold");
const issuedNativeFolds = new WeakSet<object>();

export type CodingSessionNativeTeamFoldResponse = {
  readonly schema: typeof CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA;
  readonly implementation: "buzz-core";
  readonly inputEventIds: readonly string[];
  readonly context: {
    readonly channelRef: string;
    readonly sessionRef: string;
    readonly genesisRef: string;
    readonly founderPubkey: string;
    readonly authorityHeadEventId: string | null;
    readonly authorityHeadSeq: number;
  };
  readonly includedEventIds: readonly string[];
  readonly excluded: readonly {
    readonly eventId: string;
    readonly code: string;
    readonly reason: string;
  }[];
  readonly conflicts: readonly {
    readonly subject: string;
    readonly winnerEventId: string;
    readonly contenderEventIds: readonly string[];
  }[];
  readonly assignments: readonly {
    readonly assignmentEventId: string;
    readonly governedReportEventId: string | null;
    readonly dispositionEventId: string | null;
    readonly acknowledgementEventId: string | null;
    readonly settled: boolean;
  }[];
  readonly canonicalTerminal: {
    readonly eventId: string;
    readonly type: "mission.completed" | "mission.blocked";
  } | null;
};

export type ImmutableCodingSessionTeamWireEvent = {
  readonly id: string;
  readonly pubkey: string;
  readonly created_at: number;
  readonly kind: number;
  readonly tags: readonly (readonly string[])[];
  readonly content: string;
  readonly sig: string;
};

export type NativeCodingSessionTeamFold = {
  readonly fold: CodingSessionNativeTeamFoldResponse;
  readonly wireEvents: readonly ImmutableCodingSessionTeamWireEvent[];
  readonly [nativeFoldBrand]: true;
};

/**
 * Read a result issued by the production native wrapper. This assertion never
 * brands caller data; the private WeakSet is populated only after real Tauri
 * invocation, response decoding, and request binding succeed.
 */
export function requireIssuedNativeCodingSessionTeamFold(
  value: NativeCodingSessionTeamFold,
): NativeCodingSessionTeamFold {
  if (!issuedNativeFolds.has(value)) {
    throw new Error(
      "coding-session team fold was not issued by the native wrapper",
    );
  }
  return value;
}

function isString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isEventId(value: unknown): value is string {
  return typeof value === "string" && HEX64.test(value);
}

function isNullableEventId(value: unknown): value is string | null {
  return value === null || isEventId(value);
}

function isEventIdArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) &&
    value.every(isEventId) &&
    new Set(value).size === value.length
  );
}

function cloneAndFreezeWireEvent(
  event: RelayEvent,
): ImmutableCodingSessionTeamWireEvent {
  const tags = Object.freeze(
    event.tags.map((tag) => Object.freeze([...tag]) as readonly string[]),
  );
  return Object.freeze({
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.created_at,
    kind: event.kind,
    tags,
    content: event.content,
    sig: event.sig,
  });
}

function mutableVerificationCopy(
  event: ImmutableCodingSessionTeamWireEvent,
): RelayEvent {
  return {
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.created_at,
    kind: event.kind,
    tags: event.tags.map((tag) => [...tag]),
    content: event.content,
    sig: event.sig,
  };
}

function isContext(
  value: unknown,
): value is CodingSessionNativeTeamFoldResponse["context"] {
  return (
    hasExactFields(value, [
      [
        "channelRef",
        "sessionRef",
        "genesisRef",
        "founderPubkey",
        "authorityHeadEventId",
        "authorityHeadSeq",
      ],
    ]) &&
    isString(value.channelRef) &&
    isString(value.sessionRef) &&
    isEventId(value.genesisRef) &&
    isEventId(value.founderPubkey) &&
    isNullableEventId(value.authorityHeadEventId) &&
    Number.isSafeInteger(value.authorityHeadSeq) &&
    (value.authorityHeadSeq as number) >= 0
  );
}

function isExclusion(
  value: unknown,
): value is CodingSessionNativeTeamFoldResponse["excluded"][number] {
  return (
    hasExactFields(value, [["eventId", "code", "reason"]]) &&
    isEventId(value.eventId) &&
    isString(value.code) &&
    isString(value.reason)
  );
}

function isConflict(
  value: unknown,
): value is CodingSessionNativeTeamFoldResponse["conflicts"][number] {
  return (
    hasExactFields(value, [
      ["subject", "winnerEventId", "contenderEventIds"],
    ]) &&
    isString(value.subject) &&
    isEventId(value.winnerEventId) &&
    isEventIdArray(value.contenderEventIds)
  );
}

function isSettlement(
  value: unknown,
): value is CodingSessionNativeTeamFoldResponse["assignments"][number] {
  return (
    hasExactFields(value, [
      [
        "assignmentEventId",
        "governedReportEventId",
        "dispositionEventId",
        "acknowledgementEventId",
        "settled",
      ],
    ]) &&
    isEventId(value.assignmentEventId) &&
    isNullableEventId(value.governedReportEventId) &&
    isNullableEventId(value.dispositionEventId) &&
    isNullableEventId(value.acknowledgementEventId) &&
    typeof value.settled === "boolean"
  );
}

function isTerminal(
  value: unknown,
): value is NonNullable<
  CodingSessionNativeTeamFoldResponse["canonicalTerminal"]
> {
  return (
    hasExactFields(value, [["eventId", "type"]]) &&
    isEventId(value.eventId) &&
    (value.type === "mission.completed" || value.type === "mission.blocked")
  );
}

function decodeNativeResponse(
  value: unknown,
): CodingSessionNativeTeamFoldResponse {
  if (
    !hasExactFields(value, [
      [
        "schema",
        "implementation",
        "inputEventIds",
        "context",
        "includedEventIds",
        "excluded",
        "conflicts",
        "assignments",
        "canonicalTerminal",
      ],
    ]) ||
    value.schema !== CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA ||
    value.implementation !== "buzz-core" ||
    !isEventIdArray(value.inputEventIds) ||
    !isContext(value.context) ||
    !isEventIdArray(value.includedEventIds) ||
    !Array.isArray(value.excluded) ||
    !value.excluded.every(isExclusion) ||
    !Array.isArray(value.conflicts) ||
    !value.conflicts.every(isConflict) ||
    !Array.isArray(value.assignments) ||
    !value.assignments.every(isSettlement) ||
    !(value.canonicalTerminal === null || isTerminal(value.canonicalTerminal))
  ) {
    throw new Error(
      "native coding-session team fold returned a malformed response",
    );
  }
  return value as CodingSessionNativeTeamFoldResponse;
}

function cloneAndFreezeNativeResponse(
  response: CodingSessionNativeTeamFoldResponse,
): CodingSessionNativeTeamFoldResponse {
  return Object.freeze({
    schema: response.schema,
    implementation: response.implementation,
    inputEventIds: Object.freeze([...response.inputEventIds]),
    context: Object.freeze({ ...response.context }),
    includedEventIds: Object.freeze([...response.includedEventIds]),
    excluded: Object.freeze(
      response.excluded.map((item) => Object.freeze({ ...item })),
    ),
    conflicts: Object.freeze(
      response.conflicts.map((conflict) =>
        Object.freeze({
          ...conflict,
          contenderEventIds: Object.freeze([...conflict.contenderEventIds]),
        }),
      ),
    ),
    assignments: Object.freeze(
      response.assignments.map((assignment) =>
        Object.freeze({ ...assignment }),
      ),
    ),
    canonicalTerminal: response.canonicalTerminal
      ? Object.freeze({ ...response.canonicalTerminal })
      : null,
  });
}

function sameStrings(
  left: readonly string[],
  right: readonly string[],
): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function bindResponse(input: {
  response: CodingSessionNativeTeamFoldResponse;
  expectedEventIds: readonly string[];
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  authority: CodingSessionMissionAuthorityProjection;
}): void {
  const { response } = input;
  if (
    !sameStrings(response.inputEventIds, input.expectedEventIds) ||
    response.context.channelRef !== input.channelRef ||
    response.context.sessionRef !== input.sessionRef ||
    response.context.genesisRef !== input.genesisRef ||
    response.context.founderPubkey !== input.authority.founderPubkey ||
    response.context.authorityHeadEventId !== input.authority.headEventId ||
    response.context.authorityHeadSeq !== input.authority.headSeq
  ) {
    throw new Error(
      "native coding-session team fold response does not match its request",
    );
  }
  const expected = new Set(input.expectedEventIds);
  const excludedIds = response.excluded.map((item) => item.eventId);
  const partition = [...response.includedEventIds, ...excludedIds];
  if (
    new Set(partition).size !== partition.length ||
    partition.length !== expected.size ||
    partition.some((id) => !expected.has(id))
  ) {
    throw new Error(
      "native coding-session team fold response does not partition its input ids",
    );
  }
  const references = [
    ...response.conflicts.flatMap((conflict) => [
      conflict.winnerEventId,
      ...conflict.contenderEventIds,
    ]),
    ...response.assignments.flatMap((assignment) => [
      assignment.assignmentEventId,
      assignment.governedReportEventId,
      assignment.dispositionEventId,
      assignment.acknowledgementEventId,
    ]),
    response.canonicalTerminal?.eventId ?? null,
  ].filter((id): id is string => id !== null);
  if (references.some((id) => !expected.has(id))) {
    throw new Error(
      "native coding-session team fold response cites an unknown input id",
    );
  }
}

/**
 * Invoke the native buzz-core fold and bind its response to the exact verified
 * signed inputs. TypeScript checks transport shape, echo, and provenance only;
 * semantic correctness belongs exclusively to the named Rust implementation.
 */
export async function invokeCodingSessionTeamFold(input: {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  authority: CodingSessionMissionAuthorityProjection;
  verifiedTransactions: readonly VerifiedCodingSessionTeamTransaction[];
}): Promise<NativeCodingSessionTeamFold> {
  if (
    input.authority.channelRef !== input.channelRef ||
    input.authority.genesisRef !== input.genesisRef ||
    input.authority.headSeq !== input.authority.acceptedEventIds.length ||
    input.authority.headEventId !==
      (input.authority.acceptedEventIds.at(-1) ?? null)
  ) {
    throw new Error(
      "native coding-session team fold request has a mismatched authority projection",
    );
  }
  const wireEvents = input.verifiedTransactions
    .map((event) => cloneAndFreezeWireEvent(event.wireEvent))
    .sort((a, b) => a.id.localeCompare(b.id));
  const inputEventIds = wireEvents.map((event) => event.id);
  const invalidWireEvent = wireEvents.find((event) => {
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event: mutableVerificationCopy(event),
      channelRef: input.channelRef,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
    });
    return !decoded.ok;
  });
  if (
    new Set(inputEventIds).size !== inputEventIds.length ||
    invalidWireEvent
  ) {
    throw new Error(
      "native coding-session team fold request has duplicate or invalid signed wire events",
    );
  }
  const immutableWireEvents = Object.freeze([...wireEvents]);
  const request = Object.freeze({
    schema: CODING_SESSION_TEAM_FOLD_REQUEST_SCHEMA,
    context: Object.freeze({
      channelRef: input.channelRef,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
      founderPubkey: input.authority.founderPubkey,
      authorityHeadEventId: input.authority.headEventId,
      authorityHeadSeq: input.authority.headSeq,
      activeSeats: Object.freeze(
        input.authority.activeSeats.map((seat) => Object.freeze({ ...seat })),
      ),
      activeGrants: Object.freeze(
        input.authority.activeGrants.map((grant) =>
          Object.freeze({ ...grant }),
        ),
      ),
    }),
    inputEventIds: Object.freeze([...inputEventIds]),
    events: immutableWireEvents,
  });
  const response = cloneAndFreezeNativeResponse(
    decodeNativeResponse(
      await invokeTauri(CODING_SESSION_TEAM_FOLD_COMMAND, { request }),
    ),
  );
  bindResponse({
    response,
    expectedEventIds: inputEventIds,
    channelRef: input.channelRef,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
    authority: input.authority,
  });
  const result = Object.freeze({
    fold: response,
    wireEvents: immutableWireEvents,
    [nativeFoldBrand]: true as const,
  });
  issuedNativeFolds.add(result);
  return result;
}
