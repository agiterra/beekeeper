import 'dart:convert';

/// 32-bit FNV-1a hash of [input]'s UTF-8 bytes.
///
/// Stable across runs and devices, so two apps on the same key derive the
/// same offset for the same poll and two different polls (or keys) spread
/// apart deterministically.
int fnv1a32(String input) {
  var hash = 0x811c9dc5;
  for (final byte in utf8.encode(input)) {
    hash ^= byte;
    hash = (hash * 0x01000193) & 0xffffffff;
  }
  return hash;
}

/// Where in [period] the poll named [key] should first fire.
///
/// The base phase is `fnv1a(pubkey + key) % period`, so every recurring read
/// on a device lands at a different point of its period instead of all
/// firing together at connect time, and a phone and a desktop on the same
/// key still differ by [key]. On top of that ±10 % of the period is added
/// from [random] (a uniform draw in `[0, 1)`), which keeps two polls with
/// the same key from realigning after a resume. The result is always in
/// `[0, period)`.
Duration phaseOffset(
  String key,
  Duration period, {
  required String pubkey,
  required double random,
}) {
  assert(random >= 0 && random < 1, 'random must be a uniform draw in [0, 1)');
  final periodUs = period.inMicroseconds;
  if (periodUs <= 0) return Duration.zero;
  final base = fnv1a32('$pubkey$key') % periodUs;
  final jitter = ((random * 2 - 1) * 0.1 * periodUs).round();
  final offset = (base + jitter) % periodUs;
  return Duration(microseconds: offset < 0 ? offset + periodUs : offset);
}

/// The delay until the next wall-clock tick of the poll named [key].
///
/// Unlike [phaseOffset], which spreads *different* polls apart, this aligns
/// polls that are meant to coincide: the phase is derived from [key] and
/// [pubkey] alone (no jitter) and measured against the wall clock, so two
/// providers that start at different moments — the projects index and the
/// terminals index, say — still tick together and leave the device as one
/// coalesced `POST /query`. The result is always in `(0, period]`.
Duration alignedPollDelay({
  required String key,
  required Duration period,
  required String pubkey,
  required DateTime now,
}) {
  final phase = phaseOffset(key, period, pubkey: pubkey, random: 0.5);
  final periodMs = period.inMilliseconds;
  if (periodMs <= 0) return Duration.zero;
  final position = now.millisecondsSinceEpoch % periodMs;
  var delay = phase.inMilliseconds - position;
  if (delay <= 0) delay += periodMs;
  return Duration(milliseconds: delay);
}
