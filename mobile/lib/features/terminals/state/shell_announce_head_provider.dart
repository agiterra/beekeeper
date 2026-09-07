import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/terminals_domain.dart';
import 'shell_observer_provider.dart';

/// The live announce head of one terminal.
///
/// Read directly rather than from the index so it survives a deep link, and
/// kept live so a revocation flips the observe page back to read-only
/// without a reload (`ShellObserveScreen.tsx`, `useShellSessionAnnounce`).
@immutable
class ShellAnnounceHead {
  /// The open announce, or `null` when none is open (closed, or not read).
  final RemoteTerminal? terminal;

  /// True once the head read returned, so `null` means "not open".
  final bool hasRead;

  const ShellAnnounceHead({required this.terminal, required this.hasRead});
}

/// Follows one terminal's kind:30623 head.
class ShellAnnounceHeadNotifier extends Notifier<ShellAnnounceHead> {
  ShellAnnounceHeadNotifier(this.target);

  final ShellObserverTarget target;
  final List<void Function()> _unsubscribes = [];
  int _epoch = 0;
  bool _disposed = false;

  @override
  ShellAnnounceHead build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _teardown();
    });
    if (status == SessionStatus.connected) {
      Future.microtask(_start);
    }
    return stateOrNull ??
        const ShellAnnounceHead(terminal: null, hasRead: false);
  }

  Future<void> _start() async {
    _teardown();
    final epoch = ++_epoch;
    if (_disposed) return;
    final session = ref.read(relaySessionProvider.notifier);
    final filter = NostrFilters.shellSessionHead(
      target.ownerPubkey,
      target.sessionId,
    );
    try {
      final unsubscribe = await session.subscribe(
        NostrFilter(
          kinds: filter.kinds,
          authors: filter.authors,
          tags: filter.tags,
          since: DateTime.now().millisecondsSinceEpoch ~/ 1000,
          limit: 10,
        ),
        (event) => _apply([event]),
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _unsubscribes.add(unsubscribe);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[ShellAnnounceHead] live subscribe failed: $error');
    }
    try {
      final events = await session.fetchHistory(filter);
      if (_stale(epoch)) return;
      _apply(events, initial: true);
    } catch (error) {
      if (_stale(epoch)) return;
      debugPrint('[ShellAnnounceHead] head read failed: $error');
    }
  }

  void _apply(List<NostrEvent> events, {bool initial = false}) {
    if (_disposed) return;
    final terminals = remoteTerminalsFromEvents(events);
    RemoteTerminal? head;
    for (final terminal in terminals) {
      if (terminal.sessionId == target.sessionId &&
          terminal.ownerPubkey == target.ownerPubkey) {
        head = terminal;
      }
    }
    // A live event that is a close, or a head for another session, yields
    // no terminal; only a real head for this target replaces the current
    // one, and a close is a head too (it decodes to no open terminal).
    final closes = events.any(
      (event) =>
          event.kind == EventKind.shellSession &&
          event.pubkey.toLowerCase() == target.ownerPubkey &&
          event.tags.any(
            (tag) =>
                tag.length >= 2 && tag[0] == 'd' && tag[1] == target.sessionId,
          ),
    );
    if (head == null && !closes && !initial) return;
    state = ShellAnnounceHead(terminal: head, hasRead: true);
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch;

  void _teardown() {
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
  }
}

/// The live announce head for one terminal.
final shellAnnounceHeadProvider = NotifierProvider.autoDispose
    .family<ShellAnnounceHeadNotifier, ShellAnnounceHead, ShellObserverTarget>(
      ShellAnnounceHeadNotifier.new,
    );
