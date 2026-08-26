import 'package:flutter/foundation.dart';

import '../domain/coding_sessions_domain.dart';

/// The observer's relay connection, as the page may state it.
///
/// Kept explicit rather than inferred from "is the list empty": an empty list
/// while connecting and an empty list after a clean read are different claims,
/// and the page must not present the first as the second.
enum CodingSessionObserverConnection {
  /// Nothing started — no relay session to read from yet.
  idle,

  /// History is being fetched or live subscriptions are being opened.
  connecting,

  /// History has landed and the live subscriptions are open.
  open,

  /// The last attempt failed; [CodingSessionObserverSnapshot.lastError] says
  /// how.
  error,
}

/// One immutable read of a channel's coding sessions.
///
/// Everything the observer knows at one instant, including what it had to
/// throw away. A field is never invented to fill a gap: [signaturesVerified]
/// is `null` until a read has actually happened, and [truncatedAt1000] is the
/// observer admitting a history page came back full.
@immutable
class CodingSessionObserverSnapshot {
  final String channelId;

  /// The folded read, or `null` before the first history fetch returns.
  final CodingSessionChannelView? view;

  /// Umbrella sessions, most recently active first.
  final List<CodingSessionUmbrella> sessions;

  /// Every generation across every session, most recently active first.
  final List<CodingSessionExecution> executions;

  /// Transcript blocks keyed by [CodingSessionExecution.targetKey].
  ///
  /// A block is one `(signer, target)` stream, so an execution has at most
  /// one; the list shape keeps the caller honest about that rather than
  /// pretending a missing stream is an empty transcript.
  final Map<String, List<CodingSessionTranscriptBlock>>
  transcriptBlocksByExecution;

  /// What the read refused: malformed, wrong-signer, conflicting, duplicate.
  final CodingSessionReadCounts counts;

  /// True when a history page came back full, so older facts exist unread.
  final bool truncatedAt1000;

  final CodingSessionObserverConnection connection;

  /// `null` before the first read; `false` when this device could not verify
  /// signatures at all, which the page must disclose.
  final bool? signaturesVerified;

  /// The last failure message, or `null`.
  final String? lastError;

  /// True once a lease query has returned. False makes every reachability
  /// read [CodingSessionReachabilityKind.unknown].
  final bool leasesRead;

  const CodingSessionObserverSnapshot({
    required this.channelId,
    required this.view,
    required this.sessions,
    required this.executions,
    required this.transcriptBlocksByExecution,
    required this.counts,
    required this.truncatedAt1000,
    required this.connection,
    required this.signaturesVerified,
    required this.lastError,
    required this.leasesRead,
  });

  /// The state before anything has been read.
  const CodingSessionObserverSnapshot.initial(
    this.channelId, {
    this.connection = CodingSessionObserverConnection.idle,
    this.lastError,
  }) : view = null,
       sessions = const [],
       executions = const [],
       transcriptBlocksByExecution = const {},
       counts = const CodingSessionReadCounts(),
       truncatedAt1000 = false,
       signaturesVerified = null,
       leasesRead = false;

  /// Project a folded [view] into a snapshot.
  factory CodingSessionObserverSnapshot.fromView(
    CodingSessionChannelView view, {
    required CodingSessionObserverConnection connection,
    String? lastError,
  }) {
    final executions =
        [for (final session in view.sessions) ...session.executions]
          ..sort((left, right) {
            final byActivity = right.lastActivityAt.compareTo(
              left.lastActivityAt,
            );
            return byActivity != 0
                ? byActivity
                : left.targetKey.compareTo(right.targetKey);
          });
    final blocks = <String, List<CodingSessionTranscriptBlock>>{};
    for (final session in view.sessions) {
      for (final block in view.transcriptFor(session)) {
        blocks.putIfAbsent(block.target.key, () => []).add(block);
      }
    }
    return CodingSessionObserverSnapshot(
      channelId: view.channelId,
      view: view,
      sessions: List.unmodifiable(view.sessions),
      executions: List.unmodifiable(executions),
      transcriptBlocksByExecution: Map.unmodifiable({
        for (final entry in blocks.entries)
          entry.key: List<CodingSessionTranscriptBlock>.unmodifiable(
            entry.value,
          ),
      }),
      counts: view.counts,
      truncatedAt1000: view.historyTruncated,
      connection: connection,
      signaturesVerified: view.signaturesVerified,
      lastError: lastError,
      leasesRead: view.leasesRead,
    );
  }

  /// The same read with a different [connection] or [lastError].
  CodingSessionObserverSnapshot copyWith({
    CodingSessionObserverConnection? connection,
    String? lastError,
    bool clearError = false,
  }) => CodingSessionObserverSnapshot(
    channelId: channelId,
    view: view,
    sessions: sessions,
    executions: executions,
    transcriptBlocksByExecution: transcriptBlocksByExecution,
    counts: counts,
    truncatedAt1000: truncatedAt1000,
    connection: connection ?? this.connection,
    signaturesVerified: signaturesVerified,
    lastError: clearError ? null : (lastError ?? this.lastError),
    leasesRead: leasesRead,
  );

  /// True once a read has produced a view.
  bool get hasRead => view != null;

  /// The session with [key], or `null`.
  CodingSessionUmbrella? sessionByKey(String key) => view?.sessionByKey(key);

  /// The session claiming [sessionRef], or `null`.
  CodingSessionUmbrella? sessionByRef(String sessionRef) =>
      view?.sessionByRef(sessionRef);

  /// The transcript of [session] as interleaved per-execution blocks.
  List<CodingSessionTranscriptBlock> transcriptFor(
    CodingSessionUmbrella session,
  ) => view?.transcriptFor(session) ?? const [];

  /// Whether a provider is answering for [session]'s current generation.
  ///
  /// Unknown before any read, which the page must never render as "nobody
  /// answering".
  CodingSessionReachability reachabilityFor(
    CodingSessionUmbrella session, {
    required DateTime now,
  }) =>
      view?.reachabilityFor(session, now: now) ??
      CodingSessionReachability.unknown;
}
