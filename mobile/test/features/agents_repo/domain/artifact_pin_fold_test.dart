import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/agents_repo/domain/artifact_pin_fold.dart';
import 'package:buzz/features/agents_repo/domain/artifact_pin_op.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The Dart fold binds to `conformance/project-artifact-pin-fold/fixtures/
/// fold-vectors.json`, the same corpus the Rust fold in `buzz-core` and the
/// Desktop fold load. A rule implemented in only one of the three is a defect,
/// and nothing else in CI would catch it: each fold's own tests use its own
/// table.
void main() {
  final corpus =
      jsonDecode(
            File(
              '../conformance/project-artifact-pin-fold/fixtures/fold-vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema', () {
    expect(corpus['schema'], 'buzz-project-artifact-pin-fold-vectors/v1');
    expect(
      projectArtifactPinDigestSchema,
      'buzz-project-artifact-pin-digest/v1',
    );
    expect(
      corpus['cases'] as List<dynamic>,
      isNotEmpty,
      reason: 'an empty corpus proves nothing',
    );
  });

  for (final entry
      in (corpus['cases'] as List<dynamic>).cast<Map<String, dynamic>>()) {
    test('fold vector: ${entry['name']}', () {
      final events = [
        for (final raw in (entry['events'] as List<dynamic>))
          NostrEvent.fromJson({...raw as Map<String, dynamic>, 'sig': ''}),
      ];
      final digest = foldProjectArtifactPins(
        entry['project'] as String,
        entry['repo'] as String,
        events,
      );
      expect(
        jsonDecode(jsonEncode(digest.toJson())),
        entry['expected'],
        reason: entry['name'] as String,
      );
    });
  }

  test('pinnedOnly keeps the order and drops the unpinned', () {
    final vector = (corpus['cases'] as List<dynamic>)
        .cast<Map<String, dynamic>>()
        .firstWhere(
          (c) => (c['name'] as String).startsWith('un-pinning keeps the row'),
        );
    final digest = foldProjectArtifactPins(
      vector['project'] as String,
      vector['repo'] as String,
      [
        for (final raw in (vector['events'] as List<dynamic>))
          NostrEvent.fromJson({...raw as Map<String, dynamic>, 'sig': ''}),
      ],
    );
    expect(digest.pins, hasLength(1));
    expect(digest.pins.single.pinned, isFalse);
    expect(digest.pinnedOnly, isEmpty);
  });

  test('a folder keep is never a file target; its folder is', () {
    expect(
      pinTargetError('docs/mockups/.gitkeep', PinTargetKind.file),
      contains('pin the folder'),
    );
    expect(pinTargetError('docs/mockups', PinTargetKind.folder), isNull);
    expect(pinTargetError('docs/mockups', PinTargetKind.file), isNotNull);
    expect(
      pinTargetError('plans/CURRENT_STATE.md', PinTargetKind.file),
      isNull,
    );
    expect(pinTargetError('docs', PinTargetKind.folder), isNotNull);
    expect(
      pinTargetError('docs/a/b/c/d/e/f/g/h', PinTargetKind.folder),
      contains('still fits'),
    );
  });
}
