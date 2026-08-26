import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../domain/coding_sessions_domain.dart';
import '../state/coding_sessions_state.dart' as state;

/// How the channel observer's relay read is doing right now.
///
/// The distinction between [idle]/[connecting] and [error] is load-bearing:
/// a read that has not happened is not a read that came back empty, and the
/// UI must not present the first as the second.
enum CodingSessionObserverConnection {
  /// No read has been started for this channel yet.
  idle,

  /// A read is in flight and nothing has arrived.
  connecting,

  /// A live subscription is up; whatever is on screen is what the relay sent.
  open,

  /// The read failed; see [CodingSessionObserverSnapshot.lastError].
  error,
}

/// Everything the read side of the observer hands the UI for one channel.
///
/// This is the seam between lane M2 (which owns the relay subscription and the
/// Riverpod notifier) and lane M3 (which owns the pages). The UI never decodes
/// an event, never publishes one, and never invents a field that is not here.
@immutable
class CodingSessionObserverSnapshot {
  /// The channel this snapshot describes.
  final String channelId;

  /// Umbrella sessions, most recently active first.
  final List<CodingSessionUmbrella> sessions;

  /// Every execution across those sessions, most recently active first.
  final List<CodingSessionExecution> executions;

  /// Transcript blocks keyed by [CodingSessionExecution.targetKey].
  ///
  /// Blocks, not items: two providers' `eventSeq` counters are unrelated, so
  /// the UI interleaves whole streams and never merges rows across them.
  final Map<String, List<CodingSessionTranscriptBlock>>
  transcriptBlocksByExecution;

  /// What the trust gate refused, so the UI can say so out loud.
  final CodingSessionReadCounts counts;

  /// True when a history page came back full at the 1000-event limit.
  final bool truncatedAt1000;

  final CodingSessionObserverConnection connection;

  /// Whether this device verified the signatures behind these facts.
  ///
  /// `null` means "not established yet" and is rendered as nothing; `false`
  /// means the device could not verify and *must* be disclosed.
  final bool? signaturesVerified;

  /// The message behind [CodingSessionObserverConnection.error].
  final String? lastError;

  /// Reachability per [CodingSessionUmbrella.key].
  ///
  /// A missing entry reads [CodingSessionReachability.unknown], never
  /// "nobody answering" — see D8.
  final Map<String, CodingSessionReachability> reachabilityBySession;

  const CodingSessionObserverSnapshot({
    required this.channelId,
    this.sessions = const [],
    this.executions = const [],
    this.transcriptBlocksByExecution = const {},
    this.counts = const CodingSessionReadCounts(),
    this.truncatedAt1000 = false,
    this.connection = CodingSessionObserverConnection.idle,
    this.signaturesVerified,
    this.lastError,
    this.reachabilityBySession = const {},
  });

  /// True while the first read for this channel is still outstanding.
  ///
  /// [CodingSessionObserverConnection.idle] is deliberately excluded: the
  /// notifier reports idle when the relay session is *not connected*, and
  /// nothing is being read then. Presenting that as loading left an offline
  /// device under a "Reading coding sessions" spinner forever — see
  /// [isDisconnected].
  bool get isLoadingFirstRead =>
      sessions.isEmpty &&
      connection == CodingSessionObserverConnection.connecting;

  /// True when this community's relay session is down and nothing was read.
  ///
  /// With sessions in hand the pages keep showing them (stale facts beat a
  /// blank page); with none, the honest statement is that the device is not
  /// connected, not that the read is still coming.
  bool get isDisconnected =>
      sessions.isEmpty && connection == CodingSessionObserverConnection.idle;

  /// True when the read failed and there is nothing to show instead.
  bool get hasBlockingError =>
      connection == CodingSessionObserverConnection.error && sessions.isEmpty;

  /// The reachability verdict for [sessionKey].
  CodingSessionReachability reachabilityFor(String sessionKey) =>
      reachabilityBySession[sessionKey] ?? CodingSessionReachability.unknown;

  /// The transcript of [session], as blocks ordered by their first row.
  ///
  /// Ordering mirrors `projectCodingSessionTranscript`: earliest first item
  /// wins, ties broken by the block's own stream key, so every reader sees the
  /// same sequence of blocks.
  List<CodingSessionTranscriptBlock> blocksFor(CodingSessionUmbrella session) {
    final blocks = <CodingSessionTranscriptBlock>[];
    final seen = <String>{};
    for (final execution in session.executions) {
      for (final block
          in transcriptBlocksByExecution[execution.targetKey] ??
              const <CodingSessionTranscriptBlock>[]) {
        if (seen.add(block.key)) blocks.add(block);
      }
    }
    blocks.sort((left, right) {
      if (left.items.isEmpty || right.items.isEmpty) {
        return left.key.compareTo(right.key);
      }
      final byStart = left.items.first.timestamp.compareTo(
        right.items.first.timestamp,
      );
      return byStart != 0 ? byStart : left.key.compareTo(right.key);
    });
    return List.unmodifiable(blocks);
  }

  /// The session matching [sessionKey], accepting an umbrella key, a
  /// `sessionRef`, an execution key, or a generation's target key.
  CodingSessionUmbrella? sessionFor(String sessionKey) {
    for (final session in sessions) {
      if (session.key == sessionKey) return session;
    }
    for (final session in sessions) {
      if (session.sessionRef == sessionKey) return session;
    }
    for (final session in sessions) {
      for (final execution in session.executions) {
        if (execution.executionKey == sessionKey ||
            execution.targetKey == sessionKey) {
          return session;
        }
      }
    }
    return null;
  }
}

/// What the pages say when this community's relay session is not connected.
const codingSessionDisconnectedLabel = 'Not connected to this community';

/// The line under [codingSessionDisconnectedLabel].
const codingSessionDisconnectedDetail =
    'Nothing is being read while the connection is down. Retry once the '
    'community is back.';

/// The read-side binding the pages consume.
///
/// Lane M2 ships `codingSessionChannelObserverProvider` (a family keyed by
/// channel id) plus a notifier with `refresh()`; the integrator binds an
/// implementation of this interface that forwards to them. Keeping the pages
/// behind this interface is what lets the UI and the provider be built and
/// tested independently.
abstract interface class CodingSessionObserverBinding {
  /// Watch the observer for [channelId], rebuilding on every change.
  ///
  /// Implementations call `ref.watch(codingSessionChannelObserverProvider(
  /// channelId))` so the widget's own `ref` registers the dependency.
  CodingSessionObserverSnapshot watch(WidgetRef ref, String channelId);

  /// Re-run the history read for [channelId].
  Future<void> refresh(WidgetRef ref, String channelId);
}

/// The binding used when nothing has been wired in.
///
/// It reports an error rather than pretending the channel has no sessions:
/// an unwired build has read nothing, and saying "no coding sessions" would
/// be a claim it cannot support.
final class UnboundCodingSessionObserverBinding
    implements CodingSessionObserverBinding {
  const UnboundCodingSessionObserverBinding();

  /// What the UI shows when no observer is bound.
  static const message =
      'The coding-session observer is not wired into this '
      'build, so nothing has been read from the relay.';

  @override
  CodingSessionObserverSnapshot watch(WidgetRef ref, String channelId) =>
      CodingSessionObserverSnapshot(
        channelId: channelId,
        connection: CodingSessionObserverConnection.error,
        lastError: message,
      );

  @override
  Future<void> refresh(WidgetRef ref, String channelId) async {}
}

/// The binding that forwards to the real relay observer.
///
/// The state layer keeps its own snapshot type (it carries the folded view and
/// the lease-read flag the pages have no use for); this adapter projects it
/// onto the contract above. Reachability is time-dependent, so it is resolved
/// against the clock at read time rather than stored.
final class RelayCodingSessionObserverBinding
    implements CodingSessionObserverBinding {
  const RelayCodingSessionObserverBinding();

  @override
  CodingSessionObserverSnapshot watch(WidgetRef ref, String channelId) =>
      _project(
        ref.watch(state.codingSessionChannelObserverProvider(channelId)),
      );

  @override
  Future<void> refresh(WidgetRef ref, String channelId) => ref
      .read(state.codingSessionChannelObserverProvider(channelId).notifier)
      .refresh();

  static CodingSessionObserverSnapshot _project(
    state.CodingSessionObserverSnapshot read,
  ) {
    final now = DateTime.now();
    return CodingSessionObserverSnapshot(
      channelId: read.channelId,
      sessions: read.sessions,
      executions: read.executions,
      transcriptBlocksByExecution: read.transcriptBlocksByExecution,
      counts: read.counts,
      truncatedAt1000: read.truncatedAt1000,
      connection: _connection(read.connection),
      signaturesVerified: read.signaturesVerified,
      lastError: read.lastError,
      reachabilityBySession: {
        for (final session in read.sessions)
          session.key: read.reachabilityFor(session, now: now),
      },
    );
  }

  static CodingSessionObserverConnection _connection(
    state.CodingSessionObserverConnection connection,
  ) => switch (connection) {
    state.CodingSessionObserverConnection.idle =>
      CodingSessionObserverConnection.idle,
    state.CodingSessionObserverConnection.connecting =>
      CodingSessionObserverConnection.connecting,
    state.CodingSessionObserverConnection.open =>
      CodingSessionObserverConnection.open,
    state.CodingSessionObserverConnection.error =>
      CodingSessionObserverConnection.error,
  };
}

/// The binding the coding-session pages read.
///
/// Overridden in tests with a fake. The default is the real relay observer;
/// [UnboundCodingSessionObserverBinding] remains for builds that deliberately
/// leave the observer out, and reports that rather than claiming emptiness.
final codingSessionObserverBindingProvider =
    Provider<CodingSessionObserverBinding>(
      (ref) => const RelayCodingSessionObserverBinding(),
    );
