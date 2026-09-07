import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../domain/coding_sessions_domain.dart';

/// Every turn this device has sent and not yet seen answered, by store key.
///
/// Lives above the pages: a composer that remounts while the provider still
/// holds its turn has to find the row (and the draft riding it) exactly
/// where it left it.
@immutable
class CodingSessionPendingTurns {
  final Map<String, CodingSessionPendingTurn> byKey;

  const CodingSessionPendingTurns(this.byKey);

  static const empty = CodingSessionPendingTurns({});

  /// The rows for one channel, oldest first.
  List<CodingSessionPendingTurn> forChannel(String channelId) => [
    for (final turn in byKey.values)
      if (turn.channelId == channelId) turn,
  ]..sort((left, right) => left.recordedAt.compareTo(right.recordedAt));

  /// The rows aimed at [executionKey] in [channelId], oldest first.
  List<CodingSessionPendingTurn> forExecution(
    String channelId,
    String executionKey,
  ) => [
    for (final turn in forChannel(channelId))
      if (turn.executionKey == executionKey) turn,
  ];
}

/// The store behind [pendingTurnsProvider].
class PendingTurnsNotifier extends Notifier<CodingSessionPendingTurns> {
  @override
  CodingSessionPendingTurns build() => CodingSessionPendingTurns.empty;

  /// Record a turn *before* its publish is awaited.
  void record(CodingSessionPendingTurn turn) {
    state = CodingSessionPendingTurns({...state.byKey, turn.key: turn});
  }

  /// The relay acknowledged the turn.
  void markPublished(String key) {
    final turn = state.byKey[key];
    if (turn == null) return;
    state = CodingSessionPendingTurns({
      ...state.byKey,
      key: turn.markPublished(),
    });
  }

  /// Drop a row: it was settled, or its publish never left this device.
  void forget(String key) {
    if (!state.byKey.containsKey(key)) return;
    final next = {...state.byKey}..remove(key);
    state = CodingSessionPendingTurns(next);
  }
}

/// Turns awaiting a provider's answer, across every channel.
final pendingTurnsProvider =
    NotifierProvider<PendingTurnsNotifier, CodingSessionPendingTurns>(
      PendingTurnsNotifier.new,
    );

/// Session keys the relay has accepted a steering command from this device
/// on, this app run.
///
/// The relay reads 44228 operator grants; this device does not. A relay `OK`
/// on a 44220 is therefore the one proof an operator's phone has that it holds
/// a grant, and it is remembered only for as long as the app runs — a grant
/// can be revoked, and a persisted memory of one would outlive it.
class SteerAcceptedNotifier extends Notifier<Set<String>> {
  @override
  Set<String> build() => const {};

  /// The relay accepted a command on [sessionKey].
  void accept(String sessionKey) {
    if (state.contains(sessionKey)) return;
    state = {...state, sessionKey};
  }
}

/// Session keys with a relay-accepted command from this device, this run.
final steerAcceptedProvider =
    NotifierProvider<SteerAcceptedNotifier, Set<String>>(
      SteerAcceptedNotifier.new,
    );
