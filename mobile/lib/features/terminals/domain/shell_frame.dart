import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'shell_announce.dart';

/// The five frame types a kind:24311 may carry (NIP-ST § Frame).
enum ShellFrameType {
  /// Recent scrollback replayed on attach, possibly chunked.
  tail('tail'),

  /// A full redraw of the live screen.
  snap('snap'),

  /// Only what changed since the last frame.
  diff('diff'),

  /// The owner's grid changed; a `snap` follows.
  resize('resize'),

  /// The stream ended: closed, exited, or unshared.
  end('end');

  const ShellFrameType(this.wire);

  final String wire;

  static ShellFrameType? fromWire(String? value) {
    for (final type in ShellFrameType.values) {
      if (type.wire == value) return type;
    }
    return null;
  }
}

/// One parsed frame of a terminal's stream.
@immutable
class ShellFrame {
  final ShellFrameType type;

  /// Per-session monotonic counter within [epoch].
  final int seq;

  /// Fresh per broadcast process, so a restart is detectable.
  final String epoch;
  final ShellDims? dims;

  /// Raw terminal bytes, to be written verbatim into an emulator.
  final Uint8List bytes;

  const ShellFrame({
    required this.type,
    required this.seq,
    required this.epoch,
    required this.dims,
    required this.bytes,
  });
}

String? _singleTag(NostrEvent event, String name) {
  String? found;
  for (final tag in event.tags) {
    if (tag.length < 2 || tag[0] != name) continue;
    if (found != null) return null;
    found = tag[1];
  }
  return found;
}

/// Parse one relay event as a frame for the expected terminal.
///
/// Returns `null` for anything that is not a well-formed frame from the
/// session's owner. The subscription already filters by author and session
/// id (`NostrFilters.shellFrames`), but the parser never trusts that —
/// mirror of the desktop's `parseShellFrame` (`shellObserveProtocol.ts`).
ShellFrame? parseShellFrame(
  NostrEvent event, {
  required String ownerPubkey,
  required String sessionId,
}) {
  if (event.kind != EventKind.shellFrame) return null;
  if (event.pubkey.toLowerCase() != ownerPubkey.toLowerCase()) return null;
  if (_singleTag(event, 'd') != sessionId) return null;
  final type = ShellFrameType.fromWire(_singleTag(event, 't'));
  if (type == null) return null;
  final seq = int.tryParse(_singleTag(event, 'seq') ?? '');
  if (seq == null || seq < 0) return null;
  final epoch = _singleTag(event, 'epoch');
  if (epoch == null || epoch.isEmpty) return null;
  final Uint8List bytes;
  try {
    bytes = base64.decode(event.content);
  } on FormatException {
    return null;
  }
  return ShellFrame(
    type: type,
    seq: seq,
    epoch: epoch,
    dims: ShellDims.parse(_singleTag(event, 'dims')),
    bytes: bytes,
  );
}
