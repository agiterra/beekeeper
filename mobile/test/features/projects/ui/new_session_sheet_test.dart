import 'dart:convert';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/state/coding_sessions_state.dart';
import 'package:beekeeper/features/home/home_page.dart';
import 'package:beekeeper/features/profile/user_profile.dart';
import 'package:beekeeper/features/projects/ui/new_session_sheet.dart';
import 'package:beekeeper/features/projects/ui/project_page.dart';
import 'package:beekeeper/features/projects/ui/project_tree.dart';
import 'package:beekeeper/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../../../helpers/widget_helpers.dart';
import '../../coding_sessions/ui/fake_observer.dart';
import 'fake_projects.dart';

const _transport = 'c-transport';
const _otherSigner =
    '2222222233333333444444445555555566666666777777778888888899999999';

CodingSessionProviderCatalog _catalog({
  String signer = testSignerPubkey,
  String instanceRef = 'claude-primary',
  List<String> models = const ['sonnet', 'haiku', 'opus'],
  int createdAt = 100,
}) => CodingSessionProviderCatalog(
  ref: CodingSessionEventRef(
    channelId: _transport,
    eventId: 'catalog-$signer-$instanceRef',
    signerPubkey: signer,
    createdAt: createdAt,
  ),
  revision: 1,
  providers: [
    CodingSessionProviderOffer(
      providerInstanceRef: instanceRef,
      driver: 'claude-agent-acp',
      runtime: 'claude',
      defaultModel: models.first,
      allowedModels: models,
      capabilities: const CodingSessionProviderCapabilities(
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: false,
        context: false,
        diff: false,
        plan: true,
        promptImage: false,
      ),
    ),
  ],
  projects: null,
);

Override _catalogs(
  List<CodingSessionProviderCatalog> catalogs, {
  int rejected = 0,
}) => codingSessionProviderCatalogsProvider.overrideWith(
  (ref, channelId) async => CodingSessionProviderCatalogs(
    channelId: channelId,
    catalogs: catalogs,
    rejected: rejected,
  ),
);

List<Override> _project({
  FakeObserverBinding? observer,
  List<Override> extra = const [],
  bool withTransport = true,
}) => [
  ...projectOverrides(
    projects: testProjectsRead(
      [
        testProject(channelIds: withTransport ? const [_transport] : const []),
      ],
      referenced: withTransport
          ? {_transport: testChannelData(_transport)}
          : const {},
    ),
    channels: [testChannel('c-general', projectRef: testProjectAddress)],
    users: {
      testSignerPubkey: const UserProfile(
        pubkey: testSignerPubkey,
        displayName: 'Andy\'s desktop',
      ),
    },
  ),
  fakeObserverOverride(
    observer ??
        FakeObserverBinding(
          testSnapshot(sessions: const []),
          signerPubkey: testViewer,
        ),
  ),
  ...extra,
];

Future<void> _pumpHome(WidgetTester tester, List<Override> overrides) async {
  SharedPreferences.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  await tester.pumpWidget(
    ProviderScope(
      overrides: [savedPrefsProvider.overrideWithValue(prefs), ...overrides],
      child: MaterialApp(
        theme: AppTheme.light(),
        home: HomePage(
          settingsPageBuilder: (_) => const SizedBox.shrink(),
          hasUnreadInbox: false,
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> _pumpProjectPage(
  WidgetTester tester,
  List<Override> overrides,
) async {
  SharedPreferences.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  await tester.pumpWidget(
    KeyedSubtree(
      key: UniqueKey(),
      child: WidgetHelpers.testable(
        overrides: [savedPrefsProvider.overrideWithValue(prefs), ...overrides],
        child: const ProjectPage(address: testProjectAddress),
      ),
    ),
  );
  await tester.pump();
  await tester.pump();
}

Map<String, dynamic> _action(String content) =>
    (jsonDecode(content) as Map<String, dynamic>)['action']
        as Map<String, dynamic>;

void main() {
  group('the project\'s "+" on Home', () {
    testWidgets('opens the sheet for the project\'s sessions channel', (
      tester,
    ) async {
      await _pumpHome(
        tester,
        _project(
          extra: [
            _catalogs([_catalog()]),
          ],
        ),
      );
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pumpAndSettle();
      expect(find.byType(NewCodingSessionSheet), findsOneWidget);
      expect(find.byKey(const ValueKey('new-session-start')), findsOneWidget);
      // The provider is named by signer, never by pubkey.
      expect(find.textContaining('Andy\'s desktop'), findsWidgets);
    });

    testWidgets('says why when the project has no sessions channel', (
      tester,
    ) async {
      await _pumpHome(tester, _project(withTransport: false));
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pump();
      expect(find.byType(NewCodingSessionSheet), findsNothing);
      expect(find.text(projectNoSessionsChannelLabel), findsOneWidget);
    });
  });

  group('NewCodingSessionSheet', () {
    testWidgets('Start publishes genesis, name and create for the chosen '
        'provider and model, then closes', (tester) async {
      final observer = FakeObserverBinding(
        testSnapshot(sessions: const []),
        signerPubkey: testViewer,
      );
      await _pumpHome(
        tester,
        _project(
          observer: observer,
          extra: [
            _catalogs([_catalog()]),
          ],
        ),
      );
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pumpAndSettle();

      await tester.enterText(
        find.byKey(const ValueKey('new-session-title')),
        'Fix the gate',
      );
      await tester.enterText(
        find.byKey(const ValueKey('new-session-prompt')),
        'Read the ledger first.',
      );
      await tester.tap(find.byKey(const ValueKey('new-session-model')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('haiku').last);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('new-session-start')));
      await tester.pumpAndSettle();

      expect(observer.relay.published.map((e) => e.kind), [
        44226,
        44229,
        44221,
      ]);
      final create = observer.relay.published.last;
      expect(create.tags[0], ['h', _transport]);
      final action = _action(create.content);
      expect(action['projectRef'], testProjectAddress);
      expect(action['providerInstanceRef'], 'claude-primary');
      expect(action['providerAuthorityPubkey'], testSignerPubkey);
      expect(action['model'], 'haiku');
      expect(action['title'], 'Fix the gate');
      expect(action['initialTurn'], 'Read the ledger first.');
      expect(action['genesisRef'], observer.relay.published.first.id);
      expect(find.byType(NewCodingSessionSheet), findsNothing);
      expect(find.textContaining('Asked Andy\'s desktop'), findsOneWidget);
    });

    testWidgets('prefers the signer already running a verified session '
        'here', (tester) async {
      final observer = FakeObserverBinding(
        testSnapshot(sessions: [testUmbrella()]),
        signerPubkey: testViewer,
      );
      await _pumpHome(
        tester,
        _project(
          observer: observer,
          extra: [
            _catalogs([
              _catalog(signer: _otherSigner, instanceRef: 'a-first'),
              _catalog(),
            ]),
          ],
        ),
      );
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('new-session-start')));
      await tester.pumpAndSettle();
      final action = _action(observer.relay.published.last.content);
      expect(action['providerAuthorityPubkey'], testSignerPubkey);
      expect(action['providerInstanceRef'], 'claude-primary');
      expect(action['model'], 'sonnet');
    });

    testWidgets('a relay refusal stays on the sheet, verbatim', (tester) async {
      final observer = FakeObserverBinding(
        testSnapshot(sessions: const []),
        signerPubkey: testViewer,
        publishResults: [Exception('restricted: not a member')],
      );
      await _pumpHome(
        tester,
        _project(
          observer: observer,
          extra: [
            _catalogs([_catalog()]),
          ],
        ),
      );
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('new-session-start')));
      await tester.pumpAndSettle();
      expect(find.byType(NewCodingSessionSheet), findsOneWidget);
      expect(find.byKey(const ValueKey('new-session-error')), findsOneWidget);
      expect(find.text('restricted: not a member'), findsOneWidget);
      expect(observer.relay.published.map((e) => e.kind), [44226]);
    });

    testWidgets('no catalog means no form, a reason, and a retry', (
      tester,
    ) async {
      await _pumpHome(
        tester,
        _project(extra: [_catalogs(const [], rejected: 2)]),
      );
      await tester.tap(
        find.byKey(const ValueKey('home-project-new-$testProjectAddress')),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(const ValueKey('new-session-start')), findsNothing);
      expect(find.byKey(const ValueKey('new-session-empty')), findsOneWidget);
      expect(find.textContaining('2 unreadable catalogs'), findsOneWidget);
      expect(find.byKey(const ValueKey('new-session-retry')), findsOneWidget);
    });
  });

  group('pending creates under the project', () {
    CodingSessionPendingCreate pending({bool published = true}) =>
        CodingSessionPendingCreate(
          channelId: _transport,
          commandId: 'csl-1',
          sessionRef: 'umbrella-new',
          genesisRef: 'ab' * 32,
          title: 'Fix the gate',
          providerAuthorityPubkey: testSignerPubkey,
          recordedAt: 5000,
          published: published,
        );

    testWidgets('a sent create shows as waiting, and clears once the '
        'provider\'s created receipt names it', (tester) async {
      final observer = FakeObserverBinding(
        testSnapshot(sessions: const []),
        byChannel: {_transport: testSnapshot(sessions: const [])},
      );
      await _pumpProjectPage(tester, _project(observer: observer));
      final container = ProviderScope.containerOf(
        tester.element(find.byType(ProjectTree)),
      );
      container.read(pendingCreatesProvider.notifier).record(pending());
      await tester.pump();

      expect(
        find.byKey(const ValueKey('project-pending-csl-1')),
        findsOneWidget,
      );
      expect(find.text('Fix the gate'), findsOneWidget);
      expect(
        find.textContaining('Waiting for Andy\'s desktop'),
        findsOneWidget,
      );
      expect(
        find.byKey(const ValueKey('project-empty-$testProjectAddress')),
        findsNothing,
      );

      observer.byChannel[_transport] = testSnapshot(
        sessions: const [],
        lifecycleReceiptsByCommandId: {
          'csl-1': [
            testLifecycleReceipt(
              commandId: 'csl-1',
              status: CodingSessionReceiptStatus.created,
            ),
          ],
        },
      );
      container
          .read(pendingCreatesProvider.notifier)
          .markPublished(pending().key);
      await tester.pump();
      await tester.pump();
      expect(find.byKey(const ValueKey('project-pending-csl-1')), findsNothing);
      expect(container.read(pendingCreatesProvider).byKey, isEmpty);
    });

    testWidgets('a refused create shows the provider\'s code and words until '
        'dismissed', (tester) async {
      final observer = FakeObserverBinding(
        testSnapshot(sessions: const []),
        byChannel: {
          _transport: testSnapshot(
            sessions: const [],
            lifecycleReceiptsByCommandId: {
              'csl-1': [
                testLifecycleReceipt(
                  commandId: 'csl-1',
                  status: CodingSessionReceiptStatus.failed,
                  code: 'PROJECT_CWD_UNRESOLVED',
                  message:
                      'no working directory is configured for project '
                      '$testProjectAddress',
                ),
              ],
            },
          ),
        },
      );
      await _pumpProjectPage(tester, _project(observer: observer));
      final container = ProviderScope.containerOf(
        tester.element(find.byType(ProjectTree)),
      );
      container.read(pendingCreatesProvider.notifier).record(pending());
      await tester.pump();

      expect(find.textContaining('PROJECT_CWD_UNRESOLVED'), findsOneWidget);
      expect(
        find.textContaining('no working directory is configured'),
        findsOneWidget,
      );
      // The provider names the hole; the row says whose it is to fill.
      expect(
        find.textContaining("desktop app's project settings"),
        findsOneWidget,
      );
      await tester.tap(
        find.byKey(const ValueKey('project-pending-dismiss-csl-1')),
      );
      await tester.pump();
      expect(find.byKey(const ValueKey('project-pending-csl-1')), findsNothing);
      expect(container.read(pendingCreatesProvider).byKey, isEmpty);
    });
  });
}
