/**
 * Public schema and value types for the byte-exact Project Pulse fold.
 *
 * The session half of this vocabulary now lives in
 * `shared/coordination/sessionCoordinationTypes.ts`, because Project Pulse is
 * no longer its only reader. The Pulse names below are aliases of the shared
 * ones — same values, same fields, same wire bytes — kept so the digest's
 * published vocabulary and the Rust twin's field names do not move.
 */

import type { PulseCost, PulseEntryType, PulseEvent } from "./pulseEntry.ts";
import type {
  CoordinatedGeneration,
  CoordinatedSession,
  SessionCoordinationState,
  SessionLifecycle,
  SessionReachability,
} from "../../../shared/coordination/sessionCoordinationTypes.ts";

export {
  SESSION_COMMIT_CONFIRMED as PULSE_COMMIT_CONFIRMED,
  SESSION_COMMIT_NOT_CHECKED as PULSE_COMMIT_NOT_CHECKED,
  SESSION_COMMIT_NOT_FOUND as PULSE_COMMIT_NOT_FOUND,
  SESSION_COORDINATION_KINDS as PULSE_SESSION_KINDS,
  SESSION_LEASE_TTL_SECONDS as PULSE_LEASE_TTL_SECONDS,
} from "../../../shared/coordination/sessionCoordinationTypes.ts";

/** Exact `schema` value of the digest envelope (kind 39011's content). */
export const PULSE_DIGEST_SCHEMA = "buzz-project-pulse-digest/v2";

/** The digest's session reach: the project's own channels, never wider. */
export const PULSE_SESSIONS_SCOPE = "project channels";

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
  /**
   * What the entry said its work cost, when it said anything at all.
   *
   * Present **only** when the entry carried a `cost`, so a costless entry
   * serializes byte-identically to the pre-cost digest and every
   * `conformance/project-pulse-fold/` vector keeps passing unchanged. The Rust
   * digest twin (`beekeeper_core::pulse_fold::PulseDigestEntry`) does not carry the
   * key yet, so no fold vector may carry one until it does — the two folds are
   * pinned byte-for-byte against the same corpus.
   */
  cost?: PulseCost;
};

/** Reachability evidence for one exact session generation. */
export type PulseGenerationReachability = SessionReachability;

/** One authority-proven exact execution generation. */
export type PulseDigestGeneration = CoordinatedGeneration;

/** Durable lifecycle state for an umbrella session. */
export type PulseSessionLifecycle = SessionLifecycle;

/** Coordination state derived independently from lifecycle and reachability. */
export type PulseCoordinationState = SessionCoordinationState;

/** One durable umbrella session with its authority-proven generations. */
export type PulseDigestSession = CoordinatedSession;

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
