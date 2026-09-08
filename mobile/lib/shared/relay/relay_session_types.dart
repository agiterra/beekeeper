part of 'relay_session.dart';

class _HistorySubscription {
  final List<NostrEvent> events = [];
  final Completer<List<NostrEvent>> completer;
  final Timer timeout;

  _HistorySubscription({required this.completer, required this.timeout});
}

class _LiveSubscription {
  /// The filters of the one `REQ` this subscription stands for (≤ 10).
  final List<NostrFilter> filters;
  final void Function(NostrEvent) onEvent;
  final void Function(String message)? onClosed;
  Completer<void>? readyCompleter;
  int? lastSeenCreatedAt;
  int closedRetryAttempt = 0;
  Timer? closedRetryTimer;

  _LiveSubscription({
    required this.filters,
    required this.onEvent,
    this.onClosed,
    this.readyCompleter,
  });

  /// True when any filter names [channelId] in its `#h` clause.
  bool watchesChannel(String channelId) =>
      filters.any((filter) => filter.tags['#h']?.contains(channelId) ?? false);
}

class _ClosedRetry {
  final _LiveSubscription subscription;
  final int generation;

  _ClosedRetry({required this.subscription, required this.generation});
}

class _PendingEvent {
  final Completer<NostrEvent> completer;
  final Timer timeout;

  _PendingEvent({required this.completer, required this.timeout});
}

class _BufferedEvent {
  final String subId;
  final NostrEvent event;

  _BufferedEvent(this.subId, this.event);
}
