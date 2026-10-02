import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/artifact_pin_fold.dart';

/// NIP-AR: the relay refuses a `created_at` more than this far from its
/// clock, and a peer may legally stamp this far in the past.
const artifactPinClockSkew = Duration(seconds: 900);
const artifactPinHistoryPageLimit = 500;
const artifactPinHistoryMaxPages = 10;
const artifactPinDisconnectedError =
    'Not connected to the community relay; showing what was last read.';

/// What one project's pin read reports.
@immutable
class ArtifactPinsRead {
  final ProjectArtifactPinDigest digest;
  final bool truncated;
  final bool loading;
  final String? error;
  final bool hasRead;

  const ArtifactPinsRead({
    required this.digest,
    required this.truncated,
    required this.loading,
    required this.error,
    required this.hasRead,
  });
}

/// Reads every kind 44251 op for one project and keeps the fold live — the
/// draft notifier's shape (`agents_repo_drafts_provider.dart`): the live
/// subscription first, then history newest-first, one id-keyed store, one
/// fold per microtask.
class ArtifactPinsNotifier extends Notifier<ArtifactPinsRead> {
  ArtifactPinsNotifier(this.key);

  /// `(address, repo)`: the project coordinate and the repository the fold
  /// is asked about.
  final ({String address, String repo}) key;

  final Map<String, NostrEvent> _store = {};
  final Map<String, int> _latestByTarget = {};
  final List<void Function()> _unsubscribes = [];
  int _epoch = 0;
  bool _disposed = false;
  bool _foldScheduled = false;
  bool _truncated = false;
  bool _loading = false;
  bool _hasRead = false;
  String? _error;
  ProjectArtifactPinDigest? _digest;

  @override
  ArtifactPinsRead build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });
    if (status != SessionStatus.connected) {
      final connecting = status == SessionStatus.connecting;
      _loading = connecting;
      _error = connecting ? null : artifactPinDisconnectedError;
      return _read();
    }
    Future.microtask(_start);
    _loading = true;
    _error = null;
    return _read();
  }

  ArtifactPinsRead _read() => ArtifactPinsRead(
    digest: _digest ?? ProjectArtifactPinDigest.empty(key.address, key.repo),
    truncated: _truncated,
    loading: _loading,
    error: _error,
    hasRead: _hasRead,
  );

  /// Drop everything read so far and read the project again.
  Future<void> refresh() async {
    if (ref.read(relaySessionProvider).status != SessionStatus.connected) {
      return;
    }
    _teardown();
    _store.clear();
    _latestByTarget.clear();
    _truncated = false;
    _digest = null;
    _loading = true;
    _error = null;
    _emit();
    await _start();
  }

  /// The row for [target] as the current read folded it, or `null`.
  PinRow? rowOf(String target) {
    for (final row
        in (_digest ?? stateOrNull?.digest)?.pins ?? const <PinRow>[]) {
      if (row.target == target) return row;
    }
    return null;
  }

  /// The greatest `created_at` among the ops seen on [path], or `null`.
  int? latestSeenFor(String target) => _latestByTarget[target];

  /// Add an op this device just published, so the fold shows it before the
  /// relay echoes it back.
  void addLocal(NostrEvent event) {
    if (_add(event)) _scheduleFold();
  }

  /// Forget an op this device withdrew.
  void removeLocal(String id) {
    if (_store.remove(id) != null) _scheduleFold();
  }

  Future<void> _start() async {
    _teardown();
    final epoch = ++_epoch;
    if (_disposed) return;
    final session = ref.read(relaySessionProvider.notifier);
    String? failure;
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    try {
      final unsubscribe = await session.subscribe(
        NostrFilters.artifactPinOpsLive(
          key.address,
          now - artifactPinClockSkew.inSeconds,
        ),
        _onLiveEvent,
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _unsubscribes.add(unsubscribe);
    } catch (error) {
      if (_stale(epoch)) return;
      failure = 'Pin subscription failed: $error';
      debugPrint('[ArtifactPins] live subscribe failed: $error');
    }
    try {
      await _walkHistory(session, epoch);
      if (_stale(epoch)) return;
    } catch (error) {
      if (_stale(epoch)) return;
      failure ??= 'Pin history read failed: $error';
      debugPrint('[ArtifactPins] history failed: $error');
    }
    _digest = _fold();
    _loading = false;
    _error = failure;
    _hasRead = true;
    _emit();
  }

  Future<void> _walkHistory(RelaySessionNotifier session, int epoch) async {
    int? until;
    for (var page = 0; page < artifactPinHistoryMaxPages; page++) {
      final events = await session.query(
        NostrFilters.artifactPinOps(
          key.address,
          limit: artifactPinHistoryPageLimit,
          until: until,
        ),
      );
      if (_stale(epoch)) return;
      var added = 0;
      int? oldest;
      for (final event in events) {
        if (_add(event)) added++;
        if (oldest == null || event.createdAt < oldest) {
          oldest = event.createdAt;
        }
      }
      if (events.length < artifactPinHistoryPageLimit) return;
      if (added == 0 || oldest == null) {
        _truncated = true;
        return;
      }
      until = oldest;
    }
    _truncated = true;
  }

  bool _add(NostrEvent event) {
    if (event.kind != EventKind.projectArtifactPinOp) return false;
    if (_store.containsKey(event.id)) return false;
    _store[event.id] = event;
    var named = 0;
    for (final tag in event.tags) {
      if (tag.length < 2 || tag[0] != 'ar-target') continue;
      named++;
      final latest = _latestByTarget[tag[1]];
      if (latest == null || event.createdAt > latest) {
        _latestByTarget[tag[1]] = event.createdAt;
      }
    }
    // Every pin op names exactly one target, so there is no record-shaped
    // op to account for the way the draft log has.
    if (named == 0) return true;
    return true;
  }

  void _onLiveEvent(NostrEvent event) {
    if (_disposed) return;
    if (!_add(event)) return;
    _scheduleFold();
  }

  void _scheduleFold() {
    if (_foldScheduled) return;
    _foldScheduled = true;
    scheduleMicrotask(() {
      _foldScheduled = false;
      if (_disposed) return;
      _digest = _fold();
      _emit();
    });
  }

  ProjectArtifactPinDigest _fold() =>
      foldProjectArtifactPins(key.address, key.repo, _store.values);

  void _emit() {
    if (_disposed) return;
    state = _read();
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    _epoch += 1;
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }
}

/// One project's live pin read, by its coordinate and repository.
final artifactPinsProvider =
    NotifierProvider.family<
      ArtifactPinsNotifier,
      ArtifactPinsRead,
      ({String address, String repo})
    >(ArtifactPinsNotifier.new);
