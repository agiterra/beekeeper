import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/coding_sessions/ui/coding_session_page.dart';
import 'package:buzz/features/coding_sessions/ui/coding_sessions_page.dart';
import 'package:buzz/features/projects/ui/project_page.dart';
import 'package:buzz/features/projects/ui/project_tree.dart';
import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/misc.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../../helpers/widget_helpers.dart';
import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

/// A session founded by the viewer, so the default "My sessions" shows it.
CodingSessionUmbrella _mine({
  String key = 'umbrella-mine',
  bool closed = false,
  CodingSessionFoldedStatusKind kind = CodingSessionFoldedStatusKind.working,
}) => testUmbrella(
  key: key,
  sessionRef: null,
  name: key,
  closed: closed,
  founder: const CodingSessionFounder(
    pubkey: testViewer,
    resolution: CodingSessionFounderResolution.genesis,
    genesisRef: 'g',
  ),
  status: CodingSessionFoldedStatus(kind: kind),
);

CodingSessionUmbrella _theirs() => testUmbrella(
  key: 'umbrella-theirs',
  sessionRef: null,
  name: 'theirs',
  status: const CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.reported,
  ),
);

CodingSessionUmbrella _unattributed() => testUmbrella(
  key: 'umbrella-orphan',
  sessionRef: null,
  name: 'orphan',
  founder: CodingSessionFounder.unresolved,
  status: const CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.reported,
  ),
);

Future<FakeObserverBinding> _pump(
  WidgetTester tester, {
  List<Override> extra = const [],
  TerminalsIndex? terminals,
  FakeObserverBinding? observer,
  String? viewer = testViewer,
}) async {
  SharedPreferences.setMockInitialValues({});
  final binding = observer ?? _observer([_mine()]);
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
            viewerPubkey: viewer,
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

/// Sessions live in the transport channel only; every other channel reads
/// empty, as a real relay would answer.
FakeObserverBinding _observer(List<CodingSessionUmbrella> sessions) =>
    FakeObserverBinding(
      testSnapshot(sessions: []),
      byChannel: {'c-transport': testSnapshot(sessions: sessions)},
    );

Iterable<String> _rowKeys(WidgetTester tester) => tester
    .widgetList(
      find.byWidgetPredicate(
        (widget) =>
            widget.key is ValueKey<String> &&
            (widget.key as ValueKey<String>).value.startsWith('project-row-'),
      ),
    )
    .map((w) => (w.key as ValueKey<String>).value);

void main() {
  testWidgets('one flat list: the channel row, then my open session with '
      'who started it, then the terminal; the transport is not a row', (
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
      ]),
    );
    expect(_rowKeys(tester), [
      'project-row-channel:c-general',
      'project-row-session:c-transport:umbrella-mine',
      'project-row-remote-shell:${testTerminal().key}',
    ]);
    expect(find.text('meta c-transport'), findsNothing);
    expect(find.textContaining('started by you'), findsOneWidget);
    expect(find.textContaining('collaborator · you can type'), findsOneWidget);
    expect(find.text('My sessions'), findsOneWidget);
  });

  testWidgets('a session row opens the session; a channel row opens the '
      'channel', (tester) async {
    await _pump(tester);
    await tester.tap(
      find.byKey(
        const ValueKey('project-row-session:c-transport:umbrella-mine'),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byType(CodingSessionPage), findsOneWidget);
  });

  testWidgets('a bound channel not in my list opens its sessions list', (
    tester,
  ) async {
    await _pump(
      tester,
      observer: FakeObserverBinding(testSnapshot(sessions: [])),
      extra: [],
    );
    // Make the referenced channel a plain stream so it renders as a row.
    await tester.pumpWidget(
      KeyedSubtree(
        key: UniqueKey(),
        child: WidgetHelpers.testable(
          overrides: [
            ...projectOverrides(
              projects: testProjectsRead(
                [
                  testProject(channelIds: const ['c-ref']),
                ],
                referenced: {'c-ref': testChannelData('c-ref', type: 'stream')},
              ),
            ),
            fakeObserverOverride(
              FakeObserverBinding(testSnapshot(sessions: [])),
            ),
          ],
          child: const ProjectPage(address: testProjectAddress),
        ),
      ),
    );
    await tester.pump();
    expect(find.text('not in your list'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('project-row-channel:c-ref')));
    await tester.pumpAndSettle();
    expect(find.byType(CodingSessionsPage), findsOneWidget);
  });

  testWidgets('the default filter is My sessions: others\' sessions are '
      'left out, unattributed ones are counted, and All reveals them', (
    tester,
  ) async {
    await _pump(
      tester,
      observer: _observer([_mine(), _theirs(), _unattributed()]),
    );
    expect(find.text('umbrella-mine'), findsOneWidget);
    expect(find.text('theirs'), findsNothing);
    expect(
      find.byKey(const ValueKey('project-unattributed-$testProjectAddress')),
      findsOneWidget,
    );

    await tester.tap(
      find.byKey(const ValueKey('project-filter-$testProjectAddress')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('project-filter-all')));
    await tester.pumpAndSettle();
    expect(find.text('theirs'), findsOneWidget);
    expect(find.text('orphan'), findsOneWidget);
    expect(find.textContaining('initiator unknown'), findsOneWidget);
    expect(find.text('All sessions'), findsWidgets);
  });

  testWidgets('closed sessions sit last and dimmed, and the box hides them '
      'with a count', (tester) async {
    await _pump(
      tester,
      observer: _observer([
        _mine(key: 'done', closed: true),
        _mine(key: 'live'),
      ]),
    );
    expect(_rowKeys(tester).toList(), [
      'project-row-channel:c-general',
      'project-row-session:c-transport:live',
      'project-row-session:c-transport:done',
    ]);
    await tester.tap(
      find.byKey(const ValueKey('project-filter-$testProjectAddress')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('project-filter-show-closed')));
    await tester.pumpAndSettle();
    expect(find.text('done'), findsNothing);
    expect(
      find.byKey(const ValueKey('project-hidden-by-state-$testProjectAddress')),
      findsOneWidget,
    );
  });

  testWidgets('a project with nothing in it says so in one line', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [
          ...projectOverrides(projects: testProjectsRead([testProject()])),
          fakeObserverOverride(FakeObserverBinding(testSnapshot(sessions: []))),
        ],
        child: const ProjectPage(address: testProjectAddress),
      ),
    );
    await tester.pump();
    expect(find.text(projectEmptyLabel), findsOneWidget);
    expect(find.byType(TextButton), findsNothing);
  });

  testWidgets('a project not in the read says so instead of rendering blank', (
    tester,
  ) async {
    SharedPreferences.setMockInitialValues({});
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
