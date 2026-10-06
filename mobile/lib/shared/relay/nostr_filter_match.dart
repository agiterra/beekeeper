import 'nostr_models.dart';

/// Client-side NIP-01 filter matching, mirroring the relay's
/// `crates/beekeeper-core/src/filter.rs`: fields within a filter are AND-ed,
/// filters in a list are OR-ed.
///
/// Semantics pinned to the relay:
/// - `kinds` and `authors` are exact membership tests (an empty list matches
///   nothing);
/// - `since` and `until` are inclusive on `created_at`;
/// - `ids` are prefix matches;
/// - every `#x` tag clause needs at least one event tag `x` whose value is in
///   the clause (an empty clause matches nothing).
///
/// One deliberate divergence: the relay resolves an `#h` clause against its
/// stored `channel_id` for events that carry no `h` tag at all (reactions,
/// deletions). The client cannot see that column, so such an event is *not*
/// excluded by an `#h` clause here. The coalescer's demux therefore errs on
/// the side of delivering an untagged event to every caller that asked for
/// that kind, which callers already dedupe by id.
///
/// `search` filters are refused: full-text semantics cannot be evaluated
/// locally and must never be demultiplexed from a shared response.
///
/// [permissiveChannel] is that divergence's switch: `true` (the default) lets
/// an h-less event pass an `#h` clause; `false` drops it. The coalescer turns
/// it off for a chunk whose filters name different channel sets, where a
/// permissive read would hand one caller another caller's reaction.
bool nostrFilterMatches(
  NostrFilter filter,
  NostrEvent event, {
  bool permissiveChannel = true,
}) {
  if (filter.search != null) {
    throw ArgumentError.value(
      filter.search,
      'filter.search',
      'search filters cannot be matched client-side',
    );
  }
  if (!filter.kinds.contains(event.kind)) return false;

  final authors = filter.authors;
  if (authors != null && !authors.contains(event.pubkey)) return false;

  final since = filter.since;
  if (since != null && event.createdAt < since) return false;

  final until = filter.until;
  if (until != null && event.createdAt > until) return false;

  final ids = filter.ids;
  if (ids != null && !ids.any(event.id.startsWith)) return false;

  for (final clause in filter.tags.entries) {
    if (!clause.key.startsWith('#') || clause.key.length < 2) continue;
    final tagName = clause.key.substring(1);
    final eventValues = [
      for (final tag in event.tags)
        if (tag.length >= 2 && tag[0] == tagName) tag[1],
    ];
    if (tagName == 'h' && eventValues.isEmpty && permissiveChannel) continue;
    if (!clause.value.any(eventValues.contains)) return false;
  }
  return true;
}

/// True when [event] matches any filter in [filters].
bool nostrFiltersMatch(List<NostrFilter> filters, NostrEvent event) =>
    filters.any((filter) => nostrFilterMatches(filter, event));
