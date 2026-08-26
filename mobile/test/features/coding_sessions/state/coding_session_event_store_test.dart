import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/state/coding_sessions_state.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../domain/coding_session_fixtures.dart';

void main() {
  group('CodingSessionEventStore', () {
    test('collapses a replayed event id instead of counting it twice', () {
      final store = CodingSessionEventStore();
      final event = metadataEvent(id: 'a' * 64);

      expect(store.add(event), isTrue);
      expect(store.add(event), isFalse);
      expect(store.length, 1);
    });

    test('retains at most 2000 raw events per generation', () {
      expect(maxCodingSessionEventsPerGeneration, 2000);
      final store = CodingSessionEventStore();
      final generation = target();

      for (var seq = 1; seq <= 2100; seq++) {
        store.add(
          transcriptEvent(
            eventSeq: seq,
            forTarget: generation,
            item: {'kind': 'assistant_text', 'text': 'row $seq'},
          ),
        );
      }

      final retained = store.eventsForGeneration(generation.key);
      expect(retained, hasLength(2000));
      expect(store.length, 2000);
      // The oldest were evicted, never the newest.
      final seqs = [
        for (final event in retained) int.parse(event.getTagValue('cst-seq')!),
      ]..sort();
      expect(seqs.first, 101);
      expect(seqs.last, 2100);
    });

    test('one busy generation never evicts another generation', () {
      final store = CodingSessionEventStore(cap: 3);
      final quiet = target(sessionId: 'quiet');
      final busy = target(sessionId: 'busy');

      store.add(
        transcriptEvent(
          eventSeq: 1,
          forTarget: quiet,
          item: {'kind': 'assistant_text', 'text': 'quiet'},
        ),
      );
      for (var seq = 1; seq <= 10; seq++) {
        store.add(
          transcriptEvent(
            eventSeq: seq,
            forTarget: busy,
            item: {'kind': 'assistant_text', 'text': 'busy $seq'},
          ),
        );
      }

      expect(store.eventsForGeneration(quiet.key), hasLength(1));
      expect(store.eventsForGeneration(busy.key), hasLength(3));
      expect(store.generationCount, 2);
    });

    test('events naming no generation are capped per kind', () {
      final store = CodingSessionEventStore(cap: 2);
      store.add(nameEvent(content: 'one'));
      store.add(nameEvent(content: 'two'));
      store.add(nameEvent(content: 'three'));

      expect(
        store.eventsForGeneration(
          CodingSessionEventStore.kindBucket(EventKind.codingSessionName),
        ),
        hasLength(2),
      );
      expect(store.length, 2);
    });

    test('a flood of receipts never evicts the creates and geneses', () {
      final store = CodingSessionEventStore(cap: 3);
      store.add(genesisEvent(eventId: genesisEventIdA));
      store.add(
        createEvent(
          commandId: 'cmd-1',
          sessionRef: sessionRefA,
          genesisRef: genesisEventIdA,
        ),
      );
      // 44224 carries no cs-target, and D4 decodes a receipt per turn: a busy
      // channel produces these by the thousand.
      for (var index = 0; index < 10; index++) {
        store.add(
          receiptEvent(
            commandId: 'cmd-turn-$index',
            status: 'turn_started',
            turnId: 'turn-$index',
            createdAt: 2000 + index,
          ),
        );
      }

      expect(
        store.events
            .where(
              (event) => event.kind == EventKind.codingSessionLifecycleCommand,
            )
            .length,
        1,
        reason: 'the create resolves the founder and the authority',
      );
      expect(
        store.events
            .where((event) => event.kind == EventKind.codingSessionGenesis)
            .length,
        1,
      );
      expect(
        store.eventsForGeneration(
          CodingSessionEventStore.kindBucket(
            EventKind.codingSessionLifecycleReceipt,
          ),
        ),
        hasLength(3),
      );
    });

    test('a transcript flood never evicts its own status and lease', () {
      final store = CodingSessionEventStore(cap: 3);
      final generation = target();
      // Both carry cs-target, so a single per-generation bucket puts them in
      // the transcript's queue. They are also *older* than the flood, which
      // is exactly what oldest-first eviction throws away first.
      store.add(
        metadataEvent(forTarget: generation, createdAt: 1000, id: 'a' * 64),
      );
      store.add(leaseEvent(forTarget: generation, createdAt: 1001));
      for (var seq = 1; seq <= 10; seq++) {
        store.add(
          transcriptEvent(
            eventSeq: seq,
            forTarget: generation,
            createdAt: 2000 + seq,
            item: {'kind': 'assistant_text', 'text': 'row \$seq'},
          ),
        );
      }

      expect(
        store.events
            .where((event) => event.kind == EventKind.codingSessionMetadata)
            .length,
        1,
        reason: 'the metadata carries the status, title and sessionRef',
      );
      expect(
        store.events
            .where((event) => event.kind == EventKind.codingSessionLease)
            .length,
        1,
        reason: 'the lease is the only proof a provider is answering',
      );
    });

    test('each generation caps its own status and lease facts', () {
      // The separate cap is still *per generation*: one shared metadata
      // bucket would let a second execution's status evict the first's.
      final store = CodingSessionEventStore(cap: 1);
      final one = target(sessionId: 'one');
      final two = target(sessionId: 'two');
      store.add(metadataEvent(forTarget: one, createdAt: 1000, id: 'a' * 64));
      store.add(metadataEvent(forTarget: two, createdAt: 1001, id: 'b' * 64));
      store.add(leaseEvent(forTarget: one, createdAt: 1002, id: 'c' * 64));
      store.add(leaseEvent(forTarget: two, createdAt: 1003, id: 'd' * 64));

      expect(store.length, 4);
      expect(store.generationCount, 4);
    });

    test('a flooded generation still reports its status and reachability', () {
      final store = CodingSessionEventStore(cap: 3);
      final generation = target();
      store.add(receiptEvent(commandId: 'cmd-1', status: 'created'));
      store.add(
        metadataEvent(forTarget: generation, createdAt: 1000, id: 'a' * 64),
      );
      store.add(
        leaseEvent(forTarget: generation, createdAt: 1001, leaseSequence: 1),
      );
      for (var seq = 1; seq <= 10; seq++) {
        store.add(
          transcriptEvent(
            eventSeq: seq,
            forTarget: generation,
            createdAt: 2000 + seq,
            item: {'kind': 'assistant_text', 'text': 'row \$seq'},
          ),
        );
      }

      final view = readCodingSessionChannel(
        channelId: channelId,
        events: store.events,
        verifier: null,
      );
      expect(view.sessions, hasLength(1));
      final session = view.sessions.single;
      expect(session.executions.single.status, CodingSessionStatus.running);
      expect(
        view
            .reachabilityFor(
              session,
              now: DateTime.fromMillisecondsSinceEpoch(1001 * 1000),
            )
            .kind,
        CodingSessionReachabilityKind.reachable,
      );
    });

    test('clear drops everything, including the dedupe set', () {
      final store = CodingSessionEventStore();
      final event = metadataEvent(id: 'b' * 64);
      store.add(event);
      store.clear();

      expect(store.length, 0);
      expect(store.add(event), isTrue);
    });
  });
}
