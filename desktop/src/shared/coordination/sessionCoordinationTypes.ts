/**
 * Value types for the shared session-coordination fold.
 *
 * Kept beside the fold rather than inside either feature: Project Pulse and
 * Agent Progress are both adapters over this vocabulary, and a second copy of
 * these names in a feature directory is how two surfaces start answering "is
 * this alive?" differently.
 */

/**
 * Conservative client-side lease lifetime, measured from the lease's signed
 * `created_at`. The relay's accepted-time expiry is 180s; a client that cannot
 * see accepted-time must under-claim, never over-claim.
 */
export const SESSION_LEASE_TTL_SECONDS = 150;

/** The ephemeral lease kind. It is a snapshot of Redis, never history. */
export const KIND_SESSION_LEASE = 24223;

/**
 * The durable signed facts the fold consumes: lifecycle command, metadata,
 * lifecycle receipt, goal, name, closure — in wire order.
 */
export const SESSION_COORDINATION_DURABLE_KINDS = [
  44221, 44223, 44224, 44227, 44229, 44230,
] as const;

/** Every kind the fold consumes, in wire order. */
export const SESSION_COORDINATION_KINDS = [
  KIND_SESSION_LEASE,
  ...SESSION_COORDINATION_DURABLE_KINDS,
] as const;

/** The fixed tri-state commit-confirmation strings every surface emits. */
export const SESSION_COMMIT_CONFIRMED = "Commit confirmed on relay";
export const SESSION_COMMIT_NOT_FOUND = "Commit not found on relay";
export const SESSION_COMMIT_NOT_CHECKED = "Commit not checked";

/**
 * A signature-stripped Nostr event — the shape both `POST /query` and the
 * desktop relay client hand back.
 */
export type CoordinationEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** Reachability evidence for one exact session generation. */
export type SessionReachability =
  | "provider_reachable"
  | "unverified"
  | "terminal";

/** One authority-proven exact execution generation. */
export type CoordinatedGeneration = {
  targetKey: string;
  executionKey: string;
  providerAuthorityPubkey: string;
  current: boolean;
  reachability: SessionReachability;
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
export type SessionLifecycle = "open" | "closed";

/**
 * Coordination state — derived independently from lifecycle and reachability,
 * and the *only* axis in this app that answers "can this session be reached
 * right now?". Metadata recency answers a different question (what did it last
 * report) and may never be substituted for this one.
 */
export type SessionCoordinationState =
  | "provider_reachable"
  | "open_unverified"
  | "closed";

/** One durable umbrella session with its authority-proven generations. */
export type CoordinatedSession = {
  sessionKey: string;
  sessionRef: string | null;
  name: string | null;
  goal: string | null;
  lifecycle: SessionLifecycle;
  coordinationState: SessionCoordinationState;
  latestObservationAt: number | null;
  observedAgeSeconds: number | null;
  generations: CoordinatedGeneration[];
  sourceEventIds: string[];
};

/**
 * Something the fold refused to resolve.
 *
 * The fold is fail-closed: two lifecycle commands under one id, two receipts,
 * or two distinct leases at the same sequence prove nothing, so nothing is
 * claimed. Refusing silently would let a surface print a confident session
 * count over evidence it could not read, so each refusal is reported and the
 * adapters disclose it ("at least N").
 */
export type SessionCoordinationAmbiguity = {
  /** `authority` — conflicting lifecycle proof; `lease` — a tied lease sequence. */
  scope: "authority" | "lease";
  message: string;
};

/** One failed, truncated, or unreadable source read supplied by an adapter. */
export type SessionCoordinationSourceError = {
  scope: string;
  message: string;
};

/** Everything the fold consumes. `now` is read once, after the last query. */
export type SessionCoordinationFoldInput = {
  /** Seconds since the epoch, read once after the last source query returned. */
  now: number;
  events: readonly CoordinationEvent[];
  /** Source-read failures; fold observations and ambiguities stay separate. */
  sourceErrors?: readonly SessionCoordinationSourceError[];
  /**
   * Optional scope predicate over a signed `projectRef`.
   *
   * Project Pulse passes its project's canonical coordinate check, so a
   * session belonging to another project is not that project's business. A
   * global surface passes nothing and accepts every proven session, including
   * those whose `projectRef` is null.
   */
  acceptProjectRef?: (projectRef: string | null) => boolean;
  /**
   * The keys that may **commission** an execution of these sessions — a
   * mission's founder together with the keys its accepted authority chain
   * grants `operator` (2026-09-05 refuter, B1).
   *
   * Omit it and the fold applies the weaker rule it can apply without an
   * authority projection: a create signed by the very provider it names, and
   * answering no hire anybody else signed, proves no generation. Supply it and
   * the rule is the relay's own. Either way a refused command is reported in
   * {@link SessionCoordinationFold.ambiguities} rather than dropped in silence.
   */
  commissioners?: readonly string[];
};

/** The fold's complete answer about coordination. */
export type SessionCoordinationFold = {
  /** False exactly when {@link errors} is nonempty. */
  complete: boolean;
  /** Adapter-supplied source-read failures, byte-ordered for stable rendering. */
  errors: SessionCoordinationSourceError[];
  sessions: CoordinatedSession[];
  providerReachableSessions: string[];
  openUnverifiedSessions: string[];
  closedSessions: string[];
  /** Evidence the fold refused to resolve. Empty means nothing was ambiguous. */
  ambiguities: SessionCoordinationAmbiguity[];
  /**
   * The `h` channels each session's authority proof came from, keyed by
   * `sessionKey` and byte-ordered.
   *
   * Deliberately beside {@link CoordinatedSession} rather than inside it: the
   * session object is a published wire shape with a byte-exact Rust twin, and
   * a global surface still needs the channel to route to a session and to join
   * it against locally-held transcripts.
   */
  channelsBySession: ReadonlyMap<string, string[]>;
};
