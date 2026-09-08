part of 'relay_session.dart';

/// Inbound frame handling for [RelaySessionNotifier]: `EVENT`, `EOSE`,
/// `CLOSED`, `OK`, the live-event batch buffer and `CLOSED` retry
/// bookkeeping. Split out as a same-library mixin so the session file stays
/// under the size gate; every `_member` below is implemented by the notifier.
mixin _RelaySessionInbound {
  static const _eventBatchMs = 16;
  static const _maxRecentDeliveryKeys = 5000;

  Map<String, _HistorySubscription> get _historySubscriptions;
  Map<String, _LiveSubscription> get _liveSubscriptions;
  Map<String, _ClosedRetry> get _pendingClosedRetries;
  Map<String, _PendingEvent> get _pendingEvents;
  List<_BufferedEvent> get _eventBuffer;
  Set<String> get _recentDeliveryKeys;
  Timer? get _flushTimer;
  set _flushTimer(Timer? timer);
  bool get _closedRetryReplayScheduled;
  set _closedRetryReplayScheduled(bool scheduled);
  RelayRateLimitGate get _rateLimitGate;
  RelayTimerFactory get _retryTimerFactory;
  int get _connectionGeneration;
  bool get _socketConnected;
  bool _isActiveConnection(int generation);
  void _sendClose(String subId);
  Future<void> _sendReplay(
    List<MapEntry<String, _LiveSubscription>> entries,
    int generation, {
    bool pendingClosedRetries = false,
  });

  void _handleMessage(List<dynamic> data) {
    if (data.isEmpty) return;
    final type = data[0] as String;

    switch (type) {
      case 'EVENT':
        _handleEvent(data);
      case 'EOSE':
        _handleEose(data);
      case 'CLOSED':
        _handleClosed(data);
      case 'OK':
        _handleOk(data);
    }
  }

  void _handleEvent(List<dynamic> data) {
    if (data.length < 3) return;
    final subId = data[1] as String;
    final eventJson = data[2] as Map<String, dynamic>;
    final event = NostrEvent.fromJson(eventJson);

    // History subscriptions accumulate immediately.
    final historySub = _historySubscriptions[subId];
    if (historySub != null) {
      historySub.events.add(event);
      return;
    }

    // Live subscriptions get batched.
    final liveSub = _liveSubscriptions[subId];
    if (liveSub != null) {
      _resetClosedRetry(liveSub);
      // Track last seen timestamp for reconnect replay.
      if (liveSub.lastSeenCreatedAt == null ||
          event.createdAt > liveSub.lastSeenCreatedAt!) {
        liveSub.lastSeenCreatedAt = event.createdAt;
      }
      _eventBuffer.add(_BufferedEvent(subId, event));
      _scheduleFlush();
    }
  }

  void _handleEose(List<dynamic> data) {
    if (data.length < 2) return;
    final subId = data[1] as String;

    // History subscription: resolve with collected events.
    final historySub = _historySubscriptions.remove(subId);
    if (historySub != null) {
      historySub.timeout.cancel();
      if (!historySub.completer.isCompleted) {
        historySub.completer.complete(historySub.events);
      }
      _sendClose(subId);
      return;
    }

    // Live subscription: signal ready.
    final liveSub = _liveSubscriptions[subId];
    if (liveSub != null) {
      _resetClosedRetry(liveSub);
    }
    if (liveSub != null &&
        liveSub.readyCompleter != null &&
        !liveSub.readyCompleter!.isCompleted) {
      // EOSE is the boundary between replay and live delivery. Flush any
      // replay events before resolving subscribe(), so callers that begin a
      // one-shot query immediately afterwards cannot classify a delayed batch
      // callback as having arrived during that query.
      _flushBufferedEventsNow();
      liveSub.readyCompleter!.complete();
      liveSub.readyCompleter = null;
    }
  }

  void _handleClosed(List<dynamic> data) {
    if (data.length < 2) return;
    final subId = data[1] as String;
    final message = data.length >= 3 && data[2] is String
        ? data[2] as String
        : 'subscription closed by relay';
    final closedClass = classifyRelayClosed(message);

    final historySub = _historySubscriptions.remove(subId);
    if (historySub != null) {
      if (closedClass == RelayClosedClass.rateLimited) {
        _rateLimitGate.activate(parseRateLimitRetrySeconds(message));
      }
      historySub.timeout.cancel();
      if (!historySub.completer.isCompleted) {
        historySub.completer.completeError(Exception(message));
      }
      return;
    }

    final liveSub = _liveSubscriptions[subId];
    if (liveSub == null) return;
    final readyCompleter = liveSub.readyCompleter;
    if (closedClass == RelayClosedClass.terminal) {
      if (readyCompleter != null && !readyCompleter.isCompleted) {
        readyCompleter.completeError(Exception(message));
      }
      liveSub.onClosed?.call(message);
      _removeLiveSubscription(subId, liveSub);
      return;
    }
    if (readyCompleter != null && !readyCompleter.isCompleted) {
      readyCompleter.complete();
      liveSub.readyCompleter = null;
    }
    if (liveSub.closedRetryTimer != null) return;

    if (closedClass == RelayClosedClass.rateLimited) {
      _rateLimitGate.activate(parseRateLimitRetrySeconds(message));
    }
    final attempt = liveSub.closedRetryAttempt;
    final delayMs = closedRetryDelayMs(
      attempt: attempt,
      closedClass: closedClass,
      message: message,
      gateRemainingMs: _rateLimitGate.remainingMs(),
    );

    liveSub.closedRetryAttempt = attempt + 1;
    final retryGeneration = _connectionGeneration;
    liveSub.closedRetryTimer = _retryTimerFactory(
      Duration(milliseconds: delayMs),
      () async {
        liveSub.closedRetryTimer = null;
        if (!_isActiveConnection(retryGeneration) ||
            _liveSubscriptions[subId] != liveSub) {
          return;
        }
        if (_rateLimitGate.isActive) await _rateLimitGate.wait();
        if (!_isActiveConnection(retryGeneration) ||
            _liveSubscriptions[subId] != liveSub ||
            !_socketConnected) {
          return;
        }
        _pendingClosedRetries[subId] = _ClosedRetry(
          subscription: liveSub,
          generation: retryGeneration,
        );
        _scheduleClosedRetryReplay(retryGeneration);
      },
    );
  }

  void _scheduleClosedRetryReplay(int generation) {
    if (_closedRetryReplayScheduled) return;
    _closedRetryReplayScheduled = true;
    scheduleMicrotask(() async {
      try {
        await _replayPendingClosedRetries(generation);
      } finally {
        _closedRetryReplayScheduled = false;
        _pendingClosedRetries.removeWhere(
          (_, retry) => retry.generation != _connectionGeneration,
        );
        if (_pendingClosedRetries.values.any(
          (retry) => retry.generation == _connectionGeneration,
        )) {
          _scheduleClosedRetryReplay(_connectionGeneration);
        }
      }
    });
  }

  Future<void> _replayPendingClosedRetries(int generation) async {
    if (!_isActiveConnection(generation)) return;
    final entries = _pendingClosedRetries.entries
        .where((entry) => entry.value.generation == generation)
        .map(
          (entry) => MapEntry<String, _LiveSubscription>(
            entry.key,
            entry.value.subscription,
          ),
        )
        .toList();
    await _sendReplay(entries, generation, pendingClosedRetries: true);
  }

  void _handleOk(List<dynamic> data) {
    if (data.length < 3) return;
    final eventId = data[1] as String;
    final accepted = data[2] as bool;
    final message = data.length > 3 && data[3] is String
        ? data[3] as String
        : '';

    // Back-pressure arrives here rather than as a NOTICE: the relay rejects
    // an over-quota EVENT on the OK channel. Arm the gate before looking the
    // event up, because a fire-and-forget ephemeral (typing, presence) has no
    // pending entry and would otherwise leave the next send blind.
    if (!accepted && message.startsWith('rate-limited:')) {
      _rateLimitGate.activate(parseRateLimitRetrySeconds(message));
    }

    final pending = _pendingEvents.remove(eventId);
    if (pending == null) return;
    pending.timeout.cancel();

    if (accepted) {
      // We don't have the full event here; create a minimal placeholder.
      // Command kinds (e.g. 41010, 30620, 46020) return "response:{...}" in
      // the OK message — preserve it in `content` so callers can parse it.
      if (!pending.completer.isCompleted) {
        pending.completer.complete(
          NostrEvent(
            id: eventId,
            pubkey: '',
            createdAt: 0,
            kind: 0,
            tags: [],
            content: message,
            sig: '',
          ),
        );
      }
    } else if (!pending.completer.isCompleted) {
      pending.completer.completeError(
        Exception(message.isNotEmpty ? message : 'Event rejected'),
      );
    }
  }

  void _scheduleFlush() {
    _flushTimer ??= Timer(
      const Duration(milliseconds: _eventBatchMs),
      _flushEventBuffer,
    );
  }

  void _flushBufferedEventsNow() {
    _flushTimer?.cancel();
    _flushTimer = null;
    _flushEventBuffer();
  }

  void _flushEventBuffer() {
    _flushTimer = null;
    if (_eventBuffer.isEmpty) return;

    final batch = List<_BufferedEvent>.from(_eventBuffer);
    _eventBuffer.clear();

    for (final buffered in batch) {
      final sub = _liveSubscriptions[buffered.subId];
      if (sub == null) continue;

      // Deduplicate per subscription. The same relay event can legitimately
      // match multiple live subscriptions, e.g. the channel list unread listener
      // and the open channel message listener.
      final deliveryKey = '${buffered.subId}:${buffered.event.id}';
      if (_recentDeliveryKeys.contains(deliveryKey)) continue;

      // Cap the dedup set to prevent unbounded memory growth.
      if (_recentDeliveryKeys.length >= _maxRecentDeliveryKeys) {
        _recentDeliveryKeys.clear();
      }
      _recentDeliveryKeys.add(deliveryKey);

      sub.onEvent(buffered.event);
    }
  }

  void _removeLiveSubscription(String subId, _LiveSubscription subscription) {
    if (_liveSubscriptions[subId] != subscription) return;
    _liveSubscriptions.remove(subId);
    _pendingClosedRetries.remove(subId);
    subscription.closedRetryTimer?.cancel();
    subscription.closedRetryTimer = null;
    _recentDeliveryKeys.removeWhere((key) => key.startsWith('$subId:'));
  }

  void _resetClosedRetry(_LiveSubscription subscription) {
    subscription.closedRetryAttempt = 0;
    subscription.closedRetryTimer?.cancel();
    subscription.closedRetryTimer = null;
  }

  void _cancelAllClosedRetries() {
    _pendingClosedRetries.clear();
    for (final subscription in _liveSubscriptions.values) {
      subscription.closedRetryTimer?.cancel();
      subscription.closedRetryTimer = null;
    }
  }

  void _resetAllClosedRetries() {
    _pendingClosedRetries.clear();
    for (final subscription in _liveSubscriptions.values) {
      _resetClosedRetry(subscription);
    }
  }

  void _cancelAllHistory(Object? error) {
    for (final entry in _historySubscriptions.values) {
      entry.timeout.cancel();
      if (!entry.completer.isCompleted) {
        entry.completer.completeError(error ?? Exception('Connection lost'));
      }
    }
    _historySubscriptions.clear();
  }

  void _rejectAllPending(Object? error) {
    for (final entry in _pendingEvents.values) {
      entry.timeout.cancel();
      if (!entry.completer.isCompleted) {
        entry.completer.completeError(error ?? Exception('Connection lost'));
      }
    }
    _pendingEvents.clear();
  }
}
