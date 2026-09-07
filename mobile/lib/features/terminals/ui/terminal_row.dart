import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/theme/theme.dart';
import '../domain/terminals_domain.dart';

/// What this viewer may do with a terminal, from its announce roster.
String terminalAccessLabel(RemoteTerminal terminal, String? viewerPubkey) {
  if (viewerPubkey == null) return 'observe only';
  if (viewerPubkey.toLowerCase() == terminal.ownerPubkey) {
    return 'yours · you can type';
  }
  return switch (terminal.roleOf(viewerPubkey)) {
    ShellRosterRole.collaborator => 'collaborator · you can type',
    ShellRosterRole.viewer => 'viewer · observe only',
    null => 'member · observe only',
  };
}

/// One shared terminal in a list.
///
/// Says who owns it, its grid, and what the viewer may do — never that it is
/// live. An announce marked `open` is only a claim until frames arrive, and
/// the observe page is where that is settled.
class TerminalRow extends StatelessWidget {
  final RemoteTerminal terminal;

  /// The owner's display name, or a shortened pubkey — resolved by the caller
  /// so this widget stays free of profile state.
  final String ownerLabel;
  final String? viewerPubkey;
  final VoidCallback? onTap;

  const TerminalRow({
    super.key,
    required this.terminal,
    required this.ownerLabel,
    required this.viewerPubkey,
    this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final colors = context.colors;
    final dims = terminal.dims;
    final subtitle = [
      ownerLabel,
      if (dims != null) '$dims',
      terminalAccessLabel(terminal, viewerPubkey),
    ].join(' · ');
    return ListTile(
      key: ValueKey('terminal-row-${terminal.key}'),
      leading: Icon(LucideIcons.squareTerminal, color: colors.primary),
      title: Text(
        terminal.title,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: context.textTheme.titleSmall,
      ),
      subtitle: Text(
        subtitle,
        maxLines: 2,
        overflow: TextOverflow.ellipsis,
        style: context.textTheme.bodySmall?.copyWith(
          color: colors.onSurfaceVariant,
        ),
      ),
      trailing: onTap == null
          ? null
          : Icon(LucideIcons.chevronRight, size: 18, color: colors.outline),
      onTap: onTap,
    );
  }
}
