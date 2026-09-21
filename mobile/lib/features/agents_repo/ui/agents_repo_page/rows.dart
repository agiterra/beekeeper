part of '../agents_repo_page.dart';

class _Notice extends StatelessWidget {
  final IconData icon;
  final String text;
  final bool emphasise;

  const _Notice({
    super.key,
    required this.icon,
    required this.text,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final color = emphasise ? colors.error : colors.onSurfaceVariant;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xs,
        vertical: Grid.half,
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(icon, size: 16, color: color),
          const SizedBox(width: Grid.xxs),
          Expanded(
            child: Text(
              text,
              style: context.textTheme.bodySmall?.copyWith(color: color),
            ),
          ),
        ],
      ),
    );
  }
}

/// Which tip the list shows, and when this device read it.
class _TipLine extends StatelessWidget {
  final AgentsRepoListing listing;
  const _TipLine({super.key, required this.listing});

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(
        horizontal: Grid.xs,
        vertical: Grid.half,
      ),
      child: Text(
        'main at ${listing.commit.substring(0, 8)} · read '
        '${_ago(listing.fetchedAt)}',
        style: context.textTheme.bodySmall?.copyWith(
          color: colors.onSurfaceVariant,
        ),
      ),
    );
  }
}

String _ago(DateTime when) {
  final delta = DateTime.now().difference(when);
  if (delta.inSeconds < 60) return 'just now';
  if (delta.inMinutes < 60) return '${delta.inMinutes}m ago';
  if (delta.inHours < 24) return '${delta.inHours}h ago';
  return '${delta.inDays}d ago';
}

String _agoSeconds(int unixSeconds) =>
    _ago(DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000));

/// One row of the list: a file on `main`, a draft-only file, or both.
class _FileRow {
  final String path;
  final AgentsRepoEntry? entry;
  final DraftPath? draft;
  const _FileRow({
    required this.path,
    required this.entry,
    required this.draft,
  });
  bool get notOnMain => entry == null;
}

/// The files, grouped with plans first; a draft chip on each open draft.
class _FileList extends ConsumerWidget {
  final String address;
  final String repo;
  final AgentsRepoListing? listing;
  final AgentsRepoDraftDigest? drafts;

  const _FileList({
    required this.address,
    required this.repo,
    required this.listing,
    required this.drafts,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final profiles = ref.watch(userCacheProvider);
    String name(String pubkey) =>
        profiles[pubkey.toLowerCase()]?.label ?? shortPubkey(pubkey);
    final byPath = <String, _FileRow>{};
    for (final entry in listing?.entries ?? const <AgentsRepoEntry>[]) {
      if (entry.kind != 'blob' || isGitkeep(entry.path)) continue;
      byPath[entry.path] = _FileRow(
        path: entry.path,
        entry: entry,
        draft: null,
      );
    }
    for (final draft in drafts?.paths ?? const <DraftPath>[]) {
      final existing = byPath[draft.path];
      byPath[draft.path] = _FileRow(
        path: draft.path,
        entry: existing?.entry,
        draft: draft,
      );
    }
    final groups = groupPaths<_FileRow>(byPath.values, (row) => row.path);
    final colors = context.colors;
    if (groups.isEmpty) {
      return const _Notice(
        key: ValueKey('agents-repo-empty'),
        icon: LucideIcons.folder,
        text: 'No files.',
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (final group in groups) ...[
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Grid.xs,
              Grid.xs,
              Grid.xs,
              Grid.half,
            ),
            child: Text(
              group.group.label.toUpperCase(),
              style: context.textTheme.labelSmall?.copyWith(
                color: colors.onSurfaceVariant,
                letterSpacing: 1.2,
              ),
            ),
          ),
          for (final row in group.entries)
            ListTile(
              key: ValueKey('agents-repo-file-${row.path}'),
              dense: true,
              leading: Icon(
                isMarkdownPath(row.path)
                    ? LucideIcons.fileText
                    : LucideIcons.fileCode,
                size: 18,
                color: colors.onSurfaceVariant,
              ),
              title: Text(
                displayName(row.path),
                style: row.notOnMain
                    ? const TextStyle(fontStyle: FontStyle.italic)
                    : null,
              ),
              subtitle: row.draft == null
                  ? null
                  : Text(
                      'draft · not on main · ${name(row.draft!.head.author)}, '
                      '${_agoSeconds(row.draft!.head.createdAt)}',
                    ),
              trailing: row.draft == null
                  ? null
                  : Chip(
                      key: ValueKey('agents-repo-draft-chip-${row.path}'),
                      label: const Text('draft'),
                      visualDensity: VisualDensity.compact,
                    ),
              onTap: () => _openFile(context, address, repo, row.path),
            ),
        ],
      ],
    );
  }
}
