import 'package:buzz/features/agents_repo/data/agents_repo_http_client.dart';
import 'package:buzz/features/agents_repo/domain/agents_repo_draft_fold.dart';
import 'package:buzz/features/agents_repo/domain/agents_repo_draft_op.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_actions.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_drafts_provider.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_main_provider.dart';
import 'package:buzz/features/agents_repo/state/agents_repo_source_provider.dart';
import 'package:buzz/features/agents_repo/ui/agents_repo_page.dart';
import 'package:buzz/features/profile/user_cache_provider.dart';
import 'package:buzz/features/profile/user_profile.dart';
import 'package:buzz/features/projects/state/projects_provider.dart';
import 'package:buzz/shared/relay/relay_provider.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';

import '../../projects/ui/fake_projects.dart';
import '../fake_agents_repo.dart';

/// The Files page over a fixed listing and a fixed fold: what it shows,
/// which op each control asks for, and that nothing here commits.
void main() {
  late FakeAgentsRepoActions actions;

  Future<void> pump(
    WidgetTester tester, {
    required AgentsRepoDraftsRead drafts,
    AgentsRepoSource? source,
    bool noSource = false,
    bool sourceKnown = true,
    Map<String, UserProfile> users = const {},
    String? initialPath,
    String me = repoViewer,
  }) async {
    actions = FakeAgentsRepoActions();
    final key = (address: repoAddress, repo: repoCoordinate);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          agentsRepoSourceProvider.overrideWith(
            (ref, address) async => !sourceKnown
                ? throw Exception('no relay')
                : noSource
                ? null
                : (source ?? testSource()),
          ),
          agentsRepoListingProvider.overrideWith(
            () => FakeAgentsRepoListingNotifier(repoAddress, testListing()),
          ),
          agentsRepoFileProvider.overrideWith(
            (ref, fileKey) async => AgentsRepoFile(
              path: fileKey.path,
              text: fileKey.path == 'plans/roadmap.md' ? roadmapText : 'x\n',
              state: 'on-main',
              blob: fileKey.path == 'plans/roadmap.md' ? roadmapBlob : 'b',
              commit: repoTip,
              fetchedAt: DateTime.now(),
            ),
          ),
          agentsRepoDraftsProvider.overrideWith(
            () => FakeAgentsRepoDraftsNotifier(key, drafts),
          ),
          agentsRepoActionsProvider.overrideWith((ref, k) => actions),
          projectsProvider.overrideWith(
            () => FakeProjectsNotifier(testProjectsRead([testProject()])),
          ),
          userCacheProvider.overrideWith(() => FakeUserCacheNotifier(users)),
          myPubkeyProvider.overrideWithValue(me),
        ],
        child: MaterialApp(
          theme: AppTheme.light(),
          home: AgentsRepoPage(address: repoAddress, initialPath: initialPath),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('lists main grouped with plans first, chips the open draft, '
      'says which tip, and shows no Commit control', (tester) async {
    await pump(
      tester,
      drafts: testDraftsRead(
        paths: [
          DraftPath(
            path: 'plans/roadmap.md',
            head: testDraftRow(author: repoOther),
            superseded: const [],
            diverged: false,
            updatedAt: 100,
          ),
          DraftPath(
            path: 'plans/new-one.md',
            head: testDraftRow(id: 'n', path: 'plans/new-one.md', base: null),
            superseded: const [],
            diverged: false,
            updatedAt: 100,
          ),
        ],
      ),
      users: const {
        repoOther: UserProfile(pubkey: repoOther, displayName: 'Alice'),
      },
    );
    expect(find.text('Beekeeper · Files'), findsOneWidget);
    expect(find.byKey(const ValueKey('agents-repo-tip')), findsOneWidget);
    expect(find.textContaining('main at 5c2bf839'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('agents-repo-file-plans/roadmap.md')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('agents-repo-file-plans/new-one.md')),
      findsOneWidget,
    );
    expect(
      find.byKey(const ValueKey('agents-repo-file-plans/.gitkeep')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey('agents-repo-draft-chip-plans/roadmap.md')),
      findsOneWidget,
    );
    expect(find.textContaining('Alice'), findsWidgets);

    double top(String key) => tester.getTopLeft(find.byKey(ValueKey(key))).dy;
    expect(
      top('agents-repo-file-plans/roadmap.md') <
          top('agents-repo-file-roles/lead.md'),
      isTrue,
    );
    expect(
      top('agents-repo-file-roles/lead.md') < top('agents-repo-file-team.yml'),
      isTrue,
    );

    expect(find.text(agentsRepoMobileCommitNote), findsOneWidget);
    expect(find.text('Commit'), findsNothing);
    expect(find.textContaining('Commit…'), findsNothing);
  });

  testWidgets(
    'a project with no source says so; a pack-layout source is named',
    (tester) async {
      await pump(tester, drafts: testDraftsRead(), noSource: true);
      expect(
        find.byKey(const ValueKey('agents-repo-notice-no-source')),
        findsOneWidget,
      );
    },
  );

  testWidgets(
    'the file page shows main, toggles to the draft, discloses a '
    'changed base, and an edit publishes a put with base, baseCommit and prev',
    (tester) async {
      final head = testDraftRow(author: repoOther, base: '0' * 40, prev: null);
      await pump(
        tester,
        drafts: testDraftsRead(
          paths: [
            DraftPath(
              path: 'plans/roadmap.md',
              head: head,
              superseded: const [],
              diverged: false,
              updatedAt: 100,
            ),
          ],
        ),
        users: const {
          repoOther: UserProfile(pubkey: repoOther, displayName: 'Alice'),
        },
        initialPath: 'plans/roadmap.md',
      );
      await tester.pumpAndSettle();
      expect(
        find.byKey(const ValueKey('agents-repo-file-path')),
        findsOneWidget,
      );
      // The draft is shown first when one exists.
      expect(find.textContaining('something new'), findsOneWidget);
      expect(find.textContaining('Draft by Alice'), findsOneWidget);
      expect(
        find.textContaining('main changed this file since'),
        findsOneWidget,
      );
      expect(find.text(agentsRepoMobileCommitNote), findsWidgets);
      expect(find.text('Commit'), findsNothing);

      await tester.tap(find.text('Main'));
      await tester.pumpAndSettle();
      expect(find.textContaining('Overworld first.'), findsOneWidget);

      await tester.tap(find.byKey(const ValueKey('agents-repo-file-menu')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('agents-repo-menu-edit')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byKey(const ValueKey('agents-repo-edit-field')),
        '# Roadmap\n\nv2\n',
      );
      await tester.enterText(
        find.byKey(const ValueKey('agents-repo-edit-note')),
        'why',
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey('agents-repo-edit-save')));
      await tester.pumpAndSettle();
      expect(actions.calls, [
        'saveDraft plans/roadmap.md base=$roadmapBlob openedOn=${head.id} message=why text=# Roadmap\n\nv2\n',
      ]);
    },
  );

  testWidgets('a refusal from the actions is shown as it came, and the '
      'withdraw door appears only on my own draft', (tester) async {
    final mine = testDraftRow(author: repoViewer);
    await pump(
      tester,
      drafts: testDraftsRead(
        paths: [
          DraftPath(
            path: 'plans/roadmap.md',
            head: mine,
            superseded: const [],
            diverged: false,
            updatedAt: 100,
          ),
        ],
      ),
      initialPath: 'plans/roadmap.md',
    );
    await tester.pumpAndSettle();
    actions.failure = const AgentsRepoHeadConflict(
      'Alice saved a newer draft; reload to see it.',
    );
    await tester.tap(find.byKey(const ValueKey('agents-repo-file-menu')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(const ValueKey('agents-repo-menu-withdraw')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const ValueKey('agents-repo-menu-edit')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('agents-repo-edit-field')),
      'mine\n',
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('agents-repo-edit-save')));
    await tester.pumpAndSettle();
    expect(
      find.text('Alice saved a newer draft; reload to see it.'),
      findsOneWidget,
    );
    expect(actions.calls.single, startsWith('saveDraft plans/roadmap.md'));
  });

  testWidgets(
    'the op the actions build carries the head as prev and the tags name the path',
    (tester) async {
      final op = AgentsRepoDraftOp.filePut(
        repo: repoCoordinate,
        path: 'plans/roadmap.md',
        text: 'v2\n',
        base: roadmapBlob,
        baseCommit: repoTip,
        prev: 'a'.padRight(64, '0'),
      );
      expect(
        op.tags(repoAddress),
        containsAll([
          ['ad-repo', repoCoordinate],
          ['ad-path', 'plans/roadmap.md'],
        ]),
      );
      expect(
        decodeAgentsRepoDraftOp(op.toContent(), repoCoordinate)?.prev,
        'a'.padRight(64, '0'),
      );
    },
  );
}
