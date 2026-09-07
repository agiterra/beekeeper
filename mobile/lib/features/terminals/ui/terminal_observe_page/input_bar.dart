part of '../terminal_observe_page.dart';

/// How long hardware-keyboard output is coalesced before one event goes out.
///
/// Long on purpose: every event counts against the human quota of 60 per
/// minute, shared with the paired desktop's key. A hardware keyboard is the
/// only path that produces keystrokes; the bar below sends whole lines.
const terminalInputCoalesce = Duration(milliseconds: 500);

/// One button of the key bar: a label, the emulator key, and whether it is
/// sent with Control held.
@immutable
class TerminalKeyBarEntry {
  final String label;
  final TerminalKey key;
  final bool ctrl;

  const TerminalKeyBarEntry(this.label, this.key, {this.ctrl = false});
}

/// The special keys the soft keyboard has no way to type.
///
/// Each button asks the emulator to encode the key, so application cursor
/// mode and friends are honoured, and sends the sequence as one event.
const terminalKeyBar = <TerminalKeyBarEntry>[
  TerminalKeyBarEntry('^C', TerminalKey.keyC, ctrl: true),
  TerminalKeyBarEntry('Esc', TerminalKey.escape),
  TerminalKeyBarEntry('Tab', TerminalKey.tab),
  TerminalKeyBarEntry('↑', TerminalKey.arrowUp),
  TerminalKeyBarEntry('↓', TerminalKey.arrowDown),
  TerminalKeyBarEntry('←', TerminalKey.arrowLeft),
  TerminalKeyBarEntry('→', TerminalKey.arrowRight),
  TerminalKeyBarEntry('↵', TerminalKey.enter),
];

/// Line-mode input for a collaborator, plus a key bar.
///
/// There is no local echo: typed text appears only when the owner's frames
/// stream it back, which is the one honest rendering of a keystroke that
/// crosses a relay. A `restricted:` answer replaces the bar with the relay's
/// words (NIP-ST: a refusal is a revocation); a `rate-limited:` answer
/// disables it for the relay's window and says so, and is not treated as
/// revocation.
class _TerminalInputBar extends HookConsumerWidget {
  final ShellObserverTarget target;
  final Terminal emulator;
  final ShellObserverState observer;

  const _TerminalInputBar({
    required this.target,
    required this.emulator,
    required this.observer,
  });

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controller = useTextEditingController();
    final focusNode = useFocusNode();
    final text = useState('');
    final now = useState(DateTime.now());
    final colors = context.colors;

    useEffect(() {
      void listen() => text.value = controller.text;
      controller.addListener(listen);
      return () => controller.removeListener(listen);
    }, [controller]);

    // A pause counts down on screen.
    final pausedUntil = observer.inputPausedUntil;
    useEffect(() {
      if (pausedUntil == null) return null;
      final timer = Timer.periodic(const Duration(seconds: 1), (_) {
        now.value = DateTime.now();
      });
      return timer.cancel;
    }, [pausedUntil]);

    final refused = observer.inputRefused;
    if (refused != null) {
      return _InputShell(
        key: const ValueKey('terminal-input-refused'),
        child: Row(
          children: [
            Icon(LucideIcons.ban, size: 14, color: colors.error),
            const SizedBox(width: Grid.xxs),
            Expanded(
              child: Text(
                'Input refused — $refused',
                style: context.textTheme.bodySmall?.copyWith(
                  color: colors.error,
                ),
              ),
            ),
          ],
        ),
      );
    }

    final paused = observer.inputPausedAt(now.value);
    final remaining = pausedUntil == null
        ? 0
        : pausedUntil.difference(now.value).inSeconds + 1;
    final notifier = ref.read(shellObserverProvider(target).notifier);

    Future<void> sendLine() async {
      final line = controller.text;
      if (line.isEmpty) return;
      controller.clear();
      // Enter hands the line off and keeps the keyboard: the next line is
      // the common case, not a dismissal.
      focusNode.requestFocus();
      await notifier.sendLine(line);
    }

    Future<void> sendKey(TerminalKey key, {required bool ctrl}) async {
      // Encode through the emulator so the sequence matches its modes, then
      // send whatever it emitted as one event.
      final buffer = StringBuffer();
      final previous = emulator.onOutput;
      emulator.onOutput = buffer.write;
      try {
        emulator.keyInput(key, ctrl: ctrl);
      } finally {
        emulator.onOutput = previous;
      }
      if (buffer.isEmpty) return;
      focusNode.requestFocus();
      await notifier.sendInput(Uint8List.fromList(utf8.encode('$buffer')));
    }

    return _InputShell(
      key: const ValueKey('terminal-input-bar'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (paused)
            Padding(
              padding: const EdgeInsets.only(bottom: Grid.xxs),
              child: Text(
                'Typing paused ${remaining}s — the relay asked this key to '
                'slow down',
                key: const ValueKey('terminal-input-paused'),
                style: context.textTheme.bodySmall?.copyWith(
                  color: context.appColors.warning,
                ),
              ),
            ),
          SizedBox(
            height: 28,
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                for (final entry in terminalKeyBar)
                  Padding(
                    padding: const EdgeInsets.only(right: Grid.quarter),
                    child: OutlinedButton(
                      key: ValueKey('terminal-key-${entry.label}'),
                      style: OutlinedButton.styleFrom(
                        minimumSize: const Size(36, 28),
                        padding: const EdgeInsets.symmetric(
                          horizontal: Grid.xxs,
                        ),
                        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        visualDensity: VisualDensity.compact,
                        textStyle: const TextStyle(
                          fontFamily: terminalFontFamily,
                          fontSize: 12,
                        ),
                      ),
                      onPressed: paused
                          ? null
                          : () => sendKey(entry.key, ctrl: entry.ctrl),
                      child: Text(entry.label),
                    ),
                  ),
              ],
            ),
          ),
          const SizedBox(height: Grid.quarter),
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              Expanded(
                child: TextField(
                  key: const ValueKey('terminal-input-field'),
                  controller: controller,
                  focusNode: focusNode,
                  enabled: !paused,
                  autocorrect: false,
                  enableSuggestions: false,
                  keyboardType: TextInputType.visiblePassword,
                  textInputAction: TextInputAction.send,
                  onSubmitted: (_) => sendLine(),
                  style: const TextStyle(
                    fontFamily: terminalFontFamily,
                    fontSize: 13,
                  ),
                  decoration: InputDecoration(
                    hintText: 'Type a line, Enter sends it',
                    isDense: true,
                    contentPadding: const EdgeInsets.symmetric(
                      horizontal: Grid.xxs,
                      vertical: Grid.half,
                    ),
                    border: OutlineInputBorder(
                      borderRadius: BorderRadius.circular(Radii.md),
                    ),
                  ),
                ),
              ),
              const SizedBox(width: Grid.half),
              IconButton.filled(
                key: const ValueKey('terminal-input-send'),
                tooltip: 'Send line',
                visualDensity: VisualDensity.compact,
                constraints: const BoxConstraints.tightFor(
                  width: 32,
                  height: 32,
                ),
                onPressed: paused || text.value.isEmpty ? null : sendLine,
                icon: const Icon(LucideIcons.cornerDownLeft, size: 16),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _InputShell extends StatelessWidget {
  final Widget child;

  const _InputShell({super.key, required this.child});

  @override
  Widget build(BuildContext context) => Material(
    color: context.colors.surfaceContainerLow,
    child: SafeArea(
      top: false,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(
          Grid.xxs,
          Grid.half,
          Grid.xxs,
          Grid.half,
        ),
        child: child,
      ),
    ),
  );
}
