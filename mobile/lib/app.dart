import 'package:app_badge_plus/app_badge_plus.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';

import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart' show Override;

import 'features/activity/activity_provider.dart';
import 'features/activity/inbox_local_state_provider.dart';
import 'features/activity/inbox_read_state.dart';
import 'features/channels/unread_badge/unread_badge_provider.dart';
import 'features/home/home_page.dart';
import 'features/invites/invite_create_page.dart';
import 'features/pairing/pairing_page.dart';
import 'features/channels/agent_activity/observer_subscription.dart';
import 'features/channels/deep_link_dispatcher.dart';
import 'features/profile/user_status_cache_provider.dart';
import 'features/profile/settings_profile_header.dart';
import 'features/project_todos/state/project_todos_provider.dart';
import 'features/agents_repo/domain/artifact_pin_fold.dart';
import 'features/agents_repo/domain/artifact_pin_op.dart';
import 'features/agents_repo/state/agents_repo_source_provider.dart';
import 'features/agents_repo/state/artifact_pins_provider.dart';
import 'features/agents_repo/ui/agents_repo_page.dart';
import 'features/project_todos/ui/project_todos_page.dart';
import 'features/projects/ui/project_tree.dart';
import 'features/settings/settings_page.dart';
import 'shared/auth/auth.dart';
import 'shared/deeplink/pending_deep_link_provider.dart';
import 'shared/emoji/emoji_burst.dart';
import 'shared/relay/relay.dart';
import 'shared/read_state/read_state_provider.dart';
import 'shared/theme/theme.dart';
import 'shared/widgets/beekeeper_loading_indicator.dart';

/// App-shell projection that joins Activity state for the Home navigation.
///
/// This belongs at the composition root because it deliberately aggregates
/// Activity feature providers for a sibling navigation surface.
final _unreadInboxItemCountProvider = Provider<int>((ref) {
  final readState = ref.watch(readStateProvider);
  if (!readState.isReady) return 0;

  final localState = ref.watch(inboxLocalStateProvider);
  final items = ref.watch(inboxItemsProvider);
  return items
      .where(
        (item) => !isInboxItemDone(
          item,
          markerOf: readState.effectiveTimestamp,
          localUnreadOverrides: localState.unreadIds,
          localDoneSet: localState.doneIds,
        ),
      )
      .length;
});

/// Cross-feature wiring that belongs at the composition root: a project
/// tree's "To-do" row opens the `project_todos` feature. It lives here so
/// `projects/` never imports `project_todos/`; `main.dart` installs it on
/// the root [ProviderScope].
List<Override> appFeatureOverrides() => [
  projectTodoOpenerProvider.overrideWithValue(openProjectTodos),
  projectPinnedTodoListsProvider.overrideWithValue(readPinnedProjectTodoLists),
  projectAgentsRepoOpenerProvider.overrideWithValue(openProjectAgentsRepo),
  projectPinnedArtifactsProvider.overrideWithValue(readPinnedProjectArtifacts),
];

/// Push a project's Artifacts page: its agents repository, read from main and
/// drafted through the relay (NIP-AD), on [path] when one was asked for.
void openProjectAgentsRepo(
  BuildContext context,
  String address, {
  String? path,
}) => Navigator.of(context).push(
  MaterialPageRoute<void>(
    builder: (_) => AgentsRepoPage(address: address, initialPath: path),
  ),
);

/// Push a project's to-do page, on [listId] when one was asked for.
void openProjectTodos(BuildContext context, String address, {String? listId}) =>
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        builder: (_) =>
            ProjectTodosPage(address: address, initialListId: listId),
      ),
    );

/// A project's pinned, unarchived to-do lists as its tree shows them, read
/// live from the project's fold.
List<PinnedTodoListRow> readPinnedProjectTodoLists(
  WidgetRef ref,
  String address,
) => [
  for (final list in ref.watch(projectTodosProvider(address)).digest.lists)
    if (list.pinned && !list.archived)
      PinnedTodoListRow(
        id: list.id,
        title: list.title,
        personal: list.personal,
      ),
];

/// A project's pinned artifacts as its tree shows them, read live from the
/// project's pin fold — in the project's own rank order, which is what
/// whoever reordered the pins decided.
///
/// A project whose agents repository has not resolved yet contributes no
/// rows: a pin names a path *in a repository*, and folding against a guess
/// would show rows that belong to another one.
List<PinnedArtifactRow> readPinnedProjectArtifacts(
  WidgetRef ref,
  String address,
) {
  final repo = ref
      .watch(agentsRepoSourceProvider(address))
      .maybeWhen(data: (source) => source?.repo, orElse: () => null);
  if (repo == null) return const [];
  return [
    for (final pin
        in ref
            .watch(artifactPinsProvider((address: address, repo: repo)))
            .digest
            .pinnedOnly)
      PinnedArtifactRow(
        target: pin.target,
        label: artifactPinLabel(pin),
        isFolder: pin.targetKind == PinTargetKind.folder,
      ),
  ];
}

class App extends HookConsumerWidget {
  const App({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final communityTheme = ref.watch(communityThemeProvider);
    final themeMode = communityTheme.mode;
    final accentIndex = effectiveAccentIndex(
      communityTheme.theme,
      communityTheme.accent,
    );
    final schemeName = communityTheme.theme;
    final authState = ref.watch(authProvider);

    final resolved = resolveSchemes(schemeName, themeMode);
    final lightScheme = applyAccent(resolved.light, accentIndex);
    final darkScheme = applyAccent(resolved.dark, accentIndex);
    // Light/Dark modes pin the brightness; System leaves it null so Flutter
    // follows the OS across the selected theme and its pair.
    final effectiveMode = resolved.forcedMode ?? themeMode;

    // Derive the gradient from the themes that produced each color scheme.
    // This keeps fallbacks and pinned brightness changes aligned with the
    // rendered palette rather than the raw persisted selection.
    final beekeeperLightGradient = beekeeperTopSectionGradient(
      resolved.lightTheme?.name ?? '',
      lightScheme.brightness,
    );
    final beekeeperDarkGradient = beekeeperTopSectionGradient(
      resolved.darkTheme?.name ?? '',
      darkScheme.brightness,
    );

    // Eagerly initialize websocket session and lifecycle observer when
    // authenticated. These providers connect and manage the websocket.
    var hasUnreadInbox = false;
    if (authState.value?.status == AuthStatus.authenticated) {
      ref.watch(relaySessionProvider);
      ref.watch(observerRelayProvider);
      ref.watch(appLifecycleProvider);
      ref.watch(userStatusCacheProvider);
      hasUnreadInbox = ref.watch(_unreadInboxItemCountProvider) > 0;
    }

    // Start listening for beekeeper:// links immediately (even pre-auth) so a
    // cold-start link survives until the authenticated UI can dispatch it.
    ref.watch(pendingDeepLinkProvider);

    void applyBadge(UnreadBadgeState state) {
      if (state.highPriorityCount > 0) {
        AppBadgePlus.updateBadge(state.highPriorityCount);
      } else if (state.generalUnreadCount > 0) {
        AppBadgePlus.updateBadge(1);
      } else {
        AppBadgePlus.updateBadge(0);
      }
    }

    useEffect(() {
      applyBadge(ref.read(unreadBadgeProvider));
      return null;
    }, const []);
    ref.listen<UnreadBadgeState>(unreadBadgeProvider, (_, next) {
      applyBadge(next);
    });

    return MaterialApp(
      title: 'Beekeeper',
      theme: AppTheme.light(
        colorScheme: lightScheme,
        topSectionGradient: beekeeperLightGradient,
      ),
      darkTheme: AppTheme.dark(
        colorScheme: darkScheme,
        topSectionGradient: beekeeperDarkGradient,
      ),
      themeMode: effectiveMode,
      // Above the navigator, so a burst keeps playing over a pushed thread page
      // or a modal sheet — the same reason desktop pins its canvas to the
      // viewport rather than to the message row.
      builder: (context, child) =>
          EmojiBurstOverlay(child: child ?? const SizedBox.shrink()),
      home: authState.when(
        loading: () => const _SplashScreen(),
        error: (_, _) => const PairingPage(),
        data: (state) => switch (state.status) {
          AuthStatus.authenticated => DeepLinkDispatcher(
            child: HomePage(
              settingsPageBuilder: _buildSettingsPage,
              hasUnreadInbox: hasUnreadInbox,
            ),
          ),
          _ => const DeepLinkDispatcher(
            dispatchMessageLinks: false,
            child: PairingPage(),
          ),
        },
      ),
    );
  }
}

Widget _buildSettingsPage(BuildContext context) => SettingsPage(
  profileHeader: const SettingsProfileHeader(),
  invitePageBuilder: (_) => const CommunityInvitePage(),
  identityRecoveryPageBuilder: (_) =>
      const PairingPage(addingCommunity: true, identityRecoveryOnly: true),
);

class _SplashScreen extends StatelessWidget {
  const _SplashScreen();

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: Center(
        child: BeekeeperLoadingIndicator(
          size: 56,
          semanticLabel: 'Starting Beekeeper',
        ),
      ),
    );
  }
}
