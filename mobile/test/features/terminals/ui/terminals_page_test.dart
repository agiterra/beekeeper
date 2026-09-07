import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/terminals_index_provider.dart';
import 'package:buzz/features/terminals/ui/terminals_page.dart';
import 'package:buzz/shared/relay/relay_provider.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import '../../projects/ui/fake_projects.dart';

Future<void> _pump(
  WidgetTester tester,
  TerminalsIndex index, {
  String? viewer = testViewer,
  void Function(BuildContext, RemoteTerminal)? onOpen,
}) async {
  // A fresh subtree per pump: re-pumping the same ProviderScope keeps the
  // already-built notifier, and with it the previous state.
  await tester.pumpWidget(
    KeyedSubtree(
      key: UniqueKey(),
      child: WidgetHelpers.testable(
        overrides: [
          terminalsIndexProvider.overrideWith(
            () => FakeTerminalsIndexNotifier(index),
          ),
          myPubkeyProvider.overrideWithValue(viewer),
        ],
        child: onOpen == null
            ? const TerminalsPage.inert()
            : TerminalsPage(onOpen: onOpen),
      ),
    ),
  );
  await tester.pump();
}

void main() {
  testWidgets('groups terminals by project and states the viewer\'s access', (
    tester,
  ) async {
    await _pump(
      tester,
      testTerminalsIndex([
        testTerminal(),
        testTerminal(
          sessionId: 's2',
          title: 'logs',
          projectRef: '30621:$testOwner:other',
          roster: const [
            ShellRosterEntry(pubkey: testViewer, role: ShellRosterRole.viewer),
          ],
        ),
      ]),
    );
    expect(
      find.byKey(const ValueKey('terminals-project-$testProjectAddress')),
      findsOneWidget,
    );
    expect(find.text('build shell'), findsOneWidget);
    expect(find.text('logs'), findsOneWidget);
    expect(find.textContaining('member · observe only'), findsOneWidget);
    expect(find.textContaining('viewer · observe only'), findsOneWidget);
    expect(find.textContaining('24x80'), findsNWidgets(2));
  });

  testWidgets('the owner\'s own terminal reads as yours', (tester) async {
    await _pump(
      tester,
      testTerminalsIndex([testTerminal()]),
      viewer: testOwner,
    );
    expect(find.textContaining('yours · you can type'), findsOneWidget);
  });

  testWidgets('rows are inert without an opener and tappable with one', (
    tester,
  ) async {
    RemoteTerminal? opened;
    await _pump(tester, testTerminalsIndex([testTerminal()]));
    final inert = tester.widget<ListTile>(
      find.byKey(ValueKey('terminal-row-${testTerminal().key}')),
    );
    expect(inert.onTap, isNull);

    await _pump(
      tester,
      testTerminalsIndex([testTerminal()]),
      onOpen: (_, terminal) => opened = terminal,
    );
    await tester.tap(
      find.byKey(ValueKey('terminal-row-${testTerminal().key}')),
    );
    expect(opened?.sessionId, 's1');
  });

  testWidgets('loading, error, disconnected and empty are four different '
      'statements', (tester) async {
    await _pump(
      tester,
      testTerminalsIndex(
        const [],
        connection: TerminalsConnection.connecting,
        hasRead: false,
      ),
    );
    expect(find.byKey(const ValueKey('terminals-loading')), findsOneWidget);

    await _pump(
      tester,
      testTerminalsIndex(
        const [],
        connection: TerminalsConnection.error,
        hasRead: false,
        lastError: 'nope',
      ),
    );
    expect(find.byKey(const ValueKey('terminals-error')), findsOneWidget);
    expect(find.text('nope'), findsOneWidget);

    await _pump(
      tester,
      testTerminalsIndex(
        const [],
        connection: TerminalsConnection.idle,
        hasRead: false,
      ),
    );
    expect(
      find.byKey(const ValueKey('terminals-disconnected')),
      findsOneWidget,
    );

    await _pump(tester, testTerminalsIndex(const []));
    expect(find.text(terminalsEmptyLabel), findsOneWidget);
  });
}
