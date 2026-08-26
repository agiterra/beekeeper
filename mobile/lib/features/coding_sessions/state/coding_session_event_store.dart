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
/// Events that carry no `cs-target` — geneses, creates, names, goals,
/// closures — share one bucket under [sessionScopedGeneration], capped the
/// same way so a hostile channel cannot grow this store without bound.
class CodingSessionEventStore {
  CodingSessionEventStore({this.cap = maxCodingSessionEventsPerGeneration});

  /// Maximum raw events retained per generation.
  final int cap;

  /// Bucket key for events that name no generation.
  static const sessionScopedGeneration = '';

  final Map<String, List<NostrEvent>> _byGeneration = {};
  final Set<String> _ids = <String>{};

  /// Number of retained events.
  int get length => _ids.length;

  /// Number of generations currently retained.
  int get generationCount => _byGeneration.length;

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

  /// The generation bucket [event] belongs to.
  static String generationKeyOf(NostrEvent event) =>
      event.getTagValue('cs-target') ?? sessionScopedGeneration;

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
  void clear() {
    _byGeneration.clear();
    _ids.clear();
  }

  void _evict(String key, List<NostrEvent> bucket) {
    final retained = retainNewestCodingSessionEvents(bucket, cap: cap);
    final retainedIds = {for (final event in retained) event.id};
    for (final event in bucket) {
      if (!retainedIds.contains(event.id)) _ids.remove(event.id);
    }
    _byGeneration[key] = [...retained];
  }
}
