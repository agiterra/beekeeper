part of '../project_todos_page.dart';

/// What the assignee sheet resolves to: a pubkey, or `null` to clear.
@immutable
class _AssigneePick {
  final String? pubkey;
  const _AssigneePick(this.pubkey);
}

/// Who an item can be assigned to: the project's owner and members from
/// its head, plus every agent the relay knows (an agent is a legitimate
/// assignee — it is who a "do this" item is often for), each with a bot
/// glyph; and "Unassigned" to clear. Agents are read from the relay's
/// directory and from verified NIP-OA owner tags, so an agent on the
/// project's roster shows the glyph too.
class _AssigneeSheet extends HookConsumerWidget {
  final String address;
  final String? current;

  const _AssigneeSheet({required this.address, required this.current});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final project = ref.watch(projectsProvider).byAddress(address);
    final profiles = ref.watch(userCacheProvider);
    final agents = ref.watch(knownAgentPubkeysProvider);
    final me = ref.watch(myPubkeyProvider)?.toLowerCase();

    final candidates = <String>[];
    final seen = <String>{};
    void add(String pubkey) {
      final key = pubkey.toLowerCase();
      if (seen.add(key)) candidates.add(key);
    }

    if (me != null) add(me);
    if (project != null) {
      add(project.owner);
      project.memberPubkeys.forEach(add);
    }
    if (current != null) add(current!);
    for (final agent in agents) {
      add(agent);
    }
    useEffect(() {
      ref.read(userCacheProvider.notifier).preload(candidates);
      return null;
    }, [candidates.join(',')]);

    bool isAgent(String pubkey) =>
        profiles[pubkey]?.ownerPubkey != null || agents.contains(pubkey);
    String label(String pubkey) =>
        pubkey == me ? 'You' : profiles[pubkey]?.label ?? shortPubkey(pubkey);

    final people = [
      for (final pubkey in candidates)
        if (!isAgent(pubkey)) pubkey,
    ];
    final bots = [
      for (final pubkey in candidates)
        if (isAgent(pubkey)) pubkey,
    ];

    final colors = context.colors;
    Widget row(String pubkey) {
      final profile = profiles[pubkey];
      final agent = isAgent(pubkey);
      return ListTile(
        key: ValueKey('todo-assignee-option-$pubkey'),
        dense: true,
        leading: agent
            ? Icon(LucideIcons.bot, size: 18, color: colors.primary)
            : AvatarImage(
                imageUrl: profile?.avatarUrl,
                radius: 12,
                backgroundColor: colors.primaryContainer,
                fallback: Text(
                  (profile?.initial ?? pubkey[0]).toUpperCase(),
                  style: context.textTheme.labelMedium?.copyWith(
                    color: colors.onPrimaryContainer,
                  ),
                ),
              ),
        title: Text(label(pubkey), overflow: TextOverflow.ellipsis),
        subtitle: agent ? const Text('agent') : null,
        trailing: pubkey == current
            ? Icon(LucideIcons.check, size: 18, color: colors.primary)
            : null,
        onTap: () => Navigator.of(context).pop(_AssigneePick(pubkey)),
      );
    }

    Widget heading(String text) => Padding(
      padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xxs, Grid.xs, 0),
      child: Text(
        text,
        style: context.textTheme.labelLarge?.copyWith(
          color: colors.onSurfaceVariant,
        ),
      ),
    );

    return ListView(
      shrinkWrap: true,
      padding: const EdgeInsets.only(bottom: Grid.xs),
      children: [
        ListTile(
          key: const ValueKey('todo-assignee-option-none'),
          dense: true,
          leading: Icon(
            LucideIcons.userRoundX,
            size: 18,
            color: colors.onSurfaceVariant,
          ),
          title: const Text('Unassigned'),
          trailing: current == null
              ? Icon(LucideIcons.check, size: 18, color: colors.primary)
              : null,
          onTap: () => Navigator.of(context).pop(const _AssigneePick(null)),
        ),
        if (people.isNotEmpty) heading('People'),
        for (final pubkey in people) row(pubkey),
        if (bots.isNotEmpty) heading('Agents'),
        for (final pubkey in bots) row(pubkey),
        if (project == null)
          Padding(
            padding: const EdgeInsets.all(Grid.xs),
            child: Text(
              'The project head is not in the current read, so its members '
              'cannot be listed.',
              style: context.textTheme.bodySmall?.copyWith(
                color: colors.onSurfaceVariant,
              ),
            ),
          ),
      ],
    );
  }
}
