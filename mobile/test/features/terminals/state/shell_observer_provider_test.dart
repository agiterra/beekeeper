import 'dart:convert';
import 'dart:typed_data';

import 'package:buzz/features/terminals/state/shell_announce_head_provider.dart';
import 'package:buzz/features/terminals/state/shell_observer_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter/widgets.dart' show AppLifecycleState;
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:nostr/nostr.dart' as nostr;

import '../../../helpers/recording_relay_session.dart';

const owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const project = '30621:$owner:beekeeper';

final target = ShellObserverTarget(
  ownerPubkey: owner,
  sessionId: 's1',
  projectRef: project,
);

NostrEvent _frame({
  required String type,
  required int seq,
  String epoch = 'e1',
  String? dims,
  String text = 'hi',
}) => NostrEvent(
  id: 'f-$epoch-$seq',
  pubkey: owner,
  createdAt: 100,
  kind: 24311,
  tags: [
    ['d', 's1'],
    ['a', project],
    ['t', type],
    ['seq', '$seq'],
    ['epoch', epoch],
    if (dims != null) ['dims', dims],
  ],
  content: base64.encode(utf8.encode(text)),
  sig: '',
);

NostrEvent _announce({String status = 'open', int createdAt = 100}) =>
    NostrEvent(
      id: 'a-$status',
      pubkey: owner,
      createdAt: createdAt,
      kind: 30623,
      tags: [
        ['d', 's1'],
        ['a', project],
        ['title', 'build shell'],
        ['status', status],
        ['dims', '24x80'],
      ],
      content: '',
      sig: '',
    );

class _FakeLifecycle extends AppLifecycleNotifier {
  final AppLifecycleState initial;
  _FakeLifecycle(this.initial);

  @override
  AppLifecycleState build() => initial;

  void set(AppLifecycleState next) => state = next;
}

class _FakeConfig extends RelayConfigNotifier {
  final String nsec;
  _FakeConfig(this.nsec);

  @override
  RelayConfig build() =>
      RelayConfig(baseUrl: 'https://relay.example', nsec: nsec);
}

/// Runs [body] in virtual time (`package:fake_async`).
///
/// The observer arms real `Timer`s from [ShellObserverConfig] — a 40 ms
/// keepalive, a 60 ms handshake, a 1 s liveness sweep — so on real time
/// every assertion here races the wall clock. `_settle`'s event-loop turns
/// are not free: on a starved isolate (the full mobile suite runs many test
/// files at once) six of them can outlast the 60 ms handshake, which flips
/// `status` from `connecting` to `stalled` underneath a test that never
/// asked about the handshake at all. Item 208 virtualised only the two
/// keepalive tests; item 217 virtualises the file.
void _fakeTest(String description, void Function(FakeAsync async) body) =>
    test(description, () => fakeAsync(body));

/// Drains microtasks and zero-delay timers without moving the clock, so no
/// configured timer can fire early. The virtual-time `await _settle()`.
void _settle(FakeAsync async, [int rounds = 6]) {
  for (var i = 0; i < rounds; i++) {
    async.elapse(Duration.zero);
  }
}

/// Runs [work] to completion in virtual time — the virtual-time `await`.
void _finish(FakeAsync async, Future<void> work, {int rounds = 6}) {
  var done = false;
  Object? failure;
  work.then<void>(
    (_) => done = true,
    onError: (Object error) => failure = error,
  );
  _settle(async, rounds);
  if (failure != null) throw failure!;
  expect(done, isTrue, reason: 'the future never completed in virtual time');
}

({
  ProviderContainer container,
  RecordingRelaySessionNotifier relay,
  _FakeLifecycle lifecycle,
})
_harness({
  ShellObserverConfig config = const ShellObserverConfig(
    keepalive: Duration(milliseconds: 40),
    handshake: Duration(milliseconds: 60),
    stall: Duration(milliseconds: 80),
  ),
  List<Object> publishResults = const [],
  AppLifecycleState lifecycle = AppLifecycleState.resumed,
}) {
  final relay = RecordingRelaySessionNotifier(publishResults: publishResults);
  final fakeLifecycle = _FakeLifecycle(lifecycle);
  final container = ProviderContainer(
    overrides: [
      relaySessionProvider.overrideWith(() => relay),
      relayConfigProvider.overrideWith(
        () => _FakeConfig(nostr.Keys.generate().nsec),
      ),
      appLifecycleProvider.overrideWith(() => fakeLifecycle),
      shellObserverConfigProvider.overrideWithValue(config),
    ],
  );
  addTearDown(container.dispose);
  return (container: container, relay: relay, lifecycle: fakeLifecycle);
}

Map<String, dynamic> _watchContent(NostrEvent event) =>
    jsonDecode(event.content) as Map<String, dynamic>;

void main() {
  inputTests();
  _fakeTest('subscribes to the owner\'s frames and publishes a watch', (async) {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);

    expect(h.relay.operations.first, 'subscribe');
    final filter = h.relay.liveFilters.single;
    expect(filter.kinds, [24311]);
    expect(filter.authors, [owner]);
    expect(filter.tags['#d'], ['s1']);
    expect(filter.since, isNotNull);

    final watch = h.relay.published.first;
    expect(watch.kind, 24310);
    expect(_watchContent(watch), {'action': 'watch'});
    expect(watch.tags, [
      ['p', owner],
      ['d', 's1'],
      ['a', project],
    ]);
    expect(watch.sig, isNotEmpty);
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.connecting,
    );
  });

  _fakeTest(
    'frames become writes: a snap resizes and paints, a diff appends, an '
    'end ends',
    (async) {
      final h = _harness();
      final notifier = h.container.read(shellObserverProvider(target).notifier);
      final writes = <ShellTerminalWrite>[];
      final sub = notifier.writes.listen(writes.add);
      addTearDown(sub.cancel);
      final listener = h.container.listen(
        shellObserverProvider(target),
        (_, _) {},
      );
      addTearDown(listener.close);
      _settle(async);

      h.relay.emit(_frame(type: 'snap', seq: 1, dims: '24x80', text: 'one'));
      h.relay.emit(_frame(type: 'diff', seq: 2, text: 'two'));
      _settle(async);

      // A snap with dims is two writes: the grid, then the bytes.
      expect(writes.map((w) => w.resize?.cols), [80, null, null]);
      expect(writes.map((w) => w.text), [null, 'one', 'two']);
      final state = h.container.read(shellObserverProvider(target));
      expect(state.status, ShellObserverStatus.live);
      expect(state.framesApplied, 2);

      h.relay.emit(_frame(type: 'end', seq: 3));
      _settle(async);
      expect(
        h.container.read(shellObserverProvider(target)).status,
        ShellObserverStatus.ended,
      );
    },
  );

  _fakeTest('a seq gap asks the owner for a resync', (async) {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    h.relay.emit(_frame(type: 'snap', seq: 1));
    h.relay.emit(_frame(type: 'diff', seq: 4));
    _settle(async);

    final actions = h.relay.published
        .where((event) => event.kind == 24310)
        .map((event) => _watchContent(event)['action'])
        .toList();
    expect(actions, ['watch', 'resync']);
  });

  _fakeTest('another member\'s frame for the same session is ignored', (async) {
    final h = _harness();
    final notifier = h.container.read(shellObserverProvider(target).notifier);
    final writes = <ShellTerminalWrite>[];
    final sub = notifier.writes.listen(writes.add);
    addTearDown(sub.cancel);
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    final forged = _frame(type: 'snap', seq: 1);
    h.relay.emit(
      NostrEvent(
        id: forged.id,
        pubkey:
            '1111111122222222333333334444444455555555666666667777777788888888',
        createdAt: forged.createdAt,
        kind: forged.kind,
        tags: forged.tags,
        content: forged.content,
        sig: '',
      ),
    );
    _settle(async);
    expect(writes, isEmpty);
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.connecting,
    );
  });

  _fakeTest(
    'no frame within the handshake reads "Not streaming"; the keepalive '
    'keeps watching',
    (async) {
      final h = _harness();
      final listener = h.container.listen(
        shellObserverProvider(target),
        (_, _) {},
      );
      addTearDown(listener.close);
      async.elapse(const Duration(milliseconds: 150));
      expect(
        h.container.read(shellObserverProvider(target)).status,
        ShellObserverStatus.stalled,
      );
      // The opening watch is published and waited on; every later beat is a
      // droppable ephemeral carrying the identical kind:24310 payload.
      final opening = h.relay.published.where((event) => event.kind == 24310);
      expect(opening, hasLength(1));
      final beats = h.relay.ephemeralEvents
          .where((event) => event.kind == 24310)
          .toList();
      expect(beats.length, greaterThanOrEqualTo(2));
      for (final beat in beats) {
        expect(beat.content, opening.single.content);
        expect(beat.tags, opening.single.tags);
        expect(beat.pubkey, opening.single.pubkey);
      }
      expect(_watchContent(beats.first)['action'], 'watch');
    },
  );

  _fakeTest('a dropped keepalive beat is not retried as a publish', (async) {
    final h = _harness();
    h.relay.acceptEphemeral = false;
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    async.elapse(const Duration(milliseconds: 150));
    expect(
      h.relay.ephemeralEvents.where((event) => event.kind == 24310).length,
      greaterThanOrEqualTo(2),
      reason: 'beats keep being offered; the transport decides',
    );
    expect(
      h.relay.published.where((event) => event.kind == 24310),
      hasLength(1),
      reason: 'only the opening watch is a publish',
    );
  });

  _fakeTest('a watch the relay refuses is disclosed verbatim', (async) {
    final h = _harness(
      publishResults: [Exception('restricted: not a project member')],
    );
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    expect(
      h.container.read(shellObserverProvider(target)).lastWatchError,
      'restricted: not a project member',
    );
  });

  _fakeTest('backgrounding stops the watch; resuming watches again and treats '
      'the next diff as a gap', (async) {
    final h = _harness();
    final notifier = h.container.read(shellObserverProvider(target).notifier);
    final writes = <ShellTerminalWrite>[];
    final sub = notifier.writes.listen(writes.add);
    addTearDown(sub.cancel);
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    h.relay.emit(_frame(type: 'snap', seq: 1));
    _settle(async);
    expect(h.relay.listenerCount, 1);

    h.lifecycle.set(AppLifecycleState.paused);
    _settle(async);
    expect(h.relay.listenerCount, 0);
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.ended,
    );
    final watchesWhilePaused = h.relay.published.length;

    h.lifecycle.set(AppLifecycleState.resumed);
    _settle(async);
    expect(h.relay.listenerCount, 1);
    expect(h.relay.published.length, greaterThan(watchesWhilePaused));
    expect(_watchContent(h.relay.published.last)['action'], 'watch');

    // A diff continuing the old sequence is a gap now: no write, a resync.
    h.relay.emit(_frame(type: 'diff', seq: 2));
    _settle(async);
    expect(writes.length, 1);
    expect(_watchContent(h.relay.published.last)['action'], 'resync');
  });

  _fakeTest('disposing says goodbye with a stop', (async) {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    _settle(async);
    listener.close();
    _settle(async);
    expect(_watchContent(h.relay.published.last)['action'], 'stop');
    expect(h.relay.listenerCount, 0);
  });

  _fakeTest('the announce head is read once and follows a close live', (async) {
    final relay = RecordingRelaySessionNotifier(
      historyResults: [
        [_announce()],
      ],
    );
    final container = ProviderContainer(
      overrides: [relaySessionProvider.overrideWith(() => relay)],
    );
    addTearDown(container.dispose);
    final listener = container.listen(
      shellAnnounceHeadProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);

    expect(relay.historyFilters.single.kinds, [30623]);
    expect(relay.historyFilters.single.authors, [owner]);
    expect(relay.historyFilters.single.tags['#d'], ['s1']);
    final head = container.read(shellAnnounceHeadProvider(target));
    expect(head.hasRead, isTrue);
    expect(head.terminal?.title, 'build shell');

    relay.emit(_announce(status: 'closed', createdAt: 200));
    _settle(async);
    final closed = container.read(shellAnnounceHeadProvider(target));
    expect(closed.hasRead, isTrue);
    expect(closed.terminal, isNull);
  });
}

void inputTests() {
  _fakeTest(
    'a line is one input event: the text plus a carriage return, base64',
    (async) {
      final h = _harness();
      final listener = h.container.listen(
        shellObserverProvider(target),
        (_, _) {},
      );
      addTearDown(listener.close);
      _settle(async);
      final notifier = h.container.read(shellObserverProvider(target).notifier);

      _finish(async, notifier.sendLine('ls -la'));

      final inputs = h.relay.published.where((e) => e.kind == 24312).toList();
      expect(inputs.length, 1);
      expect(utf8.decode(base64.decode(inputs.single.content)), 'ls -la\r');
      expect(inputs.single.tags, [
        ['p', owner],
        ['d', 's1'],
        ['a', project],
      ]);
      expect(h.container.read(shellObserverProvider(target)).inputsSent, 1);
    },
  );

  _fakeTest('a large paste is chunked in order under the cap', (async) {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    final notifier = h.container.read(shellObserverProvider(target).notifier);
    final big = List<int>.generate(13000, (i) => 0x61 + (i % 26));

    _finish(async, notifier.sendInput(Uint8List.fromList(big)));

    final inputs = h.relay.published.where((e) => e.kind == 24312).toList();
    expect(inputs.length, 3);
    final joined = inputs.expand((e) => base64.decode(e.content)).toList();
    expect(joined, big);
    for (final input in inputs) {
      expect(input.content.length, lessThanOrEqualTo(8 * 1024));
    }
  });

  _fakeTest(
    'a restricted answer is a revocation: kept verbatim, no more sends',
    (async) {
      final h = _harness(
        publishResults: [
          // The first publish is the watch; the second is the input.
          NostrEvent(
            id: 'ok',
            pubkey: owner,
            createdAt: 1,
            kind: 24310,
            tags: const [],
            content: '',
            sig: '',
          ),
          Exception('restricted: not a collaborator on this session'),
        ],
      );
      final listener = h.container.listen(
        shellObserverProvider(target),
        (_, _) {},
      );
      addTearDown(listener.close);
      _settle(async);
      final notifier = h.container.read(shellObserverProvider(target).notifier);

      _finish(async, notifier.sendLine('whoami'));
      expect(
        h.container.read(shellObserverProvider(target)).inputRefused,
        'restricted: not a collaborator on this session',
      );
      final before = h.relay.published.length;
      _finish(async, notifier.sendLine('again'));
      expect(h.relay.published.length, before, reason: 'revoked: nothing sent');
    },
  );

  _fakeTest('a rate-limited answer pauses input for the relay\'s window and is '
      'not a revocation', (async) {
    final h = _harness(
      publishResults: [
        NostrEvent(
          id: 'ok',
          pubkey: owner,
          createdAt: 1,
          kind: 24310,
          tags: const [],
          content: '',
          sig: '',
        ),
        Exception('rate-limited: slow down, retry in 7s'),
      ],
    );
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    _settle(async);
    final notifier = h.container.read(shellObserverProvider(target).notifier);

    _finish(async, notifier.sendLine('make'));
    final state = h.container.read(shellObserverProvider(target));
    expect(state.inputRefused, isNull);
    // The pause window is the one thing here the provider keeps on the wall
    // clock (`inputPausedUntil = DateTime.now().add(retry)`), which
    // `fakeAsync` does not virtualise. Virtual time makes that safer, not
    // less so: no real time passes between the provider's read and these,
    // so the 7 s window is still ~7 s wide.
    expect(state.inputPausedAt(DateTime.now()), isTrue);
    expect(
      state.inputPausedUntil!.difference(DateTime.now()).inSeconds,
      inInclusiveRange(5, 7),
    );
    final before = h.relay.published.length;
    _finish(async, notifier.sendLine('again'));
    expect(h.relay.published.length, before, reason: 'paused: nothing sent');
  });
}
