import 'package:buzz/features/home/home_page.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import '../../projects/ui/fake_projects.dart';

/// The one way a member reaches a pinned artifact from the phone: a row in
/// the project's tree, under the Artifacts door. Without this, a pin someone
/// made on the desktop is invisible here and the feature is half a feature.
void main() {
  Widget settingsPage(BuildContext context) => const SizedBox.shrink();

  Future<void> pumpHome(
    WidgetTester tester, {
    required ProjectAgentsRepoOpener? opener,
    ProjectPinnedArtifactsReader? pinnedArtifacts,
  }) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          savedPrefsProvider.overrideWithValue(prefs),
          ...projectOverrides(
            projects: testProjectsRead([testProject()]),
            channels: [testChannel('c-other', name: 'other')],
            agentsRepoOpener: opener,
            pinnedArtifacts: pinnedArtifacts,
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

  testWidgets('the Artifacts door opens the agents repository by address', (
    tester,
  ) async {
    final opened = <String>[];
    await pumpHome(
      tester,
      opener: (_, address, {path}) => opened.add('$address $path'),
    );

    final row = find.byKey(
      const ValueKey('project-row-files:$testProjectAddress'),
    );
    expect(row, findsOneWidget);
    // The door reads "Artifacts", not "Files": it holds the plans and the
    // documents, and the name should say so.
    expect(find.text('Artifacts'), findsOneWidget);
    await tester.tap(row);
    await tester.pumpAndSettle();
    expect(opened, ['$testProjectAddress null']);
  });

  testWidgets('a pinned document and a pinned folder are rows that open their '
      'own target, in the order given', (tester) async {
    final opened = <String>[];
    await pumpHome(
      tester,
      opener: (_, address, {path}) => opened.add('$address $path'),
      pinnedArtifacts: (_, _) => const [
        PinnedArtifactRow(
          target: 'docs/mockups',
          label: 'mockups',
          isFolder: true,
        ),
        PinnedArtifactRow(
          target: 'docs/notes/api-shape.md',
          label: 'api-shape',
          isFolder: false,
        ),
      ],
    );

    final folderRow = find.byKey(
      const ValueKey('project-row-artifact:$testProjectAddress:docs/mockups'),
    );
    final docRow = find.byKey(
      const ValueKey(
        'project-row-artifact:$testProjectAddress:docs/notes/api-shape.md',
      ),
    );
    expect(folderRow, findsOneWidget);
    expect(docRow, findsOneWidget);
    expect(find.text('mockups'), findsOneWidget);
    // `.md` comes off; a folder reads as its own last segment.
    expect(find.text('api-shape'), findsOneWidget);
    // The icons tell a folder from a document without reading the label.
    expect(find.byIcon(LucideIcons.folder), findsOneWidget);
    expect(find.byIcon(LucideIcons.fileText), findsOneWidget);

    // The order is the one the reader gave — the project's rank order, which
    // whoever reordered the pins decided. Re-sorting it here would throw that
    // away, so the row positions are asserted, not just their presence.
    final folderY = tester.getTopLeft(folderRow).dy;
    final docY = tester.getTopLeft(docRow).dy;
    expect(folderY < docY, isTrue, reason: 'mockups was pinned first');

    await tester.tap(docRow);
    await tester.pumpAndSettle();
    expect(opened, ['$testProjectAddress docs/notes/api-shape.md']);
  });

  testWidgets('no reader means no rows, and the door still opens', (
    tester,
  ) async {
    await pumpHome(tester, opener: (_, _, {path}) {});
    expect(
      find.byKey(
        const ValueKey('project-row-artifact:$testProjectAddress:docs/a.md'),
      ),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('project-row-files:$testProjectAddress')),
      findsOneWidget,
    );
  });
}
