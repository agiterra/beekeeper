import 'package:buzz/features/home/home_page.dart';
import 'package:buzz/features/projects/ui/projects_page.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

/// The one way a member reaches Projects: the fourth Home tab.
///
/// Without this the feature is unreachable from the app and every other test
/// in this directory passes over a page nothing opens.
void main() {
  Widget settingsPage(BuildContext context) => const SizedBox.shrink();

  testWidgets('the Home tab bar has a Projects destination that shows the '
      'projects page', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          savedPrefsProvider.overrideWithValue(prefs),
          ...projectOverrides(projects: testProjectsRead([testProject()])),
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
    await tester.pump();

    expect(find.bySemanticsLabel('Projects'), findsOneWidget);
    expect(find.byType(ProjectsPage), findsNothing);

    await tester.tap(find.bySemanticsLabel('Projects'));
    await tester.pumpAndSettle();

    expect(find.byType(ProjectsPage), findsOneWidget);
    expect(find.text('Beekeeper'), findsOneWidget);
  });
}
