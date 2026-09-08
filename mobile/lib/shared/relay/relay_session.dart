import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:http/http.dart' as http;

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../auth/auth.dart';
import 'nostr_models.dart';
import 'relay_client.dart';
import 'relay_closed_policy.dart';
import 'relay_closed_retry.dart';
import 'relay_http_query_client.dart';
import 'relay_nip98.dart';
import 'relay_provider.dart';
import 'relay_query_coalescer.dart';
import 'relay_rate_limit_gate.dart';
import 'relay_reconnect_policy.dart';
import 'relay_send_budget.dart';
import 'relay_socket.dart';
import 'relay_subscription_registry.dart';

export 'relay_nip98.dart' show buildNip98AuthHeader;

part 'relay_session_inbound.dart';
part 'relay_session_types.dart';

enum SessionStatus { disconnected, connecting, connected, reconnecting }

/// Connection status of the relay session. Value-equal so a bare
/// `ref.watch(relaySessionProvider)` only rebuilds when something changed.
@immutable
class SessionState {
  final SessionStatus status;
  final int reconnectAttempt;

  const SessionState({required this.status, this.reconnectAttempt = 0});

  @override
  bool operator ==(Object other) =>
      other is SessionState &&
      other.status == status &&
      other.reconnectAttempt == reconnectAttempt;

  @override
  int get hashCode => Object.hash(status, reconnectAttempt);
}

/// The relay accepts at most this many filters in one `REQ`
/// (`crates/buzz-relay/src/protocol.rs`).
const relayMaxFiltersPerReq = 10;

/// Manages websocket subscriptions, event batching, reconnection with replay,
/// and pending event tracking. Equivalent to the desktop's RelayClientSession.
typedef RelaySocketFactory =
    RelaySocket Function({
      required String wsUrl,
      required String? nsec,
      required void Function(List<dynamic> message) onMessage,
      required void Function() onConnected,
      required void Function(Object? error) onDisconnected,
    });

class RelaySessionNotifier extends Notifier<SessionState>
    with _RelaySessionInbound {
  RelaySessionNotifier({
    http.Client? httpClient,
    http.Client Function()? httpClientFactory,
    RelaySocketFactory socketFactory = RelaySocket.new,
    DateTime Function()? now,
    RelayRateLimitGate? rateLimitGate,
    RelaySendBudget? sendBudget,
    RelayTimerFactory retryTimerFactory = Timer.new,
    RelayTimerFactory queryWindowTimerFactory = Timer.new,
    double Function()? random,
  }) : _httpQueryClient = RelayHttpQueryClient(
         client: httpClient,
         clientFactory: httpClientFactory,
       ),
       _socketFactory = socketFactory,
       _now = now ?? DateTime.now,
       _rateLimitGate = rateLimitGate ?? RelayRateLimitGate(),
       _sendBudget = sendBudget ?? RelaySendBudget(now: now),
       _retryTimerFactory = retryTimerFactory,
       _queryWindowTimerFactory = queryWindowTimerFactory,
       _random = random ?? Random().nextDouble;

  final RelayHttpQueryClient _httpQueryClient;
  final RelaySocketFactory _socketFactory;
  final DateTime Function() _now;
  @override
  final RelayRateLimitGate _rateLimitGate;
  final RelaySendBudget _sendBudget;
  @override
  final RelayTimerFactory _retryTimerFactory;
  final RelayTimerFactory _queryWindowTimerFactory;
  final double Function() _random;

  static const _reconnectReplaySkewSeconds = 5;
  static const _backgroundGraceDuration = Duration(seconds: 5);

  late final RelayQueryCoalescer _queryCoalescer = RelayQueryCoalescer(
    send: queryRelay,
    fallback: fetchHistory,
    timerFactory: _queryWindowTimerFactory,
  );
  late final RelaySubscriptionRegistry _registry = RelaySubscriptionRegistry(
    open: _openLiveSubscription,
  );

  RelaySocket? _socket;
  @override
  final Map<String, _HistorySubscription> _historySubscriptions = {};
  @override
  final Map<String, _LiveSubscription> _liveSubscriptions = {};
  @override
  final Map<String, _ClosedRetry> _pendingClosedRetries = {};
  @override
  final Map<String, _PendingEvent> _pendingEvents = {};
  @override
  final List<_BufferedEvent> _eventBuffer = [];
  @override
  final Set<String> _recentDeliveryKeys = {};
  Timer? _reconnectTimer;
  @override
  Timer? _flushTimer;
  Timer? _backgroundGraceTimer;
  DateTime? _backgroundedAt;
  int _reconnectDelayMs = relayBaseRetryDelayMs;
  int _subIdCounter = 0;
  bool _disposed = false;
  bool _paused = false;
  bool _hasConnectedOnce = false;
  @override
  int _connectionGeneration = 0;
  final Map<Object, String> _visibleChannelsByOwner = {};
  @override
  bool _socketConnected = false;
  @override
  bool _closedRetryReplayScheduled = false;

  @override
  SessionState build() {
    final config = ref.watch(relayConfigProvider);
    final authState = ref.watch(authProvider);

    // Reset disposed flag — build() may re-run on the same Notifier instance
    // after a provider dependency changes (e.g. auth completing).
    _disposed = false;

    ref.onDispose(_dispose);

    // Auto-connect when authenticated and we have a signing key (NIP-42 AUTH).
    final isAuthenticated = authState.value?.status == AuthStatus.authenticated;
    if (isAuthenticated && config.nsec != null) {
      // Schedule connection after build completes.
      Future.microtask(() => _connect(config));
    }

    return const SessionState(status: SessionStatus.disconnected);
  }

  /// Execute a one-shot query via the relay's HTTP bridge (`POST /query`).
  ///
  /// One call costs one unit of the bridge's own budget however many filters
  /// it carries; prefer [query] for reads that can share a bundle.
  Future<List<NostrEvent>> queryRelay(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    final config = ref.read(relayConfigProvider);
    final url = Uri.parse(config.baseUrl).resolve('/query').toString();
    final bodyBytes = utf8.encode(
      jsonEncode(filters.map((filter) => filter.toJson()).toList()),
    );
    // Reuse the session transport on success. A timeout rotates immediately
    // for new queries, then closes the retired client after its peers finish.
    final response = await _httpQueryClient.post(
      Uri.parse(url),
      headers: {
        'Authorization': buildNip98AuthHeader(
          method: 'POST',
          url: url,
          bodyBytes: bodyBytes,
          nsec: config.nsec,
        ),
        'Content-Type': 'application/json',
      },
      body: bodyBytes,
      timeout: timeout,
    );
    if (response.statusCode < 200 || response.statusCode >= 300) {
      _activateRateLimitGateFromHttpError(response.body);
      throw RelayException(response.statusCode, response.body);
    }
    final decoded = jsonDecode(response.body);
    if (decoded is! List) {
      throw const FormatException('relay returned malformed query response');
    }
    try {
      return [
        for (final eventJson in decoded)
          if (eventJson is Map<String, dynamic>)
            NostrEvent.fromJson(eventJson)
          else
            throw const FormatException('relay returned malformed query event'),
      ];
    } catch (error) {
      if (error is FormatException) rethrow;
      throw FormatException('relay returned malformed query event: $error');
    }
  }

  /// One-shot read through the 50 ms coalescer: every [query] that starts
  /// within the same window travels in one `POST /query` (chunked at 128
  /// aggregate `#h`), and each caller receives only the events its own
  /// filter admits. If the HTTP call fails, the filter is retried alone over
  /// the WebSocket read lane ([fetchHistory]). Filters with `search` or
  /// bridge `extensions` are sent alone. See [RelayQueryCoalescer].
  Future<List<NostrEvent>> query(NostrFilter filter) =>
      _queryCoalescer.query(filter);

  void _activateRateLimitGateFromHttpError(String body) {
    final dynamic decoded;
    try {
      decoded = jsonDecode(body);
    } on FormatException {
      return;
    }
    if (decoded is! Map<String, dynamic>) return;
    final message = decoded['error'];
    if (message is! String ||
        classifyRelayClosed(message) != RelayClosedClass.rateLimited) {
      return;
    }
    _rateLimitGate.activate(parseRateLimitRetrySeconds(message));
  }

  /// Fetch historical events matching [filter]. One-filter form of
  /// [fetchHistoryAll].
  Future<List<NostrEvent>> fetchHistory(
    NostrFilter filter, {
    Duration timeout = const Duration(seconds: 8),
  }) => fetchHistoryAll([filter], timeout: timeout);

  /// Fetch historical events matching any of [filters] (at most
  /// [relayMaxFiltersPerReq]) with one `REQ`, collecting until EOSE.
  ///
  /// The result is the relay's union: an event matching several filters
  /// arrives once, and the one EOSE gives no per-filter truncation signal.
  /// Waits out the rate-limit gate and the read lane of the send budget
  /// before the `REQ` goes out.
  Future<List<NostrEvent>> fetchHistoryAll(
    List<NostrFilter> filters, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    _checkReqFilters(filters);
    final clearance = _sendClearance(RelaySendLane.read);
    if (clearance != null) await clearance;
    if (_disposed) throw StateError('Relay session is disposed');
    final subId = _nextSubId('h');
    final completer = Completer<List<NostrEvent>>();

    final timer = Timer(timeout, () {
      final sub = _historySubscriptions.remove(subId);
      if (sub != null && !sub.completer.isCompleted) {
        sub.completer.completeError(
          TimeoutException('Relay history request timed out after $timeout'),
        );
      }
      _sendClose(subId);
    });

    _historySubscriptions[subId] = _HistorySubscription(
      completer: completer,
      timeout: timer,
    );

    _sendReq(subId, filters);
    return completer.future;
  }

  /// Subscribe to live events matching [filter]. One-filter form of
  /// [subscribeAll].
  Future<void Function()> subscribe(
    NostrFilter filter,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) => subscribeAll([filter], onEvent, onClosed: onClosed);

  /// Subscribe to live events matching any of [filters] (at most
  /// [relayMaxFiltersPerReq]) with one `REQ`. Returns an unsubscribe
  /// function.
  ///
  /// Identical filter lists share one relay subscription
  /// ([RelaySubscriptionRegistry]); the `CLOSE` goes out when the last
  /// subscriber leaves. Live subscriptions survive reconnects — they are
  /// replayed with `since: lastSeenCreatedAt - 5s` on every filter.
  Future<void Function()> subscribeAll(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) {
    _checkReqFilters(filters);
    return _registry.join(filters, onEvent, onClosed: onClosed);
  }

  Future<void Function()> _openLiveSubscription(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) async {
    if (_disposed) throw StateError('Relay session is disposed');
    final subId = _nextSubId('l');
    final readyCompleter = Completer<void>();
    final liveSub = _LiveSubscription(
      filters: filters,
      onEvent: onEvent,
      onClosed: onClosed,
      readyCompleter: readyCompleter,
    );
    _liveSubscriptions[subId] = liveSub;

    final clearance = _sendClearance(RelaySendLane.read);
    if (clearance != null) await clearance;
    if (_disposed) throw StateError('Relay session is disposed');
    if (_liveSubscriptions[subId] == liveSub) _sendReq(subId, filters);

    // Wait for EOSE or a short fallback timeout.
    try {
      await readyCompleter.future.timeout(
        const Duration(milliseconds: 500),
        onTimeout: () {},
      );
    } catch (_) {
      _liveSubscriptions.remove(subId);
      _recentDeliveryKeys.removeWhere((key) => key.startsWith('$subId:'));
      rethrow;
    }
    if (_liveSubscriptions[subId] == liveSub &&
        liveSub.readyCompleter == readyCompleter) {
      liveSub.readyCompleter = null;
    }

    return () => _unsubscribe(subId);
  }

  /// Publish an event and wait for the relay's OK confirmation.
  Future<NostrEvent> publish(
    NostrEvent event, {
    Duration timeout = const Duration(seconds: 8),
  }) async {
    final generation = _connectionGeneration;
    final clearance = _sendClearance(RelaySendLane.write);
    if (clearance != null) await clearance;
    if (!_isActiveConnection(generation) || !_socketConnected) {
      throw StateError('Relay session is not connected');
    }

    final completer = Completer<NostrEvent>();

    final timer = Timer(timeout, () {
      final pending = _pendingEvents.remove(event.id);
      if (pending != null && !pending.completer.isCompleted) {
        pending.completer.completeError(
          TimeoutException(
            'Event ${event.id} not acknowledged within $timeout',
          ),
        );
      }
    });

    _pendingEvents[event.id] = _PendingEvent(
      completer: completer,
      timeout: timer,
    );

    _socket?.send(['EVENT', event.toJson()]);
    return completer.future;
  }

  /// Send a fire-and-forget ephemeral event (typing, presence, keepalive).
  ///
  /// Returns false — and sends nothing — when the socket is not connected,
  /// the rate-limit gate is active, or the ephemeral lane of the send budget
  /// is exhausted. Such frames are worth less than the user's next message,
  /// so they are dropped rather than queued.
  bool sendEphemeral(NostrEvent event) =>
      _sendEphemeralFrame(['EVENT', event.toJson()]);

  /// Send a raw ephemeral frame. Same gating as [sendEphemeral], without the
  /// drop signal.
  @Deprecated(
    'Use sendEphemeral, which also reports a drop. No caller remains.',
  )
  void sendRaw(List<dynamic> payload) {
    _sendEphemeralFrame(payload);
  }

  bool _sendEphemeralFrame(List<dynamic> payload) {
    final socket = _socket;
    if (socket == null || !_socketConnected || _disposed) return false;
    if (_rateLimitGate.isActive) return false;
    if (!_sendBudget.tryAcquire(RelaySendLane.ephemeral)) return false;
    socket.send(payload);
    return true;
  }

  /// True when the next [onAppResumed] will (re)connect — the app was in the
  /// background long enough for the socket to have been dropped, or the
  /// session is not connected right now. Callers that refresh on resume can
  /// skip their own round trip in that case: the reconnect replays every live
  /// subscription anyway.
  bool get willReconnectOnResume {
    final backgroundedAt = _backgroundedAt;
    if (backgroundedAt != null &&
        _now().difference(backgroundedAt) >= _backgroundGraceDuration) {
      return true;
    }
    return state.status != SessionStatus.connected;
  }

  @visibleForTesting
  void debugHandleMessage(List<dynamic> data) => _handleMessage(data);

  @visibleForTesting
  void debugFlushEventBuffer() => _flushEventBuffer();

  @visibleForTesting
  Future<void> debugHandleConnected() =>
      _handleConnected(_connectionGeneration);

  @visibleForTesting
  Future<void> debugReplayLiveSubscriptions() =>
      _replayLiveSubscriptions(_connectionGeneration);

  @visibleForTesting
  void debugDispose() => _dispose();

  @visibleForTesting
  void debugSupersedeConnection() => _connectionGeneration++;

  @visibleForTesting
  void debugHandleDisconnected([Object? error]) {
    _socketConnected = false;
    _handleDisconnected(_connectionGeneration, error);
  }

  @visibleForTesting
  void debugResetClosedRetriesForDisconnect() {
    _socketConnected = false;
    _resetAllClosedRetries();
  }

  @visibleForTesting
  void debugSetSessionStatus(SessionStatus status) {
    _socketConnected = status == SessionStatus.connected;
  }

  @visibleForTesting
  void debugPauseNow() => _pauseNow();

  @visibleForTesting
  void debugHandleSocketMessageForTest(List<dynamic> data) =>
      _handleMessage(data);

  @visibleForTesting
  void debugAttachSocketForTest(RelaySocket socket) {
    _socket?.dispose();
    _socket = socket;
    _socketConnected = true;
  }

  /// Registers a visible channel and returns an owner-scoped release callback.
  /// The most recently registered owner is prioritized during reconnect replay.
  void Function() registerVisibleChannel(String channelId) {
    final owner = Object();
    _visibleChannelsByOwner[owner] = channelId;
    return () => _visibleChannelsByOwner.remove(owner);
  }

  /// Force a reconnect (e.g., returning from background).
  Future<void> reconnect() async {
    _socketConnected = false;
    await _socket?.disconnect();
    _reconnectDelayMs = relayBaseRetryDelayMs;
    final config = ref.read(relayConfigProvider);
    await _connect(config);
  }

  /// Called by the app lifecycle provider when the app goes to background.
  void onAppPaused() {
    _backgroundedAt = _now();
    _backgroundGraceTimer?.cancel();
    _backgroundGraceTimer = Timer(_backgroundGraceDuration, _pauseNow);
  }

  void _pauseNow() {
    _paused = true;
    _socketConnected = false;
    _reconnectTimer?.cancel();
    _cancelAllHistory(Exception('App moved to background'));
    _rejectAllPending(Exception('App moved to background'));
    _socket?.disconnect();
    state = const SessionState(status: SessionStatus.disconnected);
  }

  /// Called by the app lifecycle provider when the app returns to foreground.
  void onAppResumed() {
    _paused = false;
    final backgroundedAt = _backgroundedAt;
    _backgroundedAt = null;
    _backgroundGraceTimer?.cancel();
    _backgroundGraceTimer = null;

    final backgroundedLongEnoughToRequireReconnect =
        backgroundedAt != null &&
        _now().difference(backgroundedAt) >= _backgroundGraceDuration;
    if (!backgroundedLongEnoughToRequireReconnect &&
        state.status == SessionStatus.connected) {
      return;
    }

    // Cancel any in-flight reconnect backoff timer so we reconnect immediately
    // instead of waiting for the (possibly large) exponential delay.
    _reconnectTimer?.cancel();
    _reconnectDelayMs = relayBaseRetryDelayMs;
    final config = ref.read(relayConfigProvider);
    _connect(config);
  }

  Future<void> _connect(RelayConfig config) async {
    if (_disposed) return;

    final generation = ++_connectionGeneration;
    state = SessionState(
      status: _hasConnectedOnce
          ? SessionStatus.reconnecting
          : SessionStatus.connecting,
      reconnectAttempt: state.reconnectAttempt,
    );

    _socket?.dispose();
    final socket = _socketFactory(
      wsUrl: config.wsUrl,
      nsec: config.nsec,
      onMessage: (message) {
        if (generation == _connectionGeneration) _handleMessage(message);
      },
      onConnected: () => _handleConnected(generation),
      onDisconnected: (error) => _handleDisconnected(generation, error),
    );
    _socket = socket;

    await socket.connect();
  }

  Future<void> _handleConnected(int generation) async {
    if (_disposed || generation != _connectionGeneration) return;
    _socketConnected = true;
    _hasConnectedOnce = true;
    _reconnectDelayMs = relayBaseRetryDelayMs;
    state = const SessionState(status: SessionStatus.connected);
    await _replayLiveSubscriptions(generation);
  }

  void _handleDisconnected(int generation, Object? error) {
    if (_disposed || generation != _connectionGeneration) return;
    _socketConnected = false;
    _cancelAllHistory(error);
    _rejectAllPending(error);
    _resetAllClosedRetries();
    _eventBuffer.clear();
    _flushTimer?.cancel();
    _flushTimer = null;
    if (error is RelayAuthRejectedException) {
      _reconnectTimer?.cancel();
      state = const SessionState(status: SessionStatus.disconnected);
      return;
    }
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    if (_disposed || _paused) return;
    final attempt = state.reconnectAttempt + 1;
    state = SessionState(
      status: SessionStatus.reconnecting,
      reconnectAttempt: attempt,
    );

    _reconnectTimer?.cancel();
    final delayMs = jitteredReconnectDelayMs(_reconnectDelayMs, _random());
    _reconnectTimer = Timer(Duration(milliseconds: delayMs), () {
      _reconnectDelayMs = min(_reconnectDelayMs * 2, relayMaxRetryDelayMs);
      final config = ref.read(relayConfigProvider);
      _connect(config);
    });
  }

  /// Replay all live subscriptions after a reconnect, with a time skew to
  /// catch events that occurred during the disconnect. The visible channel's
  /// subscriptions go first; the send budget paces the rest.
  Future<void> _replayLiveSubscriptions(int generation) async {
    if (_rateLimitGate.isActive) await _rateLimitGate.wait();
    if (!_isActiveConnection(generation)) return;

    final entries = _liveSubscriptions.entries.toList();
    final visibleChannelId = _visibleChannelsByOwner.isEmpty
        ? null
        : _visibleChannelsByOwner.values.last;
    if (visibleChannelId != null) {
      entries.sort((left, right) {
        final leftVisible = left.value.watchesChannel(visibleChannelId);
        final rightVisible = right.value.watchesChannel(visibleChannelId);
        if (leftVisible == rightVisible) return 0;
        return leftVisible ? -1 : 1;
      });
    }

    await _sendReplay(entries, generation);
  }

  @override
  Future<void> _sendReplay(
    List<MapEntry<String, _LiveSubscription>> entries,
    int generation, {
    bool pendingClosedRetries = false,
  }) async {
    for (final entry in entries) {
      if (!_isActiveConnection(generation)) return;
      if (_liveSubscriptions[entry.key] != entry.value) continue;
      final clearance = _sendClearance(RelaySendLane.read);
      if (clearance != null) await clearance;
      if (!_isActiveConnection(generation)) return;
      if (_liveSubscriptions[entry.key] != entry.value) continue;
      if (pendingClosedRetries) {
        final pendingRetry = _pendingClosedRetries[entry.key];
        if (pendingRetry?.subscription != entry.value ||
            pendingRetry?.generation != generation) {
          continue;
        }
        _pendingClosedRetries.remove(entry.key);
      }
      _sendReq(entry.key, _replayFilters(entry.value));
    }
  }

  @override
  bool _isActiveConnection(int generation) =>
      !_disposed && generation == _connectionGeneration;

  List<NostrFilter> _replayFilters(_LiveSubscription subscription) {
    final since = subscription.lastSeenCreatedAt;
    if (since == null) return subscription.filters;
    final replaySince = max(0, since - _reconnectReplaySkewSeconds);
    return [
      for (final filter in subscription.filters)
        filter.copyWithSince(replaySince),
    ];
  }

  /// Null when a metered frame may go out right now (which charges the
  /// budget); otherwise a future that resolves once the rate-limit gate has
  /// reopened and [lane] has a slot. Nothing is charged while the socket is
  /// down: the frame is a no-op and the reconnect replay re-sends it.
  Future<void>? _sendClearance(RelaySendLane lane) {
    if (!_socketConnected) return null;
    if (!_rateLimitGate.isActive && _sendBudget.tryAcquire(lane)) return null;
    return _awaitSendClearance(lane);
  }

  Future<void> _awaitSendClearance(RelaySendLane lane) async {
    while (true) {
      if (_rateLimitGate.isActive) await _rateLimitGate.wait();
      if (!_socketConnected || _sendBudget.tryAcquire(lane)) return;
      await _sendBudget.acquire(lane);
      if (!_rateLimitGate.isActive) return;
    }
  }

  void _checkReqFilters(List<NostrFilter> filters) {
    if (filters.isEmpty || filters.length > relayMaxFiltersPerReq) {
      throw ArgumentError.value(
        filters.length,
        'filters',
        'a REQ carries between 1 and $relayMaxFiltersPerReq filters',
      );
    }
  }

  String _nextSubId(String prefix) {
    _subIdCounter++;
    return '$prefix-$_subIdCounter';
  }

  void _sendReq(String subId, List<NostrFilter> filters) {
    assert(
      filters.isNotEmpty && filters.length <= relayMaxFiltersPerReq,
      'a REQ carries between 1 and $relayMaxFiltersPerReq filters',
    );
    _socket?.send([
      'REQ',
      subId,
      for (final filter in filters) filter.toJson(),
    ]);
  }

  @override
  void _sendClose(String subId) {
    _socket?.send(['CLOSE', subId]);
  }

  void _unsubscribe(String subId) {
    final subscription = _liveSubscriptions[subId];
    if (subscription != null) {
      _removeLiveSubscription(subId, subscription);
    }
    _sendClose(subId);
  }

  void _dispose() {
    _disposed = true;
    _connectionGeneration++;
    _reconnectTimer?.cancel();
    _flushTimer?.cancel();
    _backgroundGraceTimer?.cancel();
    _backgroundedAt = null;
    _cancelAllClosedRetries();
    _rateLimitGate.reset();
    _sendBudget.reset();
    _queryCoalescer.reset();
    _visibleChannelsByOwner.clear();
    _socketConnected = false;
    _cancelAllHistory(null);
    _rejectAllPending(null);
    _registry.clear();
    final subscriptions = _liveSubscriptions.values.toList();
    _liveSubscriptions.clear();
    for (final subscription in subscriptions) {
      subscription.closedRetryTimer?.cancel();
      subscription.closedRetryTimer = null;
    }
    _recentDeliveryKeys.clear();
    _socket?.dispose();
    _socket = null;
    _httpQueryClient.close();
  }
}

final relaySessionProvider =
    NotifierProvider<RelaySessionNotifier, SessionState>(
      RelaySessionNotifier.new,
    );
