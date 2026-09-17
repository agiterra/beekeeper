import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/project_todos/domain/project_todo_fold.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// The Dart fold binds to `conformance/project-todo-fold/fixtures/
/// fold-vectors.json` byte for byte: the digest it produces, re-encoded,
/// must equal the vector's expectation encoded the same way.
void main() {
  final vectors =
      jsonDecode(
            File(
              '../conformance/project-todo-fold/fixtures/fold-vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema', () {
    expect(vectors['schema'], 'buzz-project-todo-fold-vectors/v1');
    expect((vectors['cases'] as List<dynamic>).isNotEmpty, isTrue);
  });

  for (final raw in vectors['cases'] as List<dynamic>) {
    final vector = raw as Map<String, dynamic>;
    test(vector['name'] as String, () {
      final events = [
        for (final e in vector['events'] as List<dynamic>)
          NostrEvent.fromJson({...e as Map<String, dynamic>, 'sig': ''}),
      ];
      final digest = foldProjectTodos(vector['project'] as String, events);
      expect(digest.toJson(), equals(vector['expected']));
      expect(jsonEncode(digest.toJson()), jsonEncode(vector['expected']));
    });
  }

  test('an empty digest reports its project and nothing else', () {
    final project = '30621:${'a' * 64}:p';
    final digest = ProjectTodoDigest.empty(project);
    expect(digest.toJson(), {
      'schema': projectTodoDigestSchema,
      'project': project,
      'ignored': 0,
      'lists': <Object?>[],
    });
    expect(digest.listById('x'), isNull);
  });
}
