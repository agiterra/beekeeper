/// The shared-terminal (NIP-ST) domain: announce and frame decoders, the
/// ordering state machine an observer runs, and the builders for the two
/// events this device signs — watches and input.
///
/// Pure Dart — no Riverpod, no widgets, no I/O. See `docs/nips/NIP-ST.md`.
library;

export 'observe_stream.dart';
export 'shell_announce.dart';
export 'shell_events.dart';
export 'shell_frame.dart';
