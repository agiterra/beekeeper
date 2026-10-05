import '../../../shared/relay/nostr_models.dart';
import '../domain/coding_sessions_domain.dart';

/// The raw coding-session events one channel read has accumulated, bounded
/// per generation.
///
/// Retention is per generation (the signed `cs-target` value) because a
/// generation is the unit a transcript is read in: a single global cap would
/// let one chatty execution evict the middle of another one's history and the
/// reader would never know a hole had been punched in it.
///
/// Events that carry no `cs-target` — receipts, creates, geneses, names,
/// goals, closures — are bucketed by *kind* instead, each capped the same way.
/// One shared bucket would let the 44224 receipt stream (one per turn on a
/// busy channel) evict the handful of 44221 creates and 44226 geneses that
/// resolve authority and the founder, and the page would quietly degrade to
/// "Founder unresolved" and "authority unverified" with nothing to say why.
/// The kinds this observer stores are a fixed, short list, so the number of
/// buckets stays bounded.
///
/// Two kinds *do* carry `cs-target` and still need that separation: 44223
/// metadata and 24223 leases. A single long turn is thousands of 44225
/// envelopes against a handful of each, and both are older than the flood, so
/// oldest-first eviction takes them first: the session loses its status, its
/// title and its `sessionRef` (and with no readable create, the D5 fallback
/// authority with them, so the session leaves the list altogether), and the
/// last lease goes with it, which reads on screen as "No provider answering"
/// while the provider is streaming. Both are low-cardinality per generation,
/// so giving each its own sub-bucket keeps the store bounded.
class CodingSessionEventStore {
  CodingSessionEventStore({this.cap = maxCodingSessionEventsPerGeneration});

  /// Maximum raw events retained per generation.
  final int cap;

  /// Bucket key for events of [kind] that name no generation.
  static String kindBucket(int kind) => 'kind $kind';

  final Map<String, List<NostrEvent>> _byGeneration = {};
  final Set<String> _ids = <String>{};
  final Map<String, int> _evictedByGeneration = {};

  /// Number of retained events.
  int get length => _ids.length;

  /// Number of generations currently retained.
  int get generationCount => _byGeneration.length;

  /// How many events the cap threw away, per generation.
  ///
  /// Keyed by the generation's `cs-target` value, or by [kindBucket] for the
  /// kinds that name no generation. The reader is told about this: a
  /// transcript this device silently shortened looks exactly like a short
  /// transcript, and D10's cap is a property of *this device*, not of what
  /// the channel holds.
  Map<String, int> get evictedByGeneration =>
      Map.unmodifiable(_evictedByGeneration);

  /// How many events the cap threw away across the whole read.
  int get evictedCount =>
      _evictedByGeneration.values.fold(0, (total, count) => total + count);

  /// Every retained event, oldest generation-bucket first.
  ///
  /// The order is irrelevant to the reader: [readCodingSessionChannel] sorts
  /// every fact it uses by its own signed ordering keys.
  List<NostrEvent> get events => [
    for (final bucket in _byGeneration.values) ...bucket,
  ];

  /// Retained events for one generation, for assertions and diagnostics.
  List<NostrEvent> eventsForGeneration(String generationKey) =>
      List.unmodifiable(_byGeneration[generationKey] ?? const <NostrEvent>[]);

  /// Kinds that name a generation but arrive in ones and twos, and whose
  /// eviction silently degrades the page rather than shortening a transcript.
  static const _separatelyCappedTargetedKinds = <int>{
    EventKind.codingSessionMetadata,
    EventKind.codingSessionLease,
    // One per umbrella, at most a few: filed under its generation it would be
    // the oldest event in a transcript flood and the first evicted, and the
    // session would quietly fall back from its title to "Untitled session".
    EventKind.codingSessionGeneratedTitle,
  };

  /// The bucket [event] belongs to: its generation, its kind *and* generation
  /// for the kinds capped separately above, or its kind when it names no
  /// generation.
  ///
  /// The kind leads so the three key shapes cannot collide: a `cs-target`
  /// always starts with the `coding-session/v1|` prefix, so no generation key
  /// can be mistaken for a `kind NNNNN ...` one.
  static String generationKeyOf(NostrEvent event) {
    final target = event.getTagValue('cs-target');
    if (target == null) return kindBucket(event.kind);
    if (_separatelyCappedTargetedKinds.contains(event.kind)) {
      return '${kindBucket(event.kind)} $target';
    }
    return target;
  }

  /// Add one event. Returns true when the store changed.
  ///
  /// A repeat of an event id already held is dropped: the relay replays
  /// history on reconnect, and a replay is not a new fact.
  bool add(NostrEvent event) {
    if (!_ids.add(event.id)) return false;
    final key = generationKeyOf(event);
    final bucket = _byGeneration.putIfAbsent(key, () => <NostrEvent>[]);
    bucket.add(event);
    if (bucket.length > cap) _evict(key, bucket);
    return true;
  }

  /// Add many events. Returns true when the store changed.
  bool addAll(Iterable<NostrEvent> events) {
    var changed = false;
    for (final event in events) {
      if (add(event)) changed = true;
    }
    return changed;
  }

  /// Drop everything, for a full refetch.
  ///
  /// The eviction tally goes with it: a fresh read has dropped nothing yet,
  /// and carrying the old number over would make the page disclose a loss
  /// that is no longer on screen.
  void clear() {
    _byGeneration.clear();
    _ids.clear();
    _evictedByGeneration.clear();
  }

  void _evict(String key, List<NostrEvent> bucket) {
    final retained = retainNewestCodingSessionEvents(bucket, cap: cap);
    final retainedIds = {for (final event in retained) event.id};
    for (final event in bucket) {
      if (retainedIds.contains(event.id)) continue;
      _ids.remove(event.id);
      // Every event in a bucket shares a generation, so the label is the
      // same whichever evicted one it is read from. The two separately
      // capped targeted kinds report under their generation rather than
      // their sub-bucket: what the reader lost is history for *that*
      // generation, however this store filed it.
      _evictedByGeneration.update(
        event.getTagValue('cs-target') ?? kindBucket(event.kind),
        (count) => count + 1,
        ifAbsent: () => 1,
      );
    }
    _byGeneration[key] = [...retained];
  }
}
