import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/state/coding_sessions_state.dart';
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

    test('events naming no generation share one capped bucket', () {
      final store = CodingSessionEventStore(cap: 2);
      store.add(nameEvent(content: 'one'));
      store.add(nameEvent(content: 'two'));
      store.add(nameEvent(content: 'three'));

      expect(
        store.eventsForGeneration(
          CodingSessionEventStore.sessionScopedGeneration,
        ),
        hasLength(2),
      );
      expect(store.length, 2);
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
