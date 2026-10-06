import 'dart:async';

import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:beekeeper/shared/relay/relay_subscription_registry.dart';
import 'package:flutter_test/flutter_test.dart';

import 'relay_session_test_support.dart';

class _Opened {
  _Opened(this.filters, this.onEvent, this.onClosed);

  final List<NostrFilter> filters;
  final void Function(NostrEvent) onEvent;
  final void Function(String message)? onClosed;
  final Completer<void> ready = Completer<void>();
  int closes = 0;
}

class _Harness {
  final List<_Opened> opened = [];
  late final RelaySubscriptionRegistry registry = RelaySubscriptionRegistry(
    open: (filters, onEvent, {onClosed}) async {
      final entry = _Opened(filters, onEvent, onClosed);
      opened.add(entry);
      await entry.ready.future;
      return () => entry.closes++;
    },
  );
}

void main() {
  final filter = filterForChannel(testChannelId);

  test(
    'identical filter lists share one subscription until the last leaves',
    () async {
      final h = _Harness();
      final firstEvents = <NostrEvent>[];
      final secondEvents = <NostrEvent>[];

      final first = h.registry.join([filter], firstEvents.add);
      final second = h.registry.join([filter], secondEvents.add);
      expect(h.opened, hasLength(1));
      expect(h.registry.refCount([filter]), 2);

      h.opened.single.ready.complete();
      final leaveFirst = await first;
      final leaveSecond = await second;

      h.opened.single.onEvent(testEvent(id: 'e1'));
      expect(firstEvents.map((e) => e.id), ['e1']);
      expect(secondEvents.map((e) => e.id), ['e1']);

      leaveFirst();
      expect(h.opened.single.closes, 0);
      expect(h.registry.refCount([filter]), 1);
      h.opened.single.onEvent(testEvent(id: 'e2'));
      expect(firstEvents, hasLength(1));
      expect(secondEvents, hasLength(2));

      leaveSecond();
      expect(h.opened.single.closes, 1);
      expect(h.registry.refCount([filter]), 0);
      expect(h.registry.entryCount, 0);
      leaveSecond();
      expect(h.opened.single.closes, 1, reason: 'leaving twice is idempotent');
    },
  );

  test('a fresh join after the last leave opens a new subscription', () async {
    final h = _Harness();
    final leave = h.registry.join([filter], (_) {});
    h.opened.single.ready.complete();
    (await leave)();

    final again = h.registry.join([filter], (_) {});
    expect(h.opened, hasLength(2));
    h.opened.last.ready.complete();
    await again;
  });

  test('the key ignores filter and clause order but not since or limit', () {
    final a = NostrFilter(
      kinds: const [1, 2],
      tags: const {
        '#h': ['x', 'y'],
      },
      since: 5,
      limit: 0,
    );
    final b = NostrFilter(
      kinds: const [2, 1],
      tags: const {
        '#h': ['y', 'x'],
      },
      since: 5,
      limit: 0,
    );
    final other = filterForChannel('z');
    expect(
      RelaySubscriptionRegistry.keyFor([a]),
      RelaySubscriptionRegistry.keyFor([b]),
    );
    expect(
      RelaySubscriptionRegistry.keyFor([a, other]),
      RelaySubscriptionRegistry.keyFor([other, b]),
    );
    expect(
      RelaySubscriptionRegistry.keyFor([a]),
      isNot(RelaySubscriptionRegistry.keyFor([a.copyWithSince(6)])),
    );
    expect(
      RelaySubscriptionRegistry.keyFor([filterForChannel('x', limit: 0)]),
      isNot(
        RelaySubscriptionRegistry.keyFor([filterForChannel('x', limit: 5)]),
      ),
    );
  });

  test(
    'a late joiner to a backlog subscription replays what was delivered',
    () async {
      final h = _Harness();
      final withBacklog = filterForChannel(testChannelId, limit: 50);
      final first = h.registry.join([withBacklog], (_) {});
      h.opened.single.onEvent(testEvent(id: 'old-1', createdAt: 1));
      h.opened.single.ready.complete();
      await first;
      h.opened.single.onEvent(testEvent(id: 'live-1', createdAt: 2));

      final lateEvents = <NostrEvent>[];
      final late = h.registry.join([withBacklog], lateEvents.add);
      expect(lateEvents.map((e) => e.id), ['old-1', 'live-1']);
      await late;
      expect(h.opened, hasLength(1));
    },
  );

  test('a limit-0 subscription retains nothing for late joiners', () async {
    final h = _Harness();
    final first = h.registry.join([filter], (_) {});
    h.opened.single.ready.complete();
    await first;
    h.opened.single.onEvent(testEvent(id: 'live-1'));

    final lateEvents = <NostrEvent>[];
    await h.registry.join([filter], lateEvents.add);
    expect(lateEvents, isEmpty);
  });

  test('a terminal CLOSED reaches every sharer and drops the entry', () async {
    final h = _Harness();
    final closed = <String>[];
    final a = h.registry.join([filter], (_) {}, onClosed: closed.add);
    final b = h.registry.join([filter], (_) {}, onClosed: closed.add);
    h.opened.single.ready.complete();
    await Future.wait([a, b]);

    h.opened.single.onClosed!('restricted: gone');
    expect(closed, ['restricted: gone', 'restricted: gone']);
    expect(h.registry.entryCount, 0);
  });

  test('an open failure fails every joiner and leaves no entry', () async {
    final h = _Harness();
    final a = h.registry.join([filter], (_) {});
    final b = h.registry.join([filter], (_) {});
    h.opened.single.ready.completeError(Exception('nope'));
    await expectLater(a, throwsException);
    await expectLater(b, throwsException);
    expect(h.registry.entryCount, 0);
  });

  test('leaving while the open is in flight closes it once it lands', () async {
    final h = _Harness();
    final join = h.registry.join([filter], (_) {});
    // Nobody can leave before join resolves, so resolve, leave, and make
    // sure a second concurrent joiner that leaves first does not close.
    final other = h.registry.join([filter], (_) {});
    h.opened.single.ready.complete();
    final leaveJoin = await join;
    final leaveOther = await other;
    leaveOther();
    expect(h.opened.single.closes, 0);
    leaveJoin();
    expect(h.opened.single.closes, 1);
  });

  test('clear drops entries without sending CLOSE', () async {
    final h = _Harness();
    final leave = h.registry.join([filter], (_) {});
    h.opened.single.ready.complete();
    final leaveFn = await leave;
    h.registry.clear();
    expect(h.registry.entryCount, 0);
    leaveFn();
    expect(h.opened.single.closes, 0);
  });
}
