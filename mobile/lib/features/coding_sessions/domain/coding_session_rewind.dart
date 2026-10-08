import 'coding_session_models.dart';
import 'coding_session_target.dart';
import 'coding_session_wire.dart';

/// The exact keys of a 44224 receipt's optional SV-29 `rewind` value.
const codingSessionRewindReceiptKeys = [
  'checkpoint',
  'cutGeneration',
  'cutAfterSeq',
  'previousGeneration',
  'files',
  'preRewindCheckpoint',
  'head',
];

final _oidPattern = RegExp(r'^(?:[0-9a-f]{40}|[0-9a-f]{64})$');
const _maxSafeInteger = 9007199254740991;

bool _positive(Object? value) =>
    value is int && value > 0 && value <= _maxSafeInteger;

bool _nonNegative(Object? value) =>
    value is int && value >= 0 && value <= _maxSafeInteger;

/// Whether a receipt's `rewind` value is acceptable, coupled to its receipt
/// exactly as buzz-core's `validate_receipt_rewind` couples it (NIP-CSL):
///
/// - exactly seven keys, `null` refused (absent is not null);
/// - on `resumed` / `resumed_without_context` the receipt names generation
///   `previousGeneration + 1` and `files` is `kept` or `restored`;
/// - on `failed` only with `REWIND_NOT_RESTARTED` (any `files`);
/// - never on any other status.
bool codingSessionRewindReceiptAcceptable(
  Object? value, {
  required CodingSessionReceiptStatus status,
  required CodingSessionTarget? session,
  required String? errorCode,
}) {
  if (!isPlainRecord(value)) return false;
  final rewind = value! as Map<String, dynamic>;
  if (!hasExactKeys(rewind, codingSessionRewindReceiptKeys)) return false;
  final cut = rewind['cutGeneration'];
  final previous = rewind['previousGeneration'];
  final files = rewind['files'];
  final pre = rewind['preRewindCheckpoint'];
  final head = rewind['head'];
  if (!isHex64(rewind['checkpoint']) ||
      !_positive(cut) ||
      !_nonNegative(rewind['cutAfterSeq']) ||
      !_positive(previous) ||
      (cut as int) > (previous as int) ||
      (files != 'kept' && files != 'restored' && files != 'restore_failed') ||
      !(pre == null || isHex64(pre)) ||
      !(head == null || (head is String && _oidPattern.hasMatch(head)))) {
    return false;
  }
  switch (status) {
    case CodingSessionReceiptStatus.resumed:
    case CodingSessionReceiptStatus.resumedWithoutContext:
      return session != null &&
          session.generation == previous + 1 &&
          files != 'restore_failed';
    case CodingSessionReceiptStatus.failed:
      return errorCode == 'REWIND_NOT_RESTARTED';
    default:
      return false;
  }
}
