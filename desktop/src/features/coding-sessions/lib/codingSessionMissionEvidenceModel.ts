import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";
import { projectCodingSessionMissionAuthority } from "./codingSessionMissionAuthority";
import { projectNativeTeamFoldToMissionInspector } from "./codingSessionMissionTransactionProjection";
import {
  decodeVerifiedCodingSessionTeamTransaction,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  type VerifiedCodingSessionTeamTransaction,
} from "./codingSessionTeamTransactionWire";
import { invokeCodingSessionTeamFold } from "./invokeCodingSessionTeamFold";

const RECEIPT_TYPE = "coding_session_authority_transition_accepted";
export const MISSION_EVIDENCE_MAX_EVENTS_PER_KIND = 1000;
export const MISSION_EVIDENCE_REJECTION_LIMIT = 100;
export const MISSION_EVIDENCE_HISTORY_LIMIT =
  MISSION_EVIDENCE_MAX_EVENTS_PER_KIND + 1;

export const CODING_SESSION_MISSION_EVIDENCE_KINDS = [
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
] as const;

export type CodingSessionMissionEvidenceScope = {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
};

export type CodingSessionMissionEvidenceSnapshot = {
  transactions: RelayEvent[];
  transitions: RelayEvent[];
  receipts: RelayEvent[];
  rejected: Array<{ eventId: string; createdAt: number; reason: string }>;
  rejectedTotal: number | null;
  rejectedOmitted: number | null;
  rejectionsTruncated: boolean;
  overflowed: boolean;
};

function hasTag(event: RelayEvent, name: string, value: string): boolean {
  return event.tags.some(
    (tag) => tag.length === 2 && tag[0] === name && tag[1] === value,
  );
}

function receiptMatchesGenesis(event: RelayEvent, genesisRef: string): boolean {
  try {
    const value: unknown = JSON.parse(event.content);
    return (
      typeof value === "object" &&
      value !== null &&
      !Array.isArray(value) &&
      (value as Record<string, unknown>).type === RECEIPT_TYPE &&
      (value as Record<string, unknown>).genesisRef === genesisRef
    );
  } catch {
    return false;
  }
}

function isRelevant(
  event: RelayEvent,
  scope: CodingSessionMissionEvidenceScope,
): boolean {
  if (!hasTag(event, "h", scope.channelRef)) return false;
  if (event.kind === KIND_CODING_SESSION_TEAM_TRANSACTION) {
    return (
      hasTag(event, "d", scope.sessionRef) &&
      hasTag(event, "cstx-genesis", scope.genesisRef)
    );
  }
  if (event.kind === KIND_CODING_SESSION_AUTHORITY_TRANSITION) {
    return hasTag(event, "csat-genesis", scope.genesisRef);
  }
  return (
    event.kind === KIND_SYSTEM_MESSAGE &&
    receiptMatchesGenesis(event, scope.genesisRef)
  );
}

function cloneEvent(event: RelayEvent): RelayEvent {
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

function sortEvents(events: Iterable<RelayEvent>): RelayEvent[] {
  return [...events].sort(
    (left, right) =>
      left.created_at - right.created_at || left.id.localeCompare(right.id),
  );
}

function compareEvents(left: RelayEvent, right: RelayEvent): number {
  return left.created_at - right.created_at || left.id.localeCompare(right.id);
}

type Rejection = { eventId: string; createdAt: number; reason: string };

function compareRejections(left: Rejection, right: Rejection): number {
  return (
    left.createdAt - right.createdAt ||
    left.eventId.localeCompare(right.eventId)
  );
}

function retainOldestEvent(
  events: Map<string, RelayEvent>,
  event: RelayEvent,
): boolean {
  if (events.has(event.id)) return false;
  if (events.size < MISSION_EVIDENCE_MAX_EVENTS_PER_KIND) {
    events.set(event.id, cloneEvent(event));
    return true;
  }
  const newestRetained = sortEvents(events.values()).at(-1);
  if (newestRetained && compareEvents(event, newestRetained) < 0) {
    events.delete(newestRetained.id);
    events.set(event.id, cloneEvent(event));
  }
  return true;
}

function retainOldestRejection(
  rejections: Map<string, Rejection>,
  rejection: Rejection,
): "added" | "displaced" | "duplicate" | "omitted" {
  if (rejections.has(rejection.eventId)) return "duplicate";
  if (rejections.size < MISSION_EVIDENCE_REJECTION_LIMIT) {
    rejections.set(rejection.eventId, rejection);
    return "added";
  }
  const newestRetained = [...rejections.values()]
    .sort(compareRejections)
    .at(-1);
  if (newestRetained && compareRejections(rejection, newestRetained) < 0) {
    rejections.delete(newestRetained.eventId);
    rejections.set(rejection.eventId, rejection);
    return "displaced";
  }
  return "omitted";
}

export function codingSessionMissionEvidenceScopeKey(
  scope: CodingSessionMissionEvidenceScope,
): string {
  return [
    scope.channelRef,
    scope.sessionRef,
    scope.genesisRef,
    scope.founderPubkey,
  ].join("\u0000");
}

export function buildCodingSessionMissionEvidenceFilters(
  scope: CodingSessionMissionEvidenceScope,
  limit: number,
  relayPubkey?: string,
): RelaySubscriptionFilter[] {
  return [
    {
      kinds: [KIND_CODING_SESSION_TEAM_TRANSACTION],
      "#h": [scope.channelRef],
      "#d": [scope.sessionRef],
      "#cstx-genesis": [scope.genesisRef],
      limit,
    },
    {
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      "#h": [scope.channelRef],
      "#csat-genesis": [scope.genesisRef],
      limit,
    },
    {
      kinds: [KIND_SYSTEM_MESSAGE],
      "#h": [scope.channelRef],
      ...(relayPubkey ? { authors: [relayPubkey] } : {}),
      limit,
    },
  ];
}

/** Scope-local bounded raw-event retention. It owns no module-level state. */
export class CodingSessionMissionEvidenceStore {
  readonly #scope: CodingSessionMissionEvidenceScope;
  readonly #events = new Map<number, Map<string, RelayEvent>>();
  readonly #rejected = new Map<string, Rejection>();
  #rejectionsTruncated = false;
  #overflowed = false;

  constructor(scope: CodingSessionMissionEvidenceScope) {
    this.#scope = scope;
  }

  ingest(events: readonly RelayEvent[]): boolean {
    let changed = false;
    for (const event of events) {
      if (!isRelevant(event, this.#scope)) continue;
      if (!hasValidSignature(event)) {
        const retention = retainOldestRejection(this.#rejected, {
          eventId: event.id,
          createdAt: event.created_at,
          reason: "Event signature is invalid.",
        });
        if (retention === "added" || retention === "displaced") {
          if (retention === "displaced") this.#rejectionsTruncated = true;
          changed = true;
        } else if (retention === "omitted" && !this.#rejectionsTruncated) {
          this.#rejectionsTruncated = true;
          changed = true;
        }
        continue;
      }
      const kindEvents = this.#events.get(event.kind) ?? new Map();
      const wasAtLimit =
        kindEvents.size >= MISSION_EVIDENCE_MAX_EVENTS_PER_KIND;
      if (!retainOldestEvent(kindEvents, event)) continue;
      if (wasAtLimit) {
        this.#overflowed = true;
      }
      this.#events.set(event.kind, kindEvents);
      changed = true;
    }
    return changed;
  }

  snapshot(): CodingSessionMissionEvidenceSnapshot {
    return {
      transactions: sortEvents(
        this.#events.get(KIND_CODING_SESSION_TEAM_TRANSACTION)?.values() ?? [],
      ),
      transitions: sortEvents(
        this.#events.get(KIND_CODING_SESSION_AUTHORITY_TRANSITION)?.values() ??
          [],
      ),
      receipts: sortEvents(
        this.#events.get(KIND_SYSTEM_MESSAGE)?.values() ?? [],
      ),
      rejected: [...this.#rejected.values()].sort(compareRejections),
      rejectedTotal: this.#rejectionsTruncated ? null : this.#rejected.size,
      rejectedOmitted: this.#rejectionsTruncated ? null : 0,
      rejectionsTruncated: this.#rejectionsTruncated,
      overflowed: this.#overflowed,
    };
  }
}

export function emptyCodingSessionMissionInspectorInput(
  detail = "No canonical team transaction has established mission state.",
): CodingSessionMissionInspectorInput {
  return {
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    seatPlans: [],
    reports: [],
    observedChanges: { files: [], unreportedEditCount: 0 },
    observedFileSources: new Map(),
    participants: [],
    contextLoads: new Map(),
    missionState: { kind: "unknown", detail },
    usage: null,
    rejectedEventCount: 0,
    rejectionsTruncated: false,
    rejectedReasons: [],
    conflicts: [],
  };
}

export async function projectCodingSessionMissionEvidence(input: {
  scope: CodingSessionMissionEvidenceScope;
  relayPubkey: string;
  snapshot: CodingSessionMissionEvidenceSnapshot;
}): Promise<CodingSessionMissionInspectorInput> {
  if (input.snapshot.overflowed) {
    throw new Error(
      `Mission evidence exceeded the ${MISSION_EVIDENCE_MAX_EVENTS_PER_KIND}-event per-kind bound.`,
    );
  }
  const authority = projectCodingSessionMissionAuthority({
    channelRef: input.scope.channelRef,
    genesisRef: input.scope.genesisRef,
    founderPubkey: input.scope.founderPubkey,
    relayPubkey: input.relayPubkey,
    transitions: input.snapshot.transitions,
    receipts: input.snapshot.receipts,
  });
  if (!authority.ok) throw new Error(authority.error);

  const verifiedTransactions: VerifiedCodingSessionTeamTransaction[] = [];
  const ingressRejections = [...input.snapshot.rejected];
  let decodedRejectionCount = 0;
  for (const event of input.snapshot.transactions) {
    const decoded = decodeVerifiedCodingSessionTeamTransaction({
      event,
      channelRef: input.scope.channelRef,
      sessionRef: input.scope.sessionRef,
      genesisRef: input.scope.genesisRef,
    });
    if (decoded.ok) verifiedTransactions.push(decoded.value);
    else {
      decodedRejectionCount += 1;
      ingressRejections.push({
        eventId: event.id,
        createdAt: event.created_at,
        reason: decoded.error,
      });
    }
  }
  const nativeFold = await invokeCodingSessionTeamFold({
    channelRef: input.scope.channelRef,
    sessionRef: input.scope.sessionRef,
    genesisRef: input.scope.genesisRef,
    authority: authority.value,
    verifiedTransactions,
  });
  const projected = projectNativeTeamFoldToMissionInspector({ nativeFold });
  if (projected.rejectedEventCount === null) {
    throw new Error("Native fold projection did not provide an exact count.");
  }
  ingressRejections.sort(compareRejections);
  const exactIngressTotal =
    input.snapshot.rejectedTotal === null
      ? null
      : input.snapshot.rejectedTotal + decodedRejectionCount;
  const exactCombinedTotal =
    exactIngressTotal === null
      ? null
      : projected.rejectedEventCount + exactIngressTotal;
  const rejectionsTruncated =
    input.snapshot.rejectionsTruncated ||
    exactCombinedTotal === null ||
    exactCombinedTotal > MISSION_EVIDENCE_REJECTION_LIMIT;
  return {
    ...projected,
    rejectedEventCount: rejectionsTruncated ? null : exactCombinedTotal,
    rejectionsTruncated,
    rejectedReasons: [
      ...projected.rejectedReasons,
      ...ingressRejections.map(({ eventId, reason }, index) => ({
        code: `INGRESS_REJECTED_${index + 1}`,
        summary: reason,
        eventIds: [eventId],
      })),
    ],
  };
}
