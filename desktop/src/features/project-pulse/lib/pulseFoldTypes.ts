/** Public schema and value types for the byte-exact Project Pulse fold. */

import type { PulseEntryType, PulseEvent } from "./pulseEntry.ts";

/** Exact `schema` value of the digest envelope (kind 39011's content). */
export const PULSE_DIGEST_SCHEMA = "buzz-project-pulse-digest/v2";

/** The digest's session reach: the project's own channels, never wider. */
export const PULSE_SESSIONS_SCOPE = "project channels";

/** Conservative client-side lease lifetime derived from signed issue time. */
/** Conservative client expiry; relay-authoritative accepted-time expiry is 180s. */
export const PULSE_LEASE_TTL_SECONDS = 150;

/** Kinds the session half of the fold consumes, in wire order. */
export const PULSE_SESSION_KINDS = [
  24223, 44221, 44223, 44224, 44227, 44229, 44230,
] as const;

/** The fixed tri-state commit-confirmation strings every surface emits. */
export const PULSE_COMMIT_CONFIRMED = "Commit confirmed on relay";
export const PULSE_COMMIT_NOT_FOUND = "Commit not found on relay";
export const PULSE_COMMIT_NOT_CHECKED = "Commit not checked";

/** Why a `supersedes` claim was not honored; `null` on an honored claim. */
export type PulseSupersessionReason =
  | "cross-author"
  | "unresolved"
  | "out-of-order";

/** One supersession claim, from the perspective of the entry that carries it. */
export type PulseSupersessionClaim = {
  eventId: string;
  /** The other entry's author, or `null` when it is not in the result set. */
  pubkey: string | null;
  honored: boolean;
  reason: PulseSupersessionReason | null;
};

/** One folded Pulse entry. `claimedAreas` are claims, never observed facts. */
export type PulseDigestEntry = {
  eventId: string;
  pubkey: string;
  createdAt: number;
  type: PulseEntryType;
  text: string;
  claimedAreas: string[];
  branch: string | null;
  sessionRef: string | null;
  supersedes: string | null;
  supersededBy: PulseSupersessionClaim[];
  active: boolean;
};

/** Reachability evidence for one exact session generation. */
export type PulseGenerationReachability =
  | "provider_reachable"
  | "unverified"
  | "terminal";

/** One authority-proven exact execution generation. */
export type PulseDigestGeneration = {
  targetKey: string;
  executionKey: string;
  providerAuthorityPubkey: string;
  current: boolean;
  reachability: PulseGenerationReachability;
  status: string | null;
  statusAt: number | null;
  branch: string | null;
  observedCommit: string | null;
  dirty: boolean | null;
  relayReachable: boolean | null;
  verifiedAt: number | null;
  commitConfirmation: string;
  leaseState: "live" | "released" | null;
  leaseIssuedAt: number | null;
  leaseAcceptedAt: number | null;
  leaseExpiresAt: number | null;
  leaseSigner: string | null;
  leaseSourceEventId: string | null;
  leaseSequence: number | null;
  lifecycleCommandEventId: string;
  lifecycleReceiptEventId: string;
  sourceEventIds: string[];
};

/** Durable lifecycle state for an umbrella session. */
export type PulseSessionLifecycle = "open" | "closed";

/** Coordination state derived independently from lifecycle and reachability. */
export type PulseCoordinationState =
  | "provider_reachable"
  | "open_unverified"
  | "closed";

/** One durable umbrella session with its authority-proven generations. */
export type PulseDigestSession = {
  sessionKey: string;
  sessionRef: string | null;
  name: string | null;
  goal: string | null;
  lifecycle: PulseSessionLifecycle;
  coordinationState: PulseCoordinationState;
  latestObservationAt: number | null;
  observedAgeSeconds: number | null;
  generations: PulseDigestGeneration[];
  sourceEventIds: string[];
};

/** One source query that failed, was truncated, or yielded a bad event. */
export type PulseDigestError = { scope: string; message: string };

/** The complete digest envelope — the §6 kind-39011 content object. */
export type ProjectPulseDigest = {
  schema: typeof PULSE_DIGEST_SCHEMA;
  source: string;
  project: string;
  asOf: number;
  complete: boolean;
  sessionsScope: typeof PULSE_SESSIONS_SCOPE;
  sessions: PulseDigestSession[];
  providerReachableSessions: string[];
  openUnverifiedSessions: string[];
  closedSessions: string[];
  entries: PulseDigestEntry[];
  errors: PulseDigestError[];
};

/** Everything the fold consumes. `now` is read once, after the last query. */
export type ProjectPulseFoldInput = {
  project: string;
  now: number;
  events: readonly PulseEvent[];
  /** One entry per source query that failed or was truncated by `limit`. */
  sourceErrors?: readonly PulseDigestError[];
  /** `client-composed` here; a relay-side fold substitutes `relay-digest`. */
  source?: string;
};
