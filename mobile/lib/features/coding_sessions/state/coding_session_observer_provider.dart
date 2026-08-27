import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/coding_sessions_domain.dart';
import 'coding_session_event_store.dart';
import 'coding_session_observer_snapshot.dart';

/// How often the observer re-reads provider leases while a page is open.
///
/// A lease proves reachability for 150 s, so a 60 s poll keeps the page from
/// claiming "no provider answering" purely because it stopped looking.
const codingSessionLeaseRefreshInterval = Duration(seconds: 60);

/// Knobs the observer reads once per start.
///
/// Exists so tests can install an [UnavailableSignatureVerifier] or a fast
/// lease poll without reaching into the notifier.
@immutable
class CodingSessionObserverConfig {
  const CodingSessionObserverConfig({
    this.verifier = const NostrPackageSignatureVerifier(),
    this.leaseRefreshInterval = codingSessionLeaseRefreshInterval,
    this.historyTimeout = const Duration(seconds: 8),
  });

  /// The signature verifier every decode runs through.
  ///
  /// `null` disables verification entirely, which the snapshot reports as
  /// [CodingSessionObserverSnapshot.signaturesVerified] `== false` so the page
  /// can disclose it. It is never silently trusted.
  final CodingSessionSignatureVerifier? verifier;

  final Duration leaseRefreshInterval;
  final Duration historyTimeout;
}

/// Observer knobs; overridden in tests.
final codingSessionObserverConfigProvider =
    Provider<CodingSessionObserverConfig>(
      (ref) => const CodingSessionObserverConfig(),
    );

/// Reads one channel's coding sessions and keeps them live. Read-only: this
/// notifier has no publish path.
///
/// On start it fetches one history page per contract filter (facts, the three
/// creation kinds, names, goals, closures, leases and the roster kinds), runs
/// the domain trust gate and folds, then opens live subscriptions for the
/// facts plus names, closures and leases. Live subscriptions survive relay
/// reconnects — [RelaySessionNotifier.subscribe] replays from
/// `lastSeen - 5 s` — and a rebuild on reconnect re-reads history into the
/// same store, so a replayed event is a duplicate, not a second fact.
class CodingSessionChannelObserverNotifier
    extends Notifier<CodingSessionObserverSnapshot> {
  CodingSessionChannelObserverNotifier(this.channelId);

  final String channelId;

  final CodingSessionEventStore _store = CodingSessionEventStore();
  final List<void Function()> _unsubscribes = [];

  CodingSessionChannelView? _view;
  String? _lastError;
  Timer? _leaseTimer;
  int _epoch = 0;
  bool _disposed = false;
  bool _leasesRead = false;
  bool _truncated = false;
  bool _foldScheduled = false;

  @override
  CodingSessionObserverSnapshot build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });

    if (status != SessionStatus.connected) {
      // Keep whatever was already read: stale facts with an honest connection
      // state beat a blank page that implies the channel has no sessions.
      return _snapshot(_offlineConnection(status));
    }

    // Deferred: a notifier may not assign `state` until `build` has returned.
    Future.microtask(_start);
    return _snapshot(CodingSessionObserverConnection.connecting);
  }

  /// Drop everything read so far and read the channel again.
  Future<void> refresh() async {
    _teardown();
    _store.clear();
    _view = null;
    _lastError = null;
    _leasesRead = false;
    _truncated = false;
    final status = ref.read(relaySessionProvider).status;
    if (status != SessionStatus.connected) {
      _emit(_offlineConnection(status));
      return;
    }
    _emit(CodingSessionObserverConnection.connecting);
    await _start();
  }

  /// What to report while the community socket is not connected.
  ///
  /// A first connect still in flight is a spinner, not "Not connected to this
  /// community": the socket is up and about to answer, so reporting idle
  /// states a falsehood over a live attempt and offers a Retry that cannot
  /// help. [SessionStatus.reconnecting] deliberately stays idle —
  /// `RelaySessionNotifier` retries under exponential backoff and leaves that
  /// state only on success or auth rejection, so a spinner there would never
  /// end; that is the hang abf31c32 fixed.
  static CodingSessionObserverConnection _offlineConnection(
    SessionStatus status,
  ) => status == SessionStatus.connecting
      ? CodingSessionObserverConnection.connecting
      : CodingSessionObserverConnection.idle;

  Future<void> _start() async {
    final epoch = _epoch;
    if (_disposed) return;
    _emit(CodingSessionObserverConnection.connecting);
    final session = ref.read(relaySessionProvider.notifier);
    final config = ref.read(codingSessionObserverConfigProvider);

    final failures = <String>[];
    await _subscribeLive(session, epoch, failures);
    if (_stale(epoch)) return;
    await _fetchHistory(session, config, epoch, failures);
    if (_stale(epoch)) return;

    _lastError = failures.isEmpty ? null : failures.first;
    _fold(config);
    _emit(
      failures.isEmpty
          ? CodingSessionObserverConnection.open
          : CodingSessionObserverConnection.error,
    );
    _startLeaseTimer(config, epoch);
  }

  Future<void> _subscribeLive(
    RelaySessionNotifier session,
    int epoch,
    List<String> failures,
  ) async {
    // The facts stream plus the three per-session streams a page can change
    // under the reader: its name, its closure, and the leases that decide
    // whether anyone is still answering.
    final filters = <NostrFilter>[
      NostrFilters.codingSessionFactsLive(channelId),
      NostrFilters.codingSessionNames(channelId, limit: 0),
      NostrFilters.codingSessionClosures(channelId, limit: 0),
      NostrFilters.codingSessionLeases(channelId),
    ];
    for (final filter in filters) {
      try {
        final unsubscribe = await session.subscribe(filter, _onLiveEvent);
        if (_stale(epoch)) {
          unsubscribe();
          return;
        }
        _unsubscribes.add(unsubscribe);
      } catch (error) {
        if (_stale(epoch)) return;
        failures.add('Coding session subscription failed: $error');
        debugPrint(
          '[CodingSessionObserver] live subscribe failed for $channelId: $error',
        );
      }
    }
  }

  Future<void> _fetchHistory(
    RelaySessionNotifier session,
    CodingSessionObserverConfig config,
    int epoch,
    List<String> failures,
  ) async {
    final leaseFilter = NostrFilters.codingSessionLeases(channelId);
    final filters = <NostrFilter>[
      NostrFilters.codingSessionFacts(channelId),
      ...NostrFilters.codingSessionCreates(channelId),
      NostrFilters.codingSessionNames(channelId),
      NostrFilters.codingSessionGoals(channelId),
      NostrFilters.codingSessionClosures(channelId),
      leaseFilter,
      // Roster reads are part of the contract's filter set. Nothing in the v1
      // trust gate folds 44228/40099, so they are fetched and dropped rather
      // than stored under a pretence of use.
      ...NostrFilters.codingSessionRoster(channelId),
    ];

    final pages = await Future.wait([
      for (final filter in filters) _page(session, config, filter, failures),
    ]);
    if (_stale(epoch)) return;

    for (var index = 0; index < filters.length; index++) {
      final filter = filters[index];
      final page = pages[index];
      if (page == null) continue;
      if (identical(filter, leaseFilter)) {
        // A lease snapshot is not history. D2 reads 24223 at limit 1000 and
        // never paginates it, and a provider republishes its lease every few
        // seconds, so a busy channel fills that page as a matter of course.
        // Counting it as truncation would print a claim about the transcript
        // the page shows that the lease read cannot support.
        _leasesRead = true;
        _store.addAll(page);
        continue;
      }
      if (filter.kinds.contains(EventKind.codingSessionAuthorityTransition) ||
          filter.kinds.contains(EventKind.relayReceipt)) {
        // Read because the contract's filter set says so, folded by nothing.
        // A full roster page says nothing about the history this page shows,
        // so it must not raise the truncation notice either.
        continue;
      }
      // A page that came back full at the history limit means older facts
      // exist that this read never saw; the page says so rather than
      // presenting a partial history as a whole one.
      if (filter.limit >= codingSessionHistoryPageLimit &&
          page.length >= filter.limit) {
        _truncated = true;
      }
      _store.addAll(page);
    }
  }

  Future<List<NostrEvent>?> _page(
    RelaySessionNotifier session,
    CodingSessionObserverConfig config,
    NostrFilter filter,
    List<String> failures,
  ) async {
    try {
      return await session.fetchHistory(filter, timeout: config.historyTimeout);
    } catch (error) {
      failures.add('Coding session history failed: $error');
      debugPrint(
        '[CodingSessionObserver] history failed for $channelId: $error',
      );
      return null;
    }
  }

  void _startLeaseTimer(CodingSessionObserverConfig config, int epoch) {
    _leaseTimer?.cancel();
    _leaseTimer = Timer.periodic(config.leaseRefreshInterval, (_) {
      unawaited(_refreshLeases(config, epoch));
    });
  }

  Future<void> _refreshLeases(
    CodingSessionObserverConfig config,
    int epoch,
  ) async {
    if (_stale(epoch)) return;
    if (ref.read(relaySessionProvider).status != SessionStatus.connected) {
      return;
    }
    try {
      final session = ref.read(relaySessionProvider.notifier);
      final leases = await session.fetchHistory(
        NostrFilters.codingSessionLeases(channelId),
        timeout: config.historyTimeout,
      );
      if (_stale(epoch)) return;
      _leasesRead = true;
      if (!_store.addAll(leases)) {
        // Nothing new, but the page's reachability line ages against the wall
        // clock, so re-emit anyway.
        _emit(state.connection);
        return;
      }
      _fold(config);
      _emit(state.connection);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint(
        '[CodingSessionObserver] lease refresh failed for $channelId: $error',
      );
    }
  }

  void _onLiveEvent(NostrEvent event) {
    if (_disposed) return;
    if (event.channelId != channelId) return;
    if (!_store.add(event)) return;
    _scheduleFold();
  }

  /// Coalesce a burst of live events into one fold.
  void _scheduleFold() {
    if (_foldScheduled) return;
    _foldScheduled = true;
    scheduleMicrotask(() {
      _foldScheduled = false;
      if (_disposed) return;
      _fold(ref.read(codingSessionObserverConfigProvider));
      _emit(state.connection);
    });
  }

  void _fold(CodingSessionObserverConfig config) {
    _view = readCodingSessionChannel(
      channelId: channelId,
      events: _store.events,
      verifier: config.verifier,
      leasesRead: _leasesRead,
      historyTruncated: _truncated,
    );
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    _epoch += 1;
    _leaseTimer?.cancel();
    _leaseTimer = null;
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }

  void _emit(CodingSessionObserverConnection connection) {
    if (_disposed) return;
    state = _snapshot(connection);
  }

  CodingSessionObserverSnapshot _snapshot(
    CodingSessionObserverConnection connection,
  ) {
    final view = _view;
    if (view == null) {
      return CodingSessionObserverSnapshot.initial(
        channelId,
        connection: connection,
        lastError: _lastError,
      );
    }
    return CodingSessionObserverSnapshot.fromView(
      view,
      connection: connection,
      lastError: _lastError,
      evictedByGeneration: _store.evictedByGeneration,
    );
  }
}

/// One channel's live coding-session read.
final codingSessionChannelObserverProvider =
    NotifierProvider.family<
      CodingSessionChannelObserverNotifier,
      CodingSessionObserverSnapshot,
      String
    >(CodingSessionChannelObserverNotifier.new);
