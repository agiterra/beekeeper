import 'package:beekeeper/shared/relay/relay_send_budget.dart';
import 'package:flutter_test/flutter_test.dart';

import 'relay_session_test_support.dart';

void main() {
  test(
    'the constants describe half of the relay burst with a write reserve',
    () {
      expect(RelaySendBudget.deviceShare, 2);
      expect(RelaySendBudget.capacity, 25);
      expect(RelaySendBudget.writeReserve, 8);
      expect(RelaySendBudget.window, const Duration(seconds: 5));
      expect(RelaySendBudget.eventsPerMinute, 60);
    },
  );

  test('reads stop at the write reserve; writes may spend the rest', () {
    final clock = FakeClock();
    final budget = RelaySendBudget(now: () => clock.now);

    for (var i = 0; i < 17; i++) {
      expect(budget.tryAcquire(RelaySendLane.read), isTrue, reason: 'read $i');
    }
    expect(budget.tryAcquire(RelaySendLane.read), isFalse);
    expect(budget.tryAcquire(RelaySendLane.ephemeral), isFalse);
    expect(budget.available(RelaySendLane.write), 8);
    for (var i = 0; i < 8; i++) {
      expect(
        budget.tryAcquire(RelaySendLane.write),
        isTrue,
        reason: 'write $i',
      );
    }
    expect(budget.tryAcquire(RelaySendLane.write), isFalse);
    expect(budget.writeLaneBelowReserve, isTrue);
  });

  test('the window slides: a slot returns 5 s after its charge', () {
    final clock = FakeClock();
    final budget = RelaySendBudget(now: () => clock.now);

    for (var i = 0; i < 17; i++) {
      budget.tryAcquire(RelaySendLane.read);
      clock.advance(const Duration(milliseconds: 100));
    }
    expect(budget.available(RelaySendLane.read), 0);
    clock.advance(const Duration(milliseconds: 3200));
    expect(budget.available(RelaySendLane.read), 0);
    clock.advance(const Duration(milliseconds: 100));
    expect(budget.available(RelaySendLane.read), 1);
    clock.advance(const Duration(seconds: 5));
    expect(budget.available(RelaySendLane.read), 17);
  });

  test('acquire waits for the window and serves writes before reads', () async {
    final clock = FakeClock();
    final timers = ManualTimers();
    final budget = RelaySendBudget(
      now: () => clock.now,
      timerFactory: timers.factory,
    );
    for (var i = 0; i < 25; i++) {
      budget.tryAcquire(RelaySendLane.write);
    }

    final order = <String>[];
    final read = budget
        .acquire(RelaySendLane.read)
        .then((_) => order.add('read'));
    final write = budget
        .acquire(RelaySendLane.write)
        .then((_) => order.add('write'));
    await settle();
    expect(order, isEmpty);
    expect(timers.active, hasLength(1));
    expect(
      timers.active.single.duration,
      const Duration(seconds: 5, milliseconds: 1),
    );

    clock.advance(const Duration(seconds: 5, milliseconds: 1));
    timers.fireAll();
    await Future.wait([read, write]);
    expect(order, ['write', 'read']);
  });

  test('a waiting read does not block a write that can go now', () async {
    final clock = FakeClock();
    final timers = ManualTimers();
    final budget = RelaySendBudget(
      now: () => clock.now,
      timerFactory: timers.factory,
    );
    for (var i = 0; i < 17; i++) {
      budget.tryAcquire(RelaySendLane.read);
    }

    var readDone = false;
    final read = budget
        .acquire(RelaySendLane.read)
        .then((_) => readDone = true);
    var writeDone = false;
    await budget.acquire(RelaySendLane.write).then((_) => writeDone = true);
    expect(writeDone, isTrue);
    expect(readDone, isFalse);

    clock.advance(const Duration(seconds: 6));
    timers.fireAll();
    await read;
    expect(readDone, isTrue);
  });

  test('EVENT frames also count against the per-minute counter', () {
    final clock = FakeClock();
    final budget = RelaySendBudget(now: () => clock.now);

    for (var i = 0; i < 60; i++) {
      expect(
        budget.tryAcquire(RelaySendLane.write),
        isTrue,
        reason: 'event $i',
      );
      if (i % 20 == 19) clock.advance(const Duration(seconds: 6));
    }
    expect(budget.tryAcquire(RelaySendLane.write), isFalse);
    expect(budget.tryAcquire(RelaySendLane.ephemeral), isFalse);
    expect(budget.available(RelaySendLane.read), 17);
    clock.advance(const Duration(seconds: 60));
    expect(budget.tryAcquire(RelaySendLane.write), isTrue);
  });

  test('ephemerals leave the write reserve of the minute counter alone', () {
    final clock = FakeClock();
    final budget = RelaySendBudget(now: () => clock.now);

    var sent = 0;
    while (budget.tryAcquire(RelaySendLane.ephemeral)) {
      sent++;
      clock.advance(const Duration(milliseconds: 300));
    }
    expect(sent, 52);
    expect(budget.available(RelaySendLane.write), greaterThanOrEqualTo(8));
  });

  test('reset releases every waiter and forgets the charges', () async {
    final clock = FakeClock();
    final timers = ManualTimers();
    final budget = RelaySendBudget(
      now: () => clock.now,
      timerFactory: timers.factory,
    );
    for (var i = 0; i < 25; i++) {
      budget.tryAcquire(RelaySendLane.write);
    }
    final waiter = budget.acquire(RelaySendLane.read);
    budget.reset();
    await waiter;
    expect(timers.active, isEmpty);
    expect(budget.available(RelaySendLane.read), 17);
  });
}
