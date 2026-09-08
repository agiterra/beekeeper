import 'dart:async';

import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart' show AppLifecycleState;
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../../shared/relay/relay.dart';
import '../domain/terminals_domain.dart';

/// Which terminal an observer watches: the announce's `(owner, d)` plus the
/// project coordinate every NIP-ST event has to carry.
@immutable
class ShellObserverTarget {
  final String ownerPubkey;
  final String sessionId;
  final String projectRef;

  ShellObserverTarget({
    required String ownerPubkey,
    required this.sessionId,
    required this.projectRef,
  }) : ownerPubkey = ownerPubkey.toLowerCase();

  /// The target of [terminal].
  factory ShellObserverTarget.of(RemoteTerminal terminal) =>
      ShellObserverTarget(
        ownerPubkey: terminal.ownerPubkey,
        sessionId: terminal.sessionId,
        projectRef: terminal.projectRef,
      );

  @override
  bool operator ==(Object other) =>
      other is ShellObserverTarget &&
      other.ownerPubkey == ownerPubkey &&
      other.sessionId == sessionId &&
      other.projectRef == projectRef;

  @override
  int get hashCode => Object.hash(ownerPubkey, sessionId, projectRef);
}

/// The observer's timing, overridable so tests need not wait real seconds.
@immutable
class ShellObserverConfig {
  /// How often a `watch` is re-sent; the owner expires a watcher at 45 s.
  final Duration keepalive;

  /// No frame within this after the first watch → "Not streaming".
  final Duration handshake;

  /// No frame for this while live → "Stalled".
  final Duration stall;

  /// How far back the frame subscription asks the relay to replay.
  final Duration replay;

  const ShellObserverConfig({
    this.keepalive = const Duration(seconds: 15),
    this.handshake = const Duration(seconds: 10),
    this.stall = const Duration(seconds: 30),
    this.replay = const Duration(seconds: 30),
  });
}

/// The timing the observers run on.
final shellObserverConfigProvider = Provider<ShellObserverConfig>(
  (ref) => const ShellObserverConfig(),
);

/// What the observe page may say about the stream.
enum ShellObserverStatus {
  /// A watch has been sent and no frame has come back yet.
  connecting,

  /// Frames are arriving.
  live,

  /// The owner is not streaming: no frame within the handshake window, or
  /// none for the stall window while live. The owner may be away, the
  /// session may be unshared, or this device's watch may not be reaching it.
  stalled,

  /// An `end` frame closed the stream, or the app is in the background.
  ended,

  /// This community's relay session is not connected.
  offline,
}

/// One instruction for the emulator: text to write, or a grid to adopt.
@immutable
class ShellTerminalWrite {
  final String? text;
  final ShellDims? resize;

  const ShellTerminalWrite({this.text, this.resize});
}

/// The read side of one observed terminal.
@immutable
class ShellObserverState {
  final ShellObserverStatus status;

  /// How many frames were applied since the observer started.
  final int framesApplied;

  /// The last failure to publish a watch, verbatim.
  final String? lastWatchError;

  /// The relay's verbatim refusal of an input event (`restricted: …`).
  ///
  /// NIP-ST says a refusal is a revocation: the roster no longer names this
  /// key a collaborator, or the session closed. The page drops the input
  /// bar and shows the words.
  final String? inputRefused;

  /// Until when input is paused after a `rate-limited:` answer.
  ///
  /// Not a revocation — the quota is per key and shared with the paired
  /// desktop — so the bar stays and says "paused", never "revoked".
  final DateTime? inputPausedUntil;

  /// How many input events this observer published.
  final int inputsSent;

  const ShellObserverState({
    required this.status,
    this.framesApplied = 0,
    this.lastWatchError,
    this.inputRefused,
    this.inputPausedUntil,
    this.inputsSent = 0,
  });

  /// True while a rate-limit pause is in force at [now].
  bool inputPausedAt(DateTime now) {
    final until = inputPausedUntil;
    return until != null && now.isBefore(until);
  }

  ShellObserverState copyWith({
    ShellObserverStatus? status,
    int? framesApplied,
    String? lastWatchError,
    bool clearWatchError = false,
    String? inputRefused,
    DateTime? inputPausedUntil,
    bool clearInputPause = false,
    int? inputsSent,
  }) => ShellObserverState(
    status: status ?? this.status,
    framesApplied: framesApplied ?? this.framesApplied,
    lastWatchError: clearWatchError
        ? null
        : (lastWatchError ?? this.lastWatchError),
    inputRefused: inputRefused ?? this.inputRefused,
    inputPausedUntil: clearInputPause
        ? null
        : (inputPausedUntil ?? this.inputPausedUntil),
    inputsSent: inputsSent ?? this.inputsSent,
  );
}

/// Observes one shared terminal: subscribes to the owner's frames,
/// heartbeats a kind:24310 watch, and hands parsed frames to the page.
///
/// A port of the desktop's `useShellObserver`, plus the phone's lifecycle:
/// the relay session drops its socket five seconds after the app backgrounds
/// and rejects publishes, so the keepalive stops while the app is not
/// resumed, and on resume a `watch` goes out at once and the next frame is
/// treated as a gap (the owner expired this watcher after 45 s).
class ShellObserverNotifier extends Notifier<ShellObserverState> {
  ShellObserverNotifier(this.target);

  final ShellObserverTarget target;

  final _stream = ObserveStream();
  final _decoder = ShellByteDecoder();
  final _writes = StreamController<ShellTerminalWrite>.broadcast();
  final List<void Function()> _unsubscribes = [];

  /// The signing relay, captured when the observer starts so the goodbye
  /// `stop` can be published from the dispose callback, where `ref` is
  /// off-limits.
  SignedEventRelay? _relay;
  Timer? _keepalive;
  Timer? _liveness;
  Timer? _handshake;
  DateTime? _lastFrameAt;
  int _epoch = 0;
  bool _disposed = false;
  bool _running = false;

  /// Text and grid changes for the emulator, in arrival order.
  Stream<ShellTerminalWrite> get writes => _writes.stream;

  @override
  ShellObserverState build() {
    final status = ref.watch(
      relaySessionProvider.select((session) => session.status),
    );
    final lifecycle = ref.watch(appLifecycleProvider);
    _disposed = false;
    ref.onDispose(() {
      _disposed = true;
      _stop(sayGoodbye: true);
      _writes.close();
    });

    if (status != SessionStatus.connected) {
      _stop(sayGoodbye: false);
      return (stateOrNull ??
              const ShellObserverState(status: ShellObserverStatus.offline))
          .copyWith(status: ShellObserverStatus.offline);
    }
    if (lifecycle != AppLifecycleState.resumed) {
      _stop(sayGoodbye: false);
      return (stateOrNull ??
              const ShellObserverState(status: ShellObserverStatus.ended))
          .copyWith(status: ShellObserverStatus.ended);
    }
    if (!_running) Future.microtask(_start);
    return (stateOrNull ??
            const ShellObserverState(status: ShellObserverStatus.connecting))
        .copyWith(status: ShellObserverStatus.connecting);
  }

  /// Ask the owner for a fresh snapshot.
  Future<void> resync() {
    _relay ??= _makeRelay();
    return _publishWatch(ShellWatchAction.resync);
  }

  /// Sends are chained so chunks reach the relay in typing order.
  Future<void> _sendChain = Future.value();

  /// Type [bytes] into the owner's PTY (kind:24312), chunked and in order.
  ///
  /// Every EVENT counts against the human quota of 60 per minute, shared
  /// with the paired desktop's key, so callers send whole lines or single
  /// special keys — never a keystroke stream. A `restricted:` answer is a
  /// revocation and is kept as [ShellObserverState.inputRefused]; a
  /// `rate-limited:` answer pauses input for the relay's retry window and
  /// is not.
  Future<void> sendInput(Uint8List bytes) {
    if (bytes.isEmpty || _disposed) return Future.value();
    if (state.inputRefused != null) return Future.value();
    if (state.inputPausedAt(DateTime.now())) return Future.value();
    _relay ??= _makeRelay();
    final chunks = chunkShellInput(bytes);
    final send = _sendChain.then((_) => _publishChunks(chunks));
    _sendChain = send.catchError((_) {});
    return send;
  }

  /// Type one line: the text and a carriage return, as one event.
  Future<void> sendLine(String line) =>
      sendInput(Uint8List.fromList(utf8.encode('$line\r')));

  Future<void> _publishChunks(List<Uint8List> chunks) async {
    final relay = _relay;
    if (relay == null) return;
    for (final chunk in chunks) {
      if (_disposed) return;
      final event = buildShellInputEvent(
        ownerPubkey: target.ownerPubkey,
        sessionId: target.sessionId,
        projectRef: target.projectRef,
        bytes: chunk,
      );
      try {
        await relay.submit(
          kind: event.kind,
          content: event.content,
          tags: event.tags,
        );
        _emit(inputsSent: state.inputsSent + 1, clearInputPause: true);
      } catch (error) {
        final message = _message(error);
        if (message.startsWith('rate-limited:')) {
          final seconds =
              parseRateLimitRetrySeconds(message) ??
              RelayRateLimitGate.defaultRetrySeconds;
          _emit(
            inputPausedUntil: DateTime.now().add(Duration(seconds: seconds)),
          );
        } else {
          _emit(inputRefused: message);
        }
        return;
      }
    }
  }

  SignedEventRelay _makeRelay() => SignedEventRelay(
    session: ref.read(relaySessionProvider.notifier),
    nsec: ref.read(relayConfigProvider).nsec,
  );

  Future<void> _start() async {
    if (_disposed || _running) return;
    _running = true;
    final epoch = ++_epoch;
    final config = ref.read(shellObserverConfigProvider);
    final session = ref.read(relaySessionProvider.notifier);
    _relay = _makeRelay();
    _lastFrameAt = null;
    // The owner expired us while we were away, and a restarted broadcaster
    // may be on a new epoch: whatever comes next is a gap until its snap.
    _stream.markGap();
    try {
      final since =
          DateTime.now().subtract(config.replay).millisecondsSinceEpoch ~/ 1000;
      final unsubscribe = await session.subscribe(
        NostrFilters.shellFrames(
          target.ownerPubkey,
          target.sessionId,
          sinceSeconds: since,
        ),
        _onEvent,
      );
      if (_stale(epoch)) {
        unsubscribe();
        return;
      }
      _unsubscribes.add(unsubscribe);
    } catch (error) {
      if (_stale(epoch)) return;
      _emit(lastWatchError: 'Frame subscription failed: $error');
    }
    await _publishWatch(ShellWatchAction.watch);
    if (_stale(epoch)) return;
    _keepalive = Timer.periodic(config.keepalive, (_) => _sendKeepalive());
    _handshake = Timer(config.handshake, () {
      if (_stale(epoch) || _lastFrameAt != null) return;
      _emit(status: ShellObserverStatus.stalled);
    });
    _liveness = Timer.periodic(const Duration(seconds: 1), (_) {
      if (_stale(epoch)) return;
      final last = _lastFrameAt;
      if (last == null || state.status == ShellObserverStatus.ended) return;
      if (DateTime.now().difference(last) > config.stall &&
          state.status == ShellObserverStatus.live) {
        _emit(status: ShellObserverStatus.stalled);
      }
    });
  }

  void _stop({required bool sayGoodbye}) {
    if (!_running) return;
    _running = false;
    _epoch += 1;
    _keepalive?.cancel();
    _keepalive = null;
    _liveness?.cancel();
    _liveness = null;
    _handshake?.cancel();
    _handshake = null;
    for (final unsubscribe in _unsubscribes) {
      unsubscribe();
    }
    _unsubscribes.clear();
    if (sayGoodbye) {
      // Best effort: the owner expires us in 45 s regardless.
      _publishWatch(ShellWatchAction.stop, quiet: true);
    }
  }

  void _onEvent(NostrEvent event) {
    if (_disposed || !_running) return;
    final frame = parseShellFrame(
      event,
      ownerPubkey: target.ownerPubkey,
      sessionId: target.sessionId,
    );
    if (frame == null) return;
    _lastFrameAt = DateTime.now();
    final action = _stream.apply(frame);
    if (action.resize != null) {
      _writes.add(ShellTerminalWrite(resize: action.resize));
    }
    final bytes = action.write;
    if (bytes != null) {
      if (frame.type == ShellFrameType.snap) _decoder.reset();
      final text = _decoder.decode(bytes);
      if (text.isNotEmpty) _writes.add(ShellTerminalWrite(text: text));
    }
    if (action.needsResync) {
      _publishWatch(ShellWatchAction.resync, quiet: true);
    }
    _emit(
      status: action.ended
          ? ShellObserverStatus.ended
          : ShellObserverStatus.live,
      framesApplied: state.framesApplied + 1,
    );
  }

  /// The periodic `watch` beat, as a droppable ephemeral.
  ///
  /// Same kind:24310 payload as the opening watch, but sent through
  /// [RelaySessionNotifier.sendEphemeral] instead of a publish that waits
  /// for OK: the owner expires a watcher after 45 s — three beats — so one
  /// dropped under the rate-limit gate or a thin write lane is harmless,
  /// and a beat never queues behind the user's own input.
  void _sendKeepalive() {
    if (_disposed || !_running) return;
    final event = buildShellWatchEvent(
      ownerPubkey: target.ownerPubkey,
      sessionId: target.sessionId,
      projectRef: target.projectRef,
      action: ShellWatchAction.watch,
    );
    final relay = _relay;
    if (relay == null) return;
    relay.sendEphemeral(
      kind: event.kind,
      content: event.content,
      tags: event.tags,
    );
  }

  Future<void> _publishWatch(
    ShellWatchAction action, {
    bool quiet = false,
  }) async {
    final relay = _relay;
    if (relay == null) return;
    final event = buildShellWatchEvent(
      ownerPubkey: target.ownerPubkey,
      sessionId: target.sessionId,
      projectRef: target.projectRef,
      action: action,
    );
    try {
      await relay.submit(
        kind: event.kind,
        content: event.content,
        tags: event.tags,
      );
      if (!quiet && !_disposed) _emit(clearWatchError: true);
    } catch (error) {
      if (quiet || _disposed) return;
      _emit(lastWatchError: _message(error));
    }
  }

  static String _message(Object error) {
    final text = error.toString();
    const prefix = 'Exception: ';
    return text.startsWith(prefix) ? text.substring(prefix.length) : text;
  }

  bool _stale(int epoch) => _disposed || epoch != _epoch || !_running;

  void _emit({
    ShellObserverStatus? status,
    int? framesApplied,
    String? lastWatchError,
    bool clearWatchError = false,
    String? inputRefused,
    DateTime? inputPausedUntil,
    bool clearInputPause = false,
    int? inputsSent,
  }) {
    if (_disposed) return;
    state = state.copyWith(
      status: status,
      framesApplied: framesApplied,
      lastWatchError: lastWatchError,
      clearWatchError: clearWatchError,
      inputRefused: inputRefused,
      inputPausedUntil: inputPausedUntil,
      clearInputPause: clearInputPause,
      inputsSent: inputsSent,
    );
  }
}

/// One observed terminal's live read, keyed by target.
///
/// Auto-disposed: the watch keepalive must stop when the page that watches
/// leaves, or the owner keeps streaming to nobody.
final shellObserverProvider = NotifierProvider.autoDispose
    .family<ShellObserverNotifier, ShellObserverState, ShellObserverTarget>(
      ShellObserverNotifier.new,
    );
