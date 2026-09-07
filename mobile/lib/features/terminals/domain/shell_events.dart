import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';

/// The three things a watch (kind:24310) can say.
enum ShellWatchAction {
  /// Open, or keep alive.
  watch('watch'),

  /// The observer left.
  stop('stop'),

  /// A gap was detected; send a fresh snapshot.
  resync('resync');

  const ShellWatchAction(this.wire);

  final String wire;
}

/// Raw bytes per input event: base64 expands 4/3, so 6000 raw bytes keeps
/// each event's content under the 8 KiB cap the relay and the owner host
/// enforce (`ShellObserveScreen.tsx:25`).
const shellInputChunkBytes = 6000;

/// The cap on one input event's base64 content.
const shellInputMaxBase64Bytes = 8 * 1024;

/// The unsigned shape of an event this device is about to sign and publish.
@immutable
class ShellEvent {
  final int kind;
  final String content;
  final List<List<String>> tags;

  const ShellEvent({
    required this.kind,
    required this.content,
    required this.tags,
  });
}

List<List<String>> _tags(String ownerPubkey, String sessionId, String ref) => [
  ['p', ownerPubkey.toLowerCase()],
  ['d', sessionId],
  ['a', ref],
];

/// Build a watch (kind:24310) addressed to the owner.
///
/// Mirrors `build_shell_watch_event` in the desktop's Rust
/// (`desktop/src-tauri/src/commands/shell_sessions.rs`).
ShellEvent buildShellWatchEvent({
  required String ownerPubkey,
  required String sessionId,
  required String projectRef,
  required ShellWatchAction action,
}) => ShellEvent(
  kind: EventKind.shellWatch,
  content: jsonEncode({'action': action.wire}),
  tags: _tags(ownerPubkey, sessionId, projectRef),
);

/// Build one input event (kind:24312) carrying [bytes] for the owner's PTY.
///
/// [bytes] must fit one event; use [chunkShellInput] first. Throws
/// [ArgumentError] otherwise — a caller that got here without chunking has a
/// bug, and the relay would refuse the event anyway.
ShellEvent buildShellInputEvent({
  required String ownerPubkey,
  required String sessionId,
  required String projectRef,
  required Uint8List bytes,
}) {
  final content = base64.encode(bytes);
  if (content.length > shellInputMaxBase64Bytes) {
    throw ArgumentError.value(
      bytes.length,
      'bytes',
      'exceeds the 8 KiB base64 cap; chunk first',
    );
  }
  return ShellEvent(
    kind: EventKind.shellInput,
    content: content,
    tags: _tags(ownerPubkey, sessionId, projectRef),
  );
}

/// Split raw input into event-sized chunks, in order.
List<Uint8List> chunkShellInput(
  Uint8List bytes, {
  int chunkBytes = shellInputChunkBytes,
}) {
  if (bytes.isEmpty) return const [];
  final chunks = <Uint8List>[];
  for (var offset = 0; offset < bytes.length; offset += chunkBytes) {
    final end = offset + chunkBytes > bytes.length
        ? bytes.length
        : offset + chunkBytes;
    chunks.add(Uint8List.sublistView(bytes, offset, end));
  }
  return chunks;
}
