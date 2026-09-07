import 'package:flutter/material.dart';
import 'package:flutter_hooks/flutter_hooks.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:xterm/xterm.dart';

import '../../../shared/relay/relay_provider.dart';
import '../../../shared/theme/theme.dart';
import '../../../shared/utils/string_utils.dart';
import '../../../shared/widgets/frosted_app_bar.dart';
import '../../../shared/widgets/frosted_scaffold.dart';
import '../domain/terminals_domain.dart';
import '../state/shell_announce_head_provider.dart';
import '../state/shell_observer_provider.dart';

part 'terminal_observe_page/header.dart';

/// The monospace face the emulator draws with; bundled with the app.
const terminalFontFamily = 'GeistMono';

/// The emulator's type size. Small on purpose: an owner's 120-column grid
/// has to fit a phone in landscape at all.
const terminalFontSize = 11.0;

/// Watch another member's shared terminal (NIP-ST), read-only.
///
/// The grid keeps the owner's dimensions and scrolls sideways rather than
/// reflowing — observers cannot resize, and reflowing would draw a screen
/// the owner never saw. Nothing is claimed to be live until a frame arrives:
/// the status chip says "Connecting…", then "LIVE", "Not streaming", or
/// "Ended", from the frames alone.
class TerminalObservePage extends HookConsumerWidget {
  final ShellObserverTarget target;

  /// The announce this page was opened from, shown until the live head
  /// read replaces it.
  final RemoteTerminal? initial;

  /// Resolves an owner pubkey to a display name; falls back to a short key.
  final String Function(String ownerPubkey)? ownerLabel;

  TerminalObservePage({
    super.key,
    required RemoteTerminal terminal,
    this.ownerLabel,
  }) : target = ShellObserverTarget.of(terminal),
       initial = terminal;

  /// Open by target alone — a deep link — with the head read on arrival.
  const TerminalObservePage.byTarget({
    super.key,
    required this.target,
    this.ownerLabel,
  }) : initial = null;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final observer = ref.watch(shellObserverProvider(target));
    final head = ref.watch(shellAnnounceHeadProvider(target));
    final me = ref.watch(myPubkeyProvider);
    final terminal = head.terminal ?? (head.hasRead ? null : initial);
    final dims = terminal?.dims ?? initial?.dims;
    final emulator = useMemoized(() => Terminal(maxLines: 5000), [target]);
    final cols = useState(dims?.cols ?? 80);
    final rows = useState(dims?.rows ?? 24);

    // The emulator takes the owner's grid before the first byte, and again
    // on every resize frame; it never resizes itself to this screen.
    useEffect(() {
      emulator.resize(cols.value, rows.value);
      return null;
    }, [emulator, cols.value, rows.value]);

    useEffect(() {
      final notifier = ref.read(shellObserverProvider(target).notifier);
      final subscription = notifier.writes.listen((write) {
        final resize = write.resize;
        if (resize != null) {
          cols.value = resize.cols;
          rows.value = resize.rows;
          emulator.resize(resize.cols, resize.rows);
        }
        final text = write.text;
        if (text != null) emulator.write(text);
      });
      return subscription.cancel;
    }, [emulator, target]);

    final label = ownerLabel ?? shortPubkey;
    final ownerName = label(target.ownerPubkey);
    final closed = head.hasRead && head.terminal == null;
    final role = terminal == null
        ? null
        : me != null && me.toLowerCase() == target.ownerPubkey
        ? 'Yours, from another device'
        : switch (terminal.roleOf(me)) {
            ShellRosterRole.collaborator => 'Collaborator',
            ShellRosterRole.viewer => 'Viewer',
            null => 'Member',
          };

    return FrostedScaffold(
      backgroundColor: context.colors.surface,
      appBar: FrostedAppBar(
        title: Text(
          terminal?.title ?? initial?.title ?? 'Terminal',
          overflow: TextOverflow.ellipsis,
        ),
        actions: [
          if (observer.status != ShellObserverStatus.ended &&
              observer.status != ShellObserverStatus.offline)
            IconButton(
              key: const ValueKey('terminal-observe-resync'),
              tooltip: 'Refresh',
              icon: const Icon(LucideIcons.refreshCw, size: 20),
              onPressed: () =>
                  ref.read(shellObserverProvider(target).notifier).resync(),
            ),
        ],
      ),
      body: Column(
        children: [
          SizedBox(height: frostedAppBarHeight(context)),
          _ObserveHeader(
            ownerName: ownerName,
            role: role,
            closed: closed,
            status: observer.status,
            dims: ShellDims(rows: rows.value, cols: cols.value),
            watchError: observer.lastWatchError,
          ),
          Expanded(
            child: _TerminalGrid(
              emulator: emulator,
              cols: cols.value,
              rows: rows.value,
            ),
          ),
        ],
      ),
    );
  }
}

/// The emulator at the owner's exact grid, letterboxed and scrollable.
class _TerminalGrid extends StatelessWidget {
  final Terminal emulator;
  final int cols;
  final int rows;

  const _TerminalGrid({
    required this.emulator,
    required this.cols,
    required this.rows,
  });

  @override
  Widget build(BuildContext context) {
    const style = TerminalStyle(
      fontSize: terminalFontSize,
      fontFamily: terminalFontFamily,
    );
    final cell = _cellSize(context, style);
    final width = cell.width * cols + Grid.xs * 2;
    final height = cell.height * rows + Grid.xs * 2;
    return ColoredBox(
      color: const Color(0xFF1E1E2E),
      child: SingleChildScrollView(
        key: const ValueKey('terminal-observe-scroll'),
        scrollDirection: Axis.horizontal,
        child: SizedBox(
          width: width,
          height: height,
          child: TerminalView(
            emulator,
            key: const ValueKey('terminal-observe-view'),
            autoResize: false,
            readOnly: true,
            hardwareKeyboardOnly: true,
            textStyle: style,
            padding: const EdgeInsets.all(Grid.xs),
            theme: TerminalThemes.defaultTheme,
          ),
        ),
      ),
    );
  }

  /// One cell's box in this text scale, measured the way the emulator lays
  /// out its glyphs.
  static Size _cellSize(BuildContext context, TerminalStyle style) {
    final painter = TextPainter(
      text: TextSpan(text: 'mmmmmmmmmm', style: style.toTextStyle()),
      textDirection: TextDirection.ltr,
      textScaler: MediaQuery.textScalerOf(context),
    )..layout();
    return Size(painter.width / 10, painter.height);
  }
}
