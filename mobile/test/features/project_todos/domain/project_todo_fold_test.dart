import 'dart:convert';
import 'dart:io';

import 'package:buzz/features/project_todos/domain/project_todo_fold.dart';
import 'package:buzz/features/project_todos/domain/project_todo_op.dart';
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

  test('an event with no, two, or an unknown td-vis tag is ignored and '
      'counted; the list it names keeps its create\'s visibility', () {
    final owner = 'a' * 64;
    final project = '30621:$owner:p';
    const listId = '11111111111111111111111111111111';
    const create = ProjectTodoOp.listCreate(
      listId: listId,
      visibility: TodoVisibility.personal,
      title: 'Mine',
    );
    const retitle = ProjectTodoOp.listTitle(
      listId: listId,
      visibility: TodoVisibility.personal,
      title: 'Renamed',
    );
    NostrEvent event(
      String id,
      int at,
      ProjectTodoOp op, {
      List<List<String>>? tags,
    }) => NostrEvent(
      id: id.padLeft(64, '0'),
      pubkey: owner,
      createdAt: at,
      kind: 44248,
      tags: tags ?? op.tags(project),
      content: op.toContent(),
      sig: '',
    );
    final digest = foldProjectTodos(project, [
      event('1', 1, create),
      event(
        '2',
        2,
        retitle,
        tags: retitle.tags(project).where((t) => t[0] != 'td-vis').toList(),
      ),
      event(
        '3',
        3,
        retitle,
        tags: [
          ...retitle.tags(project),
          ['td-vis', 'personal'],
        ],
      ),
      event(
        '4',
        4,
        retitle,
        tags: [
          for (final t in retitle.tags(project))
            if (t[0] == 'td-vis') ['td-vis', 'team'] else t,
        ],
      ),
      event('5', 5, retitle),
    ]);
    expect(digest.ignored, 3);
    final list = digest.lists.single;
    expect(list.visibility, TodoVisibility.personal);
    expect(list.personal, isTrue);
    expect(list.title, 'Renamed');
    expect(list.updatedAt, 5);
    expect(list.pinned, isFalse);
  });

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
