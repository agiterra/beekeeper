/**
 * The kind-24223 provider liveness lease, and the reachability it proves.
 *
 * Mirrors `readSessionLease` and the reachability arm of
 * `desktop/src/shared/coordination/sessionCoordinationFold.ts`. A lease is the
 * ONLY evidence that anything can answer for a generation right now. Metadata
 * recency answers a different question — what the provider last *said* — and
 * may never be substituted for this one: that substitution is how a header
 * read `Idle` for two hours over an app that had quit.
 */
import { KIND_CODING_SESSION_LEASE } from "../../../shared/lib/kinds.ts";
import { buildCodingSessionTargetKey } from "./keys.ts";
import type {
  CodingSessionReachability,
  CodingSessionReachabilityReport,
  CodingSessionStatus,
  CodingSessionTarget,
  ObservedEvent,
} from "./types.ts";
import {
  decodeTarget,
  hasExactKeys,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./wireDecode.ts";

export const CODING_SESSION_LEASE_TAG_VERSION = "cslease1-1" as const;

/**
 * Conservative client-side lease lifetime, measured from the lease's signed
 * `created_at`. The relay's accepted-time expiry is 180 s; a client that
 * cannot see accepted-time must under-claim, never over-claim.
 */
export const CODING_SESSION_LEASE_TTL_SECONDS = 150;

const MAX_LEASE_CONTENT_BYTES = 4 * 1024;

export const CODING_SESSION_LEASE_SCHEMA =
  "buzz-coding-session-lease/v1" as const;

export type CodingSessionLease = {
  eventId: string;
  channelId: string;
  createdAt: number;
  signerPubkey: string;
  targetKey: string;
  target: CodingSessionTarget;
  /** The 44221 commandId this lease claims to serve. */
  commandId: string;
  state: "live" | "released";
  leaseSequence: number;
};

/**
 * Decode one 24223, or null.
 *
 * The tag list is exact AND ordered: the producer emits these five in one
 * fixed order, so a lease with the right tags in the wrong order is not one
 * this client signed off on.
 */
export function parseCodingSessionLease(
  event: ObservedEvent,
): CodingSessionLease | null {
  if (event.kind !== KIND_CODING_SESSION_LEASE) return null;
  const content = parseBoundedJson(event.content, MAX_LEASE_CONTENT_BYTES);
  if (
    !isPlainRecord(content) ||
    !hasExactKeys(content, ["schema", "target", "state", "leaseSequence"]) ||
    content.schema !== CODING_SESSION_LEASE_SCHEMA ||
    (content.state !== "live" && content.state !== "released") ||
    !Number.isSafeInteger(content.leaseSequence) ||
    (content.leaseSequence as number) <= 0
  ) {
    return null;
  }
  const target = decodeTarget(content.target);
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!target || !signerPubkey) return null;
  const targetKey = buildCodingSessionTargetKey(target);
  const tags = parseExactTags(event.tags, [
    "h",
    "cslease-v",
    "cs-target",
    "csl-command",
    "cslease-seq",
  ]);
  if (
    !tags ||
    tags[0].length === 0 ||
    tags[1] !== CODING_SESSION_LEASE_TAG_VERSION ||
    tags[2] !== targetKey ||
    tags[3].length === 0 ||
    tags[4] !== String(content.leaseSequence)
  ) {
    return null;
  }
  return {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    signerPubkey,
    targetKey,
    target,
    commandId: tags[3],
    state: content.state,
    leaseSequence: content.leaseSequence as number,
  };
}

/** Everything the reachability fold needs to know about one generation. */
export type ReachabilityInput = {
  /** Leases already scoped to this generation's target key. */
  leases: readonly CodingSessionLease[];
  /**
   * `commandId` of the create the provider actually confirmed for this
   * generation, or null when it has not been resolved. A lease that names a
   * different command proves liveness for a different session.
   */
  acceptedCommandId: string | null;
  /** The provider authority whose leases count. */
  providerAuthorityPubkey: string;
  /** True when this is the execution's highest generation. */
  isCurrentGeneration: boolean;
  /** True once a lease read for this generation has actually completed. */
  leasesRead: boolean;
  lastReportedStatus: CodingSessionStatus | null;
  /** `created_at` (ms) of that report, or null. */
  lastReportedAt: number | null;
  /** Wall clock in ms, read once after the last query returned. */
  nowMs: number;
};

/**
 * Reachability for one generation, per D8.
 *
 * The highest-sequence live lease for the CURRENT generation, younger than
 * {@link CODING_SESSION_LEASE_TTL_SECONDS}, proves `provider_reachable`. Two
 * distinct leases at that sequence prove nothing — the provider's own sequence
 * is supposed to break the tie, so a tie is evidence the snapshot cannot be
 * trusted. An unread lease query stays `unknown`; it must never render as
 * "nobody answering".
 */
export function resolveCodingSessionReachability(
  input: ReachabilityInput,
): CodingSessionReachabilityReport {
  const lastReportedAgeSeconds =
    input.lastReportedAt === null
      ? null
      : Math.max(0, Math.floor((input.nowMs - input.lastReportedAt) / 1000));
  const report = {
    lastReportedStatus: input.lastReportedStatus,
    lastReportedAgeSeconds,
  };
  if (!input.leasesRead) {
    return { reachability: "unknown", ...report };
  }
  const relevant = input.leases.filter(
    (lease) =>
      lease.signerPubkey === input.providerAuthorityPubkey &&
      // A lease must name the create the provider confirmed for this
      // generation. Until that is resolved nothing is proven either way.
      input.acceptedCommandId !== null &&
      lease.commandId === input.acceptedCommandId,
  );
  if (relevant.length === 0 || !input.isCurrentGeneration) {
    return { reachability: "no_provider_answering", ...report };
  }
  const highestSequence = relevant.reduce(
    (highest, lease) => Math.max(highest, lease.leaseSequence),
    Number.NEGATIVE_INFINITY,
  );
  const highest = [
    ...new Map(
      relevant
        .filter((lease) => lease.leaseSequence === highestSequence)
        .map((lease) => [lease.eventId, lease]),
    ).values(),
  ];
  if (highest.length !== 1) {
    return { reachability: "no_provider_answering", ...report };
  }
  const winner = highest[0];
  const expiresAtMs =
    (winner.createdAt + CODING_SESSION_LEASE_TTL_SECONDS) * 1000;
  return winner.state === "live" && input.nowMs < expiresAtMs
    ? { reachability: "provider_reachable", ...report }
    : { reachability: "no_provider_answering", ...report };
}

/** True exactly when the header may print a live-sounding status verbatim. */
export function isCodingSessionProviderReachable(
  reachability: CodingSessionReachability,
): boolean {
  return reachability === "provider_reachable";
}
