import 'package:buzz/features/coding_sessions/ui/coding_session_page.dart';
import 'package:buzz/features/coding_sessions/ui/coding_sessions_page.dart';
import 'package:buzz/features/projects/ui/project_page.dart';
import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/misc.dart';

import '../../../helpers/widget_helpers.dart';
import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

Future<FakeObserverBinding> _pump(
  WidgetTester tester, {
  List<Override> extra = const [],
  TerminalsIndex? terminals,
  FakeObserverBinding? observer,
}) async {
  final binding = observer ?? FakeObserverBinding(testSnapshot());
  await tester.pumpWidget(
    KeyedSubtree(
      key: UniqueKey(),
      child: WidgetHelpers.testable(
        overrides: [
          ...projectOverrides(
            projects: testProjectsRead(
              [
                testProject(channelIds: const ['c-transport']),
              ],
              referenced: {'c-transport': testChannelData('c-transport')},
            ),
            terminals: terminals,
            channels: [
              testChannel('c-general', projectRef: testProjectAddress),
              testChannel('c-other', name: 'other'),
            ],
          ),
          fakeObserverOverride(binding),
          ...extra,
        ],
        child: const ProjectPage(address: testProjectAddress),
      ),
    ),
  );
  await tester.pump();
  await tester.pump();
  return binding;
}

void main() {
  testWidgets('shows the project\'s channels — the transport first, the '
      'back-referenced chat channel, never an unbound one', (tester) async {
    await _pump(tester);

    final rows = find.byWidgetPredicate(
      (widget) =>
          widget.key is ValueKey<String> &&
          (widget.key as ValueKey<String>).value.startsWith(
            'project-channel-row-',
          ),
    );
    expect(rows, findsNWidgets(2));
    final keys = tester
        .widgetList(rows)
        .map((w) => (w.key as ValueKey<String>).value)
        .toList();
    expect(keys, [
      'project-channel-row-c-transport',
      'project-channel-row-c-general',
    ]);
    expect(find.text('sessions channel · not in your list'), findsOneWidget);
  });

  testWidgets('the sessions channel starts expanded with its sessions, and '
      'a session row opens the session page', (tester) async {
    await _pump(tester);

    expect(
      find.byKey(const ValueKey('project-channel-sessions-c-transport')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('project-channel-sessions-c-general')),
      findsNothing,
    );
    expect(find.text('Ship the observer'), findsOneWidget);

    await tester.tap(
      find.byKey(const ValueKey('project-session-row-umbrella-1')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(CodingSessionPage), findsOneWidget);
  });

  testWidgets('a collapsed channel can be expanded, and a channel not in '
      'my list opens its sessions list on tap', (tester) async {
    await _pump(tester);
    await tester.tap(
      find.byKey(const ValueKey('project-channel-toggle-c-general')),
    );
    await tester.pump();
    expect(
      find.byKey(const ValueKey('project-channel-sessions-c-general')),
      findsOneWidget,
    );

    await tester.tap(
      find.byKey(const ValueKey('project-channel-row-c-transport')),
    );
    await tester.pumpAndSettle();
    expect(find.byType(CodingSessionsPage), findsOneWidget);
  });

  testWidgets('terminals sit under the project with the viewer\'s access', (
    tester,
  ) async {
    await _pump(
      tester,
      terminals: testTerminalsIndex([
        testTerminal(
          roster: const [
            ShellRosterEntry(
              pubkey: testViewer,
              role: ShellRosterRole.collaborator,
            ),
          ],
        ),
        testTerminal(
          sessionId: 's2',
          title: 'logs',
          projectRef: '30621:$testOwner:elsewhere',
        ),
      ]),
    );

    expect(find.byKey(const ValueKey('project-terminals')), findsOneWidget);
    expect(find.text('build shell'), findsOneWidget);
    expect(find.text('logs'), findsNothing);
    expect(find.textContaining('collaborator · you can type'), findsOneWidget);
  });

  testWidgets('terminals not yet read, and none shared, each say so', (
    tester,
  ) async {
    await _pump(
      tester,
      terminals: testTerminalsIndex(
        const [],
        connection: TerminalsConnection.connecting,
        hasRead: false,
      ),
    );
    expect(
      find.byKey(const ValueKey('project-terminals-unread')),
      findsOneWidget,
    );

    await _pump(tester, terminals: testTerminalsIndex(const []));
    expect(
      find.byKey(const ValueKey('project-terminals-empty')),
      findsOneWidget,
    );
  });

  testWidgets('a project not in the read says so instead of rendering blank', (
    tester,
  ) async {
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [
          ...projectOverrides(projects: testProjectsRead(const [])),
          fakeObserverOverride(FakeObserverBinding(testSnapshot())),
        ],
        child: const ProjectPage(address: '30621:$testOwner:missing'),
      ),
    );
    await tester.pump();
    expect(find.byKey(const ValueKey('project-missing')), findsOneWidget);
  });
}
