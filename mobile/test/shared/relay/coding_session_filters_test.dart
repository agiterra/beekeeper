import 'package:buzz/shared/relay/nostr_filters.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

const _channelId = 'c0ffee00-0000-4000-8000-000000000001';

void main() {
  group('coding-session event kinds', () {
    test('match the desktop kind integers', () {
      expect(EventKind.codingSessionCommand, 44220);
      expect(EventKind.codingSessionLifecycleCommand, 44221);
      expect(EventKind.codingSessionProviderCatalog, 44222);
      expect(EventKind.codingSessionMetadata, 44223);
      expect(EventKind.codingSessionLifecycleReceipt, 44224);
      expect(EventKind.codingSessionTranscript, 44225);
      expect(EventKind.codingSessionGenesis, 44226);
      expect(EventKind.codingSessionGoal, 44227);
      expect(EventKind.codingSessionAuthorityTransition, 44228);
      expect(EventKind.codingSessionName, 44229);
      expect(EventKind.codingSessionClosure, 44230);
      expect(EventKind.codingSessionTeamTransaction, 44244);
      expect(EventKind.codingSessionLease, 24223);
      expect(EventKind.relayReceipt, 40099);
    });
  });

  group('coding-session filters', () {
    test('every filter carries kinds and an #h scope', () {
      final filters = <NostrFilter>[
        NostrFilters.codingSessionFacts(_channelId),
        NostrFilters.codingSessionFactsLive(_channelId),
        ...NostrFilters.codingSessionCreates(_channelId),
        NostrFilters.codingSessionNames(_channelId),
        NostrFilters.codingSessionGoals(_channelId),
        NostrFilters.codingSessionClosures(_channelId),
        NostrFilters.codingSessionLeases(_channelId),
        ...NostrFilters.codingSessionRoster(_channelId),
      ];
      for (final filter in filters) {
        // A filter without kinds trips the relay's p-gate with a 403.
        expect(filter.kinds, isNotEmpty);
        expect(filter.tags['#h'], [_channelId]);
        expect(filter.authors, isNull);
      }
    });

    test('the facts filter reads the three per-generation kinds', () {
      final filter = NostrFilters.codingSessionFacts(_channelId);
      expect(filter.kinds, [44223, 44224, 44225]);
      expect(filter.limit, 1000);
    });

    test('the live facts filter is the same kinds at limit 0', () {
      final filter = NostrFilters.codingSessionFactsLive(_channelId);
      expect(filter.kinds, [44223, 44224, 44225]);
      expect(filter.limit, 0);
    });

    test('creates are one filter per kind so none starves the others', () {
      final filters = NostrFilters.codingSessionCreates(_channelId);
      expect(filters, hasLength(3));
      expect(filters.map((filter) => filter.kinds).toList(), [
        [44221],
        [44224],
        [44226],
      ]);
      expect(filters.every((filter) => filter.limit == 1000), isTrue);
    });

    test('names, goals and closures each read their own kind', () {
      expect(NostrFilters.codingSessionNames(_channelId).kinds, [44229]);
      expect(NostrFilters.codingSessionGeneratedTitles(_channelId).kinds, [
        44252,
      ]);
      expect(NostrFilters.codingSessionGoals(_channelId).kinds, [44227]);
      expect(NostrFilters.codingSessionClosures(_channelId).kinds, [44230]);
    });

    test('leases read a single unpaginated page', () {
      final filter = NostrFilters.codingSessionLeases(_channelId);
      expect(filter.kinds, [24223]);
      expect(filter.limit, 1000);
      expect(filter.until, isNull);
    });

    test('the roster reads transitions and relay receipts at 500 each', () {
      final filters = NostrFilters.codingSessionRoster(_channelId);
      expect(filters.map((filter) => filter.kinds).toList(), [
        [44228],
        [40099],
      ]);
      expect(filters.every((filter) => filter.limit == 500), isTrue);
    });
  });
}
