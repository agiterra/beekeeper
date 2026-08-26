/// The coding-session observer's domain layer: strict decoders for the signed
/// wire contract, the trust gate that decides whose facts count, the folds
/// that turn those facts into sessions, and the transcript projection.
///
/// Pure Dart — no Riverpod, no widgets, no I/O. Everything here is a function
/// of its inputs, so the same events read the same way on any device.
library;

export 'coding_session_decode_result.dart';
export 'coding_session_decoders.dart';
export 'coding_session_fold.dart';
export 'coding_session_keys.dart';
export 'coding_session_models.dart';
export 'coding_session_session_decoders.dart';
export 'coding_session_signature.dart';
export 'coding_session_target.dart';
export 'coding_session_transcript.dart';
export 'coding_session_transcript_item.dart';
export 'coding_session_trust.dart';
export 'coding_session_view.dart';
