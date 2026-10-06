import 'package:flutter/material.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/bee_refresh_indicator.dart';
import '../../../shared/widgets/beekeeper_loading_indicator.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../domain/terminals_domain.dart';
import '../state/terminals_index_provider.dart';
import 'terminal_observe_page.dart';
import 'terminal_row.dart';

/// The line shown when the relay answered and nobody is sharing a terminal.
const terminalsEmptyLabel = 'Nobody is sharing a terminal right now';

/// Push the observe page for [terminal].
void openTerminalObservePage(BuildContext context, RemoteTerminal terminal) {
  Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (_) => TerminalObservePage(terminal: terminal),
    ),
  );
}

/// Every shared terminal this reader may see, grouped by project address.
///
/// The project page is the primary door (terminals sit under their project
/// there); this page is the flat list behind it, and the one the tests pin.
/// Tapping a row opens it through [onOpen]; when no opener is wired the rows
/// are inert, and say nothing that would suggest otherwise.
class TerminalsPage extends ConsumerWidget {
  /// Resolves a project address to a display name; falls back to the address.
  final String Function(String projectAddress)? projectLabel;

  /// Resolves an owner pubkey to a display name; falls back to a short key.
  final String Function(String ownerPubkey)? ownerLabel;

  /// Opens a terminal; defaults to the observe page. Pass
  /// [TerminalsPage.inert] to leave the rows inert.
  final void Function(BuildContext context, RemoteTerminal terminal)? onOpen;

  const TerminalsPage({
    super.key,
    this.projectLabel,
    this.ownerLabel,
    this.onOpen = openTerminalObservePage,
  });

  /// A page whose rows open nothing.
  const TerminalsPage.inert({super.key, this.projectLabel, this.ownerLabel})
    : onOpen = null;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final index = ref.watch(terminalsIndexProvider);
    final me = ref.watch(myPubkeyProvider);
    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: const FrostedAppBar(title: Text('Terminals')),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          Expanded(
            child: BeeRefreshIndicator(
              onRefresh: () =>
                  ref.read(terminalsIndexProvider.notifier).refresh(),
              child: TerminalsList(
                index: index,
                viewerPubkey: me,
                projectLabel: projectLabel ?? (address) => address,
                ownerLabel: ownerLabel ?? shortPubkey,
                onOpen: onOpen,
                onRetry: () =>
                    ref.read(terminalsIndexProvider.notifier).refresh(),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// The grouped list, with the honest empty, loading and error states.
class TerminalsList extends StatelessWidget {
  final TerminalsIndex index;
  final String? viewerPubkey;
  final String Function(String projectAddress) projectLabel;
  final String Function(String ownerPubkey) ownerLabel;
  final void Function(BuildContext context, RemoteTerminal terminal)? onOpen;
  final Future<void> Function() onRetry;

  const TerminalsList({
    super.key,
    required this.index,
    required this.viewerPubkey,
    required this.projectLabel,
    required this.ownerLabel,
    required this.onOpen,
    required this.onRetry,
  });

  @override
  Widget build(BuildContext context) {
    if (!index.hasRead) {
      return switch (index.connection) {
        TerminalsConnection.connecting => ListView(
          key: const ValueKey('terminals-loading'),
          padding: const EdgeInsets.only(top: Grid.xxl),
          children: const [
            Center(
              child: BeekeeperLoadingIndicator(
                size: 40,
                semanticLabel: 'Reading shared terminals',
              ),
            ),
          ],
        ),
        TerminalsConnection.error => _TerminalsMessage(
          key: const ValueKey('terminals-error'),
          icon: LucideIcons.triangleAlert,
          title: 'Terminals could not be read',
          detail: index.lastError ?? 'The relay read failed.',
          onRetry: onRetry,
        ),
        TerminalsConnection.idle ||
        TerminalsConnection.open => _TerminalsMessage(
          key: const ValueKey('terminals-disconnected'),
          icon: LucideIcons.plugZap,
          title: 'Not connected to this community',
          detail: 'Nothing is being read while the connection is down.',
          onRetry: onRetry,
        ),
      };
    }
    final children = <Widget>[
      if (index.connection == TerminalsConnection.error)
        Padding(
          key: const ValueKey('terminals-stale'),
          padding: const EdgeInsets.fromLTRB(Grid.xs, Grid.xxs, Grid.xs, 0),
          child: Text(
            'This list may be out of date: '
            '${index.lastError ?? 'the relay read failed'}',
            style: context.textTheme.bodySmall?.copyWith(
              color: context.colors.error,
            ),
          ),
        ),
      if (index.terminals.isEmpty)
        const Padding(
          key: ValueKey('terminals-empty'),
          padding: EdgeInsets.all(Grid.gutter),
          child: Center(child: Text(terminalsEmptyLabel)),
        )
      else
        for (final entry in index.byProject.entries) ...[
          Padding(
            padding: const EdgeInsets.fromLTRB(
              Grid.gutter,
              Grid.xs,
              Grid.gutter,
              Grid.quarter,
            ),
            child: Text(
              projectLabel(entry.key),
              key: ValueKey('terminals-project-${entry.key}'),
              style: context.textTheme.labelLarge?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ),
          for (final terminal in entry.value)
            TerminalRow(
              terminal: terminal,
              ownerLabel: ownerLabel(terminal.ownerPubkey),
              viewerPubkey: viewerPubkey,
              onTap: onOpen == null ? null : () => onOpen!(context, terminal),
            ),
        ],
    ];
    return ListView(
      padding: const EdgeInsets.only(bottom: Grid.xl),
      children: children,
    );
  }
}

class _TerminalsMessage extends StatelessWidget {
  final IconData icon;
  final String title;
  final String detail;
  final Future<void> Function() onRetry;

  const _TerminalsMessage({
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
          key: const ValueKey('terminals-retry'),
          onPressed: onRetry,
          icon: const Icon(LucideIcons.refreshCw, size: 16),
          label: const Text('Retry'),
        ),
      ),
    ],
  );
}
