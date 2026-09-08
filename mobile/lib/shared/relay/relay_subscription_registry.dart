import 'dart:async';
import 'dart:convert';

import 'nostr_models.dart';

/// Opens one live relay subscription; returns its unsubscribe (CLOSE).
typedef RelayLiveOpener =
    Future<void Function()> Function(
      List<NostrFilter> filters,
      void Function(NostrEvent) onEvent, {
      void Function(String message)? onClosed,
    });

/// Shares one relay `REQ` between every caller that asks for the same
/// filter list.
///
/// Two widgets that subscribe to the same channel (say a list badge and the
/// open timeline) used to cost two `REQ`s and two matching event streams;
/// now the second joins the first, and the relay sees one subscription
/// until the last of them leaves, at which point the `CLOSE` is sent.
///
/// Identity is the canonical JSON of the whole filter list, `since` and
/// `limit` included: a subscription that asks for backlog is not the same
/// subscription as one that asks for none. For a shared subscription that
/// does carry backlog (`limit > 0`), events already delivered are retained
/// (bounded by [maxRetainedEvents]) and replayed to a caller that joins
/// after the first EOSE, so a late joiner still sees what its own `REQ`
/// would have returned.
class RelaySubscriptionRegistry {
  RelaySubscriptionRegistry({
    required RelayLiveOpener open,
    this.maxRetainedEvents = 1000,
  }) : _open = open;

  /// Upper bound on the backlog kept per shared subscription for late joiners.
  final int maxRetainedEvents;

  final RelayLiveOpener _open;
  final Map<String, _Entry> _entries = {};

  /// Canonical identity of a filter list: key-sorted JSON per filter with
  /// list-valued clauses sorted, and the filters themselves sorted, so
  /// order never splits an otherwise identical subscription.
  static String keyFor(List<NostrFilter> filters) {
    final parts = [for (final filter in filters) _canonical(filter.toJson())]
      ..sort();
    return '[${parts.join(',')}]';
  }

  /// How many callers currently share the subscription for [filters].
  int refCount(List<NostrFilter> filters) =>
      _entries[keyFor(filters)]?.listeners.length ?? 0;

  /// Number of distinct relay subscriptions open through this registry.
  int get entryCount => _entries.length;

  /// Join (or open) the shared subscription for [filters]. Resolves once the
  /// underlying subscription is ready; the returned callback leaves it.
  Future<void Function()> join(
    List<NostrFilter> filters,
    void Function(NostrEvent) onEvent, {
    void Function(String message)? onClosed,
  }) async {
    final key = keyFor(filters);
    final existing = _entries[key];
    final entry = existing ?? _openEntry(key, filters);
    final listener = _Listener(onEvent, onClosed);
    final lateJoiner = existing != null && entry.ready;
    if (lateJoiner) {
      for (final event in entry.retained) {
        onEvent(event);
      }
    }
    entry.listeners.add(listener);
    try {
      await entry.opening;
    } catch (_) {
      entry.listeners.remove(listener);
      rethrow;
    }
    return () => _leave(entry, listener);
  }

  /// Drop every entry without sending CLOSE (the socket is gone).
  void clear() {
    final entries = List.of(_entries.values);
    _entries.clear();
    for (final entry in entries) {
      entry.closed = true;
      entry.listeners.clear();
    }
  }

  _Entry _openEntry(String key, List<NostrFilter> filters) {
    final entry = _Entry(retains: filters.any((filter) => filter.limit > 0));
    _entries[key] = entry;
    entry.opening =
        _open(
          filters,
          (event) => _deliver(entry, event),
          onClosed: (message) => _closedByRelay(key, entry, message),
        ).then(
          (unsubscribe) {
            entry.ready = true;
            if (entry.closed) {
              unsubscribe();
            } else if (entry.listeners.isEmpty) {
              _drop(key, entry);
              unsubscribe();
            } else {
              entry.unsubscribe = unsubscribe;
            }
          },
          onError: (Object error, StackTrace stackTrace) {
            _drop(key, entry);
            throw error;
          },
        );
    return entry;
  }

  void _deliver(_Entry entry, NostrEvent event) {
    if (entry.closed) return;
    if (entry.retains) {
      entry.retained.add(event);
      if (entry.retained.length > maxRetainedEvents) {
        entry.retained.removeAt(0);
      }
    }
    for (final listener in List.of(entry.listeners)) {
      listener.onEvent(event);
    }
  }

  void _closedByRelay(String key, _Entry entry, String message) {
    _drop(key, entry);
    entry.closed = true;
    final listeners = List.of(entry.listeners);
    entry.listeners.clear();
    for (final listener in listeners) {
      listener.onClosed?.call(message);
    }
  }

  void _leave(_Entry entry, _Listener listener) {
    if (!entry.listeners.remove(listener)) return;
    if (entry.listeners.isNotEmpty || entry.closed) return;
    entry.closed = true;
    _entries.removeWhere((_, candidate) => identical(candidate, entry));
    final unsubscribe = entry.unsubscribe;
    entry.unsubscribe = null;
    unsubscribe?.call();
  }

  void _drop(String key, _Entry entry) {
    if (identical(_entries[key], entry)) _entries.remove(key);
  }

  static String _canonical(Object? value) {
    if (value is Map) {
      final keys = value.keys.map((key) => key.toString()).toList()..sort();
      return '{${keys.map((key) => '${jsonEncode(key)}:${_canonical(value[key])}').join(',')}}';
    }
    if (value is List) {
      final items = value.map(_canonical).toList();
      if (value.every((item) => item is num || item is String)) items.sort();
      return '[${items.join(',')}]';
    }
    return jsonEncode(value);
  }
}

class _Entry {
  _Entry({required this.retains});

  final bool retains;
  final List<_Listener> listeners = [];
  final List<NostrEvent> retained = [];
  late final Future<void> opening;
  void Function()? unsubscribe;
  bool ready = false;
  bool closed = false;
}

class _Listener {
  _Listener(this.onEvent, this.onClosed);

  final void Function(NostrEvent) onEvent;
  final void Function(String message)? onClosed;
}
