import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/ui/observer_contract.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart';

const testChannelId = 'channel-1';
const testSignerPubkey =
    'aaaaaaaabbbbbbbbccccccccddddddddeeeeeeeeffffffff0000000011111111';
const testOperatorPubkey =
    '11111111222222223333333344444444555555556666666677777777dddddddd';

/// A binding that hands the pages a fixed snapshot and counts refreshes.
class FakeObserverBinding implements CodingSessionObserverBinding {
  FakeObserverBinding(this.snapshot);

  final CodingSessionObserverSnapshot snapshot;
  int refreshCount = 0;

  @override
  CodingSessionObserverSnapshot watch(WidgetRef ref, String channelId) =>
      snapshot;

  @override
  Future<void> refresh(WidgetRef ref, String channelId) async {
    refreshCount++;
  }
}

/// Overrides the pages' binding with [binding].
Override fakeObserverOverride(FakeObserverBinding binding) =>
    codingSessionObserverBindingProvider.overrideWithValue(binding);

CodingSessionTarget testTarget({
  String driver = 'provider-a',
  String instanceId = 'instance-1',
  String sessionId = 'session-1',
  int generation = 1,
}) => CodingSessionTarget(
  driver: driver,
  instanceId: instanceId,
  sessionId: sessionId,
  generation: generation,
);

CodingSessionEventRef testRef({
  String eventId = 'event-1',
  String signerPubkey = testSignerPubkey,
  int createdAt = 1000,
}) => CodingSessionEventRef(
  channelId: testChannelId,
  eventId: eventId,
  signerPubkey: signerPubkey,
  createdAt: createdAt,
);

CodingSessionExecution testExecution({
  CodingSessionTarget? target,
  CodingSessionStatus status = CodingSessionStatus.running,
  String? sessionRef = 'umbrella-1',
  String runtime = 'claude-code',
  String model = 'sonnet',
  String? agentRef,
  bool authorityVerified = true,
  int lastActivityAt = 1000,
  int? statusAt,
  bool isCurrentGeneration = true,
}) {
  final resolved = target ?? testTarget();
  return CodingSessionExecution(
    channelId: testChannelId,
    target: resolved,
    authority: CodingSessionAuthority(
      pubkey: testSignerPubkey,
      verified: authorityVerified,
    ),
    status: status,
    statusAt: statusAt ?? lastActivityAt,
    metadata: CodingSessionMetadata(
      ref: testRef(createdAt: lastActivityAt),
      target: resolved,
      status: status,
      capabilities: const {},
      canonicalPayload: '{"fake":"$runtime"}',
      runtime: runtime,
      model: model,
      agentRef: agentRef,
      sessionRef: sessionRef,
    ),
    sessionRef: sessionRef,
    isCurrentGeneration: isCurrentGeneration,
    statusConflict: false,
    lastActivityAt: lastActivityAt,
    commandId: 'command-1',
  );
}

CodingSessionUmbrella testUmbrella({
  String key = 'umbrella-1',
  String? sessionRef = 'umbrella-1',
  List<CodingSessionExecution>? executions,
  CodingSessionFounder founder = const CodingSessionFounder(
    pubkey: testSignerPubkey,
    resolution: CodingSessionFounderResolution.genesis,
  ),
  String? name = 'Ship the observer',
  String? goal,
  bool closed = false,
  CodingSessionFoldedStatus status = const CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.working,
    status: CodingSessionStatus.running,
  ),
  int lastActivityAt = 1000,
}) => CodingSessionUmbrella(
  channelId: testChannelId,
  key: key,
  sessionRef: sessionRef,
  executions: executions ?? [testExecution()],
  founder: founder,
  name: name,
  goal: goal,
  closed: closed,
  status: status,
  lastActivityAt: lastActivityAt,
);

/// A transcript envelope, projected by the real domain projector in tests so
/// the UI is exercised against the shapes the wire actually produces.
CodingSessionTranscriptEnvelope testEnvelope({
  required int eventSeq,
  required Map<String, dynamic> item,
  String? turnId = 'turn-1',
  CodingSessionTarget? target,
  int createdAt = 1000,
}) => CodingSessionTranscriptEnvelope(
  ref: testRef(eventId: 'event-$eventSeq', createdAt: createdAt),
  target: target ?? testTarget(),
  eventSeq: eventSeq,
  timestamp: 1700000000000 + eventSeq,
  turnId: turnId,
  item: item,
);

/// The snapshot the session-page tests read.
CodingSessionObserverSnapshot testSnapshot({
  List<CodingSessionUmbrella>? sessions,
  List<CodingSessionTranscriptEnvelope> envelopes = const [],
  CodingSessionObserverConnection connection =
      CodingSessionObserverConnection.open,
  bool? signaturesVerified = true,
  bool truncatedAt1000 = false,
  Map<String, int> evictedByGeneration = const {},
  String? lastError,
  CodingSessionReadCounts counts = const CodingSessionReadCounts(),
  Map<String, CodingSessionReachability> reachabilityBySession = const {},
}) {
  final resolved = sessions ?? [testUmbrella()];
  final blocks = <String, List<CodingSessionTranscriptBlock>>{};
  if (envelopes.isNotEmpty) {
    final labels = {
      for (final session in resolved)
        for (final execution in session.executions)
          execution.targetKey: execution.label,
    };
    for (final block in projectCodingSessionTranscript(
      envelopes,
      labelsByTargetKey: labels,
    )) {
      blocks.putIfAbsent(block.target.key, () => []).add(block);
    }
  }
  return CodingSessionObserverSnapshot(
    channelId: testChannelId,
    sessions: resolved,
    executions: [for (final session in resolved) ...session.executions],
    transcriptBlocksByExecution: blocks,
    counts: counts,
    truncatedAt1000: truncatedAt1000,
    evictedByGeneration: evictedByGeneration,
    connection: connection,
    signaturesVerified: signaturesVerified,
    lastError: lastError,
    reachabilityBySession: reachabilityBySession,
  );
}
