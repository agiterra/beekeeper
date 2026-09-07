import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/state/coding_sessions_state.dart'
    show CodingSessionCommands, CodingSessionPendingTurns, pendingTurnsProvider;
import 'package:buzz/features/coding_sessions/ui/observer_contract.dart';
import 'package:buzz/shared/relay/signed_event_relay.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart';
import 'package:nostr/nostr.dart' as nostr;

import '../../../helpers/recording_relay_session.dart';

const testChannelId = 'channel-1';
const testSignerPubkey =
    'aaaaaaaabbbbbbbbccccccccddddddddeeeeeeeeffffffff0000000011111111';
const testOperatorPubkey =
    '11111111222222223333333344444444555555556666666677777777dddddddd';

/// A binding that hands the pages a fixed snapshot and counts refreshes.
///
/// Publishing goes through a real [CodingSessionCommands] over a
/// [RecordingRelaySessionNotifier], signed with a throwaway key, so a test
/// can assert the exact event a control produced. [signerPubkey] is what the
/// pages *believe* this device's key is — it defaults to none, which keeps
/// the composer hidden; pass [testSignerPubkey] to read as the founder of
/// [testUmbrella].
class FakeObserverBinding implements CodingSessionObserverBinding {
  FakeObserverBinding(
    this.snapshot, {
    String? signerPubkey,
    Set<String> steerAccepted = const {},
    List<Object> publishResults = const [],
    Map<String, CodingSessionObserverSnapshot> byChannel = const {},
  }) : _signerPubkey = signerPubkey,
       steerAccepted = {...steerAccepted},
       byChannel = {...byChannel},
       relay = RecordingRelaySessionNotifier(publishResults: publishResults);

  /// What the pages read; reassign and pump to simulate a new relay read.
  CodingSessionObserverSnapshot snapshot;

  /// Per-channel snapshots, for pages that read more than one channel; a
  /// channel not listed here reads [snapshot].
  final Map<String, CodingSessionObserverSnapshot> byChannel;
  final String? _signerPubkey;
  final Set<String> steerAccepted;

  /// Every event the pages published, in order.
  final RecordingRelaySessionNotifier relay;
  final String _nsec = nostr.Keys.generate().nsec;
  int refreshCount = 0;

  @override
  CodingSessionObserverSnapshot watch(WidgetRef ref, String channelId) =>
      byChannel[channelId] ?? snapshot;

  @override
  Future<void> refresh(WidgetRef ref, String channelId) async {
    refreshCount++;
  }

  @override
  String? signerPubkey(WidgetRef ref) => _signerPubkey;

  @override
  CodingSessionCommands commands(WidgetRef ref, String channelId) =>
      CodingSessionCommands(
        channelId: channelId,
        relay: SignedEventRelay(session: relay, nsec: _nsec),
        isDeliveryValid: () => true,
        pending: ref.read(pendingTurnsProvider.notifier),
      );

  @override
  CodingSessionPendingTurns watchPendingTurns(WidgetRef ref) =>
      ref.watch(pendingTurnsProvider);

  @override
  void forgetPendingTurn(WidgetRef ref, String key) =>
      ref.read(pendingTurnsProvider.notifier).forget(key);

  @override
  Set<String> watchSteerAccepted(WidgetRef ref) => steerAccepted;

  @override
  void markSteerAccepted(WidgetRef ref, String sessionKey) {
    steerAccepted.add(sessionKey);
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
  Map<String, List<CodingSessionReceipt>> turnReceiptsByCommandId = const {},
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
    turnReceiptsByCommandId: turnReceiptsByCommandId,
  );
}

/// A turn-stage receipt answering [commandId] for [target].
CodingSessionReceipt testTurnReceipt({
  required String commandId,
  required CodingSessionReceiptStatus status,
  CodingSessionTarget? target,
  String? code,
  String? message,
  int createdAt = 2000,
  String? eventId,
}) => CodingSessionReceipt(
  ref: testRef(
    eventId: eventId ?? 'receipt-$commandId-${status.wire}',
    createdAt: createdAt,
  ),
  commandId: commandId,
  status: status,
  session: target ?? testTarget(),
  error: code == null
      ? null
      : CodingSessionReceiptError(code: code, message: message ?? ''),
  turnId: status == CodingSessionReceiptStatus.turnStarted ? 'turn-x' : null,
);
