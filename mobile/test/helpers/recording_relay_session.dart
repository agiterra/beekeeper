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
/// Every relay call is appended to [operations] (`'subscribe'`, `'fetch'`,
/// `'query'`, `'publish'`) in call order, and the filters and events are kept
/// verbatim so a test can assert the exact shape sent to the relay.
class RecordingRelaySessionNotifier extends RelaySessionNotifier {
  final bool failSubscribe;
  final Queue<Object> _queryResults;
  final Queue<List<NostrEvent>> _historyResults;
  final Queue<Object> _publishResults;

  /// Relay calls in order.
  final List<String> operations = [];

  /// Filters handed to [subscribe], in order.
  final List<NostrFilter> liveFilters = [];

  /// Filters handed to [fetchHistory], in order.
  final List<NostrFilter> historyFilters = [];

  /// Filters handed to [queryRelay], in order.
  final List<NostrFilter> queryFilters = [];

  /// Events handed to [publish], in order, exactly as signed.
  final List<NostrEvent> published = [];

  final List<void Function(NostrEvent)> _listeners = [];
  final Completer<void> _subscribed = Completer<void>();
  final Completer<List<NostrEvent>> _history = Completer<List<NostrEvent>>();
  final Queue<Completer<List<NostrEvent>>> _targetHistories = Queue();

  /// [queryResults] and [historyResults] are consumed one per call;
  /// [publishResults] likewise, where an entry may be an [Exception] to throw
  /// or a [NostrEvent] to return as the relay's OK. When [publishResults] is
  /// exhausted, [publish] echoes the submitted event back as accepted.
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
  Future<List<NostrEvent>> queryRelay(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    operations.add('query');
    queryFilters.addAll(filters);
    if (_queryResults.isEmpty) throw Exception('unsupported');
    final result = _queryResults.removeFirst();
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
  }) async {
    operations.add('subscribe');
    liveFilters.add(filter);
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
