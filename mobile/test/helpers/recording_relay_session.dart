import 'dart:async';
import 'dart:collection';

import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:buzz/shared/relay/relay_session.dart';

/// A relay session that records what a provider asks of it and lets a test
/// answer on its own schedule.
///
/// Lifted from the private fake in
/// `test/features/channels/channel_messages_provider_test.dart` so the
/// coding-session command, terminal and project providers can share one.
/// Every relay call is appended to [operations] in call order:
///
/// - `'subscribe'` for [subscribe], `'subscribeAll'` for [subscribeAll];
/// - `'fetch'` for [fetchHistory], `'fetchAll'` for [fetchHistoryAll];
/// - `'query'` for [queryRelay];
/// - `'query1'` once per *group* of [query] calls — every call made in the
///   same event-loop turn joins one group, the way the real 50 ms coalescer
///   folds them into one `POST /query`;
/// - `'publish'` for [publish], `'ephemeral'` for [sendEphemeral] (and the
///   deprecated `sendRaw`).
///
/// Filters and events are kept verbatim so a test can assert the exact
/// shape sent to the relay. The one-filter wrappers also feed the bundle
/// lists, so a provider that moves from `subscribe` to `subscribeAll` can
/// keep asserting on [liveFilters] while [liveFilterBundles] pins the
/// grouping.
class RecordingRelaySessionNotifier extends RelaySessionNotifier {
  final bool failSubscribe;
  final Queue<Object> _queryResults;
  final Queue<List<NostrEvent>> _historyResults;
  final Queue<Object> _publishResults;

  /// Relay calls in order.
  final List<String> operations = [];

  /// Filters handed to [subscribe] and [subscribeAll] (flattened), in order.
  final List<NostrFilter> liveFilters = [];

  /// Each [subscribe] (as a one-element list) or [subscribeAll] filter list.
  final List<List<NostrFilter>> liveFilterBundles = [];

  /// Filters handed to [fetchHistory] and [fetchHistoryAll] (flattened).
  final List<NostrFilter> historyFilters = [];

  /// Each [fetchHistory] (as a one-element list) or [fetchHistoryAll] list.
  final List<List<NostrFilter>> historyFilterBundles = [];

  /// Filters handed to [queryRelay], in order.
  final List<NostrFilter> queryFilters = [];

  /// Filters handed to [query], in order.
  final List<NostrFilter> coalescedQueryFilters = [];

  /// Filters handed to [query], grouped by event-loop turn — one inner list
  /// per `'query1'` operation.
  final List<List<NostrFilter>> coalescedQueryGroups = [];

  /// Events handed to [publish], in order, exactly as signed.
  final List<NostrEvent> published = [];

  /// Events handed to [sendEphemeral] (or `sendRaw`), in order.
  final List<NostrEvent> ephemeralEvents = [];

  /// What [sendEphemeral] answers; set false to simulate the budget or gate
  /// dropping the frame (the event is still recorded).
  bool acceptEphemeral = true;

  /// What [willReconnectOnResume] reports.
  bool reconnectOnResume = false;

  final List<void Function(NostrEvent)> _listeners = [];
  final Completer<void> _subscribed = Completer<void>();
  final Completer<List<NostrEvent>> _history = Completer<List<NostrEvent>>();
  final Queue<Completer<List<NostrEvent>>> _targetHistories = Queue();
  List<_PendingCoalescedQuery>? _openQueryGroup;

  /// [queryResults] and [historyResults] are consumed one per call;
  /// [publishResults] likewise, where an entry may be an [Exception] to throw
  /// or a [NostrEvent] to return as the relay's OK. When [publishResults] is
  /// exhausted, [publish] echoes the submitted event back as accepted.
  ///
  /// [query] draws from [queryResults] too, one entry per call in call
  /// order; when that queue is empty it falls back to the answer given to
  /// [completeHistory] / [failHistory].
  RecordingRelaySessionNotifier({
    this.failSubscribe = false,
    List<Object> queryResults = const [],
    List<List<NostrEvent>> historyResults = const [],
    List<Object> publishResults = const [],
  }) : _queryResults = Queue<Object>.of(queryResults),
       _historyResults = Queue<List<NostrEvent>>.of(historyResults),
       _publishResults = Queue<Object>.of(publishResults);

  /// Resolves once the first live subscription has been requested.
  Future<void> get subscribed => _subscribed.future;

  @override
  SessionState build() => const SessionState(status: SessionStatus.connected);

  /// Flip the reported connection status.
  void setConnected(bool connected) {
    state = SessionState(
      status: connected ? SessionStatus.connected : SessionStatus.disconnected,
    );
  }

  @override
  bool get willReconnectOnResume => reconnectOnResume;

  @override
  Future<List<NostrEvent>> queryRelay(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    operations.add('query');
    queryFilters.addAll(filters);
    if (_queryResults.isEmpty) throw Exception('unsupported');
    return _resolveQueryResult(_queryResults.removeFirst());
  }

  @override
  Future<List<NostrEvent>> query(NostrFilter filter) {
    coalescedQueryFilters.add(filter);
    final pending = _PendingCoalescedQuery(filter);
    final group = _openQueryGroup;
    if (group != null) {
      group.add(pending);
    } else {
      final newGroup = [pending];
      _openQueryGroup = newGroup;
      operations.add('query1');
      coalescedQueryGroups.add([filter]);
      Timer(Duration.zero, () {
        _openQueryGroup = null;
        for (final entry in newGroup) {
          entry.completer.complete(_answerQuery());
        }
      });
      return pending.completer.future;
    }
    coalescedQueryGroups.last.add(filter);
    return pending.completer.future;
  }

  Future<List<NostrEvent>> _answerQuery() {
    if (_queryResults.isEmpty) return _history.future;
    return _resolveQueryResult(_queryResults.removeFirst());
  }

  Future<List<NostrEvent>> _resolveQueryResult(Object result) async {
    if (result is Exception) throw result;
    if (result is Future<List<NostrEvent>>) return await result;
    return (result as List<NostrEvent>).toList();
  }

  @override
  Future<List<NostrEvent>> fetchHistory(
    NostrFilter filter, {
    Duration timeout = const Duration(seconds: 8),
  }) {
    operations.add('fetch');
    historyFilters.add(filter);
    historyFilterBundles.add([filter]);
    return _answerHistory(filter);
  }

  @override
  Future<List<NostrEvent>> fetchHistoryAll(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) {
    operations.add('fetchAll');
    historyFilters.addAll(filters);
    historyFilterBundles.add(List.of(filters));
    return _answerHistory(filters.first);
  }

  Future<List<NostrEvent>> _answerHistory(NostrFilter filter) {
    if (filter.ids != null) {
      final completer = Completer<List<NostrEvent>>();
      _targetHistories.add(completer);
      return completer.future;
    }
    if (_historyResults.isNotEmpty) {
      return Future.value(_historyResults.removeFirst());
    }
    return _history.future;
  }

  @override
  Future<void Function()> subscribe(
    NostrFilter filter,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) {
    operations.add('subscribe');
    liveFilters.add(filter);
    liveFilterBundles.add([filter]);
    return _attach(onEvent);
  }

  @override
  Future<void Function()> subscribeAll(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) {
    operations.add('subscribeAll');
    liveFilters.addAll(filters);
    liveFilterBundles.add(List.of(filters));
    return _attach(onEvent);
  }

  Future<void Function()> _attach(void Function(NostrEvent) onEvent) async {
    if (!_subscribed.isCompleted) {
      _subscribed.complete();
    }
    if (failSubscribe) {
      throw Exception('subscribe failed');
    }
    _listeners.add(onEvent);
    return () {
      _listeners.remove(onEvent);
    };
  }

  @override
  Future<NostrEvent> publish(
    NostrEvent event, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    operations.add('publish');
    published.add(event);
    if (_publishResults.isEmpty) return event;
    final result = _publishResults.removeFirst();
    if (result is Exception) throw result;
    if (result is Error) throw result;
    return result as NostrEvent;
  }

  @override
  bool sendEphemeral(NostrEvent event) {
    operations.add('ephemeral');
    ephemeralEvents.add(event);
    return acceptEphemeral;
  }

  @override
  // ignore: deprecated_member_use_from_same_package
  void sendRaw(List<dynamic> payload) {
    final body = payload.length > 1 ? payload[1] : null;
    if (body is Map<String, dynamic>) {
      sendEphemeral(NostrEvent.fromJson(body));
    } else {
      operations.add('ephemeral');
    }
  }

  /// Deliver [event] to every live listener.
  void emit(NostrEvent event) {
    for (final listener in List.of(_listeners)) {
      listener(event);
    }
  }

  /// Number of live listeners currently attached.
  int get listenerCount => _listeners.length;

  /// Answer the oldest pending `ids` history read.
  void completeTargetHistory(List<NostrEvent> events) {
    _targetHistories.removeFirst().complete(events);
  }

  /// Answer every history read that was not pre-seeded.
  void completeHistory(List<NostrEvent> events) {
    if (!_history.isCompleted) {
      _history.complete(events);
    }
  }

  /// Fail every history read that was not pre-seeded.
  void failHistory(Object error) {
    if (!_history.isCompleted) {
      _history.completeError(error);
    }
  }
}

class _PendingCoalescedQuery {
  _PendingCoalescedQuery(this.filter);

  final NostrFilter filter;
  final Completer<List<NostrEvent>> completer = Completer<List<NostrEvent>>();
}
