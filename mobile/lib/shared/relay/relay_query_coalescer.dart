import 'dart:async';

import 'nostr_filter_match.dart';
import 'nostr_models.dart';
import 'relay_rate_limit_gate.dart';

/// Sends one bundle of filters to the relay bridge (`POST /query`).
typedef RelayQuerySender =
    Future<List<NostrEvent>> Function(List<NostrFilter> filters);

/// Runs one filter through the WebSocket read lane (`fetchHistory`).
typedef RelayQueryFallback =
    Future<List<NostrEvent>> Function(NostrFilter filter);

/// Folds every one-shot read that starts within one [window] into a single
/// `POST /query`, which the relay charges as one call against its own
/// 300/min budget instead of one `REQ` each against the WebSocket burst.
///
/// - Callers are grouped by arrival: the first `query` opens a window and
///   every call before it closes joins the same bundle.
/// - A bundle is split into calls of at most [maxChannelTags] aggregate
///   `#h` values (the relay's cap per request), never splitting one filter.
/// - The shared response is demultiplexed by [nostrFilterMatches], so each
///   caller sees only what its own filter admits, deduplicated by id and cut
///   to its own `limit` (newest first) when the union overshoots it.
/// - When the HTTP call fails, every caller in that chunk is retried
///   individually over [fallback], which waits out the rate-limit gate the
///   failure may have armed.
/// - Filters carrying `search` or bridge `extensions` bypass bundling and
///   go alone: neither can be demultiplexed client-side.
class RelayQueryCoalescer {
  /// Default grouping window.
  static const defaultWindow = Duration(milliseconds: 50);

  /// The relay's aggregate `#h` cap per `/query` request.
  static const maxChannelTags = 128;

  RelayQueryCoalescer({
    required RelayQuerySender send,
    required RelayQueryFallback fallback,
    Duration window = defaultWindow,
    RelayTimerFactory timerFactory = Timer.new,
  }) : _send = send,
       _fallback = fallback,
       _window = window,
       _timerFactory = timerFactory;

  final RelayQuerySender _send;
  final RelayQueryFallback _fallback;
  final Duration _window;
  final RelayTimerFactory _timerFactory;
  final List<_PendingQuery> _pending = [];
  Timer? _windowTimer;

  /// Resolve [filter] through the next bundle (or alone, see class docs).
  Future<List<NostrEvent>> query(NostrFilter filter) {
    if (filter.search != null || filter.extensions.isNotEmpty) {
      return _sendAlone(filter);
    }
    final pending = _PendingQuery(filter);
    _pending.add(pending);
    _windowTimer ??= _timerFactory(_window, _flush);
    return pending.completer.future;
  }

  /// Fail every caller still waiting for a window to close (session
  /// disposal). The coalescer stays usable afterwards, like the session.
  void reset() {
    _windowTimer?.cancel();
    _windowTimer = null;
    final pending = List.of(_pending);
    _pending.clear();
    for (final entry in pending) {
      entry.completer.completeError(StateError('Relay session is disposed'));
    }
  }

  Future<List<NostrEvent>> _sendAlone(NostrFilter filter) async {
    try {
      return await _send([filter]);
    } catch (_) {
      return _fallback(filter);
    }
  }

  void _flush() {
    _windowTimer = null;
    final batch = List.of(_pending);
    _pending.clear();
    for (final chunk in _chunk(batch, (entry) => _channelTags(entry.filter))) {
      unawaited(_sendChunk(chunk));
    }
  }

  Future<void> _sendChunk(List<_PendingQuery> entries) async {
    final List<NostrEvent> events;
    try {
      events = await _send([for (final entry in entries) entry.filter]);
    } catch (_) {
      for (final entry in entries) {
        unawaited(
          _fallback(entry.filter).then(
            entry.completer.complete,
            onError: entry.completer.completeError,
          ),
        );
      }
      return;
    }
    // One channel set across the chunk keeps the permissive read for h-less
    // events; a mix of sets demuxes strictly so nothing leaks between callers.
    final scopes = {for (final entry in entries) _channelScope(entry.filter)};
    final permissive = scopes.length <= 1;
    for (final entry in entries) {
      entry.completer.complete(
        demuxQueryResult(entry.filter, events, permissiveChannel: permissive),
      );
    }
  }

  static String _channelScope(NostrFilter filter) {
    final channels = filter.tags['#h'];
    if (channels == null) return '';
    return (List.of(channels)..sort()).join('\u0000');
  }

  /// Split [filters] (in order) into runs whose aggregate `#h` count stays
  /// within [maxChannelTags]. A single filter above the cap forms its own
  /// run: the relay decides its fate.
  static List<List<NostrFilter>> chunkByChannelTags(
    Iterable<NostrFilter> filters,
  ) => _chunk(filters, _channelTags);

  static int _channelTags(NostrFilter filter) => filter.tags['#h']?.length ?? 0;

  static List<List<T>> _chunk<T>(Iterable<T> items, int Function(T) tagsOf) {
    final chunks = <List<T>>[];
    var current = <T>[];
    var currentTags = 0;
    for (final item in items) {
      final tags = tagsOf(item);
      if (current.isNotEmpty && currentTags + tags > maxChannelTags) {
        chunks.add(current);
        current = <T>[];
        currentTags = 0;
      }
      current.add(item);
      currentTags += tags;
    }
    if (current.isNotEmpty) chunks.add(current);
    return chunks;
  }

  /// The subset of [events] that [filter] admits, in the relay's order,
  /// deduplicated by id and trimmed to the newest `limit` when the shared
  /// response carries more matches than the filter alone would have.
  static List<NostrEvent> demuxQueryResult(
    NostrFilter filter,
    List<NostrEvent> events, {
    bool permissiveChannel = true,
  }) {
    final seen = <String>{};
    final matched = [
      for (final event in events)
        if (nostrFilterMatches(
              filter,
              event,
              permissiveChannel: permissiveChannel,
            ) &&
            seen.add(event.id))
          event,
    ];
    if (filter.limit <= 0 || matched.length <= filter.limit) return matched;
    final newest = List.of(matched)
      ..sort((a, b) => b.createdAt.compareTo(a.createdAt));
    final keep = {for (final event in newest.take(filter.limit)) event.id};
    return [
      for (final event in matched)
        if (keep.contains(event.id)) event,
    ];
  }
}

class _PendingQuery {
  _PendingQuery(this.filter);

  final NostrFilter filter;
  final Completer<List<NostrEvent>> completer = Completer<List<NostrEvent>>();
}
