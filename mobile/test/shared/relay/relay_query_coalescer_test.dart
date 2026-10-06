import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:beekeeper/shared/relay/relay_query_coalescer.dart';
import 'package:flutter_test/flutter_test.dart';

import 'relay_session_test_support.dart';

const _channelA = 'aaaaaaaa-0000-4000-8000-000000000000';
const _channelB = 'bbbbbbbb-0000-4000-8000-000000000000';

class _Harness {
  final ManualTimers timers = ManualTimers();
  final List<List<NostrFilter>> sentBodies = [];
  final List<NostrFilter> fallbacks = [];
  List<NostrEvent> Function(List<NostrFilter> filters) answer = (_) => [];
  Object? sendError;
  late final RelayQueryCoalescer coalescer = RelayQueryCoalescer(
    send: (filters) async {
      sentBodies.add(filters);
      final error = sendError;
      if (error != null) throw error;
      return answer(filters);
    },
    fallback: (filter) async {
      fallbacks.add(filter);
      return [testEvent(id: 'fallback-${fallbacks.length}')];
    },
    timerFactory: timers.factory,
  );

  /// Close the open window: fire the 50 ms timer and drain the sends.
  Future<void> flush() async {
    timers.fireAll();
    await settle();
  }
}

NostrFilter _channelFilter(String channelId, {int limit = 100}) => NostrFilter(
  kinds: const [40002],
  tags: {
    '#h': [channelId],
  },
  limit: limit,
);

void main() {
  test(
    'three callers in one window share one body and get their own matches',
    () async {
      final h = _Harness();
      final inA = testEvent(id: 'a1', channelId: _channelA, kind: 40002);
      final inB = testEvent(id: 'b1', channelId: _channelB, kind: 40002);
      final profile = NostrEvent(
        id: 'p1',
        pubkey: 'alice',
        createdAt: 5,
        kind: 0,
        tags: const [],
        content: '{}',
        sig: '',
      );
      h.answer = (_) => [inA, inB, profile];

      final a = h.coalescer.query(_channelFilter(_channelA));
      final b = h.coalescer.query(_channelFilter(_channelB));
      final c = h.coalescer.query(
        const NostrFilter(kinds: [0], authors: ['alice']),
      );
      expect(h.sentBodies, isEmpty);
      expect(h.timers.active.single.duration, const Duration(milliseconds: 50));

      await h.flush();

      expect(h.sentBodies, hasLength(1));
      expect(h.sentBodies.single, hasLength(3));
      expect((await a).map((e) => e.id), ['a1']);
      expect((await b).map((e) => e.id), ['b1']);
      expect((await c).map((e) => e.id), ['p1']);
    },
  );

  test('a caller arriving after the window closes opens a new one', () async {
    final h = _Harness();
    final first = h.coalescer.query(_channelFilter(_channelA));
    await h.flush();
    final second = h.coalescer.query(_channelFilter(_channelB));
    await h.flush();
    await Future.wait([first, second]);
    expect(h.sentBodies, hasLength(2));
  });

  test(
    '130 aggregate #h split into two calls, never splitting a filter',
    () async {
      final h = _Harness();
      final big = NostrFilter(
        kinds: const [40002],
        tags: {
          '#h': [for (var i = 0; i < 100; i++) 'c-$i'],
        },
      );
      final medium = NostrFilter(
        kinds: const [40002],
        tags: {
          '#h': [for (var i = 0; i < 28; i++) 'd-$i'],
        },
      );
      final small = _channelFilter(_channelA);
      final tiny = _channelFilter(_channelB);

      final futures = [
        big,
        medium,
        small,
        tiny,
      ].map(h.coalescer.query).toList();
      await h.flush();
      await Future.wait(futures);

      expect(h.sentBodies, hasLength(2));
      expect(h.sentBodies.first, [big, medium]);
      expect(h.sentBodies.last, [small, tiny]);
    },
  );

  test('a search filter bypasses the window and goes alone', () async {
    final h = _Harness();
    final hit = testEvent(id: 's1', kind: 40002);
    h.answer = (_) => [hit];
    const search = NostrFilter(kinds: [40002], search: 'hello');

    final result = h.coalescer.query(search);
    final other = h.coalescer.query(_channelFilter(_channelA));
    await settle();

    expect(h.sentBodies, [
      [search],
    ]);
    expect((await result).map((e) => e.id), ['s1']);
    await h.flush();
    await other;
    expect(h.sentBodies, hasLength(2));
  });

  test('a filter with bridge extensions goes alone too', () async {
    final h = _Harness();
    const paged = NostrFilter(kinds: [0], limit: 50, extensions: {'page': 1});
    final result = h.coalescer.query(paged);
    await settle();
    expect(h.sentBodies, [
      [paged],
    ]);
    await result;
  });

  test(
    'an HTTP failure retries every caller of the chunk over the fallback',
    () async {
      final h = _Harness();
      h.sendError = Exception('429');
      final a = h.coalescer.query(_channelFilter(_channelA));
      final b = h.coalescer.query(_channelFilter(_channelB));
      await h.flush();

      expect(h.sentBodies, hasLength(1));
      expect(h.fallbacks, hasLength(2));
      expect((await a).map((e) => e.id), ['fallback-1']);
      expect((await b).map((e) => e.id), ['fallback-2']);
    },
  );

  test('demux dedupes by id and trims to the caller limit, newest first', () {
    final events = [
      testEvent(id: 'e1', createdAt: 10, kind: 40002, channelId: _channelA),
      testEvent(id: 'e2', createdAt: 30, kind: 40002, channelId: _channelA),
      testEvent(id: 'e2', createdAt: 30, kind: 40002, channelId: _channelA),
      testEvent(id: 'e3', createdAt: 20, kind: 40002, channelId: _channelA),
      testEvent(id: 'x1', createdAt: 40, kind: 40002, channelId: _channelB),
    ];
    final trimmed = RelayQueryCoalescer.demuxQueryResult(
      _channelFilter(_channelA, limit: 2),
      events,
    );
    expect(trimmed.map((e) => e.id), ['e2', 'e3']);

    final all = RelayQueryCoalescer.demuxQueryResult(
      _channelFilter(_channelA, limit: 10),
      events,
    );
    expect(all.map((e) => e.id), ['e1', 'e2', 'e3']);
  });

  test('reset fails callers still waiting for the window', () async {
    final h = _Harness();
    final pending = h.coalescer.query(_channelFilter(_channelA));
    h.coalescer.reset();
    await expectLater(pending, throwsStateError);
    expect(h.timers.active, isEmpty);

    final next = h.coalescer.query(_channelFilter(_channelA));
    await h.flush();
    expect(await next, isEmpty);
  });

  test('chunkByChannelTags keeps order and respects the 128 cap', () {
    final filters = [
      for (var i = 0; i < 5; i++)
        NostrFilter(
          kinds: const [1],
          tags: {
            '#h': [for (var j = 0; j < 40; j++) 'c-$i-$j'],
          },
        ),
    ];
    final chunks = RelayQueryCoalescer.chunkByChannelTags(filters);
    expect(chunks.map((chunk) => chunk.length), [3, 2]);
    expect(chunks.first, filters.sublist(0, 3));
  });

  test(
    'an h-less event never leaks across callers with different channels',
    () async {
      // The relay resolves a reaction with no `h` tag to its stored channel and
      // would return it to exactly one of these filters; the client cannot tell
      // which, so a mixed-scope chunk demuxes strictly and drops it.
      final h = _Harness();
      final reaction = NostrEvent(
        id: 'r1',
        pubkey: 'alice',
        createdAt: 5,
        kind: 7,
        tags: const [],
        content: '+',
        sig: '',
      );
      h.answer = (_) => [reaction];
      final a = h.coalescer.query(
        NostrFilter(
          kinds: const [7],
          tags: {
            '#h': [_channelA],
          },
          limit: 10,
        ),
      );
      final b = h.coalescer.query(
        NostrFilter(
          kinds: const [7],
          tags: {
            '#h': [_channelB],
          },
          limit: 10,
        ),
      );
      await h.flush();
      expect(await a, isEmpty);
      expect(await b, isEmpty);

      // The same channel set across the chunk keeps the permissive read.
      final h2 = _Harness();
      h2.answer = (_) => [reaction];
      final c = h2.coalescer.query(
        NostrFilter(
          kinds: const [7],
          tags: {
            '#h': [_channelA],
          },
          limit: 10,
        ),
      );
      final d = h2.coalescer.query(
        NostrFilter(
          kinds: const [9],
          tags: {
            '#h': [_channelA],
          },
          limit: 10,
        ),
      );
      await h2.flush();
      expect((await c).map((e) => e.id), ['r1']);
      expect(await d, isEmpty);
    },
  );
}
