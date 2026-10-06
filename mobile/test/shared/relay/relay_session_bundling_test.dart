import 'dart:convert';

import 'package:beekeeper/shared/relay/relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart' as http_testing;
import 'package:nostr/nostr.dart' as nostr;

import 'relay_session_test_support.dart';

const _channelA = 'aaaaaaaa-0000-4000-8000-000000000000';
const _channelB = 'bbbbbbbb-0000-4000-8000-000000000000';

/// A session wired to a recording socket, a fake clock, and manual timers
/// for the gate, the send budget, and the query window.
class _Harness {
  _Harness({http.Client? httpClient}) {
    gate = RelayRateLimitGate(
      now: () => clock.now,
      timerFactory: gateTimers.factory,
    );
    budget = RelaySendBudget(
      now: () => clock.now,
      timerFactory: budgetTimers.factory,
    );
    session = RelaySessionNotifier(
      httpClient: httpClient,
      now: () => clock.now,
      rateLimitGate: gate,
      sendBudget: budget,
      retryTimerFactory: retryTimers.factory,
      queryWindowTimerFactory: queryTimers.factory,
      random: () => 0.5,
    );
    session.debugAttachSocketForTest(socket);
  }

  final clock = FakeClock();
  final socket = RecordingRelaySocket();
  final gateTimers = ManualTimers();
  final budgetTimers = ManualTimers();
  final retryTimers = ManualTimers();
  final queryTimers = ManualTimers();
  late final RelayRateLimitGate gate;
  late final RelaySendBudget budget;
  late final RelaySessionNotifier session;

  /// Subscribe and answer EOSE for the sub id the session will assign.
  Future<void Function()> subscribeReady(
    List<NostrFilter> filters,
    int subNumber, {
    void Function(NostrEvent)? onEvent,
  }) {
    final subscribe = session.subscribeAll(filters, onEvent ?? (_) {});
    session.debugHandleMessage(['EOSE', 'l-$subNumber']);
    return subscribe;
  }

  /// Let the budget window slide past every charge so far.
  void freeBudget() {
    clock.advance(RelaySendBudget.window + const Duration(seconds: 1));
    budgetTimers.fireAll();
  }
}

class _FakeRelayConfigNotifier extends RelayConfigNotifier {
  _FakeRelayConfigNotifier(this._nsec);

  final String _nsec;

  @override
  RelayConfig build() =>
      RelayConfig(baseUrl: 'https://relay.example', nsec: _nsec);
}

void main() {
  test('subscribeAll sends one REQ carrying every filter in order', () async {
    final h = _Harness();
    final f1 = filterForChannel(_channelA);
    final f2 = filterForChannel(_channelB);

    final unsubscribe = await h.subscribeReady([f1, f2], 1);

    expect(h.socket.messages, [
      ['REQ', 'l-1', f1.toJson(), f2.toJson()],
    ]);
    unsubscribe();
    expect(h.socket.messages.last, ['CLOSE', 'l-1']);
  });

  test('a REQ refuses more than ten filters or none', () {
    final h = _Harness();
    final many = [for (var i = 0; i < 11; i++) filterForChannel('c-$i')];
    expect(() => h.session.subscribeAll(many, (_) {}), throwsArgumentError);
    expect(() => h.session.fetchHistoryAll(many), throwsArgumentError);
    expect(() => h.session.fetchHistoryAll(const []), throwsArgumentError);
    expect(h.socket.messages, isEmpty);
  });

  test('fetchHistoryAll resolves with the union at EOSE', () async {
    final h = _Harness();
    final f1 = filterForChannel(_channelA, limit: 10);
    final f2 = filterForChannel(_channelB, limit: 10);

    final history = h.session.fetchHistoryAll([f1, f2]);
    expect(h.socket.messages, [
      ['REQ', 'h-1', f1.toJson(), f2.toJson()],
    ]);
    final inA = testEvent(id: 'a1', channelId: _channelA);
    final inB = testEvent(id: 'b1', channelId: _channelB);
    h.session.debugHandleMessage(['EVENT', 'h-1', inA.toJson()]);
    h.session.debugHandleMessage(['EVENT', 'h-1', inB.toJson()]);
    h.session.debugHandleMessage(['EOSE', 'h-1']);

    expect((await history).map((e) => e.id), ['a1', 'b1']);
    expect(h.socket.messages.last, ['CLOSE', 'h-1']);
  });

  test('reconnect replay carries since on every filter of a bundle', () async {
    final h = _Harness();
    final f1 = filterForChannel(_channelA);
    final f2 = filterForChannel(_channelB);
    await h.subscribeReady([f1, f2], 1);
    h.session.debugHandleMessage([
      'EVENT',
      'l-1',
      testEvent(id: 'a1', createdAt: 100).toJson(),
    ]);
    h.socket.messages.clear();

    await h.session.debugReplayLiveSubscriptions();

    final replay = h.socket.reqs.single;
    expect(replay[1], 'l-1');
    expect(replay, hasLength(4));
    expect((replay[2] as Map<String, dynamic>)['since'], 95);
    expect((replay[3] as Map<String, dynamic>)['since'], 95);
    expect((replay[2] as Map<String, dynamic>)['#h'], [_channelA]);
    expect((replay[3] as Map<String, dynamic>)['#h'], [_channelB]);
  });

  test('no REQ leaves while the rate-limit gate is active', () async {
    final h = _Harness();
    h.gate.activate(4);

    final subscribe = h.session.subscribe(filterForChannel(_channelA), (_) {});
    final history = h.session.fetchHistory(filterForChannel(_channelB));
    await settle();
    expect(h.socket.messages, isEmpty);

    h.clock.advance(const Duration(seconds: 4));
    h.gateTimers.fireAll();
    await settle();
    expect(h.socket.reqs.map((req) => req[1]), ['l-1', 'h-2']);

    h.session.debugHandleMessage(['EOSE', 'l-1']);
    h.session.debugHandleMessage(['EOSE', 'h-2']);
    (await subscribe)();
    expect(await history, isEmpty);
  });

  test(
    'sendEphemeral is false under the gate and when the lane is dry',
    () async {
      final h = _Harness();
      final typing = testEvent(id: 't1', kind: EventKind.typingIndicator);

      expect(h.session.sendEphemeral(typing), isTrue);
      expect(h.socket.events, hasLength(1));

      h.gate.activate(4);
      expect(h.session.sendEphemeral(typing), isFalse);
      expect(h.socket.events, hasLength(1));
      h.clock.advance(const Duration(seconds: 4));
      h.gateTimers.fireAll();

      while (h.budget.tryAcquire(RelaySendLane.ephemeral)) {}
      expect(h.session.sendEphemeral(typing), isFalse);
      expect(h.budget.available(RelaySendLane.write), greaterThanOrEqualTo(8));

      h.session.debugSetSessionStatus(SessionStatus.reconnecting);
      h.freeBudget();
      expect(h.session.sendEphemeral(typing), isFalse);
      expect(h.socket.events, hasLength(1));
    },
  );

  test('an unsolicited rate-limited OK arms the gate', () async {
    final h = _Harness();
    final typing = testEvent(id: 't1', kind: EventKind.typingIndicator);
    expect(h.session.sendEphemeral(typing), isTrue);

    h.session.debugHandleMessage([
      'OK',
      't1',
      false,
      'rate-limited: quota exceeded; retry in 3s',
    ]);

    expect(h.gate.isActive, isTrue);
    expect(h.gateTimers.active.single.duration, const Duration(seconds: 3));
    expect(h.session.sendEphemeral(typing), isFalse);
  });

  test(
    '60 live subscriptions replay with no 5 s window above 25 REQs',
    () async {
      final h = _Harness();
      for (var i = 0; i < 60; i++) {
        if (i % 15 == 0) h.freeBudget();
        await h.subscribeReady([filterForChannel('channel-$i')], i + 1);
      }
      h.socket.messages.clear();
      h.freeBudget();

      final sentAt = <DateTime>[];
      final replay = h.session.debugReplayLiveSubscriptions();
      // Drive the fake clock: whenever the replay parks on the budget, jump
      // to the wake it asked for and fire it.
      for (var round = 0; round < 20; round++) {
        await settle();
        final newSends = h.socket.reqs.length - sentAt.length;
        sentAt.addAll(List.filled(newSends, h.clock.now));
        if (sentAt.length == 60) break;
        final wake = h.budgetTimers.active;
        expect(wake, isNotEmpty, reason: 'replay parked without a wake timer');
        h.clock.advance(wake.first.duration);
        wake.first.fire();
      }
      await replay;

      expect(sentAt, hasLength(60));
      for (var i = 0; i < sentAt.length; i++) {
        final windowStart = sentAt[i];
        final inWindow = sentAt
            .where(
              (at) =>
                  !at.isBefore(windowStart) &&
                  at.difference(windowStart) < RelaySendBudget.window,
            )
            .length;
        expect(inWindow, lessThanOrEqualTo(25), reason: 'window from $i');
      }
      expect(
        h.clock.now.difference(sentAt.first),
        greaterThanOrEqualTo(const Duration(seconds: 10)),
        reason: '60 REQs need at least three 17-wide windows',
      );
    },
  );

  test('replay is visible-first and still complete under pacing', () async {
    final h = _Harness();
    const visible = 'visible-channel';
    for (var i = 0; i < 20; i++) {
      final channelId = i == 19 ? visible : 'channel-$i';
      if (i == 17) h.freeBudget();
      await h.subscribeReady([filterForChannel(channelId)], i + 1);
    }
    final release = h.session.registerVisibleChannel(visible);
    h.socket.messages.clear();
    h.freeBudget();

    final replay = h.session.debugReplayLiveSubscriptions();
    await settle();
    expect(h.socket.reqs, hasLength(17));
    expect((h.socket.reqs.first[2] as Map<String, dynamic>)['#h'], [visible]);

    h.freeBudget();
    await replay;
    expect(h.socket.reqs, hasLength(20));
    release();
  });

  test('two subscribers to the same filter share one REQ', () async {
    final h = _Harness();
    final filter = filterForChannel(_channelA);
    final firstEvents = <NostrEvent>[];
    final secondEvents = <NostrEvent>[];

    final first = h.subscribeReady([filter], 1, onEvent: firstEvents.add);
    final leaveFirst = await first;
    final leaveSecond = await h.session.subscribe(filter, secondEvents.add);
    expect(h.socket.reqs, hasLength(1));

    h.session.debugHandleMessage(['EVENT', 'l-1', testEvent().toJson()]);
    h.session.debugFlushEventBuffer();
    expect(firstEvents, hasLength(1));
    expect(secondEvents, hasLength(1));

    leaveFirst();
    expect(h.socket.messages.where((m) => m.first == 'CLOSE'), isEmpty);
    leaveSecond();
    expect(h.socket.messages.last, ['CLOSE', 'l-1']);
  });

  test('query folds one window into one POST /query and demuxes', () async {
    final keychain = nostr.Keys.generate();
    final bodies = <String>[];
    final inA = testEvent(id: 'a1', channelId: _channelA);
    final inB = testEvent(id: 'b1', channelId: _channelB);
    final h = _Harness(
      httpClient: http_testing.MockClient((request) async {
        bodies.add(request.body);
        return http.Response(jsonEncode([inA.toJson(), inB.toJson()]), 200);
      }),
    );
    final container = ProviderContainer(
      overrides: [
        relaySessionProvider.overrideWith(() => h.session),
        relayConfigProvider.overrideWith(
          () => _FakeRelayConfigNotifier(keychain.nsec),
        ),
      ],
    );
    addTearDown(container.dispose);
    // Keep a listener so the auth-driven rebuild runs eagerly instead of
    // leaving the notifier invalidated (which runs `_dispose` on it).
    final subscription = container.listen(relaySessionProvider, (_, _) {});
    addTearDown(subscription.close);
    await settle();
    h.session.debugAttachSocketForTest(h.socket);

    final a = h.session.query(filterForChannel(_channelA, limit: 10));
    final b = h.session.query(filterForChannel(_channelB, limit: 10));
    expect(bodies, isEmpty);
    h.queryTimers.fireAll();

    expect((await a).map((e) => e.id), ['a1']);
    expect((await b).map((e) => e.id), ['b1']);
    expect(bodies, hasLength(1));
    expect(h.socket.reqs, isEmpty);
  });

  test(
    'a 429 on /query arms the gate and falls back to the read lane',
    () async {
      final keychain = nostr.Keys.generate();
      var calls = 0;
      final h = _Harness(
        httpClient: http_testing.MockClient((request) async {
          calls++;
          return http.Response(
            '{"error":"rate-limited: quota exceeded; retry in 2s"}',
            429,
          );
        }),
      );
      final container = ProviderContainer(
        overrides: [
          relaySessionProvider.overrideWith(() => h.session),
          relayConfigProvider.overrideWith(
            () => _FakeRelayConfigNotifier(keychain.nsec),
          ),
        ],
      );
      addTearDown(container.dispose);
      // Keep a listener so the auth-driven rebuild runs eagerly instead of
      // leaving the notifier invalidated (which runs `_dispose` on it).
      final subscription = container.listen(relaySessionProvider, (_, _) {});
      addTearDown(subscription.close);
      await settle();
      h.session.debugAttachSocketForTest(h.socket);

      final a = h.session.query(filterForChannel(_channelA, limit: 10));
      final b = h.session.query(filterForChannel(_channelB, limit: 10));
      h.queryTimers.fireAll();
      await settle();

      expect(calls, 1);
      expect(h.gate.isActive, isTrue);
      expect(h.socket.reqs, isEmpty, reason: 'the fallback waits for the gate');

      h.clock.advance(const Duration(seconds: 2));
      h.gateTimers.fireAll();
      await settle();
      expect(h.socket.reqs.map((req) => req[1]), ['h-1', 'h-2']);
      h.session.debugHandleMessage([
        'EVENT',
        'h-1',
        testEvent(id: 'a1', channelId: _channelA).toJson(),
      ]);
      h.session.debugHandleMessage(['EOSE', 'h-1']);
      h.session.debugHandleMessage(['EOSE', 'h-2']);
      expect((await a).map((e) => e.id), ['a1']);
      expect(await b, isEmpty);
    },
  );

  test('SessionState is value-equal', () {
    expect(
      const SessionState(status: SessionStatus.connected),
      const SessionState(status: SessionStatus.connected),
    );
    expect(
      const SessionState(status: SessionStatus.connected).hashCode,
      const SessionState(status: SessionStatus.connected).hashCode,
    );
    expect(
      const SessionState(
        status: SessionStatus.reconnecting,
        reconnectAttempt: 1,
      ),
      isNot(
        const SessionState(
          status: SessionStatus.reconnecting,
          reconnectAttempt: 2,
        ),
      ),
    );
  });

  test('willReconnectOnResume follows the background grace period', () {
    final h = _Harness();
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => h.session)],
    );
    addTearDown(container.dispose);
    container.read(relaySessionProvider);
    h.session.debugHandleConnected();
    expect(h.session.willReconnectOnResume, isFalse);

    h.session.onAppPaused();
    h.clock.advance(const Duration(seconds: 3));
    expect(h.session.willReconnectOnResume, isFalse);
    h.clock.advance(const Duration(seconds: 3));
    expect(h.session.willReconnectOnResume, isTrue);

    h.session.debugHandleDisconnected();
    expect(h.session.willReconnectOnResume, isTrue);
  });
}
