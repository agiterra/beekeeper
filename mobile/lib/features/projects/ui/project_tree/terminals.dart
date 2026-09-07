part of '../project_tree.dart';

/// The terminals shared under the project.
///
/// Under the project, not a channel: a kind:30623 announce names only the
/// project it is shared under. The section says so once, in its own words,
/// rather than sorting rows under a channel nothing on the wire supports.
class _ProjectTerminalsSection extends StatelessWidget {
  final Project project;
  final List<RemoteTerminal> terminals;
  final TerminalsIndex index;
  final String? viewerPubkey;
  final String Function(String ownerPubkey) ownerLabel;
  final ProjectTerminalOpener? opener;

  const _ProjectTerminalsSection({
    required this.project,
    required this.terminals,
    required this.index,
    required this.viewerPubkey,
    required this.ownerLabel,
    required this.opener,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final Widget body;
    if (!index.hasRead) {
      body = ProjectSectionNote(
        key: const ValueKey('project-terminals-unread'),
        text: switch (index.connection) {
          TerminalsConnection.connecting => 'Reading shared terminals…',
          TerminalsConnection.error =>
            'Terminals could not be read: ${index.lastError}',
          TerminalsConnection.idle ||
          TerminalsConnection.open => 'Not connected to this community',
        },
      );
    } else if (terminals.isEmpty) {
      body = const ProjectSectionNote(
        key: ValueKey('project-terminals-empty'),
        text: 'No terminal is shared under this project right now',
      );
    } else {
      body = Column(
        children: [
          for (final terminal in terminals)
            TerminalRow(
              terminal: terminal,
              ownerLabel: ownerLabel(terminal.ownerPubkey),
              viewerPubkey: viewerPubkey,
              onTap: opener == null ? null : () => opener!(context, terminal),
            ),
        ],
      );
    }
    return Padding(
      padding: const EdgeInsets.only(top: Grid.xs),
      child: AppListCard(
        key: const ValueKey('project-terminals'),
        label: 'Terminals',
        children: [
          body,
          if (index.hasRead && index.connection == TerminalsConnection.error)
            Padding(
              padding: const EdgeInsets.fromLTRB(Grid.xs, 0, Grid.xs, Grid.xxs),
              child: Text(
                'This list may be out of date: ${index.lastError}',
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.error,
                ),
              ),
            ),
        ],
      ),
    );
  }
}
