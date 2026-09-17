import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/project_todo_fold.dart';
import '../domain/project_todo_op.dart';

/// NIP-TD: the relay refuses a `created_at` more than this far from its
/// clock, and a peer may legally stamp this far in the past — so the live
/// subscription starts this far back and the reader dedupes by id.
const projectTodoClockSkew = Duration(seconds: 900);

/// Ops per history page.
const projectTodoHistoryPageLimit = 500;

/// The most history pages one cold read walks before it stops and says so.
const projectTodoHistoryMaxPages = 10;

/// The error a read reports while the community socket is down.
const projectTodosDisconnectedError =
    'Not connected to the community relay; showing what was last read.';

/// What one project's to-do read reports.
@immutable
class ProjectTodosRead {
  /// The fold of every op read so far.
  final ProjectTodoDigest digest;

  /// `true` when the history walk stopped before it reached the oldest op:
  /// the page cap was hit, or a full page could not be advanced past. The
  /// digest then describes the ops that were read, not the whole list.
  final bool truncated;

  /// `true` until the cold read has settled (or failed).
  final bool loading;

  /// The last read or subscribe failure, verbatim, or `null`.
  final String? error;

  /// `true` once at least one history page has been folded.
  final bool hasRead;

  const ProjectTodosRead({
    required this.digest,
    required this.truncated,
    required this.loading,
    required this.error,
    required this.hasRead,
  });
}

/// Reads every kind 44248 op for one project coordinate and keeps the fold
/// live.
///
/// Order matters: the live subscription opens first (from `now - 900 s`, per
/// NIP-TD), then history is walked newest-first with `until` until a page
/// comes back short. Both feed one id-keyed store, so an op that arrives on
/// both paths is one op; a burst of live ops folds once per microtask.
class ProjectTodosNotifier extends Notifier<ProjectTodosRead> {
  ProjectTodosNotifier(this.address);

  /// The canonical `30621:<owner>:<dtag>` coordinate.
  final String address;

  final Map<String, NostrEvent> _store = {};

  /// Greatest `created_at` seen per target (`listId` for list ops,
  /// `listId/itemId` for item ops), for the writer's timestamp rule.
  final Map<String, int> _latestByTarget = {};
  final List<void Function()> _unsubscribes = [];
  int _epoch = 0;
  bool _disposed = false;
  bool _foldScheduled = false;
  bool _truncated = false;
  bool _loading = false;
  bool _hasRead = false;
  String? _error;
  ProjectTodoDigest? _digest;

  @override
  ProjectTodosRead build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });
    // `stateOrNull` is null on a rebuild (Riverpod 3.1 clears the element's
    // value before `build` runs), so what was read is kept in this
    // notifier's own fields, which survive the rebuild: the instance does.
    if (status != SessionStatus.connected) {
      // Keep whatever was read: stale lists over an honest "not connected"
      // beat a blank page that implies the project has no lists. A first
      // connect still in flight is a spinner, not a failure.
      final connecting = status == SessionStatus.connecting;
      _loading = connecting;
      _error = connecting ? null : projectTodosDisconnectedError;
      return _read();
    }
    Future.microtask(_start);
    _loading = true;
    _error = null;
    return _read();
  }

  ProjectTodosRead _read() => ProjectTodosRead(
    digest: _digest ?? ProjectTodoDigest.empty(address),
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

  /// The greatest `created_at` among the ops seen on one target, or `null`.
  /// A writer stamps `max(now, this + 1)` so its write wins the field even
  /// on a slightly slow clock (NIP-TD § Timestamps).
  int? latestSeenFor(String listId, [String? itemId]) =>
      _latestByTarget[_targetKey(listId, itemId)];

  static String _targetKey(String listId, String? itemId) =>
      itemId == null ? listId : '$listId/$itemId';

  Future<void> _start() async {
    _teardown();
    final epoch = ++_epoch;
    if (_disposed) return;
    final session = ref.read(relaySessionProvider.notifier);
    String? failure;

    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    try {
      final unsubscribe = await session.subscribe(
        NostrFilters.projectTodoOpsLive(
          address,
          now - projectTodoClockSkew.inSeconds,
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
      failure = 'To-do subscription failed: $error';
      debugPrint('[ProjectTodos] live subscribe failed for $address: $error');
    }

    try {
      await _walkHistory(session, epoch);
      if (_stale(epoch)) return;
    } catch (error) {
      if (_stale(epoch)) return;
      failure ??= 'To-do history read failed: $error';
      debugPrint('[ProjectTodos] history failed for $address: $error');
    }
    _digest = _fold();
    _loading = false;
    _error = failure;
    _hasRead = true;
    _emit();
  }

  /// Newest page first, then older pages with `until` set to the oldest
  /// `created_at` seen, until a page comes back short. `until` is inclusive,
  /// so the boundary second is re-read and deduped by id; a full page that
  /// adds nothing new cannot be advanced past and is disclosed as
  /// truncation rather than looped on.
  Future<void> _walkHistory(RelaySessionNotifier session, int epoch) async {
    int? until;
    for (var page = 0; page < projectTodoHistoryMaxPages; page++) {
      final events = await session.query(
        NostrFilters.projectTodoOps(
          address,
          limit: projectTodoHistoryPageLimit,
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
      if (events.length < projectTodoHistoryPageLimit) return;
      if (added == 0 || oldest == null) {
        _truncated = true;
        return;
      }
      until = oldest;
    }
    _truncated = true;
  }

  bool _add(NostrEvent event) {
    if (event.kind != EventKind.projectTodoOp) return false;
    if (_store.containsKey(event.id)) return false;
    _store[event.id] = event;
    final op = decodeProjectTodoEvent(event, address);
    if (op != null) {
      final key = _targetKey(op.listId, op.itemId);
      final latest = _latestByTarget[key];
      if (latest == null || event.createdAt > latest) {
        _latestByTarget[key] = event.createdAt;
      }
    }
    return true;
  }

  void _onLiveEvent(NostrEvent event) {
    if (_disposed) return;
    if (!_add(event)) return;
    _scheduleFold();
  }

  /// Coalesce a burst of live events into one fold.
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

  ProjectTodoDigest _fold() => foldProjectTodos(address, _store.values);

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

/// One project's live to-do read, by its canonical coordinate.
final projectTodosProvider =
    NotifierProvider.family<ProjectTodosNotifier, ProjectTodosRead, String>(
      ProjectTodosNotifier.new,
    );
