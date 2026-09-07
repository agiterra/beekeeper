import 'dart:convert';

import 'package:flutter/foundation.dart';

import 'shell_announce.dart';
import 'shell_frame.dart';

/// What the terminal should do with one applied frame.
@immutable
class ObserveAction {
  /// Bytes to write into the terminal, already prefixed with a clear when a
  /// snapshot must replace stale content.
  final Uint8List? write;

  /// A new grid to apply before writing, when the frame carries one.
  final ShellDims? resize;

  /// The observer should publish a `resync` watch.
  final bool needsResync;

  /// The stream ended.
  final bool ended;

  const ObserveAction({
    this.write,
    this.resize,
    this.needsResync = false,
    this.ended = false,
  });

  static const none = ObserveAction();
}

/// `ESC[H ESC[2J` — home + clear, prepended when a snapshot must replace
/// whatever stale content a resync left on screen.
final Uint8List shellClearSequence = Uint8List.fromList(const [
  0x1b, 0x5b, 0x48, 0x1b, 0x5b, 0x32, 0x4a, // ESC [ H ESC [ 2 J
]);

Uint8List _withClear(Uint8List bytes) {
  final out = Uint8List(shellClearSequence.length + bytes.length);
  out.setRange(0, shellClearSequence.length, shellClearSequence);
  out.setRange(shellClearSequence.length, out.length, bytes);
  return out;
}

/// Ordering state machine for one observed terminal.
///
/// A port of the desktop's `ObserveStream` (`shellObserveProtocol.ts`).
/// Frames may arrive after a gap (dropped ephemeral events) or from a
/// restarted broadcaster (new epoch); either way diffs become unsafe until
/// the next snapshot repaints, and the observer asks for one.
class ObserveStream {
  String? _epoch;
  int _lastSeq = 0;
  bool _awaitingSnap = false;

  /// True once any snapshot has painted (a resync-snap must clear first).
  bool _painted = false;

  /// Treat whatever arrives next as arriving after a gap.
  ///
  /// For an observer that was away (the app backgrounded, the socket
  /// dropped): the owner expired its watch, so diffs are unsafe until the
  /// next snapshot repaints, and a restarted owner may be on a new epoch.
  void markGap() {
    if (_epoch != null) _awaitingSnap = true;
  }

  /// Fold [frame] in and say what to do with it.
  ObserveAction apply(ShellFrame frame) {
    final epochChanged = _epoch != null && frame.epoch != _epoch;
    final gap = _epoch != null && !epochChanged && frame.seq != _lastSeq + 1;
    if (_epoch == null || epochChanged) {
      _epoch = frame.epoch;
      _lastSeq = frame.seq;
      if (epochChanged) _awaitingSnap = true;
    } else if (gap) {
      if (frame.seq <= _lastSeq) {
        // Stale replay — drop silently.
        return ObserveAction.none;
      }
      _lastSeq = frame.seq;
      _awaitingSnap = true;
    } else {
      _lastSeq = frame.seq;
    }

    switch (frame.type) {
      case ShellFrameType.end:
        return const ObserveAction(ended: true);
      case ShellFrameType.resize:
        // A resize invalidates the diff chain; the owner follows with a snap.
        _awaitingSnap = true;
        return ObserveAction(resize: frame.dims);
      case ShellFrameType.snap:
        final mustClear = _awaitingSnap && _painted;
        _awaitingSnap = false;
        _painted = true;
        return ObserveAction(
          resize: frame.dims,
          write: mustClear ? _withClear(frame.bytes) : frame.bytes,
        );
      case ShellFrameType.tail:
        // Scrollback replay ahead of the first snapshot; mid-stream while
        // awaiting a snap, a stale tail would corrupt the screen — skip it.
        return _awaitingSnap && _painted
            ? const ObserveAction(needsResync: true)
            : ObserveAction(write: frame.bytes);
      case ShellFrameType.diff:
        if (_awaitingSnap) return const ObserveAction(needsResync: true);
        _painted = true;
        return ObserveAction(write: frame.bytes);
    }
  }
}

/// Turns raw frame bytes into text an emulator can take, across frames.
///
/// Frames chunk arbitrarily, so a multibyte glyph may be split between two
/// of them. The decoder holds back an incomplete trailing sequence and
/// prepends it to the next frame instead of emitting a replacement
/// character. [reset] discards any carried partial — call it on a snapshot,
/// which repaints from scratch.
class ShellByteDecoder {
  Uint8List _carry = Uint8List(0);

  /// Decode [bytes], returning the text completed so far.
  String decode(Uint8List bytes) {
    final input = _carry.isEmpty
        ? bytes
        : Uint8List.fromList([..._carry, ...bytes]);
    final keep = incompleteUtf8Tail(input);
    final complete = keep == 0 ? input : input.sublist(0, input.length - keep);
    _carry = keep == 0 ? Uint8List(0) : input.sublist(input.length - keep);
    return utf8.decode(complete, allowMalformed: true);
  }

  /// Drop any carried partial sequence.
  void reset() {
    _carry = Uint8List(0);
  }
}

/// How many trailing bytes of [bytes] begin a UTF-8 sequence that is not
/// yet complete — 0 when the buffer ends on a boundary.
///
/// Looks back at most three bytes for a lead byte; a lead byte whose
/// declared length exceeds the bytes available is the incomplete tail. A
/// malformed run is not held back: it is handed on to decode as
/// replacement characters rather than kept forever.
int incompleteUtf8Tail(Uint8List bytes) {
  for (var back = 1; back <= 3 && back <= bytes.length; back++) {
    final byte = bytes[bytes.length - back];
    if (byte & 0xc0 == 0x80) continue; // continuation byte, keep looking
    final needed = byte >= 0xf0
        ? 4
        : byte >= 0xe0
        ? 3
        : byte >= 0xc0
        ? 2
        : 1;
    return needed > back ? back : 0;
  }
  return 0;
}
