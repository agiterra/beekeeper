import 'dart:convert';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_keys.dart';

/// Shared strict decoders for the 442xx wire payloads.
///
/// Every one of these is a rejection primitive, not a coercion: an input that
/// is not exactly the declared shape decodes to `null`/`false` rather than to
/// a best guess. Mirror of the desktop's `codingSessionWireDecode.ts`.

/// Parse bounded JSON content; `null` for non-JSON or oversized input.
Object? parseBoundedJson(String content, int maxBytes) {
  if (utf8ByteLength(content) > maxBytes) return null;
  try {
    return jsonDecode(content);
  } on FormatException {
    return null;
  }
}

/// True when [value] is a non-blank string within [maxBytes] UTF-8 bytes.
bool boundedNonempty(Object? value, int maxBytes) =>
    value is String &&
    value.trim().isNotEmpty &&
    utf8ByteLength(value) <= maxBytes;

/// True when [value] is `null` or a [boundedNonempty] string.
bool boundedNullable(Object? value, int maxBytes) =>
    value == null || boundedNonempty(value, maxBytes);

/// True when [value] is a JSON object (not a list, not a scalar).
bool isPlainRecord(Object? value) => value is Map<String, dynamic>;

/// True when [value] has exactly [keys], no more and no fewer.
bool hasExactKeys(Map<String, dynamic> value, List<String> keys) =>
    value.length == keys.length && value.keys.every(keys.contains);

/// True when every required key is present and no unexpected key is.
bool hasRequiredAndOptionalKeys(
  Map<String, dynamic> value,
  List<String> required,
  List<String> optional,
) =>
    required.every(value.containsKey) &&
    value.keys.every((key) => required.contains(key) || optional.contains(key));

/// A key group that must appear as a unit or not at all.
///
/// This is how a payload amendment stays unambiguous: a producer either speaks
/// the amended dialect (every key present, if only as nulls) or the base one.
/// A partial subset is a corrupted payload, not a version skew.
bool hasAllOrNoneKeys(Map<String, dynamic> value, List<String> keys) {
  final present = keys.where(value.containsKey).length;
  return present == 0 || present == keys.length;
}

/// Depth- and cycle-bounded structural probe for untrusted nested payloads.
bool isWithinDepth(Object? value, int maxDepth) {
  final stack = <(Object?, int)>[(value, 0)];
  final seen = <Object>{};
  while (stack.isNotEmpty) {
    final (current, depth) = stack.removeLast();
    if (depth > maxDepth) return false;
    if (current is! Map && current is! List) continue;
    if (seen.any((element) => identical(element, current))) return false;
    seen.add(current!);
    final nested = current is Map ? current.values : (current as List);
    for (final item in nested) {
      stack.add((item, depth + 1));
    }
  }
  return true;
}

/// Read an event's tags as an exact, ordered list of single-value tags.
///
/// Order and count are part of the contract: the producer emits these tags in
/// one fixed order, so an event with the right tags in the wrong order, or
/// with an extra tag appended, is not one this observer signed off on.
List<String>? parseExactTags(List<List<String>> tags, List<String> names) {
  if (tags.length != names.length) return null;
  final values = <String>[];
  for (var index = 0; index < names.length; index += 1) {
    final tag = tags[index];
    if (tag.length != 2 || tag[0] != names[index]) return null;
    values.add(tag[1]);
  }
  return values;
}

/// True when the event carries a tag with the given name.
bool hasTagNamed(NostrEvent event, String name) =>
    event.tags.any((tag) => tag.isNotEmpty && tag[0] == name);

final _sessionRefPattern = RegExp(
  r'^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$',
);
final _hex64Pattern = RegExp(r'^[0-9a-f]{64}$');

/// Canonical umbrella session reference: a lowercase hyphenated UUID.
bool isCodingSessionSessionRef(Object? value) =>
    value is String && _sessionRefPattern.hasMatch(value);

/// True for a canonical lowercase 64-hex id (event id or pubkey).
bool isHex64(Object? value) => value is String && _hex64Pattern.hasMatch(value);

/// Lowercase a pubkey when it is exactly 64 hex characters, else `''`.
String normalizePubkey(Object? value) {
  if (value is! String) return '';
  final normalized = value.trim().toLowerCase();
  return _hex64Pattern.hasMatch(normalized) ? normalized : '';
}

/// Deterministic canonical JSON for equality comparison of two payloads.
///
/// Object keys are sorted so two byte-different encodings of the same facts
/// compare equal, which is what makes the duplicate/conflict distinction in
/// the trust gate a statement about facts rather than about whitespace.
String? canonicalJson(Object? value) {
  try {
    final buffer = StringBuffer();
    _writeCanonical(value, buffer, 0);
    return buffer.toString();
  } on FormatException {
    return null;
  }
}

void _writeCanonical(Object? value, StringBuffer buffer, int depth) {
  if (depth > 64) throw const FormatException('canonical json too deep');
  if (value is Map) {
    final keys = value.keys.map((key) => '$key').toList()..sort();
    buffer.write('{');
    for (var index = 0; index < keys.length; index += 1) {
      if (index > 0) buffer.write(',');
      buffer
        ..write(jsonEncode(keys[index]))
        ..write(':');
      _writeCanonical(value[keys[index]], buffer, depth + 1);
    }
    buffer.write('}');
    return;
  }
  if (value is List) {
    buffer.write('[');
    for (var index = 0; index < value.length; index += 1) {
      if (index > 0) buffer.write(',');
      _writeCanonical(value[index], buffer, depth + 1);
    }
    buffer.write(']');
    return;
  }
  buffer.write(jsonEncode(value));
}

const _routingEfforts = {'low', 'medium', 'high'};
const _routingTiers = {'fast', 'standard', 'deep'};

/// A review reason is a bounded token, not a member of a closed set.
///
/// The canonical router renders the two numeric §6 triggers with the value
/// that fired them — `risk 80 >= 40`, `irreversibility 4 >= 4` — so a reader
/// is told the fact rather than the rule
/// (`crates/buzz-core/src/coding_session_routing.rs:1555`). A closed
/// vocabulary here would make this decoder reject the router's own record as
/// malformed.
const _maxRoutingReviewReasons = 16;
const _maxRoutingTokenBytes = 256;
const _routingTraits = {
  'reasoning',
  'coding',
  'taste',
  'judgment',
  'agency',
  'discipline',
  'context',
  'verification',
  'velocity',
  'costEfficiency',
};
const _routingRecordFields = [
  'class',
  'tier',
  'risk',
  'chosen',
  'runnerUp',
  'reason',
  'reviewRequired',
  'reviewReasons',
  'challengerSample',
  'override',
  'registryVersion',
  'catalogRevision',
];
const _maxRoutingReferenceBytes = 2 * 1024;
final RegExp _routingClassPattern = RegExp(r'^[a-z0-9_-]{1,64}$');

/// The `routing` record a routed 44221 create and its 44223 metadata carry
/// (Brian's routing ruling, 2026-08-30).
///
/// Written out here rather than shared with the desktop because this app
/// shares no code with it; the rule is identical. Two of the checks are about
/// honesty rather than shape:
///
/// * `risk.score` must be `impact × uncertainty × irreversibility` — a record
///   whose score disagrees with its own factors is a claim nobody can redo.
/// * `chosen.effort` must be one the router is allowed to buy. `xhigh`, `max`
///   and `ultra` are human-override only, so a record presenting one as a
///   routed effort is refused rather than shown.
bool isStrictRoutingRecord(Object? value) {
  if (value is! Map<String, dynamic>) return false;
  final allowed = {..._routingRecordFields, 'profile'};
  if (value.keys.any((key) => !allowed.contains(key))) return false;
  if (_routingRecordFields.any((key) => !value.containsKey(key))) return false;
  final className = value['class'];
  if (className is! String || !_routingClassPattern.hasMatch(className)) {
    return false;
  }
  if (!_routingTiers.contains(value['tier'])) return false;
  if (!_isRoutingRisk(value['risk'])) return false;
  if (!_isRoutingTarget(value['chosen'])) return false;
  if (value['runnerUp'] != null && !_isRoutingTarget(value['runnerUp'])) {
    return false;
  }
  if (!boundedNonempty(value['reason'], _maxRoutingReferenceBytes)) {
    return false;
  }
  final reviewRequired = value['reviewRequired'];
  if (reviewRequired is! bool) return false;
  final reasons = value['reviewReasons'];
  if (reasons is! List ||
      reasons.length > _maxRoutingReviewReasons ||
      reasons.any((entry) => !boundedNonempty(entry, _maxRoutingTokenBytes))) {
    return false;
  }
  if (reviewRequired != reasons.isNotEmpty) return false;
  if (value['challengerSample'] is! bool) return false;
  if (value['override'] != null && !_isRoutingOverride(value['override'])) {
    return false;
  }
  if (value['registryVersion'] is! int) return false;
  final revision = value['catalogRevision'];
  if (revision != null && (revision is! int || revision <= 0)) return false;
  if (!value.containsKey('profile')) return true;
  final profile = value['profile'];
  if (profile is! Map<String, dynamic>) return false;
  return profile.entries.every(
    (entry) =>
        _routingTraits.contains(entry.key) &&
        entry.value is num &&
        (entry.value as num) >= 1 &&
        (entry.value as num) <= 5,
  );
}

bool _isRoutingRisk(Object? value) {
  if (value is! Map<String, dynamic>) return false;
  const keys = ['impact', 'uncertainty', 'irreversibility', 'score'];
  if (value.length != keys.length) return false;
  if (keys.any((key) => value[key] is! int)) return false;
  for (final key in ['impact', 'uncertainty', 'irreversibility']) {
    final factor = value[key]! as int;
    if (factor < 1 || factor > 5) return false;
  }
  return value['score'] ==
      (value['impact']! as int) *
          (value['uncertainty']! as int) *
          (value['irreversibility']! as int);
}

bool _isRoutingTarget(Object? value) =>
    value is Map<String, dynamic> &&
    value.length == 3 &&
    boundedNonempty(value['provider'], _maxRoutingReferenceBytes) &&
    boundedNonempty(value['model'], _maxRoutingReferenceBytes) &&
    _routingEfforts.contains(value['effort']);

bool _isRoutingOverride(Object? value) {
  if (value is! Map<String, dynamic>) return false;
  if (value.keys.any((key) => !['model', 'effort', 'because'].contains(key))) {
    return false;
  }
  if (!boundedNonempty(value['model'], _maxRoutingReferenceBytes)) return false;
  if (!boundedNonempty(value['because'], _maxRoutingReferenceBytes)) {
    return false;
  }
  return !value.containsKey('effort') ||
      _routingEfforts.contains(value['effort']);
}
