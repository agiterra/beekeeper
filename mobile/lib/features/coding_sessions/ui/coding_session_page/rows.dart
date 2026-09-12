part of '../coding_session_page.dart';

/// Dispatches one projected item to the row that suits its type.
class _TranscriptRow extends StatelessWidget {
  final CodingSessionTranscriptItem item;

  const _TranscriptRow({required this.item});

  @override
  Widget build(BuildContext context) => switch (item.type) {
    CodingSessionItemType.tool => _ToolRowView(item: item),
    CodingSessionItemType.thought => _FoldedTextRow(
      item: item,
      icon: LucideIcons.brain,
    ),
    CodingSessionItemType.message => _MessageRow(item: item),
    CodingSessionItemType.plan => _PlainRow(item: item, icon: LucideIcons.list),
    CodingSessionItemType.lifecycle => _LifecycleRow(item: item),
  };
}

/// A prompt or an assistant reply.
class _MessageRow extends StatelessWidget {
  final CodingSessionTranscriptItem item;

  const _MessageRow({required this.item});

  @override
  Widget build(BuildContext context) {
    final isUser = item.role == CodingSessionItemRole.user;
    final meta = <String>[
      if (item.operatorPubkey case final operator?)
        'operator ${shortPubkey(operator)}',
      // A mid-turn correction the running turn took, said beside the sender
      // rather than only in the row's title.
      if (item.steered) 'steered',
      if (item.commandId case final command?) 'command ${shortPubkey(command)}',
    ];
    return _RowShell(
      itemKey: item.eventId,
      icon: isUser ? LucideIcons.user : LucideIcons.bot,
      title: item.title,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (meta.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.quarter),
              child: Text(
                meta.join(' · '),
                style: context.textTheme.labelSmall?.copyWith(
                  color: context.colors.onSurfaceVariant,
                ),
              ),
            ),
          if (item.text.isNotEmpty)
            SelectableText(item.text, style: context.textTheme.bodyMedium),
        ],
      ),
    );
  }
}

/// A row whose text is shown as-is, with no folding.
class _PlainRow extends StatelessWidget {
  final CodingSessionTranscriptItem item;
  final IconData icon;

  const _PlainRow({required this.item, required this.icon});

  @override
  Widget build(BuildContext context) => _RowShell(
    itemKey: item.eventId,
    icon: icon,
    title: item.title,
    child: item.text.isEmpty
        ? null
        : SelectableText(item.text, style: context.textTheme.bodyMedium),
  );
}

/// A bounded lifecycle marker: continuity, compaction, elision, turn results.
class _LifecycleRow extends StatelessWidget {
  final CodingSessionTranscriptItem item;

  const _LifecycleRow({required this.item});

  @override
  Widget build(BuildContext context) {
    final result = item.result;
    final facts = <String>[
      ?result?.outcome,
      if (result?.durationMs case final duration?) '${duration}ms',
      if (result?.costUsd case final cost?) '\$${cost.toStringAsFixed(4)}',
      if (result?.isError == true) 'error',
    ];
    return _RowShell(
      itemKey: item.eventId,
      icon: item.unknownKind != null
          ? LucideIcons.circleHelp
          : LucideIcons.info,
      title: item.title,
      emphasise: result?.isError == true,
      child: (facts.isEmpty && item.text.isEmpty)
          ? null
          : Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (facts.isNotEmpty)
                  Text(
                    facts.join(' · '),
                    key: ValueKey('coding-session-result-${item.eventId}'),
                    style: context.textTheme.labelSmall?.copyWith(
                      color: context.colors.onSurfaceVariant,
                    ),
                  ),
                if (item.text.isNotEmpty)
                  Text(item.text, style: context.textTheme.bodySmall),
              ],
            ),
    );
  }
}

/// Reasoning: present, named, and closed until the reader opens it.
class _FoldedTextRow extends HookWidget {
  final CodingSessionTranscriptItem item;
  final IconData icon;

  const _FoldedTextRow({required this.item, required this.icon});

  @override
  Widget build(BuildContext context) {
    final expanded = useState(!item.foldedByDefault);
    return _RowShell(
      itemKey: item.eventId,
      icon: icon,
      title: item.title,
      trailing: _DisclosureButton(
        buttonKey: ValueKey('coding-session-reasoning-toggle-${item.eventId}'),
        expanded: expanded.value,
        onTap: () => expanded.value = !expanded.value,
      ),
      child: expanded.value && item.text.isNotEmpty
          ? SelectableText(
              item.text,
              key: ValueKey('coding-session-reasoning-body-${item.eventId}'),
              style: context.textTheme.bodySmall,
            )
          : null,
    );
  }
}

/// A tool call folded to one line, expandable to its arguments and result.
class _ToolRowView extends HookWidget {
  final CodingSessionTranscriptItem item;

  const _ToolRowView({required this.item});

  @override
  Widget build(BuildContext context) {
    final expanded = useState(false);
    final tool = item.tool;
    final colors = context.colors;
    final summary = tool?.argsSummary ?? '';
    return _RowShell(
      itemKey: item.eventId,
      icon: LucideIcons.wrench,
      title: item.title,
      emphasise: tool?.isError == true,
      trailing: _DisclosureButton(
        buttonKey: ValueKey('coding-session-tool-toggle-${item.eventId}'),
        expanded: expanded.value,
        onTap: () => expanded.value = !expanded.value,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (summary.isNotEmpty)
            Text(
              summary,
              key: ValueKey('coding-session-tool-summary-${item.eventId}'),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.textTheme.bodySmall?.copyWith(
                color: colors.onSurfaceVariant,
              ),
            ),
          if (expanded.value) ...[
            if (tool != null && tool.args.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: Grid.half),
                child: SelectableText(
                  _prettyArgs(tool.args),
                  key: ValueKey('coding-session-tool-args-${item.eventId}'),
                  style: context.textTheme.bodySmall?.copyWith(
                    fontFamily: 'monospace',
                  ),
                ),
              ),
            if (tool?.result case final result?)
              Padding(
                padding: const EdgeInsets.only(top: Grid.half),
                child: SelectableText(
                  result,
                  key: ValueKey('coding-session-tool-result-${item.eventId}'),
                  style: context.textTheme.bodySmall?.copyWith(
                    color: tool?.isError == true
                        ? colors.error
                        : colors.onSurface,
                  ),
                ),
              ),
            if (tool != null &&
                tool.status == CodingSessionToolStatus.executing)
              Padding(
                padding: const EdgeInsets.only(top: Grid.half),
                child: Text(
                  'No result published yet',
                  key: ValueKey('coding-session-tool-pending-${item.eventId}'),
                  style: context.textTheme.labelSmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ),
          ],
        ],
      ),
    );
  }

  static String _prettyArgs(Map<String, dynamic> args) {
    try {
      return const JsonEncoder.withIndent('  ').convert(args);
    } on JsonUnsupportedObjectError {
      return args.toString();
    }
  }
}

/// The chevron that opens a folded row.
class _DisclosureButton extends StatelessWidget {
  final Key buttonKey;
  final bool expanded;
  final VoidCallback onTap;

  const _DisclosureButton({
    required this.buttonKey,
    required this.expanded,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) => IconButton(
    key: buttonKey,
    onPressed: onTap,
    visualDensity: VisualDensity.compact,
    padding: EdgeInsets.zero,
    constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
    tooltip: expanded ? 'Collapse' : 'Expand',
    icon: Icon(
      expanded ? LucideIcons.chevronDown : LucideIcons.chevronRight,
      size: 16,
      color: context.colors.onSurfaceVariant,
    ),
  );
}

/// The common frame every transcript row shares.
class _RowShell extends StatelessWidget {
  final String itemKey;
  final IconData icon;
  final String title;
  final Widget? child;
  final Widget? trailing;
  final bool emphasise;

  const _RowShell({
    required this.itemKey,
    required this.icon,
    required this.title,
    this.child,
    this.trailing,
    this.emphasise = false,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final titleColor = emphasise ? colors.error : colors.onSurfaceVariant;
    return Padding(
      key: ValueKey('coding-session-item-$itemKey'),
      padding: const EdgeInsets.only(bottom: Grid.twelve),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: Grid.quarter),
            child: Icon(icon, size: 16, color: titleColor),
          ),
          const SizedBox(width: Grid.xxs),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        title,
                        style: context.textTheme.labelSmall?.copyWith(
                          color: titleColor,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                    ?trailing,
                  ],
                ),
                ?child,
              ],
            ),
          ),
        ],
      ),
    );
  }
}
