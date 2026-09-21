import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../../../shared/widgets/modal_presentation.dart';
import '../../profile/user_cache_provider.dart';
import '../../projects/state/projects_provider.dart';
import '../data/agents_repo_http_client.dart';
import '../domain/agents_repo_draft_fold.dart';
import '../domain/agents_repo_draft_op.dart';
import '../domain/agents_repo_paths.dart';
import '../state/agents_repo_actions.dart';
import '../state/agents_repo_drafts_provider.dart';
import '../state/agents_repo_main_provider.dart';
import '../state/agents_repo_source_provider.dart';

part 'agents_repo_page/edit_sheet.dart';
part 'agents_repo_page/file_view.dart';
part 'agents_repo_page/rows.dart';

/// The line every reader sees: mobile has no git, so nothing here commits.
const agentsRepoMobileCommitNote =
    'Drafts become real when someone commits them from a desktop.';

/// A project's agents repository (spec § 4.12): `main`'s files grouped
/// with plans first, each with its open draft chip; a file read as `main`
/// has it or as its draft has it; and an edit sheet that publishes a
/// draft (NIP-AD, kind 44250). There is no Commit here — mobile has no git
/// — and the page says so rather than showing a control that cannot act.
class AgentsRepoPage extends HookConsumerWidget {
  /// The canonical `30621:<owner>:<dtag>` coordinate.
  final String address;

  /// The file to open first, or `null` for the list.
  final String? initialPath;

  const AgentsRepoPage({super.key, required this.address, this.initialPath});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final project = ref.watch(projectsProvider).byAddress(address);
    final source = ref.watch(agentsRepoSourceProvider(address));
    final colors = context.colors;
    final repo = source.value?.repo;
    final drafts = repo == null
        ? null
        : ref.watch(agentsRepoDraftsProvider((address: address, repo: repo)));
    final listing = repo == null
        ? null
        : ref.watch(agentsRepoListingProvider(address));

    useEffect(() {
      final path = initialPath;
      if (path == null || repo == null) return null;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (context.mounted) _openFile(context, address, repo, path);
      });
      return null;
    }, [repo]);

    return FrostedScaffold(
      backgroundColor: colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          '${project?.name ?? 'Project'} · Files',
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: () async {
                ref.invalidate(agentsRepoSourceProvider(address));
                if (repo != null) {
                  await ref
                      .read(agentsRepoListingProvider(address).notifier)
                      .refresh();
                  await ref
                      .read(
                        agentsRepoDraftsProvider((
                          address: address,
                          repo: repo,
                        )).notifier,
                      )
                      .refresh();
                }
              },
              child: ListView(
                padding: const EdgeInsets.fromLTRB(
                  Grid.xs,
                  Grid.xxs,
                  Grid.xs,
                  Grid.xl,
                ),
                children: [
                  const _Notice(
                    key: ValueKey('agents-repo-mobile-note'),
                    icon: LucideIcons.info,
                    text: agentsRepoMobileCommitNote,
                  ),
                  ...switch (source) {
                    AsyncError(:final error) => [
                      _Notice(
                        key: const ValueKey('agents-repo-notice-source-error'),
                        icon: LucideIcons.triangleAlert,
                        text:
                            'Could not read the project\'s source: '
                            '${agentsRepoErrorMessage(error)}',
                        emphasise: true,
                      ),
                    ],
                    AsyncData(value: null) => const [
                      _Notice(
                        key: ValueKey('agents-repo-notice-no-source'),
                        icon: LucideIcons.info,
                        text:
                            'This project has no agents repository yet. '
                            'Finish repository setup from a desktop.',
                      ),
                    ],
                    AsyncData(value: final value?) when !value.isAgentsRepo => [
                      _Notice(
                        key: const ValueKey('agents-repo-notice-not-agents'),
                        icon: LucideIcons.info,
                        text:
                            'This project\'s source is a pack-layout '
                            'repository (${value.repo}); the Files page '
                            'reads an agents repository.',
                      ),
                    ],
                    AsyncData() => [
                      if (drafts?.error != null)
                        _Notice(
                          key: const ValueKey(
                            'agents-repo-notice-drafts-error',
                          ),
                          icon: LucideIcons.triangleAlert,
                          text: drafts!.error!,
                          emphasise: true,
                        ),
                      if (drafts?.truncated ?? false)
                        const _Notice(
                          key: ValueKey('agents-repo-notice-truncated'),
                          icon: LucideIcons.triangleAlert,
                          text:
                              'The draft log was too long to read fully; '
                              'older drafts may be missing.',
                          emphasise: true,
                        ),
                      if ((drafts?.digest.ignored ?? 0) > 0)
                        _Notice(
                          key: const ValueKey('agents-repo-notice-ignored'),
                          icon: LucideIcons.info,
                          text:
                              '${drafts!.digest.ignored} draft '
                              '${drafts.digest.ignored == 1 ? 'op' : 'ops'} '
                              'could not be read and '
                              '${drafts.digest.ignored == 1 ? 'is' : 'are'} '
                              'not shown.',
                        ),
                      if ((drafts?.digest.otherRepo ?? 0) > 0)
                        _Notice(
                          key: const ValueKey('agents-repo-notice-other-repo'),
                          icon: LucideIcons.info,
                          text:
                              '${drafts!.digest.otherRepo} '
                              '${drafts.digest.otherRepo == 1 ? 'draft belongs' : 'drafts belong'} '
                              'to a repository this project no longer pins; '
                              'kept, not shown.',
                        ),
                      ...switch (listing) {
                        null || AsyncLoading() => const [
                          Padding(
                            padding: EdgeInsets.all(Grid.sm),
                            child: Center(child: CircularProgressIndicator()),
                          ),
                        ],
                        AsyncError(:final error) => [
                          _Notice(
                            key: const ValueKey(
                              'agents-repo-notice-main-error',
                            ),
                            icon: LucideIcons.triangleAlert,
                            text:
                                'Could not read the agents repository from '
                                'the relay: ${agentsRepoErrorMessage(error)}. '
                                'Showing drafts only.',
                            emphasise: true,
                          ),
                          _FileList(
                            address: address,
                            repo: repo!,
                            listing: null,
                            drafts: drafts?.digest,
                          ),
                        ],
                        AsyncData(:final value) => [
                          _TipLine(
                            key: const ValueKey('agents-repo-tip'),
                            listing: value,
                          ),
                          _FileList(
                            address: address,
                            repo: repo!,
                            listing: value,
                            drafts: drafts?.digest,
                          ),
                        ],
                      },
                    ],
                    _ => const [
                      Padding(
                        padding: EdgeInsets.all(Grid.sm),
                        child: Center(child: CircularProgressIndicator()),
                      ),
                    ],
                  },
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// Push the file page for [path].
void _openFile(BuildContext context, String address, String repo, String path) {
  Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (_) =>
          AgentsRepoFilePage(address: address, repo: repo, path: path),
    ),
  );
}

/// The text of a refusal, without the `Exception:` wrapper Dart adds.
String agentsRepoErrorMessage(Object error) {
  final text = error.toString();
  for (final prefix in const ['Exception: ', 'Bad state: ']) {
    if (text.startsWith(prefix)) return text.substring(prefix.length);
  }
  return text;
}

/// Run one write and show a refusal as it came.
Future<bool> runAgentsRepoAction(
  BuildContext context,
  Future<void> Function() action,
) async {
  try {
    await action();
    return true;
  } catch (error) {
    if (!context.mounted) return false;
    ScaffoldMessenger.maybeOf(
      context,
    )?.showSnackBar(SnackBar(content: Text(agentsRepoErrorMessage(error))));
    return false;
  }
}
