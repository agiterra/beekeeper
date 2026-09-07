import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../domain/coding_sessions_domain.dart';

/// Every session this device asked a provider to create and has not yet
/// seen answered, by store key.
///
/// Lives above the pages for the same reason pending turns do: the sheet
/// that sent the create is gone by the time the provider answers, and the
/// project list that shows the row remounts as the person moves around.
@immutable
class CodingSessionPendingCreates {
  final Map<String, CodingSessionPendingCreate> byKey;

  const CodingSessionPendingCreates(this.byKey);

  static const empty = CodingSessionPendingCreates({});

  /// The rows for one channel, oldest first.
  List<CodingSessionPendingCreate> forChannel(String channelId) => [
    for (final row in byKey.values)
      if (row.channelId == channelId) row,
  ]..sort((left, right) => left.recordedAt.compareTo(right.recordedAt));
}

/// The store behind [pendingCreatesProvider].
class PendingCreatesNotifier extends Notifier<CodingSessionPendingCreates> {
  @override
  CodingSessionPendingCreates build() => CodingSessionPendingCreates.empty;

  /// Record a create *before* its publishes are awaited.
  void record(CodingSessionPendingCreate row) {
    state = CodingSessionPendingCreates({...state.byKey, row.key: row});
  }

  /// The genesis went out: the create can now name it.
  void markGenesis(String key, String genesisRef) =>
      _update(key, (row) => row.copyWith(genesisRef: genesisRef));

  /// The relay acknowledged the create.
  void markPublished(String key) =>
      _update(key, (row) => row.copyWith(published: true));

  /// Drop a row: it settled, or its publish never left this device.
  void forget(String key) {
    if (!state.byKey.containsKey(key)) return;
    final next = {...state.byKey}..remove(key);
    state = CodingSessionPendingCreates(next);
  }

  void _update(
    String key,
    CodingSessionPendingCreate Function(CodingSessionPendingCreate) change,
  ) {
    final row = state.byKey[key];
    if (row == null) return;
    state = CodingSessionPendingCreates({...state.byKey, key: change(row)});
  }
}

/// Creates awaiting a provider's answer, across every channel.
final pendingCreatesProvider =
    NotifierProvider<PendingCreatesNotifier, CodingSessionPendingCreates>(
      PendingCreatesNotifier.new,
    );
