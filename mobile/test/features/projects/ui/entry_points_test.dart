import 'package:buzz/features/home/home_page.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

/// The one way a member reaches a project: its section on the Home screen.
///
/// Without this the feature is unreachable from the app and every other test
/// in this directory passes over a tree nothing shows.
void main() {
  Widget settingsPage(BuildContext context) => const SizedBox.shrink();

  testWidgets('the Home screen lists each project as a section over its '
      'tree, and keeps its channels out of the plain list', (tester) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          savedPrefsProvider.overrideWithValue(prefs),
          ...projectOverrides(
            projects: testProjectsRead(
              [
                testProject(channelIds: const ['c-transport']),
              ],
              referenced: {'c-transport': testChannelData('c-transport')},
            ),
            channels: [
              testChannel('c-general', projectRef: testProjectAddress),
              testChannel('c-other', name: 'other'),
            ],
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
    // The tab content fades in behind an IgnorePointer; settle it first or
    // the header cannot be tapped.
    await tester.pumpAndSettle();

    expect(find.bySemanticsLabel('Projects'), findsNothing);
    expect(
      find.byKey(const ValueKey('home-project-$testProjectAddress')),
      findsOneWidget,
    );
    expect(find.text('Beekeeper'), findsOneWidget);
    // Collapsed by default: the header alone, until tapped.
    expect(find.byType(ProjectTree), findsNothing);
    await tester.tap(
      find.byKey(const ValueKey('home-project-$testProjectAddress')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(ProjectTree), findsOneWidget);
    expect(
      find.byKey(const ValueKey('project-row-channel:c-general')),
      findsOneWidget,
    );
    // The transport carries sessions and is not a channel row; the
    // project-bound channel is not repeated in the plain list; the unbound
    // one still is.
    expect(find.text('meta c-transport'), findsNothing);
    expect(find.text('general'), findsOneWidget);
    expect(find.text('other'), findsOneWidget);
  });
}
