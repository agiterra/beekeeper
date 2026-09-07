import 'package:buzz/features/projects/state/projects_provider.dart';
import 'package:buzz/features/projects/ui/project_page.dart';
import 'package:buzz/features/projects/ui/projects_page.dart';
import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

Future<void> _pump(
  WidgetTester tester, {
  required ProjectsRead projects,
  TerminalsIndex? terminals,
}) async {
  await tester.pumpWidget(
    WidgetHelpers.testable(
      overrides: [
        ...projectOverrides(projects: projects, terminals: terminals),
        fakeObserverOverride(FakeObserverBinding(testSnapshot(sessions: []))),
      ],
      child: const ProjectsPage(),
    ),
  );
  await tester.pump();
}

void main() {
  testWidgets('lists projects with access, channel and terminal counts', (
    tester,
  ) async {
    await _pump(
      tester,
      projects: testProjectsRead([
        testProject(channelIds: const ['c1', 'c2']),
        testProject(dtag: 'zeta', name: 'Zeta', isPrivate: true),
      ]),
      terminals: testTerminalsIndex([testTerminal()]),
    );

    expect(find.text('Beekeeper'), findsOneWidget);
    expect(find.text('public · 2 channels · 1 terminal'), findsOneWidget);
    expect(find.text('Zeta'), findsOneWidget);
    expect(find.text('private'), findsOneWidget);
  });

  testWidgets('a read that has not returned is loading, not empty', (
    tester,
  ) async {
    await _pump(
      tester,
      projects: testProjectsRead(
        const [],
        connection: ProjectsConnection.connecting,
        hasRead: false,
      ),
    );
    expect(find.byKey(const ValueKey('projects-loading')), findsOneWidget);
    expect(find.text(projectsEmptyLabel), findsNothing);
  });

  testWidgets('a failed read says so and offers Retry', (tester) async {
    await _pump(
      tester,
      projects: testProjectsRead(
        const [],
        connection: ProjectsConnection.error,
        hasRead: false,
        lastError: 'relay refused',
      ),
    );
    expect(find.byKey(const ValueKey('projects-error')), findsOneWidget);
    expect(find.text('relay refused'), findsOneWidget);
    expect(find.byKey(const ValueKey('projects-retry')), findsOneWidget);
  });

  testWidgets('an empty successful read is the only "no projects"', (
    tester,
  ) async {
    await _pump(tester, projects: testProjectsRead(const []));
    expect(find.text(projectsEmptyLabel), findsOneWidget);
  });

  testWidgets('tapping a project opens its page', (tester) async {
    await _pump(tester, projects: testProjectsRead([testProject()]));
    await tester.tap(
      find.byKey(const ValueKey('project-row-$testProjectAddress')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(ProjectPage), findsOneWidget);
    expect(find.byKey(const ValueKey('project-header')), findsOneWidget);
  });
}
