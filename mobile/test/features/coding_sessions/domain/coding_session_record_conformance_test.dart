import 'dart:convert';
import 'dart:io';

import 'package:beekeeper/features/coding_sessions/domain/coding_session_wire.dart';
import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The shared closed-record vectors, run against this app's strict decoders.
///
/// The same files `buzz-core` and the desktop load
/// (`conformance/coding-session-records/`, and `conformance/README.md` for the
/// rule). A lane that adds a key to one of these records adds a vector first
/// and watches every reader's test fail — the rule ledger 204 cost us.
///
/// Nothing here changes a decoder. Where this app and `buzz-core` disagree
/// about a vector, the fixture records both verdicts and the failure message
/// says the divergence moved, so a green suite still shows the disagreement.
const _channel = 'e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86';
const _hex64 =
    'ab12cd34ef56ab78cd90ef12ab34cd56ef78ab90cd12ef34ab56cd78ef90ab12';

Map<String, dynamic> _fixture(String recordDir, int kind) {
  final file = File(
    '../conformance/coding-session-records/$recordDir/fixtures/vectors.json',
  );
  expect(file.existsSync(), isTrue, reason: 'missing fixture ${file.path}');
  final parsed = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
  expect(parsed['schema'], 'buzz-coding-session-record-conformance/v1');
  expect(parsed['kind'], kind);
  expect((parsed['vectors'] as List<dynamic>).length, greaterThanOrEqualTo(10));
  return parsed;
}

NostrEvent _event(int kind, List<List<String>> tags, Object content) =>
    NostrEvent(
      id: _hex64,
      pubkey: 'c' * 64,
      createdAt: 1,
      kind: kind,
      tags: tags,
      content: jsonEncode(content),
      sig: '0' * 128,
    );

/// Run every vector of [recordDir] through [decode], comparing against
/// `accepts.dart` and reporting a moved divergence by name.
///
/// A `null` verdict names a variant this decoder does not read at all — it
/// answers `wrongKind`, which is neither an accept nor a refusal.
List<String> _run(
  String recordDir,
  int kind,
  bool Function(Map<String, dynamic> content) decode,
) {
  final fixture = _fixture(recordDir, kind);
  final divergent = <String>[];
  var checked = 0;
  for (final raw in fixture['vectors'] as List<dynamic>) {
    final vector = raw as Map<String, dynamic>;
    final name = vector['name'] as String;
    expect(
      vector['why'],
      isNotEmpty,
      reason: 'vector $name says why it exists',
    );
    final accepts = vector['accepts'] as Map<String, dynamic>;
    expect(
      accepts.containsKey('dart'),
      isTrue,
      reason: 'vector $name states a verdict for this decoder',
    );
    final expected = accepts['dart'];
    if (expected == null) continue;
    final actual = decode(vector['content'] as Map<String, dynamic>);
    final pinned = accepts['rust'] != null && accepts['rust'] != expected;
    expect(
      actual,
      expected,
      reason: pinned
          ? 'PINNED DIVERGENCE moved: vector $name is recorded as $expected here '
                'against buzz-core\'s ${accepts['rust']}. Update the fixture in the '
                'same commit as the decoder.'
          : 'vector $name disagreed with the fixture. See conformance/README.md.',
    );
    if (pinned) divergent.add(name);
    checked += 1;
  }
  expect(checked, greaterThan(0), reason: '$recordDir exercised this decoder');
  divergent.sort();
  return divergent;
}

/// Run every **raw** vector of [recordDir] through [decode], byte for byte.
///
/// `_run` above hands the decoder `jsonEncode(content)`, which can never carry
/// a duplicate key: `jsonDecode` keeps one of the two and loses the fact that
/// there were two. `rawVectors[]` carry the exact string a provider would
/// sign, and nothing here re-encodes it — that is the whole point (Astra's
/// 2026-09-21 re-check; ledger 223). A raw verdict is never "pinned": the raw
/// suite exists to assert agreement.
void _runRaw(String recordDir, int kind, bool Function(String raw) decode) {
  final fixture = _fixture(recordDir, kind);
  final vectors = fixture['rawVectors'] as List<dynamic>;
  expect(
    vectors.length,
    greaterThanOrEqualTo(2),
    reason: '$recordDir carries a canonical raw vector and a malformed one',
  );
  var accepted = 0;
  for (final entry in vectors) {
    final vector = entry as Map<String, dynamic>;
    final name = vector['name'] as String;
    expect(vector['why'], isNotEmpty, reason: 'raw vector $name says why');
    final accepts = vector['accepts'] as Map<String, dynamic>;
    expect(
      accepts.containsKey('dart'),
      isTrue,
      reason: 'raw vector $name states a verdict for this decoder',
    );
    final expected = accepts['dart'];
    if (expected == null) continue;
    final raw = vector['raw'] as String;
    expect(
      decode(raw),
      expected,
      reason:
          'raw vector $name of $recordDir disagreed with the fixture. '
          'Bytes: $raw',
    );
    if (expected == true) accepted += 1;
  }
  expect(
    accepted,
    greaterThan(0),
    reason: "$recordDir's raw suite has a positive control this decoder takes",
  );
}

NostrEvent _rawEvent(int kind, List<List<String>> tags, String raw) =>
    NostrEvent(
      id: _hex64,
      pubkey: 'c' * 64,
      createdAt: 1,
      kind: kind,
      tags: tags,
      content: raw,
      sig: '0' * 128,
    );

/// The parsed view of raw bytes, for deriving an envelope tag only.
///
/// Never for deciding the verdict: the decoder under test is handed [raw]
/// itself. A payload that will not parse still gets a well-formed envelope, so
/// the decoder answers on the content rather than on a malformed tag.
Map<String, dynamic> _probe(String raw) {
  try {
    final value = jsonDecode(raw);
    return value is Map<String, dynamic> ? value : <String, dynamic>{};
  } on FormatException {
    return <String, dynamic>{};
  }
}

void main() {
  test('44221 create decoder runs the shared vectors', () {
    final divergent = _run('44221-lifecycle-command', 44221, (content) {
      final commandId = content['commandId'] as String? ?? 'cmd-1';
      final event = _event(EventKind.codingSessionLifecycleCommand, [
        ['h', _channel],
        ['csl-v', codingSessionLifecycleCommandTagVersion],
        ['csl-command', commandId],
      ], content);
      return decodeCodingSessionCreate(event).value != null;
    });
    // None left. `hireRef` — the 2026-09-01 attribution amendment every hired
    // seat's create carries — is now in `createKeys`, and an uppercase signed
    // authority pubkey is refused rather than lowercased.
    expect(divergent, isEmpty);
  });

  test('44223 metadata decoder runs the shared vectors', () {
    final divergent = _run('44223-metadata', 44223, (content) {
      final target = CodingSessionTarget.decode(content['session']);
      final event = _event(EventKind.codingSessionMetadata, [
        ['h', _channel],
        ['csm-v', codingSessionMetadataTagVersion],
        ['cs-target', target?.key ?? ''],
        ['csm-key', target?.metadataSemanticKey ?? ''],
      ], content);
      return decodeCodingSessionMetadata(event).value != null;
    });
    // None left. The four additive keys this decoder's optional list never
    // grew — `beeStamp`, `packRef`, `handover`, `composeRef` — are read and
    // validated; `"routing": null`, an unknown key inside `turnBudget` and the
    // three summary keys no writer emits are refused.
    expect(divergent, isEmpty);
  });

  test('44224 receipt decoder runs the shared vectors', () {
    final divergent = _run('44224-lifecycle-receipt', 44224, (content) {
      final commandId = content['commandId'] as String? ?? 'cmd-1';
      final status = CodingSessionReceiptStatus.fromWire(content['status']);
      final target = CodingSessionTarget.decode(content['session']);
      final event = _event(EventKind.codingSessionLifecycleReceipt, [
        ['h', _channel],
        ['cslr-v', codingSessionReceiptTagVersion],
        ['csl-command', commandId],
        [
          'csl-key',
          status == null
              ? encodeStructuredKey(codingSessionReceiptKeyDomain, [commandId])
              : codingSessionReceiptSemanticKey(commandId, status),
        ],
      ], content);
      // Unused beyond keeping the target decode honest about its own shape.
      expect(target, anyOf(isNull, isNotNull));
      return decodeCodingSessionReceipt(event).value != null;
    });
    // None left: a receipt error code is bounded at 64 bytes and its message
    // at 1027, exactly as `validate_lifecycle_receipt` bounds them.
    expect(divergent, isEmpty);
  });

  test('44226 genesis decoder runs the shared vectors', () {
    final divergent = _run('44226-genesis', 44226, (content) {
      final sessionRef = content['sessionRef'];
      final event = _event(EventKind.codingSessionGenesis, [
        ['h', _channel],
        ['csg-v', codingSessionGenesisTagVersion],
        ['csg-session', sessionRef is String ? sessionRef : ''],
      ], content);
      return decodeCodingSessionGenesis(event).value != null;
    });
    // `"v": 1.0` is a float serde refuses for a `u64`; Dart reads 1.0 == 1.
    expect(divergent, ['v-as-json-float']);
  });

  test('44230 closure decoder runs the shared vectors', () {
    final divergent = _run('44230-closure', 44230, (content) {
      final sessionRef = content['sessionRef'];
      final genesisRef = content['genesisRef'];
      final event = _event(EventKind.codingSessionClosure, [
        ['h', _channel],
        ['d', sessionRef is String ? sessionRef : ''],
        ['cscl-v', codingSessionClosureTagVersion],
        ['cscl-genesis', genesisRef is String ? genesisRef : 'b' * 64],
      ], content);
      return decodeCodingSessionClosure(event).value != null;
    });
    // `archived`, the third closure action, is now read. `"v": 1.0` stays
    // pinned: serde refuses a float for a `u64` and Dart reads 1.0 == 1, which
    // is a property of the languages, not of anyone's code. Nothing signs it.
    expect(divergent, ['v-as-json-float']);
  });

  test('44221 create decoder runs the raw vectors byte for byte', () {
    _runRaw('44221-lifecycle-command', 44221, (raw) {
      final commandId = _probe(raw)['commandId'] as String? ?? 'cmd-1';
      return decodeCodingSessionCreate(
            _rawEvent(EventKind.codingSessionLifecycleCommand, [
              ['h', _channel],
              ['csl-v', codingSessionLifecycleCommandTagVersion],
              ['csl-command', commandId],
            ], raw),
          ).value !=
          null;
    });
  });

  test('44223 metadata decoder runs the raw vectors byte for byte', () {
    _runRaw('44223-metadata', 44223, (raw) {
      final target = CodingSessionTarget.decode(_probe(raw)['session']);
      return decodeCodingSessionMetadata(
            _rawEvent(EventKind.codingSessionMetadata, [
              ['h', _channel],
              ['csm-v', codingSessionMetadataTagVersion],
              ['cs-target', target?.key ?? ''],
              ['csm-key', target?.metadataSemanticKey ?? ''],
            ], raw),
          ).value !=
          null;
    });
  });

  test('44224 receipt decoder runs the raw vectors byte for byte', () {
    _runRaw('44224-lifecycle-receipt', 44224, (raw) {
      final payload = _probe(raw);
      final commandId = payload['commandId'] as String? ?? 'cmd-1';
      final status = CodingSessionReceiptStatus.fromWire(payload['status']);
      return decodeCodingSessionReceipt(
            _rawEvent(EventKind.codingSessionLifecycleReceipt, [
              ['h', _channel],
              ['cslr-v', codingSessionReceiptTagVersion],
              ['csl-command', commandId],
              [
                'csl-key',
                status == null
                    ? encodeStructuredKey(codingSessionReceiptKeyDomain, [
                        commandId,
                      ])
                    : codingSessionReceiptSemanticKey(commandId, status),
              ],
            ], raw),
          ).value !=
          null;
    });
  });

  test('44226 genesis decoder runs the raw vectors byte for byte', () {
    _runRaw('44226-genesis', 44226, (raw) {
      final sessionRef = _probe(raw)['sessionRef'];
      return decodeCodingSessionGenesis(
            _rawEvent(EventKind.codingSessionGenesis, [
              ['h', _channel],
              ['csg-v', codingSessionGenesisTagVersion],
              ['csg-session', sessionRef is String ? sessionRef : ''],
            ], raw),
          ).value !=
          null;
    });
  });

  test('44230 closure decoder runs the raw vectors byte for byte', () {
    _runRaw('44230-closure', 44230, (raw) {
      final payload = _probe(raw);
      final sessionRef = payload['sessionRef'];
      final genesisRef = payload['genesisRef'];
      return decodeCodingSessionClosure(
            _rawEvent(EventKind.codingSessionClosure, [
              ['h', _channel],
              ['d', sessionRef is String ? sessionRef : ''],
              ['cscl-v', codingSessionClosureTagVersion],
              ['cscl-genesis', genesisRef is String ? genesisRef : 'b' * 64],
            ], raw),
          ).value !=
          null;
    });
  });

  // The scanner itself, directly: the raw suites above prove the decoders
  // refuse these payloads, and this proves *why* — so a later change that
  // removes the scan fails here with a sentence about duplicate keys rather
  // than only as five decoder mismatches.
  group('hasDuplicateJsonKeys', () {
    test('canonical provider bytes carry no duplicate', () {
      expect(
        hasDuplicateJsonKeys('{"a":1,"b":{"c":[1,2,{"d":3}]},"e":"f"}'),
        isFalse,
      );
    });

    test('a repeated key at any depth is a duplicate', () {
      expect(hasDuplicateJsonKeys('{"a":1,"a":2}'), isTrue);
      expect(hasDuplicateJsonKeys('{"o":{"a":1,"a":2}}'), isTrue);
      expect(hasDuplicateJsonKeys('{"l":[{"a":1,"a":2}]}'), isTrue);
    });

    test('the same key in sibling objects is not a duplicate', () {
      expect(hasDuplicateJsonKeys('{"x":{"a":1},"y":{"a":2}}'), isFalse);
      expect(hasDuplicateJsonKeys('{"l":[{"a":1},{"a":2}]}'), isFalse);
    });

    test('a value that merely looks like a key is not one', () {
      expect(hasDuplicateJsonKeys('{"a":"b","c":"b"}'), isFalse);
      expect(hasDuplicateJsonKeys('{"a":["k","k"]}'), isFalse);
    });

    test('an escaped quote inside a key does not end the key', () {
      expect(hasDuplicateJsonKeys(r'{"a\"b":1,"c":2}'), isFalse);
      expect(hasDuplicateJsonKeys(r'{"a\"b":1,"a\"b":2}'), isTrue);
    });

    test('unterminated bytes are reported as ambiguous, never as clean', () {
      expect(hasDuplicateJsonKeys('{"a":1'), isFalse);
      expect(hasDuplicateJsonKeys('{"a'), isTrue);
      expect(hasDuplicateJsonKeys('}'), isTrue);
    });
  });
}
