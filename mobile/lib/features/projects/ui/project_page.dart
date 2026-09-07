import 'package:flutter/material.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../channels/channel.dart';
import '../../channels/channels_provider.dart';
import '../../terminals/state/terminals_index_provider.dart';
import '../domain/project_models.dart';
import '../state/projects_provider.dart';
import 'project_tree.dart';

export 'project_tree.dart'
    show ProjectTerminalOpener, projectTerminalOpenerProvider;

/// One project on its own page: the same tree the Home screen shows under
/// the project's header, with a description line above it.
///
/// Reached from a project header's title; the Home list is the primary door.
class ProjectPage extends ConsumerWidget {
  /// The `30621:<owner>:<d>` address.
  final String address;

  const ProjectPage({super.key, required this.address});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final read = ref.watch(projectsProvider);
    final project = read.byAddress(address);
    final channelsAsync = ref.watch(channelsProvider);

    Future<void> refresh() => Future.wait([
      ref.read(projectsProvider.notifier).refresh(),
      ref.read(terminalsIndexProvider.notifier).refresh(),
    ]);

    if (project == null) {
      return FrostedScaffold(
        backgroundColor: context.colors.surface,
        appBar: const FrostedAppBar(title: Text('Project')),
        body: Column(
          children: [
            SizedBox(height: frostedAppBarHeight(context)),
            Expanded(
              child: ProjectsMessage(
                key: const ValueKey('project-missing'),
                icon: LucideIcons.triangleAlert,
                title: read.hasRead
                    ? 'This project is not in the current read'
                    : 'Projects have not been read yet',
                detail:
                    read.lastError ??
                    'It may have been deleted, or the relay withholds it.',
                onRetry: refresh,
              ),
            ),
          ],
        ),
      );
    }

    final myChannels = channelsAsync.asData?.value ?? const <Channel>[];
    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(project.name, overflow: TextOverflow.ellipsis),
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: refresh,
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  Grid.xs,
                  Grid.xxs,
                  Grid.xs,
                  Grid.xl,
                ),
                children: [
                  _ProjectHeader(project: project),
                  ProjectTree(project: project, myChannels: myChannels),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ProjectHeader extends StatelessWidget {
  final Project project;

  const _ProjectHeader({required this.project});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final members = project.memberPubkeys.length + 1;
    return Container(
      key: const ValueKey('project-header'),
      margin: const EdgeInsets.only(bottom: Grid.twelve),
      padding: const EdgeInsets.all(Grid.twelve),
      decoration: BoxDecoration(
        color: colors.surfaceContainerLow,
        borderRadius: BorderRadius.circular(Radii.card),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (project.description.trim().isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.half),
              child: Text(
                project.description.trim(),
                style: context.textTheme.bodyMedium,
              ),
            ),
          Text(
            '${project.isPrivate ? 'Private' : 'Public'} · '
            '$members ${members == 1 ? 'member' : 'members'} · '
            'owner ${shortPubkey(project.owner)}',
            style: context.textTheme.bodySmall?.copyWith(
              color: colors.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
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
