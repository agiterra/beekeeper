import 'dart:async';

import 'package:buzz/shared/relay/relay.dart';

/// A socket that records every frame the session sends.
class RecordingRelaySocket extends RelaySocket {
  RecordingRelaySocket()
    : super(
        wsUrl: 'wss://relay.example',
        nsec: null,
        onMessage: (_) {},
        onConnected: () {},
        onDisconnected: (_) {},
      );

  final List<List<dynamic>> messages = [];

  @override
  void send(List<dynamic> payload) => messages.add(payload);

  @override
  void dispose() {}

  /// Every `REQ` frame sent so far.
  List<List<dynamic>> get reqs =>
      messages.where((message) => message.first == 'REQ').toList();

  /// Every `EVENT` frame sent so far.
  List<List<dynamic>> get events =>
      messages.where((message) => message.first == 'EVENT').toList();
}

/// A timer the test fires by hand.
class ManualTimer implements Timer {
  ManualTimer(this.duration, this._callback);

  final Duration duration;
  final void Function() _callback;
  bool _active = true;

  void fire() {
    if (!_active) return;
    _active = false;
    _callback();
  }

  @override
  void cancel() => _active = false;

  @override
  bool get isActive => _active;

  @override
  int get tick => _active ? 0 : 1;
}

/// Collects the timers a component asks for, so a test can fire them.
class ManualTimers {
  final List<ManualTimer> created = [];

  Timer factory(Duration duration, void Function() callback) {
    final timer = ManualTimer(duration, callback);
    created.add(timer);
    return timer;
  }

  /// Timers not yet fired or cancelled.
  List<ManualTimer> get active =>
      created.where((timer) => timer.isActive).toList();

  /// Fire every active timer once, in creation order.
  void fireAll() {
    for (final timer in active) {
      timer.fire();
    }
  }
}

/// A clock the test advances by hand.
class FakeClock {
  FakeClock([DateTime? start]) : now = start ?? DateTime(2026, 9, 7, 12);

  DateTime now;

  void advance(Duration duration) => now = now.add(duration);
}

const testChannelId = '11111111-1111-4111-8111-111111111111';

NostrFilter filterForChannel(String channelId, {int limit = 0}) => NostrFilter(
  kinds: EventKind.channelEventKinds,
  tags: {
    '#h': [channelId],
  },
  limit: limit,
);

NostrEvent testEvent({
  int createdAt = 20,
  String id = 'event-1',
  String channelId = testChannelId,
  int kind = EventKind.streamMessageV2,
  String pubkey = 'alice',
  List<List<String>> extraTags = const [],
}) {
  return NostrEvent(
    id: id,
    pubkey: pubkey,
    createdAt: createdAt,
    kind: kind,
    tags: [
      ['h', channelId],
      ...extraTags,
    ],
    content: 'hello',
    sig: 'sig',
  );
}

/// Drain microtasks and zero-delay timers.
Future<void> settle() => Future<void>.delayed(Duration.zero);
