import 'dart:convert';

import 'package:buzz/features/terminals/state/shell_announce_head_provider.dart';
import 'package:buzz/features/terminals/state/shell_observer_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
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

Future<void> _settle([int rounds = 6]) async {
  for (var i = 0; i < rounds; i++) {
    await Future<void>.delayed(Duration.zero);
  }
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
  test('subscribes to the owner\'s frames and publishes a watch', () async {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    await _settle();

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

  test('frames become writes: a snap resizes and paints, a diff appends, an '
      'end ends', () async {
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
    await _settle();

    h.relay.emit(_frame(type: 'snap', seq: 1, dims: '24x80', text: 'one'));
    h.relay.emit(_frame(type: 'diff', seq: 2, text: 'two'));
    await _settle();

    // A snap with dims is two writes: the grid, then the bytes.
    expect(writes.map((w) => w.resize?.cols), [80, null, null]);
    expect(writes.map((w) => w.text), [null, 'one', 'two']);
    final state = h.container.read(shellObserverProvider(target));
    expect(state.status, ShellObserverStatus.live);
    expect(state.framesApplied, 2);

    h.relay.emit(_frame(type: 'end', seq: 3));
    await _settle();
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.ended,
    );
  });

  test('a seq gap asks the owner for a resync', () async {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    await _settle();
    h.relay.emit(_frame(type: 'snap', seq: 1));
    h.relay.emit(_frame(type: 'diff', seq: 4));
    await _settle();

    final actions = h.relay.published
        .where((event) => event.kind == 24310)
        .map((event) => _watchContent(event)['action'])
        .toList();
    expect(actions, ['watch', 'resync']);
  });

  test('another member\'s frame for the same session is ignored', () async {
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
    await _settle();
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
    await _settle();
    expect(writes, isEmpty);
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.connecting,
    );
  });

  test('no frame within the handshake reads "Not streaming"; the keepalive '
      'keeps watching', () async {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    await Future<void>.delayed(const Duration(milliseconds: 150));
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.stalled,
    );
    final watches = h.relay.published
        .where((event) => event.kind == 24310)
        .length;
    expect(watches, greaterThanOrEqualTo(3));
  });

  test('a watch the relay refuses is disclosed verbatim', () async {
    final h = _harness(
      publishResults: [Exception('restricted: not a project member')],
    );
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    addTearDown(listener.close);
    await _settle();
    expect(
      h.container.read(shellObserverProvider(target)).lastWatchError,
      'restricted: not a project member',
    );
  });

  test('backgrounding stops the watch; resuming watches again and treats '
      'the next diff as a gap', () async {
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
    await _settle();
    h.relay.emit(_frame(type: 'snap', seq: 1));
    await _settle();
    expect(h.relay.listenerCount, 1);

    h.lifecycle.set(AppLifecycleState.paused);
    await _settle();
    expect(h.relay.listenerCount, 0);
    expect(
      h.container.read(shellObserverProvider(target)).status,
      ShellObserverStatus.ended,
    );
    final watchesWhilePaused = h.relay.published.length;

    h.lifecycle.set(AppLifecycleState.resumed);
    await _settle();
    expect(h.relay.listenerCount, 1);
    expect(h.relay.published.length, greaterThan(watchesWhilePaused));
    expect(_watchContent(h.relay.published.last)['action'], 'watch');

    // A diff continuing the old sequence is a gap now: no write, a resync.
    h.relay.emit(_frame(type: 'diff', seq: 2));
    await _settle();
    expect(writes.length, 1);
    expect(_watchContent(h.relay.published.last)['action'], 'resync');
  });

  test('disposing says goodbye with a stop', () async {
    final h = _harness();
    final listener = h.container.listen(
      shellObserverProvider(target),
      (_, _) {},
    );
    await _settle();
    listener.close();
    await _settle();
    expect(_watchContent(h.relay.published.last)['action'], 'stop');
    expect(h.relay.listenerCount, 0);
  });

  test('the announce head is read once and follows a close live', () async {
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
    await _settle();

    expect(relay.historyFilters.single.kinds, [30623]);
    expect(relay.historyFilters.single.authors, [owner]);
    expect(relay.historyFilters.single.tags['#d'], ['s1']);
    final head = container.read(shellAnnounceHeadProvider(target));
    expect(head.hasRead, isTrue);
    expect(head.terminal?.title, 'build shell');

    relay.emit(_announce(status: 'closed', createdAt: 200));
    await _settle();
    final closed = container.read(shellAnnounceHeadProvider(target));
    expect(closed.hasRead, isTrue);
    expect(closed.terminal, isNull);
  });
}
