import 'package:flutter/foundation.dart';

/// Why a strict decoder refused an event.
enum CodingSessionDecodeReason {
  /// Not an event of the kind this decoder reads. Not a defect: the caller
  /// hands one mixed stream to every decoder.
  wrongKind,

  /// The kind was right but the tag envelope was not exactly the producer's.
  badTags,

  /// The signature did not verify, or the event id did not recompute.
  badSignature,

  /// The tags were right but the JSON content was not exactly the contract.
  malformedPayload,
}

/// The outcome of a strict decode: a value, or a reason and no value.
///
/// Decoders never throw — every input off a relay is untrusted — and never
/// return a partially decoded value.
@immutable
class CodingSessionDecoded<T extends Object> {
  final T? value;
  final CodingSessionDecodeReason? reason;

  const CodingSessionDecoded.ok(T this.value) : reason = null;

  const CodingSessionDecoded.failed(CodingSessionDecodeReason this.reason)
    : value = null;

  /// True when the event decoded to a usable value.
  bool get isValid => value != null;

  /// True when the event was simply not for this decoder.
  bool get isWrongKind => reason == CodingSessionDecodeReason.wrongKind;

  /// True when the event was for this decoder and failed it — the only
  /// outcome that belongs in a "malformed" count shown to a reader.
  bool get isRejected => reason != null && !isWrongKind;
}
