import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";

export type CodingSessionStatus =
  | "starting"
  | "idle"
  | "running"
  | "waiting_for_input"
  | "completed"
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
  provider: string | null;
  runtime: string | null;
  model: string | null;
  capabilities: CodingSessionCapabilities | null;
};

export type CodingSessionCatalogSnapshot = {
  channelId: string | null;
  entries: CodingSessionCatalogRecord[];
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
  | { kind: "unknown"; label: "Status unknown" };
