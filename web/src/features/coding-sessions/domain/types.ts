/**
 * The value vocabulary the browser observer reads sessions in.
 *
 * Copied from `desktop/src/features/coding-sessions/lib/codingSessionTypes.ts`
 * and trimmed to the read side: nothing here describes a write path, because
 * the web client has none.
 */

/** A signature-stripped-or-signed Nostr event as the relay hands it back. */
export type ObservedEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

/** Provider-neutral target for an external coding-session provider adapter. */
export type CodingSessionTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/** Every status a provider may report for one generation. */
export type CodingSessionStatus =
  | "starting"
  | "idle"
  | "running"
  | "waiting_for_input"
  | "completed"
  | "stopped"
  | "failed"
  | "interrupted"
  | "disconnected"
  | "unknown";

export type CodingSessionCapabilities = {
  threadTurnStart: boolean;
  threadTurnInterrupt: boolean;
  threadSteer: boolean;
  context: boolean;
  diff: boolean;
  plan: boolean;
};

/**
 * How this generation's provider authority was reached.
 *
 * `create` is the 44221 that named it. `disclosed-fallback` is the D5 escape
 * hatch — no readable create, so the first-seen metadata signer stands in and
 * every surface must say "authority unverified" rather than imply proof.
 */
export type CodingSessionAuthoritySource = "create" | "disclosed-fallback";

/** One generation of one execution, as observed. */
export type CodingSessionGenerationRecord = {
  generationId: string;
  channelId: string;
  target: CodingSessionTarget;
  /** The signer whose facts these are. Never merged across signers. */
  providerAuthorityPubkey: string;
  authoritySource: CodingSessionAuthoritySource;
  title: string | null;
  projectRef: string | null;
  repoRef: string | null;
  sessionRef: string | null;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  agentRef: string | null;
  capabilities: CodingSessionCapabilities | null;
  status: CodingSessionStatus;
  /** `created_at` (ms) of the metadata that reported `status`, or null. */
  statusAt: number | null;
  /** Max `created_at` (ms) across every accepted fact for this generation. */
  lastEventAt: number;
  /** Distinct payloads that could not be resolved; each one renders nothing. */
  conflictCount: number;
  transcript: ProjectedTranscriptItem[];
};

/** One provider runtime session across its generations, per signer. */
export type CodingSessionExecution = {
  executionKey: string;
  signerPubkey: string;
  authoritySource: CodingSessionAuthoritySource;
  activeGeneration: CodingSessionGenerationRecord;
  /** Earlier generations, ascending. */
  priorGenerations: CodingSessionGenerationRecord[];
  /** Human who signed the 44221 that minted this execution, when known. */
  operatorPubkey: string | null;
  /** Compact "runtime · model", or the agentRef when the provider named one. */
  label: string;
};

/** How an umbrella's founder was reached — never guessed. */
export type CodingSessionFounderResolution =
  | "governed"
  | "legacy"
  | "unresolved"
  | "conflict";

/** Reachability, derived from the 24223 lease alone. */
export type CodingSessionReachability =
  /** A live lease for the current generation, younger than the TTL. */
  | "provider_reachable"
  /** Leases were read and none proves anybody is answering. */
  | "no_provider_answering"
  /** No lease read has completed — must never render as "nobody answering". */
  | "unknown";

/** The reachability line a session header prints, per D8. */
export type CodingSessionReachabilityReport = {
  reachability: CodingSessionReachability;
  /** The provider's newest signed status, demoted to history when unreachable. */
  lastReportedStatus: CodingSessionStatus | null;
  /** Age of that report in seconds, or null when nothing has been reported. */
  lastReportedAgeSeconds: number | null;
};

/** All executions sharing a `sessionRef`; a record without one is one of these too. */
export type CodingSessionUmbrella = {
  /** `sessionRef`, or `implicit:<executionKey>` for an umbrella of one. */
  umbrellaKey: string;
  channelId: string;
  sessionRef: string | null;
  /** Newest 44229 name, else the newest metadata title/label, else a fallback. */
  name: string;
  executions: CodingSessionExecution[];
  founderPubkey: string | null;
  genesisRef: string | null;
  founderResolution: CodingSessionFounderResolution;
  status: CodingSessionStatus;
  /** True when the newest 44230 for this session says closed. */
  closed: boolean;
  lastEventAt: number;
  conflictCount: number;
  /** Executions whose known operator is not the founder — flagged, never merged. */
  foreignAttachmentCount: number;
};

/** A rendered transcript row. Deliberately flat: the browser renders it directly. */
export type ProjectedTranscriptItem = {
  /** Stable, collision-free identity for this row. */
  id: string;
  /** Which execution stream produced it — blocks interleave, items never do. */
  blockKey: string;
  /** Turn identity for separator placement, or null when the row owns no turn. */
  turnId: string | null;
  role: "user" | "assistant" | "tool" | "lifecycle";
  title: string;
  /** Rendered body. Empty for rows that carry no payload (unknown kinds). */
  text: string;
  /** True for rows the reader opens deliberately (tool calls, reasoning). */
  folded: boolean;
  timestamp: number;
  eventSeq: number;
  /** Tool rows only. */
  tool: {
    toolName: string;
    toolId: string | null;
    args: Record<string, unknown>;
    status: "pending" | "completed" | "error";
    result: string;
  } | null;
  /** `result` rows only — structured, never baked into `text`. */
  lifecycle: {
    durationMs: number | null;
    costUsd: number | null;
    isError: boolean;
  } | null;
  /** Metadata chips: operator short id, command short id, elision reason, … */
  meta: string[];
  /** Set when the item's `kind` was not one this reader knows. */
  unknownKind: string | null;
};
