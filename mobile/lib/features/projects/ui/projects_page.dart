import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/widgets/app_list.dart';
import '../../../shared/widgets/app_list_card.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/buzz_loading_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../terminals/state/terminals_index_provider.dart';
import '../domain/project_models.dart';
import '../state/projects_provider.dart';
import 'project_page.dart';

/// The line shown when the relay answered and there are no projects.
const projectsEmptyLabel = 'No projects on this community yet';

/// The Projects tab: every project this reader may see.
///
/// Each row opens the project's page — its channels, the coding sessions
/// under them, and the terminals shared under the project. Every line here
/// comes from a signed head the relay returned; a read that has not come
/// back shows as loading and a failed one as an error, never as "no
/// projects".
class ProjectsPage extends HookConsumerWidget {
  /// Bumped when the tab is re-selected; the list scrolls to the top.
  final ValueListenable<int>? tabReselection;

  const ProjectsPage({super.key, this.tabReselection});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectsProvider);
    final terminals = ref.watch(terminalsIndexProvider);
    final scrollController = useScrollController();

    useEffect(() {
      final reselection = tabReselection;
      if (reselection == null) return null;
      void onReselect() {
        if (!scrollController.hasClients) return;
        scrollController.animateTo(
          0,
          duration: const Duration(milliseconds: 240),
          curve: Curves.easeOutCubic,
        );
      }

      reselection.addListener(onReselect);
      return () => reselection.removeListener(onReselect);
    }, [tabReselection, scrollController]);

    Future<void> refresh() => Future.wait([
      ref.read(projectsProvider.notifier).refresh(),
      ref.read(terminalsIndexProvider.notifier).refresh(),
    ]);

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: const FrostedAppBar(title: Text('Projects')),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: refresh,
              child: _ProjectsBody(
                read: read,
                terminals: terminals,
                controller: scrollController,
                onRetry: refresh,
                onOpen: (project) => Navigator.of(context).push(
                  MaterialPageRoute<void>(
                    builder: (_) => ProjectPage(address: project.address),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ProjectsBody extends StatelessWidget {
  final ProjectsRead read;
  final TerminalsIndex terminals;
  final ScrollController controller;
  final Future<void> Function() onRetry;
  final void Function(Project project) onOpen;

  const _ProjectsBody({
    required this.read,
    required this.terminals,
    required this.controller,
    required this.onRetry,
    required this.onOpen,
  });

  @override
  Widget build(BuildContext context) {
    if (!read.hasRead) {
      return switch (read.connection) {
        ProjectsConnection.connecting => ListView(
          key: const ValueKey('projects-loading'),
          controller: controller,
          padding: const EdgeInsets.only(top: Grid.xxl),
          children: const [
            Center(
              child: BuzzLoadingIndicator(
                size: 40,
                semanticLabel: 'Reading projects',
              ),
            ),
          ],
        ),
        ProjectsConnection.error => ProjectsMessage(
          key: const ValueKey('projects-error'),
          icon: LucideIcons.triangleAlert,
          title: 'Projects could not be read',
          detail: read.lastError ?? 'The relay read failed.',
          onRetry: onRetry,
        ),
        ProjectsConnection.idle || ProjectsConnection.open => ProjectsMessage(
          key: const ValueKey('projects-disconnected'),
          icon: LucideIcons.plugZap,
          title: 'Not connected to this community',
          detail: 'Nothing is being read while the connection is down.',
          onRetry: onRetry,
        ),
      };
    }
    return ListView(
      controller: controller,
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xxs, Grid.xs, Grid.xl),
      children: [
        if (read.connection == ProjectsConnection.error)
          Padding(
            key: const ValueKey('projects-stale'),
            padding: const EdgeInsets.only(bottom: Grid.xxs),
            child: Text(
              'This list may be out of date: '
              '${read.lastError ?? 'the relay read failed'}',
              style: context.textTheme.bodySmall?.copyWith(
                color: context.colors.error,
              ),
            ),
          ),
        if (read.projects.isEmpty)
          const Padding(
            key: ValueKey('projects-empty'),
            padding: EdgeInsets.all(Grid.gutter),
            child: Center(child: Text(projectsEmptyLabel)),
          )
        else
          AppListCard(
            children: [
              for (final project in read.projects)
                AppListRow(
                  key: ValueKey('project-row-${project.address}'),
                  icon: project.isPrivate
                      ? LucideIcons.folderLock
                      : LucideIcons.folderCode,
                  title: project.name,
                  subtitle: _subtitle(project, terminals),
                  onTap: () => onOpen(project),
                ),
            ],
          ),
      ],
    );
  }

  static String _subtitle(Project project, TerminalsIndex terminals) {
    final shared = terminals.forProject(project.address).length;
    final parts = <String>[
      project.isPrivate ? 'private' : 'public',
      if (project.channelIds.isNotEmpty)
        '${project.channelIds.length} '
            '${project.channelIds.length == 1 ? 'channel' : 'channels'}',
      if (shared > 0) '$shared ${shared == 1 ? 'terminal' : 'terminals'}',
    ];
    return parts.join(' · ');
  }
}

/// A full-height message with a Retry, kept scrollable for pull-to-refresh.
class ProjectsMessage extends StatelessWidget {
  final IconData icon;
  final String title;
  final String detail;
  final Future<void> Function() onRetry;

  const ProjectsMessage({
    super.key,
    required this.icon,
    required this.title,
    required this.detail,
    required this.onRetry,
  });

  @override
  Widget build(BuildContext context) => ListView(
    padding: const EdgeInsets.fromLTRB(Grid.gutter, Grid.xxl, Grid.gutter, 0),
    children: [
      Icon(icon, size: 32, color: context.colors.onSurfaceVariant),
      const SizedBox(height: Grid.twelve),
      Text(
        title,
        textAlign: TextAlign.center,
        style: context.textTheme.titleSmall,
      ),
      const SizedBox(height: Grid.half),
      Text(
        detail,
        textAlign: TextAlign.center,
        style: context.textTheme.bodySmall?.copyWith(
          color: context.colors.onSurfaceVariant,
        ),
      ),
      const SizedBox(height: Grid.xs),
      Center(
        child: TextButton.icon(
          key: const ValueKey('projects-retry'),
          onPressed: onRetry,
          icon: const Icon(LucideIcons.refreshCw, size: 16),
          label: const Text('Retry'),
        ),
      ),
    ],
  );
}
