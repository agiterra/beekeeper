import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:buzz/features/agents_repo/domain/agents_repo_draft_op.dart';

/// The Dart reader binds to `conformance/agents-repo-draft-path/fixtures/
/// path-vectors.json`, the same corpus the Rust reader in `buzz-core` and the
/// Desktop reader load. A rule implemented in only one of the three is a
/// defect, and nothing else in CI would catch it: each reader's own tests use
/// its own table.
void main() {
  test('every banked path vector classifies identically here', () {
    final corpus =
        jsonDecode(
              File(
                '../conformance/agents-repo-draft-path/fixtures/path-vectors.json',
              ).readAsStringSync(),
            )
            as Map<String, dynamic>;
    expect(corpus['schema'], 'buzz-agents-repo-draft-path-vectors/v2');
    final cases = corpus['cases'] as List<dynamic>;
    expect(cases, isNotEmpty, reason: 'an empty corpus proves nothing');

    const wire = <DraftPathClass, String>{
      DraftPathClass.rootFile: 'root-file',
      DraftPathClass.role: 'role',
      DraftPathClass.archivedRole: 'archived-role',
      DraftPathClass.roleSkill: 'role-skill',
      DraftPathClass.sharedSkill: 'shared-skill',
      DraftPathClass.plan: 'plan',
      DraftPathClass.archivedPlan: 'archived-plan',
      DraftPathClass.document: 'document',
      DraftPathClass.documentAsset: 'document-asset',
      DraftPathClass.documentFolder: 'document-folder',
    };

    for (final entry in cases.cast<Map<String, dynamic>>()) {
      final path = entry['path'] as String;
      final expected = entry['class'] as String?;
      final note = entry['note'] as String? ?? '';
      final actual = draftPathClass(path);
      if (expected == null) {
        expect(actual, isNull, reason: '"$path" should be refused — $note');
      } else {
        expect(
          actual,
          isNotNull,
          reason: '"$path" should be $expected — $note',
        );
        expect(wire[actual!], expected, reason: '"$path" — $note');
      }
    }

    final moves = corpus['moves'] as List<dynamic>;
    expect(moves, isNotEmpty, reason: 'the destination rule needs vectors too');
    for (final entry in moves.cast<Map<String, dynamic>>()) {
      final from = entry['from'] as String;
      final to = entry['to'] as String;
      final ok = entry['ok'] as bool;
      final note = entry['note'] as String? ?? '';
      final error = moveDestinationError(from, to);
      expect(
        error == null,
        ok,
        reason:
            '"$from" -> "$to" should be ${ok ? "legal" : "refused"} — '
            '$note${error == null ? "" : ": $error"}',
      );
    }
  });
}
