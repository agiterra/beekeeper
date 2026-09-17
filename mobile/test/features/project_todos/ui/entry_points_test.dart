import 'package:buzz/features/home/home_page.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import '../../projects/ui/fake_projects.dart';

/// The one way a member reaches a project's to-do lists: the "To-do" row in
/// the project's tree on the Home screen (and on the project page, which
/// renders the same tree). Without this the feature is unreachable, and the
/// page tests pass over a screen nothing opens.
void main() {
  Widget settingsPage(BuildContext context) => const SizedBox.shrink();

  Future<void> pumpHome(
    WidgetTester tester, {
    required ProjectTodoOpener? opener,
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
    await pumpHome(tester, opener: (_, address) => opened.add(address));

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
    expect(opened, [testProjectAddress]);
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
