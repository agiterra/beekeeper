/**
 * The coding-session observer domain: everything the browser needs to read a
 * session, and nothing that could write one.
 *
 * This is the whole public surface lanes W2 (relay reads / state) and W3 (UI)
 * build against. Everything here is pure: no React, no fetch, no WebSocket.
 */
export {
  type CodingSessionObserverFacts,
  CodingSessionObserverStore,
  codingSessionTargetFactKey,
  generationExecutionLabel,
  MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
} from "./catalog.ts";
export {
  CODING_SESSION_HISTORY_LIMIT,
  CODING_SESSION_ROSTER_LIMIT,
  type CodingSessionFilter,
  codingSessionClosuresFilter,
  codingSessionCreatesFilters,
  codingSessionFactsFilter,
  codingSessionFactsLiveFilter,
  codingSessionGoalsFilter,
  codingSessionHistoryFilters,
  codingSessionLeasesFilter,
  codingSessionNamesFilter,
  codingSessionRosterFilters,
  isTruncatedHistoryPage,
} from "./filters.ts";
export {
  type BuzzCodingSessionMetadataV1,
  type CodingSessionLifecycleReceipt,
  isCodingSessionTurnReceiptStatus,
  parseBuzzCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./ingressPayloads.ts";
export {
  buildCodingSessionExecutionKey,
  buildCodingSessionGenerationId,
  buildCodingSessionTargetKey,
  buildCodingSessionTranscriptScopeKey,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  codingSessionTranscriptSemanticKey,
  encodeStructuredKey,
} from "./keys.ts";
export {
  formatCodingSessionExecutionLabel,
  formatCodingSessionModelSummary,
  formatCodingSessionRuntimeLabel,
  splitCodingSessionModelId,
} from "./labels.ts";
export {
  CODING_SESSION_LEASE_TTL_SECONDS,
  type CodingSessionLease,
  isCodingSessionProviderReachable,
  parseCodingSessionLease,
  resolveCodingSessionReachability,
} from "./lease.ts";
export {
  type CodingSessionLifecycleCommand,
  parseCodingSessionLifecycleCommand,
} from "./lifecycleCommand.ts";
export {
  type CodingSessionClosure,
  type CodingSessionGenesis,
  type CodingSessionGoal,
  type CodingSessionName,
  foldNewestByKey,
  parseCodingSessionClosure,
  parseCodingSessionGenesis,
  parseCodingSessionGoal,
  parseCodingSessionName,
} from "./sessionRecords.ts";
export {
  type BuzzCodingSessionTranscriptV1,
  parseBuzzCodingSessionTranscript,
} from "./transcriptEnvelope.ts";
export {
  type CodingSessionTranscriptBlock,
  type CodingSessionTranscriptEnvelope,
  orderCodingSessionTranscriptBlocks,
  projectCodingSessionTranscript,
} from "./transcriptProjection.ts";
export {
  classifyCodingSessionEvent,
  isSignatureVerified,
  type TrustedIngressClassification,
} from "./trust.ts";
export type {
  CodingSessionAuthoritySource,
  CodingSessionCapabilities,
  CodingSessionExecution,
  CodingSessionFounderResolution,
  CodingSessionGenerationRecord,
  CodingSessionReachability,
  CodingSessionReachabilityReport,
  CodingSessionStatus,
  CodingSessionTarget,
  CodingSessionUmbrella,
  ObservedEvent,
  ProjectedTranscriptItem,
} from "./types.ts";
export {
  type BuildSnapshotOptions,
  buildCodingSessionObserverSnapshot,
  codingSessionFounderLabel,
  codingSessionReachabilityLine,
  codingSessionStatusChipLabel,
  type CodingSessionObserverSnapshot,
  foldUmbrellaStatus,
  groupCodingSessionGenerations,
} from "./umbrella.ts";
