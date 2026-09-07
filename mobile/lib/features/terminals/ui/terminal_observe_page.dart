import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
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
import '../../profile/user_cache_provider.dart';
import '../domain/terminals_domain.dart';
import '../state/shell_announce_head_provider.dart';
import '../state/shell_observer_provider.dart';

part 'terminal_observe_page/header.dart';
part 'terminal_observe_page/input_bar.dart';

/// The monospace face the emulator draws with; bundled with the app.
const terminalFontFamily = 'GeistMono';

/// The emulator's type size. Small on purpose: an owner's 120-column grid
/// has to fit a phone in landscape at all.
const terminalFontSize = 11.0;

/// Watch another member's shared terminal (NIP-ST), and type into it when
/// the owner's roster says this key may.
///
/// The grid keeps the owner's dimensions and scrolls sideways rather than
/// reflowing — observers cannot resize, and reflowing would draw a screen
/// the owner never saw. Nothing is claimed to be live until a frame arrives:
/// the status chip says "Connecting…", then "LIVE", "Not streaming", or
/// "Ended", from the frames alone. Input exists only when the *live*
/// announce roster names this key a collaborator (or it is the owner's own
/// key), so a revocation removes the bar without a reload; keystrokes are
/// sent as whole lines or single keys, never streamed.
class TerminalObservePage extends HookConsumerWidget {
  final ShellObserverTarget target;

  /// The announce this page was opened from, shown until the live head
  /// read replaces it.
  final RemoteTerminal? initial;

  /// Resolves an owner pubkey to a display name; by default the profile
  /// cache, which fetches a profile it has not seen, then a short key.
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

    // Watching the cache rebuilds when the owner's profile lands; `get`
    // schedules the fetch when it is not there yet.
    ref.watch(userCacheProvider);
    final ownerName = ownerLabel != null
        ? ownerLabel!(target.ownerPubkey)
        : ref.read(userCacheProvider.notifier).get(target.ownerPubkey)?.label ??
              shortPubkey(target.ownerPubkey);
    final closed = head.hasRead && head.terminal == null;
    final isOwner = me != null && me.toLowerCase() == target.ownerPubkey;
    // The owner's own terminal needs no role line: the title already says
    // whose it is, and the input bar says the rest.
    final role = terminal == null || isOwner
        ? null
        : switch (terminal.roleOf(me)) {
            ShellRosterRole.collaborator => 'Collaborator',
            ShellRosterRole.viewer => 'Viewer',
            null => 'Member',
          };
    // Typing needs the *live* head's word, not the row this page was opened
    // from: a roster the owner revoked a minute ago must not still type.
    final canType =
        head.hasRead &&
        head.terminal != null &&
        head.terminal!.mayType(me) &&
        observer.inputRefused == null &&
        observer.status != ShellObserverStatus.ended &&
        observer.status != ShellObserverStatus.offline;

    // A hardware keyboard reaches the emulator's input handler; its output
    // is coalesced and sent as one event, never a keystroke stream.
    final pendingKeys = useRef(StringBuffer());
    final flushTimer = useRef<Timer?>(null);
    useEffect(() {
      if (!canType) {
        emulator.onOutput = null;
        return null;
      }
      final notifier = ref.read(shellObserverProvider(target).notifier);
      emulator.onOutput = (data) {
        pendingKeys.value.write(data);
        flushTimer.value ??= Timer(terminalInputCoalesce, () {
          flushTimer.value = null;
          final text = pendingKeys.value.toString();
          pendingKeys.value.clear();
          if (text.isNotEmpty) {
            notifier.sendInput(Uint8List.fromList(utf8.encode(text)));
          }
        });
      };
      return () {
        flushTimer.value?.cancel();
        flushTimer.value = null;
        emulator.onOutput = null;
      };
    }, [emulator, canType, target]);

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
            ownerName: isOwner ? null : ownerName,
            role: role,
            closed: closed,
            canType: canType,
            status: observer.status,
            dims: ShellDims(rows: rows.value, cols: cols.value),
            watchError: observer.lastWatchError,
          ),
          Expanded(
            child: _TerminalGrid(
              emulator: emulator,
              cols: cols.value,
              rows: rows.value,
              canType: canType,
            ),
          ),
          if (canType || observer.inputRefused != null)
            _TerminalInputBar(
              target: target,
              emulator: emulator,
              observer: observer,
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

  /// Whether a hardware keyboard may type; the soft keyboard never opens
  /// from a tap either way (`hardwareKeyboardOnly`).
  final bool canType;

  const _TerminalGrid({
    required this.emulator,
    required this.cols,
    required this.rows,
    required this.canType,
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
            readOnly: !canType,
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
