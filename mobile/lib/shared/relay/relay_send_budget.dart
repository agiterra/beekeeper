import 'dart:async';
import 'dart:collection';
import 'dart:math';

import 'relay_rate_limit_gate.dart';

/// Which relay admission budget a frame is charged to.
enum RelaySendLane {
  /// `REQ` and `COUNT` frames.
  read,

  /// Durable `EVENT` frames the caller waits on (`publish`).
  write,

  /// Fire-and-forget `EVENT` frames (typing, presence, keepalives) that are
  /// dropped rather than queued when the budget is short.
  ephemeral,
}

/// Proactive client-side pacing for every frame the relay meters.
///
/// The relay counts `EVENT`, `REQ` and `COUNT` (never `AUTH` or `CLOSE`)
/// against one burst window of [relayBurstCapacity] per [window] keyed by
/// (community, pubkey) — not per connection — and durable-or-ephemeral
/// `EVENT`s against [eventsPerMinute] per minute on the same key
/// (`crates/buzz-relay/src/admission.rs`). A phone and a desktop paired on
/// one key therefore share both counters, which is why this bucket only
/// claims `1 / deviceShare` of the burst.
///
/// The [write] lane may spend the whole share; [read] and [ephemeral] stop
/// [writeReserve] frames short of it so a history storm can never leave the
/// user unable to send a message. Reads that cannot go now wait ([acquire]);
/// ephemerals never wait ([tryAcquire] answers false and the caller drops
/// the frame).
class RelaySendBudget {
  /// How many devices are assumed to share this key's relay budget.
  ///
  /// Until the relay's per-connection budgets deploy, a paired phone and
  /// desktop draw from one 50-frame window, so each side claims half. Flip
  /// to `1` once `limitation.rate_limits` advertises per-connection reads.
  static const deviceShare = 2;

  /// The relay's burst allowance per [window] per key.
  static const relayBurstCapacity = 50;

  /// Sliding window over which [relayBurstCapacity] applies.
  static const window = Duration(seconds: 5);

  /// This device's share of the burst window.
  static const capacity = relayBurstCapacity ~/ deviceShare;

  /// Frames held back from reads and ephemerals for durable writes.
  static const writeReserve = 8;

  /// The relay's per-key durable + ephemeral `EVENT` allowance per minute.
  static const eventsPerMinute = 60;

  /// Window over which [eventsPerMinute] applies.
  static const eventWindow = Duration(minutes: 1);

  RelaySendBudget({
    DateTime Function()? now,
    RelayTimerFactory timerFactory = Timer.new,
  }) : _now = now ?? DateTime.now,
       _timerFactory = timerFactory;

  final DateTime Function() _now;
  final RelayTimerFactory _timerFactory;
  final Queue<DateTime> _sends = Queue();
  final Queue<DateTime> _events = Queue();
  final List<_Waiter> _waiters = [];
  Timer? _wakeTimer;

  /// Frames of [lane] that could be sent right now without waiting.
  int available(RelaySendLane lane) {
    final now = _now();
    _prune(now);
    final ceiling = lane == RelaySendLane.write
        ? capacity
        : capacity - writeReserve;
    var slots = max(0, ceiling - _sends.length);
    if (lane != RelaySendLane.read) {
      final eventCeiling = lane == RelaySendLane.write
          ? eventsPerMinute
          : eventsPerMinute - writeReserve;
      slots = min(slots, max(0, eventCeiling - _events.length));
    }
    return slots;
  }

  /// True when fewer than [writeReserve] durable writes could go right now —
  /// the signal for presence and keepalives to skip a beat.
  bool get writeLaneBelowReserve =>
      available(RelaySendLane.write) < writeReserve;

  /// Charge one frame to [lane] if the budget allows; never waits.
  bool tryAcquire(RelaySendLane lane) {
    if (available(lane) == 0) return false;
    _charge(lane, _now());
    return true;
  }

  /// Charge one frame to [lane], waiting for the window to free a slot when
  /// necessary. Writes are served before reads when both are waiting, and a
  /// frame that can go now never queues behind one that cannot.
  Future<void> acquire(RelaySendLane lane) {
    if (tryAcquire(lane)) return Future.value();
    final waiter = _Waiter(lane);
    _waiters.add(waiter);
    _scheduleWake();
    return waiter.completer.future;
  }

  /// Forget every charge and release every waiter (session disposal).
  void reset() {
    _wakeTimer?.cancel();
    _wakeTimer = null;
    _sends.clear();
    _events.clear();
    final waiters = List.of(_waiters);
    _waiters.clear();
    for (final waiter in waiters) {
      if (!waiter.completer.isCompleted) waiter.completer.complete();
    }
  }

  void _charge(RelaySendLane lane, DateTime at) {
    _sends.addLast(at);
    if (lane != RelaySendLane.read) _events.addLast(at);
  }

  void _prune(DateTime now) {
    while (_sends.isNotEmpty && now.difference(_sends.first) >= window) {
      _sends.removeFirst();
    }
    while (_events.isNotEmpty && now.difference(_events.first) >= eventWindow) {
      _events.removeFirst();
    }
  }

  void _grantWaiters() {
    // Writes first so a read storm cannot starve the user's own message.
    for (final lane in [RelaySendLane.write, RelaySendLane.read]) {
      for (final waiter in List.of(_waiters)) {
        if (waiter.lane != lane) continue;
        if (available(lane) == 0) break;
        _charge(lane, _now());
        _waiters.remove(waiter);
        waiter.completer.complete();
      }
    }
    if (_waiters.isNotEmpty) _scheduleWake();
  }

  /// Wake when the oldest charge that could be blocking a waiter expires.
  /// Waking early is harmless: [_grantWaiters] re-arms when nothing freed.
  void _scheduleWake() {
    if (_wakeTimer != null) return;
    final now = _now();
    _prune(now);
    Duration? delay;
    if (_sends.isNotEmpty) delay = window - now.difference(_sends.first);
    final eventWaiter = _waiters.any(
      (waiter) => waiter.lane != RelaySendLane.read,
    );
    if (eventWaiter && _events.isNotEmpty) {
      final eventDelay = eventWindow - now.difference(_events.first);
      if (delay == null || eventDelay < delay) delay = eventDelay;
    }
    final wait = delay == null || delay.isNegative ? Duration.zero : delay;
    _wakeTimer = _timerFactory(wait + const Duration(milliseconds: 1), () {
      _wakeTimer = null;
      _grantWaiters();
    });
  }
}

class _Waiter {
  _Waiter(this.lane);

  final RelaySendLane lane;
  final Completer<void> completer = Completer<void>();
}
