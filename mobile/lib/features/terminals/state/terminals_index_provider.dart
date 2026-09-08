import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/terminals_domain.dart';

/// How the terminals read is doing, as the pages may state it.
enum TerminalsConnection {
  /// No relay session to read from.
  idle,

  /// The announce read is in flight.
  connecting,

  /// The announces on screen are what the relay sent, and live changes are
  /// being followed.
  open,

  /// The last read failed; [TerminalsIndex.lastError] says how.
  error,
}

/// Every open shared terminal the relay will show this reader.
///
/// An empty list under [TerminalsConnection.open] is a claim about the relay;
/// under any other connection it is not, and the pages say so.
@immutable
class TerminalsIndex {
  final List<RemoteTerminal> terminals;
  final Map<String, List<RemoteTerminal>> byProject;
  final TerminalsConnection connection;
  final String? lastError;

  /// True once a read has returned, so an empty list means "none".
  final bool hasRead;

  const TerminalsIndex({
    required this.terminals,
    required this.byProject,
    required this.connection,
    required this.lastError,
    required this.hasRead,
  });

  const TerminalsIndex.initial({
    this.connection = TerminalsConnection.idle,
    this.lastError,
  }) : terminals = const [],
       byProject = const {},
       hasRead = false;

  /// The terminals shared under [projectAddress], in list order.
  List<RemoteTerminal> forProject(String projectAddress) =>
      byProject[projectAddress] ?? const [];

  /// The terminal announced under [ownerPubkey] and [sessionId], if open.
  RemoteTerminal? find(String ownerPubkey, String sessionId) {
    final wanted = '${ownerPubkey.toLowerCase()} $sessionId';
    for (final terminal in terminals) {
      if (terminal.key == wanted) return terminal;
    }
    return null;
  }

  TerminalsIndex copyWith({
    TerminalsConnection? connection,
    String? lastError,
    bool clearError = false,
  }) => TerminalsIndex(
    terminals: terminals,
    byProject: byProject,
    connection: connection ?? this.connection,
    lastError: clearError ? null : (lastError ?? this.lastError),
    hasRead: hasRead,
  );
}

/// How often the announce list is re-read when no live head arrives — the
/// fallback for a missed replaceable-head race, as on the desktop.
const terminalsPollInterval = Duration(seconds: 30);

/// Reads every kind:30623 announce, follows live republishes, and polls.
class TerminalsIndexNotifier extends Notifier<TerminalsIndex> {
  final List<void Function()> _unsubscribes = [];
  Timer? _poll;
  int _epoch = 0;
  bool _disposed = false;
  bool _refetchScheduled = false;

  @override
  TerminalsIndex build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });
    if (status != SessionStatus.connected) {
      // Keep whatever was read: stale announces with an honest connection
      // state beat a blank list that implies nobody is sharing anything.
      return (stateOrNull ?? const TerminalsIndex.initial()).copyWith(
        connection: status == SessionStatus.connecting
            ? TerminalsConnection.connecting
            : TerminalsConnection.idle,
      );
    }
    Future.microtask(_start);
    return (stateOrNull ?? const TerminalsIndex.initial()).copyWith(
      connection: TerminalsConnection.connecting,
    );
  }

  /// Read the announces again.
  Future<void> refresh() async {
    if (ref.read(relaySessionProvider).status != SessionStatus.connected) {
      return;
    }
    await _fetch(_epoch);
  }

  Future<void> _start() async {
    _teardown();
    final epoch = ++_epoch;
    if (_disposed) return;
    final session = ref.read(relaySessionProvider.notifier);
    try {
      final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final unsubscribe = await session.subscribe(
        NostrFilters.shellSessionsLive(now),
        (_) => _scheduleRefetch(epoch),
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _unsubscribes.add(unsubscribe);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[Terminals] live subscribe failed: $error');
    }
    await _fetch(epoch);
    if (_stale(epoch)) return;
    _armPoll(epoch);
  }

  /// First tick at the wall-clock phase the projects poll also uses
  /// ([_indexPollDelay]), so the two 30 s reads share one coalescer flush.
  void _armPoll(int epoch) {
    _poll?.cancel();
    final delay = alignedPollDelay(
      key: 'mobile-index-poll',
      period: terminalsPollInterval,
      pubkey: ref.read(myPubkeyProvider) ?? '',
      now: DateTime.now(),
    );
    _poll = Timer(delay, () {
      if (_stale(epoch)) return;
      _fetch(epoch);
      _poll = Timer.periodic(terminalsPollInterval, (_) => _fetch(epoch));
    });
  }

  void _scheduleRefetch(int epoch) {
    if (_refetchScheduled || _stale(epoch)) return;
    _refetchScheduled = true;
    scheduleMicrotask(() {
      _refetchScheduled = false;
      _fetch(epoch);
    });
  }

  Future<void> _fetch(int epoch) async {
    if (_stale(epoch)) return;
    final session = ref.read(relaySessionProvider.notifier);
    try {
      final events = await session.query(NostrFilters.shellSessions());
      if (_stale(epoch)) return;
      // Nothing is excluded by owner: the paired desktop shares this key,
      // and watching your own terminal from the phone is the case NIP-ST
      // names first (owner typing from another device).
      final terminals = remoteTerminalsFromEvents(events);
      state = TerminalsIndex(
        terminals: List.unmodifiable(terminals),
        byProject: Map.unmodifiable({
          for (final entry in terminalsByProject(terminals).entries)
            entry.key: List<RemoteTerminal>.unmodifiable(entry.value),
        }),
        connection: TerminalsConnection.open,
        lastError: null,
        hasRead: true,
      );
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[Terminals] announce read failed: $error');
      state = state.copyWith(
        connection: TerminalsConnection.error,
        lastError: 'Terminal list read failed: $error',
      );
    }
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    _poll?.cancel();
    _poll = null;
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }
}

/// Every open shared terminal this reader may see, grouped by project.
final terminalsIndexProvider =
    NotifierProvider<TerminalsIndexNotifier, TerminalsIndex>(
      TerminalsIndexNotifier.new,
    );
