import 'package:flutter/foundation.dart';

import 'coding_session_keys.dart';

/// Largest UTF-8 byte length any identity field of a target may carry.
const maxCodingSessionTargetIdentityBytes = 512;

/// The provider-neutral execution identity a coding-session fact is about.
///
/// A generation is part of the identity, not a version of it: each generation
/// is its own stream, a resume mints `generation + 1` and resets `eventSeq`.
/// Facts for one generation never merge with facts for another.
@immutable
class CodingSessionTarget {
  /// The provider driver, e.g. `claude-agent-acp`.
  final String driver;

  /// The provider instance the execution runs on.
  final String instanceId;

  /// The provider's own session identifier.
  final String sessionId;

  /// The generation number; strictly positive.
  final int generation;

  const CodingSessionTarget({
    required this.driver,
    required this.instanceId,
    required this.sessionId,
    required this.generation,
  });

  /// The signed `cs-target` tag value for this tuple.
  ///
  /// Byte-identical to the desktop's `buildCodingSessionTargetKey`.
  String get key => encodeStructuredKey(codingSessionTargetKeyDomain, [
    driver,
    instanceId,
    sessionId,
    '$generation',
  ]);

  /// Identity of the execution across generations (`generation` dropped).
  ///
  /// Two targets share an execution key exactly when one is a resume of the
  /// other, which is what makes "the current generation" a well-defined idea.
  String get executionKey => encodeStructuredKey(
    'coding-session-execution/v1',
    [driver, instanceId, sessionId],
  );

  /// The 44223 `csm-key` semantic key for this target.
  String get metadataSemanticKey => encodeStructuredKey(
    codingSessionMetadataKeyDomain,
    [driver, instanceId, sessionId, '$generation'],
  );

  /// The 44225 `cst-key` semantic key for `eventSeq` under this target.
  String transcriptSemanticKey(int eventSeq) => encodeStructuredKey(
    codingSessionTranscriptKeyDomain,
    [driver, instanceId, sessionId, '$generation', '$eventSeq'],
  );

  /// Strictly decode a wire target object; `null` when it is not exactly one.
  ///
  /// Exact four keys, non-blank bounded identity strings, and a positive
  /// integer generation — anything looser is a rejection, never a coercion.
  static CodingSessionTarget? decode(
    Object? value, {
    int maxIdentityBytes = maxCodingSessionTargetIdentityBytes,
  }) {
    if (value is! Map) return null;
    if (value.length != 4) return null;
    final driver = value['driver'];
    final instanceId = value['instanceId'];
    final sessionId = value['sessionId'];
    final generation = value['generation'];
    if (!_boundedNonempty(driver, maxIdentityBytes) ||
        !_boundedNonempty(instanceId, maxIdentityBytes) ||
        !_boundedNonempty(sessionId, maxIdentityBytes) ||
        generation is! int ||
        generation <= 0) {
      return null;
    }
    return CodingSessionTarget(
      driver: driver! as String,
      instanceId: instanceId! as String,
      sessionId: sessionId! as String,
      generation: generation,
    );
  }

  /// Parse a `cs-target` tag value back into a target.
  ///
  /// The inverse of [key]; returns `null` for any value this observer could
  /// not have minted itself.
  static CodingSessionTarget? fromKey(String key) {
    const prefix = '$codingSessionTargetKeyDomain|';
    if (!key.startsWith(prefix)) return null;
    final fields = _decodeLengthPrefixedFields(key.substring(prefix.length));
    if (fields == null || fields.length != 4) return null;
    final generation = int.tryParse(fields[3]);
    if (generation == null || generation <= 0) return null;
    if (fields[3] != '$generation') return null;
    if (fields[0].isEmpty || fields[1].isEmpty || fields[2].isEmpty) {
      return null;
    }
    return CodingSessionTarget(
      driver: fields[0],
      instanceId: fields[1],
      sessionId: fields[2],
      generation: generation,
    );
  }

  /// This target's successor generation, as a resume would mint it.
  CodingSessionTarget nextGeneration() => CodingSessionTarget(
    driver: driver,
    instanceId: instanceId,
    sessionId: sessionId,
    generation: generation + 1,
  );

  Map<String, Object?> toJson() => {
    'driver': driver,
    'instanceId': instanceId,
    'sessionId': sessionId,
    'generation': generation,
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is CodingSessionTarget &&
          driver == other.driver &&
          instanceId == other.instanceId &&
          sessionId == other.sessionId &&
          generation == other.generation;

  @override
  int get hashCode => Object.hash(driver, instanceId, sessionId, generation);

  @override
  String toString() => 'CodingSessionTarget($key)';
}

bool _boundedNonempty(Object? value, int maxBytes) =>
    value is String &&
    value.trim().isNotEmpty &&
    utf8ByteLength(value) <= maxBytes;

/// Decode the `<byteLength>:<value>` run a structured key body is made of.
///
/// Byte-length prefixed, so the split walks UTF-8 bytes rather than UTF-16
/// code units; a prefix that does not land on a field boundary is a rejection.
List<String>? _decodeLengthPrefixedFields(String body) {
  final bytes = utf8Bytes(body);
  final fields = <String>[];
  var offset = 0;
  while (offset < bytes.length) {
    final separator = bytes.indexOf(0x3a, offset); // ':'
    if (separator < 0) return null;
    final digits = String.fromCharCodes(bytes.sublist(offset, separator));
    if (digits.isEmpty || !RegExp(r'^[0-9]+$').hasMatch(digits)) return null;
    final length = int.tryParse(digits);
    if (length == null) return null;
    final start = separator + 1;
    final end = start + length;
    if (end > bytes.length) return null;
    fields.add(decodeUtf8Bytes(bytes.sublist(start, end)));
    offset = end;
  }
  return fields;
}
