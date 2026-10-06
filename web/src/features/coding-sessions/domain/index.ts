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
  codingSessionGeneratedTitlesFilter,
  codingSessionGoalsFilter,
  codingSessionHistoryFilters,
  codingSessionLeasesFilter,
  codingSessionNamesFilter,
  codingSessionRosterFilters,
  isTruncatedHistoryPage,
} from "./filters.ts";
export {
  type BeekeeperCodingSessionMetadataV1,
  type CodingSessionLifecycleReceipt,
  isCodingSessionTurnReceiptStatus,
  parseBeekeeperCodingSessionMetadata,
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
  type CodingSessionGeneratedTitle,
  type CodingSessionGenesis,
  type CodingSessionGoal,
  type CodingSessionName,
  foldNewestByKey,
  parseCodingSessionClosure,
  parseCodingSessionGeneratedTitle,
  parseCodingSessionGenesis,
  parseCodingSessionGoal,
  parseCodingSessionName,
} from "./sessionRecords.ts";
export {
  type CodingSessionTitleStandingGap,
  codingSessionTitleStandingFilters,
  codingSessionTitleStandingGaps,
  isTitleStandingPageTruncated,
  MAX_TITLE_STANDING_GAPS_PER_READ,
} from "./titleStanding.ts";
export {
  parseCodingSessionTitleParts,
  resolveSessionDisplayName,
  type SessionDisplayName,
  type SessionDisplayNameDiagnostics,
  type SessionDisplayNameOrigin,
  type SessionDisplayNameScope,
  type SessionExecutionAuthority,
  type SessionNameRecord,
  UNTITLED_SESSION_NAME,
} from "./sessionTitle.ts";
export {
  type BeekeeperCodingSessionTranscriptV1,
  parseBeekeeperCodingSessionTranscript,
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
  codingSessionTitleOriginDetail,
  type CodingSessionObserverSnapshot,
  foldUmbrellaStatus,
  groupCodingSessionGenerations,
} from "./umbrella.ts";
