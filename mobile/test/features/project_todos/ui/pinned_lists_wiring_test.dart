import 'package:buzz/app.dart';
import 'package:buzz/features/project_todos/domain/project_todo_op.dart';
import 'package:buzz/features/project_todos/state/project_todos_provider.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart' show Override;

import '../fake_project_todos.dart';

/// The composition root's reader: what the project tree gets when
/// `appFeatureOverrides` is installed — the fold's pinned, unarchived
/// lists, personal ones flagged — and nothing when it is not.
void main() {
  Future<List<PinnedTodoListRow>?> readRows(
    WidgetTester tester, {
    required List<Override> overrides,
  }) async {
    List<PinnedTodoListRow>? rows;
    var read = false;
    await tester.pumpWidget(
      ProviderScope(
        overrides: overrides,
        child: Consumer(
          builder: (context, ref, _) {
            final reader = ref.watch(projectPinnedTodoListsProvider);
            rows = reader?.call(ref, todoAddress);
            read = true;
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    await tester.pump();
    expect(read, isTrue);
    return rows;
  }

  testWidgets('appFeatureOverrides reads pinned, unarchived lists from the '
      'project fold', (tester) async {
    final lists = [
      testTodoList(id: listA, title: 'Launch', pinned: true),
      testTodoList(
        id: listB,
        title: 'Mine',
        visibility: TodoVisibility.personal,
        pinned: true,
        createdAt: 200,
      ),
      testTodoList(
        id: 'cccccccccccccccccccccccccccccccc',
        title: 'Unpinned',
        createdAt: 300,
      ),
      testTodoList(
        id: 'dddddddddddddddddddddddddddddddd',
        title: 'Archived pin',
        pinned: true,
        archived: true,
        createdAt: 400,
      ),
    ];
    final rows = await readRows(
      tester,
      overrides: [
        ...appFeatureOverrides(),
        projectTodosProvider.overrideWith(
          () => FakeProjectTodosNotifier(
            todoAddress,
            testTodosRead(lists: lists),
          ),
        ),
      ],
    );
    expect(rows, isNotNull);
    expect(rows!.map((r) => '${r.id} ${r.title} ${r.personal}'), [
      '$listA Launch false',
      '$listB Mine true',
    ]);
  });

  testWidgets('without the overrides the reader is null', (tester) async {
    final rows = await readRows(tester, overrides: const []);
    expect(rows, isNull);
  });
}
