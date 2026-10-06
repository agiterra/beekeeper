import 'dart:io';

import 'package:beekeeper/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_session_page.dart';
import 'package:beekeeper/features/coding_sessions/ui/coding_sessions_page.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../../helpers/widget_helpers.dart';
import 'fake_observer.dart';

/// SV-31 on mobile: a session nobody named reads the title its own provider
/// generated, marked "Auto-named" with the provider and model one tap away; a
/// person's name carries no label; and a title signed by anyone who is not a
/// provider of this umbrella is ignored, so the session keeps its fallback.
void main() {
  setUpAll(_loadFonts);

  const generatedText = 'Login redirect fix';
  const detail =
      'Named automatically from the first message by claude-code '
      '(aaaaaaaa…) · claude-haiku-4-5';
  final header = find.byKey(const ValueKey('coding-session-header'));
  final label = find.byKey(const ValueKey('coding-session-title-origin'));
  final labelDetail = find.byKey(
    const ValueKey('coding-session-title-origin-detail'),
  );

  CodingSessionGeneratedTitle title({String signer = testSignerPubkey}) =>
      CodingSessionGeneratedTitle(
        ref: testRef(eventId: 'title-1', signerPubkey: signer, createdAt: 1100),
        sessionRef: 'umbrella-1',
        targetKey: testTarget().key,
        title: generatedText,
        model: 'claude-haiku-4-5',
        sourceCommand: null,
        createEventId: 'ca' * 32,
      );

  // Older than the title on purpose: tiers are ranked, never timed.
  final rename = CodingSessionName(
    ref: testRef(eventId: 'name-1', createdAt: 900),
    sessionRef: 'umbrella-1',
    content: 'Auth rework',
  );

  /// An umbrella resolved by the real shared rule, as the fold builds it.
  CodingSessionUmbrella umbrella({
    List<CodingSessionName> names = const [],
    List<CodingSessionGeneratedTitle> titles = const [],
  }) {
    final executions = [testExecution()];
    final resolved = resolveCodingSessionUmbrellaName(
      channelId: testChannelId,
      sessionRef: 'umbrella-1',
      founderPubkey: testSignerPubkey,
      executions: executions,
      names: names,
      titles: titles,
    );
    final base = testUmbrella(executions: executions, name: null);
    return CodingSessionUmbrella(
      channelId: base.channelId,
      key: base.key,
      sessionRef: base.sessionRef,
      executions: base.executions,
      founder: base.founder,
      name: resolved.origin == CodingSessionNameOrigin.person
          ? resolved.name
          : null,
      nameResolution: resolved,
      goal: null,
      closed: false,
      status: base.status,
      lastActivityAt: base.lastActivityAt,
    );
  }

  Future<void> pumpPage(
    WidgetTester tester,
    CodingSessionUmbrella session,
  ) async {
    tester.view.physicalSize = const Size(390 * 3, 520 * 3);
    tester.view.devicePixelRatio = 3;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      RepaintBoundary(
        key: const ValueKey('sv31-capture'),
        child: WidgetHelpers.testable(
          overrides: [
            fakeObserverOverride(
              FakeObserverBinding(testSnapshot(sessions: [session])),
            ),
          ],
          child: const CodingSessionPage(
            channelId: testChannelId,
            sessionKey: 'umbrella-1',
          ),
        ),
      ),
    );
    await tester.pump();
  }

  testWidgets('a generated title reads with an Auto-named label whose '
      'detail names the provider and model, one tap away', (tester) async {
    await pumpPage(tester, umbrella(titles: [title()]));

    expect(
      find.descendant(of: header, matching: find.text(generatedText)),
      findsOneWidget,
    );
    expect(find.descendant(of: header, matching: label), findsOneWidget);
    expect(find.text('Auto-named'), findsOneWidget);
    expect(labelDetail, findsNothing);

    await tester.tap(label);
    await tester.pump();
    expect(find.text(detail), findsOneWidget);

    await _golden(tester, 'sv31-auto-named.png');

    await tester.tap(label);
    await tester.pump();
    expect(labelDetail, findsNothing);
  });

  testWidgets('a person\'s rename wins over the title and carries no label', (
    tester,
  ) async {
    await pumpPage(tester, umbrella(names: [rename], titles: [title()]));

    expect(
      find.descendant(of: header, matching: find.text('Auth rework')),
      findsOneWidget,
    );
    expect(find.text(generatedText), findsNothing);
    expect(label, findsNothing);
    expect(find.text('Auto-named'), findsNothing);

    await _golden(tester, 'sv31-renamed.png');
  });

  testWidgets('a title from a signer that is no provider of this session is '
      'ignored: the fallback stands, unlabelled', (tester) async {
    final session = umbrella(titles: [title(signer: testOperatorPubkey)]);
    expect(session.resolvedName.origin, CodingSessionNameOrigin.fallback);
    expect(session.resolvedName.diagnostics.foreignTitles, 1);

    await pumpPage(tester, session);

    expect(
      find.descendant(
        of: header,
        matching: find.text(codingSessionUntitledName),
      ),
      findsOneWidget,
    );
    expect(find.text(generatedText), findsNothing);
    expect(label, findsNothing);

    await _golden(tester, 'sv31-foreign-signer-fallback.png');
  });

  testWidgets('the session card labels a generated title, and tapping the '
      'label opens its detail rather than the session', (tester) async {
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [
          fakeObserverOverride(
            FakeObserverBinding(
              testSnapshot(
                sessions: [
                  umbrella(titles: [title()]),
                ],
              ),
            ),
          ),
        ],
        child: const CodingSessionsPage(
          channelId: testChannelId,
          channelName: 'general',
        ),
      ),
    );
    await tester.pump();

    final cardLabel = find.byKey(
      const ValueKey('coding-session-card-title-origin'),
    );
    expect(find.text(generatedText), findsOneWidget);
    expect(cardLabel, findsOneWidget);

    await tester.tap(cardLabel);
    await tester.pump();
    expect(find.text(detail), findsOneWidget);
    expect(find.byType(CodingSessionPage), findsNothing);
  });

  testWidgets('the session card shows a person\'s name with no label', (
    tester,
  ) async {
    await tester.pumpWidget(
      WidgetHelpers.testable(
        overrides: [
          fakeObserverOverride(
            FakeObserverBinding(
              testSnapshot(
                sessions: [
                  umbrella(names: [rename], titles: [title()]),
                ],
              ),
            ),
          ),
        ],
        child: const CodingSessionsPage(
          channelId: testChannelId,
          channelName: 'general',
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Auth rework'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('coding-session-card-title-origin')),
      findsNothing,
    );
  });
}

/// Golden captures are compared on macOS only: font rasterisation differs
/// across hosts, and these exist as screenshots, not as the assertion — the
/// finders above are the assertion and run everywhere.
Future<void> _golden(WidgetTester tester, String name) async {
  if (!Platform.isMacOS) return;
  await expectLater(
    find.byKey(const ValueKey('sv31-capture')),
    matchesGoldenFile('goldens/$name'),
  );
}

/// Real glyphs in the goldens: the app's Inter and the Lucide icon font,
/// instead of the test harness's box font.
Future<void> _loadFonts() async {
  final inter = FontLoader('Inter')
    ..addFont(rootBundle.load('assets/fonts/InterVariable.ttf'));
  final lucide = FontLoader('packages/lucide_icons_flutter/Lucide')
    ..addFont(
      rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
    );
  await Future.wait([inter.load(), lucide.load()]);
}
