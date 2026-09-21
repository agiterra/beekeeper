import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
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
    // `hireRef` is the create key this decoder was never taught: the 2026-09-01
    // attribution amendment every hired seat's create carries.
    expect(divergent, [
      'create-all-amendments',
      'create-seated-with-hire-ref',
      'invalid-create-uppercase-authority-pubkey',
    ]);
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
    // Four additive keys buzz-core accepts that this decoder's optional list
    // never grew: `beeStamp`, `packRef`, `handover`, `composeRef`.
    expect(divergent, [
      'bee-stamp',
      'bee-stamp-unparsed',
      'compose-ref',
      'context-summary',
      'every-amendment-at-once',
      'handover',
      'invalid-routing-null',
      'invalid-turn-budget-extra-key',
      'pack-ref',
      'pack-ref-shipped',
    ]);
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
    // Two bounds buzz-core sets and this decoder does not.
    expect(divergent, ['oversized-error-message', 'oversized-turn-error-code']);
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
    // `archived` is the third closure action this decoder was never taught,
    // and `"v": 1.0` is the float Dart cannot tell from 1.
    expect(divergent, ['archived', 'v-as-json-float']);
  });
}
