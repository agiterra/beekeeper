part of '../agents_repo_page.dart';

/// One file: a Main / Draft toggle over `main`'s text and the open draft's,
/// the facts about the draft (who, when, whether main moved since), and
/// the edit, archive and withdraw doors.
class AgentsRepoFilePage extends HookConsumerWidget {
  final String address;
  final String repo;
  final String path;

  const AgentsRepoFilePage({
    super.key,
    required this.address,
    required this.repo,
    required this.path,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final key = (address: address, repo: repo);
    final drafts = ref.watch(agentsRepoDraftsProvider(key));
    final file = ref.watch(
      agentsRepoFileProvider((address: address, path: path)),
    );
    final listing = ref.watch(agentsRepoListingProvider(address));
    final me = ref.watch(myPubkeyProvider);
    final profiles = ref.watch(userCacheProvider);
    String name(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);
    final entry = drafts.digest.byPath(path);
    final head = entry?.head;
    final showDraft = useState(head != null);
    useEffect(() {
      if (head == null) showDraft.value = false;
      return null;
    }, [head?.id]);

    final main = file.value;
    final tip = listing.value?.commit;
    final mainText = main?.text;
    final draftText = head == null
        ? null
        : switch (head.op) {
            DraftOpKind.filePut => head.text,
            DraftOpKind.fileMove => head.to == path ? mainText : null,
            _ => null,
          };
    final showing = showDraft.value && head != null ? draftText : mainText;
    final colors = context.colors;

    final facts = <(String, bool)>[
      if (head != null) ...[
        (
          'Draft by ${name(head.author)}, ${_agoSeconds(head.createdAt)}',
          false,
        ),
        if (entry!.superseded.isNotEmpty)
          (
            '${entry.superseded.length} earlier '
                '${entry.superseded.length == 1 ? 'draft' : 'drafts'} superseded',
            false,
          ),
        if (entry.diverged)
          (
            'Two people saved from the same starting point; the newer save '
                'is the head and the other\'s text is not in it.',
            true,
          ),
        if (main != null && head.base != main.blob)
          (
            'main changed this file since this draft was based on it — a '
                'commit will refuse it until the draft is re-applied.',
            true,
          )
        else if (head.baseCommit != null &&
            tip != null &&
            head.baseCommit != tip)
          (
            'main moved since this draft was based '
                '(${head.baseCommit!.substring(0, 8)} → ${tip.substring(0, 8)}); '
                'the file itself is unchanged.',
            false,
          ),
      ],
    ];

    Future<void> edit() async {
      if (main == null && head == null) return;
      final result = await showBeekeeperModalBottomSheet<_EditResult>(
        context: context,
        title: path,
        isScrollControlled: true,
        builder: (_) => _EditSheet(initial: draftText ?? mainText ?? ''),
      );
      if (result == null || !context.mounted) return;
      await runAgentsRepoAction(
        context,
        () => ref
            .read(agentsRepoActionsProvider(key))
            .saveDraft(
              path: path,
              text: result.text,
              base: main?.blob,
              baseCommit: main?.commit,
              openedOn: head?.id,
              message: result.message,
              authorName: name,
            ),
      );
    }

    Future<void> archive() async {
      final blob = main?.blob;
      if (blob == null) return;
      await runAgentsRepoAction(
        context,
        () => ref
            .read(agentsRepoActionsProvider(key))
            .moveDraft(
              path: path,
              base: blob,
              baseCommit: main?.commit,
              openedOn: head?.id,
              message: null,
              authorName: name,
            ),
      );
    }

    Future<void> withdraw() async {
      final id = head?.id;
      if (id == null) return;
      await runAgentsRepoAction(
        context,
        () => ref.read(agentsRepoActionsProvider(key)).withdraw(id),
      );
    }

    final counterpart = archiveCounterpart(path);
    final klass = draftPathClass(path);
    final isArchived =
        klass == DraftPathClass.archivedRole ||
        klass == DraftPathClass.archivedPlan;
    final canEdit = isDraftablePath(path) && (main != null || head != null);

    return FrostedScaffold(
      backgroundColor: colors.surface,
      appBar: FrostedAppBar(
        title: Text(displayName(path), overflow: TextOverflow.ellipsis),
        actions: [
          PopupMenuButton<_FileMenuAction>(
            key: const ValueKey('agents-repo-file-menu'),
            icon: const Icon(LucideIcons.ellipsisVertical, size: 20),
            onSelected: (action) {
              switch (action) {
                case _FileMenuAction.edit:
                  edit();
                case _FileMenuAction.archive:
                  archive();
                case _FileMenuAction.withdraw:
                  withdraw();
              }
            },
            itemBuilder: (_) => [
              if (canEdit)
                const PopupMenuItem(
                  key: ValueKey('agents-repo-menu-edit'),
                  value: _FileMenuAction.edit,
                  child: Text('Edit as draft'),
                ),
              if (counterpart != null && main?.state == 'on-main')
                PopupMenuItem(
                  key: const ValueKey('agents-repo-menu-archive'),
                  value: _FileMenuAction.archive,
                  child: Text(isArchived ? 'Put back in force' : 'Archive'),
                ),
              if (head != null && me != null && head.author == me)
                const PopupMenuItem(
                  key: ValueKey('agents-repo-menu-withdraw'),
                  value: _FileMenuAction.withdraw,
                  child: Text('Withdraw my draft'),
                ),
            ],
          ),
        ],
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.fromLTRB(
                Grid.xs,
                Grid.xxs,
                Grid.xs,
                Grid.xl,
              ),
              children: [
                Text(
                  path,
                  key: const ValueKey('agents-repo-file-path'),
                  style: context.textTheme.bodySmall?.copyWith(
                    fontFamily: 'monospace',
                    color: colors.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: Grid.xxs),
                if (head != null)
                  SegmentedButton<bool>(
                    key: const ValueKey('agents-repo-source-toggle'),
                    segments: const [
                      ButtonSegment(value: false, label: Text('Main')),
                      ButtonSegment(value: true, label: Text('Draft')),
                    ],
                    selected: {showDraft.value},
                    onSelectionChanged: (selection) =>
                        showDraft.value = selection.first,
                  ),
                for (final (text, warn) in facts)
                  _Notice(
                    icon: warn ? LucideIcons.triangleAlert : LucideIcons.info,
                    text: text,
                    emphasise: warn,
                  ),
                const _Notice(
                  key: ValueKey('agents-repo-file-mobile-note'),
                  icon: LucideIcons.info,
                  text: agentsRepoMobileCommitNote,
                ),
                const SizedBox(height: Grid.xxs),
                ...switch (file) {
                  AsyncLoading() when head == null => const [
                    Center(child: CircularProgressIndicator()),
                  ],
                  AsyncError(:final error) when head == null => [
                    _Notice(
                      icon: LucideIcons.triangleAlert,
                      text:
                          'Could not read main: ${agentsRepoErrorMessage(error)}',
                      emphasise: true,
                    ),
                  ],
                  _ => [
                    if (showDraft.value && head != null && draftText == null)
                      Text(
                        head.op == DraftOpKind.fileMove
                            ? 'Draft moves this file to ${head.to}.'
                            : 'Draft removes this file.',
                        key: const ValueKey('agents-repo-draft-removes'),
                      )
                    else if (showing == null)
                      Text(switch (main?.state) {
                        'too-large' =>
                          'This file is larger than a draft can carry; edit it with git.',
                        'not-text' => 'This file is not text.',
                        _ => 'Not on main.',
                      }, key: const ValueKey('agents-repo-file-absent'))
                    else if (isMarkdownPath(path))
                      GptMarkdown(
                        showing,
                        key: const ValueKey('agents-repo-file-markdown'),
                        style: context.textTheme.bodyMedium,
                      )
                    else
                      SelectableText(
                        showing,
                        key: const ValueKey('agents-repo-file-text'),
                        style: context.textTheme.bodySmall?.copyWith(
                          fontFamily: 'monospace',
                        ),
                      ),
                  ],
                },
              ],
            ),
          ),
        ],
      ),
    );
  }
}

enum _FileMenuAction { edit, archive, withdraw }
