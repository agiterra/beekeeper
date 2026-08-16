import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import type { CodingSessionUmbrellaCreateObservation } from "./codingSessionUmbrellaModel";

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

/** Provider-neutral catalog record consumed by coding-session surfaces. */
export type CodingSessionCatalogRecord = {
  generationId: string;
  label: string;
  title: string;
  /** Exact trusted provider authority derived from this generation's transcripts. */
  providerAuthorityPubkey: string | null;
  /** Exact authority whose metadata enriched this record, or null when unenriched. */
  metadataAuthorityPubkey: string | null;
  lastEventAt: string;
  status: CodingSessionStatus;
  transcript: TranscriptItem[];
  conflictCount: number;
  commandTarget: CodingSessionCommandTarget | null;
  projectRef: string | null;
  repoRef: string | null;
  /**
   * Umbrella session reference echoed by the provider's 44223 metadata, or
   * null for a pre-umbrella session (an implicit umbrella of one).
   */
  sessionRef: string | null;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  capabilities: CodingSessionCapabilities | null;
};

/**
 * One provider runtime session across its generations: the `cs-target` minus
 * `generation`, per signer. The active generation is the highest one with
 * events; prior generations stay reachable as collapsed history.
 */
export type CodingSessionExecution = {
  executionKey: string;
  /** The fact-stream signer (provider authority) behind this execution. */
  signerPubkey: string;
  activeGeneration: CodingSessionCatalogRecord;
  /** Earlier generations, ascending. */
  priorGenerations: CodingSessionCatalogRecord[];
  /**
   * The human who signed this execution's 44221 create, when a create
   * observation resolved it; null when unknown.
   */
  operatorPubkey: string | null;
};

/**
 * All executions sharing a `sessionRef`, plus (for umbrella-claiming
 * sessions) the conversation lane. Records with no sessionRef form an
 * implicit umbrella of one.
 */
export type CodingSessionUmbrellaRecord = {
  /** `sessionRef`, or `implicit:<executionKey>` for an umbrella of one. */
  umbrellaKey: string;
  sessionRef: string | null;
  title: string;
  executions: CodingSessionExecution[];
  /** Genesis signer when linked, else the legacy founder projection. */
  founderPubkey: string | null;
  /** Exact genesis event id reached through a receipt-joined create. */
  genesisRef: string | null;
  /**
   * Derived: `running` if any execution runs, else `waiting_for_input` if any
   * waits, else the most recently active execution's status.
   */
  status: CodingSessionStatus;
  lastEventAt: string;
  conflictCount: number;
  /** Executions whose known operator is not the founder — flagged, never merged. */
  foreignAttachmentCount: number;
};

export type CodingSessionCatalogSnapshot = {
  channelId: string | null;
  entries: CodingSessionCatalogRecord[];
  /**
   * Receipt-joined 44221 create observations for this channel, when the
   * consumer collected them. Optional: a caller that has none (a test fixture,
   * or a surface that never resolves authority) leaves founder and operator
   * unresolved, which is the permissive fallback rather than a gate.
   */
  creates?: readonly CodingSessionUmbrellaCreateObservation[];
  isLoading: boolean;
  errorMessage: string | null;
  authorityErrorMessage: string | null;
  rejectedAuthorCount: number;
  invalidSignatureCount: number;
};

export type GlobalCodingSessionCatalogRecord = {
  channelId: string;
  session: CodingSessionCatalogRecord;
};

export type GlobalCodingSessionCatalogSnapshot = {
  entries: GlobalCodingSessionCatalogRecord[];
  isLoading: boolean;
  errorMessage: string | null;
  authorityErrorMessage: string | null;
};

export type CodingSessionWorkspaceStatus =
  | { kind: "working"; label: "Working" }
  | { kind: "idle"; label: "Idle" }
  | { kind: "ended"; label: "Ended" }
  | { kind: "unknown"; label: "Status unknown" };
