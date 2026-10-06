import 'package:beekeeper/features/home/home_page.dart';
import 'package:beekeeper/features/projects/ui/project_tree.dart';
import 'package:beekeeper/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import '../../projects/ui/fake_projects.dart';

/// The one way a member reaches a project's to-do lists: the "To-do" row in
/// the project's tree on the Home screen (and on the project page, which
/// renders the same tree). Without this the feature is unreachable, and the
/// page tests pass over a screen nothing opens.
const pinnedA = '0123456789abcdef0123456789abcdef';
const pinnedB = 'fedcba9876543210fedcba9876543210';

void main() {
  Widget settingsPage(BuildContext context) => const SizedBox.shrink();

  Future<void> pumpHome(
    WidgetTester tester, {
    required ProjectTodoOpener? opener,
    ProjectPinnedTodoListsReader? pinnedTodoLists,
  }) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          savedPrefsProvider.overrideWithValue(prefs),
          ...projectOverrides(
            projects: testProjectsRead([testProject()]),
            // Home lists projects only above a non-empty channel list; the
            // project itself binds nothing, so its tree is otherwise empty.
            channels: [testChannel('c-other', name: 'other')],
            todoOpener: opener,
            pinnedTodoLists: pinnedTodoLists,
          ),
          fakeObserverOverride(FakeObserverBinding(testSnapshot())),
        ],
        child: MaterialApp(
          theme: AppTheme.light(),
          home: HomePage(
            settingsPageBuilder: settingsPage,
            hasUnreadInbox: false,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(
      find.byKey(const ValueKey('home-project-$testProjectAddress')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(ProjectTree), findsOneWidget);
  }

  testWidgets('the project tree carries a To-do row that opens the '
      'project\'s lists by address', (tester) async {
    final opened = <String>[];
    await pumpHome(
      tester,
      opener: (_, address, {listId}) => opened.add('$address $listId'),
    );

    final row = find.byKey(
      const ValueKey('project-row-todo:$testProjectAddress'),
    );
    expect(row, findsOneWidget);
    expect(find.text('To-do'), findsOneWidget);
    // The door sits above the "nothing here" line of an otherwise empty
    // project rather than replacing it.
    expect(find.text(projectEmptyLabel), findsOneWidget);
    expect(
      tester.getTopLeft(row).dy <
          tester.getTopLeft(find.text(projectEmptyLabel)).dy,
      isTrue,
    );

    await tester.tap(row);
    await tester.pumpAndSettle();
    expect(opened, ['$testProjectAddress null']);
    // No reader: no pinned rows, and nothing claims there are any.
    expect(
      find.byKey(
        const ValueKey('project-row-todo-list:$testProjectAddress:$pinnedA'),
      ),
      findsNothing,
    );
  });

  testWidgets('pinned lists render under the To-do row, a personal one '
      'with a lock, and open their list by id', (tester) async {
    final opened = <String>[];
    await pumpHome(
      tester,
      opener: (_, address, {listId}) => opened.add('$address $listId'),
      pinnedTodoLists: (ref, address) => address == testProjectAddress
          ? const [
              PinnedTodoListRow(id: pinnedA, title: 'Launch', personal: false),
              PinnedTodoListRow(id: pinnedB, title: 'Mine', personal: true),
            ]
          : const [],
    );

    final todoRow = find.byKey(
      const ValueKey('project-row-todo:$testProjectAddress'),
    );
    final launchRow = find.byKey(
      const ValueKey('project-row-todo-list:$testProjectAddress:$pinnedA'),
    );
    final mineRow = find.byKey(
      const ValueKey('project-row-todo-list:$testProjectAddress:$pinnedB'),
    );
    expect(launchRow, findsOneWidget);
    expect(mineRow, findsOneWidget);
    expect(find.text('Launch'), findsOneWidget);
    expect(find.text('Mine'), findsOneWidget);
    // Under the To-do door, above the "nothing here" line.
    expect(
      tester.getTopLeft(todoRow).dy < tester.getTopLeft(launchRow).dy,
      isTrue,
    );
    expect(
      tester.getTopLeft(launchRow).dy < tester.getTopLeft(mineRow).dy,
      isTrue,
    );
    expect(
      tester.getTopLeft(mineRow).dy <
          tester.getTopLeft(find.text(projectEmptyLabel)).dy,
      isTrue,
    );
    // Only the personal list carries the lock.
    expect(
      find.byKey(const ValueKey('project-row-todo-list-lock:$pinnedB')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('project-row-todo-list-lock:$pinnedA')),
      findsNothing,
    );
    expect(
      find.descendant(of: mineRow, matching: find.byIcon(LucideIcons.lock)),
      findsOneWidget,
    );
    expect(
      find.descendant(of: launchRow, matching: find.byIcon(LucideIcons.lock)),
      findsNothing,
    );

    await tester.tap(mineRow);
    await tester.pumpAndSettle();
    expect(opened, ['$testProjectAddress $pinnedB']);
    await tester.tap(launchRow);
    await tester.pumpAndSettle();
    expect(opened.last, '$testProjectAddress $pinnedA');
  });

  testWidgets('without an opener there is no row to lie about', (tester) async {
    await pumpHome(tester, opener: null);
    expect(
      find.byKey(const ValueKey('project-row-todo:$testProjectAddress')),
      findsNothing,
    );
    expect(find.text('To-do'), findsNothing);
  });
}
