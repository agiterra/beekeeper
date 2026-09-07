import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';

import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/shell_announce_head_provider.dart';
import 'package:buzz/features/terminals/state/shell_observer_provider.dart';
import 'package:buzz/features/terminals/ui/terminal_observe_page.dart';
import 'package:buzz/features/profile/user_cache_provider.dart';
import 'package:buzz/features/profile/user_profile.dart';
import 'package:buzz/shared/relay/relay_provider.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:xterm/xterm.dart';

import '../../../helpers/widget_helpers.dart';
import '../../projects/ui/fake_projects.dart';

class FakeShellObserverNotifier extends ShellObserverNotifier {
  final ShellObserverState initial;
  final StreamController<ShellTerminalWrite> controller;
  int resyncCount = 0;

  FakeShellObserverNotifier(super.target, this.initial, this.controller);

  @override
  ShellObserverState build() => initial;

  @override
  Stream<ShellTerminalWrite> get writes => controller.stream;

  /// Every input the page asked to send, as raw bytes.
  final List<Uint8List> sent = [];

  @override
  Future<void> resync() async {
    resyncCount++;
  }

  @override
  Future<void> sendInput(Uint8List bytes) async {
    sent.add(bytes);
  }

  void setStatus(ShellObserverStatus status) =>
      state = state.copyWith(status: status);

  void setInputRefused(String message) =>
      state = state.copyWith(inputRefused: message);

  void setInputPausedUntil(DateTime until) =>
      state = state.copyWith(inputPausedUntil: until);
}

class FakeShellAnnounceHeadNotifier extends ShellAnnounceHeadNotifier {
  final ShellAnnounceHead head;

  FakeShellAnnounceHeadNotifier(super.target, this.head);

  @override
  ShellAnnounceHead build() => head;
}

Future<
  ({
    FakeShellObserverNotifier observer,
    StreamController<ShellTerminalWrite> writes,
  })
>
_pump(
  WidgetTester tester, {
  ShellObserverStatus status = ShellObserverStatus.connecting,
  ShellAnnounceHead? head,
  String? viewer = testViewer,
}) async {
  final writes = StreamController<ShellTerminalWrite>.broadcast();
  addTearDown(writes.close);
  final target = ShellObserverTarget.of(testTerminal());
  // Riverpod 3's family override is a zero-argument factory; the fakes are
  // handed the one target this test opens.
  final observer = FakeShellObserverNotifier(
    target,
    ShellObserverState(status: status),
    writes,
  );
  await tester.pumpWidget(
    KeyedSubtree(
      key: UniqueKey(),
      child: WidgetHelpers.testable(
        overrides: [
          shellObserverProvider.overrideWith(() => observer),
          shellAnnounceHeadProvider.overrideWith(
            () => FakeShellAnnounceHeadNotifier(
              target,
              head ??
                  ShellAnnounceHead(terminal: testTerminal(), hasRead: true),
            ),
          ),
          myPubkeyProvider.overrideWithValue(viewer),
          userCacheProvider.overrideWith(
            () => FakeUserCacheNotifier({
              testOwner: const UserProfile(
                pubkey: testOwner,
                displayName: 'Andy',
              ),
            }),
          ),
        ],
        child: TerminalObservePage(terminal: testTerminal()),
      ),
    ),
  );
  await tester.pump();
  return (observer: observer, writes: writes);
}

void main() {
  inputBarTests();
  testWidgets('shows whose terminal, the grid, the role and the status', (
    tester,
  ) async {
    await _pump(tester);
    expect(find.text('Andy’s terminal · 24x80'), findsOneWidget);
    expect(find.text('Member — read-only'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('terminal-observe-status-connecting')),
      findsOneWidget,
    );
    expect(find.text('Connecting…'), findsOneWidget);
    expect(find.byType(TerminalView), findsOneWidget);
    final view = tester.widget<TerminalView>(find.byType(TerminalView));
    expect(view.readOnly, isTrue);
    expect(view.hardwareKeyboardOnly, isTrue);
    expect(view.autoResize, isFalse);
    expect(view.terminal.viewWidth, 80);
    expect(view.terminal.viewHeight, 24);
  });

  testWidgets('writes reach the emulator and a resize changes the grid', (
    tester,
  ) async {
    final h = await _pump(tester);
    h.writes.add(const ShellTerminalWrite(text: 'hello from the owner'));
    h.writes.add(
      const ShellTerminalWrite(resize: ShellDims(rows: 40, cols: 100)),
    );
    await tester.pump();
    await tester.pump();

    final view = tester.widget<TerminalView>(find.byType(TerminalView));
    expect(view.terminal.viewWidth, 100);
    expect(view.terminal.viewHeight, 40);
    expect(view.terminal.buffer.getText(), contains('hello from the owner'));
    expect(find.textContaining('40x100'), findsOneWidget);
  });

  testWidgets('the status chip follows the observer', (tester) async {
    final h = await _pump(tester, status: ShellObserverStatus.live);
    expect(find.text('LIVE'), findsOneWidget);
    h.observer.setStatus(ShellObserverStatus.stalled);
    await tester.pump();
    expect(find.text('Not streaming'), findsOneWidget);
    h.observer.setStatus(ShellObserverStatus.ended);
    await tester.pump();
    expect(find.text('Ended'), findsOneWidget);
    expect(find.byKey(const ValueKey('terminal-observe-resync')), findsNothing);
  });

  testWidgets('Refresh asks for a resync', (tester) async {
    final h = await _pump(tester, status: ShellObserverStatus.live);
    await tester.tap(find.byKey(const ValueKey('terminal-observe-resync')));
    await tester.pump();
    expect(h.observer.resyncCount, 1);
  });

  testWidgets('the owner\'s own key, a roster role, and a closed head each '
      'read differently', (tester) async {
    // The viewer's own terminal: no role line, the title says whose it is,
    // and the input bar is there.
    await _pump(tester, viewer: testOwner);
    expect(find.byKey(const ValueKey('terminal-observe-role')), findsNothing);
    expect(find.textContaining('Your terminal · 24x80'), findsOneWidget);
    expect(find.byKey(const ValueKey('terminal-input-bar')), findsOneWidget);

    await _pump(
      tester,
      head: ShellAnnounceHead(
        terminal: testTerminal(
          roster: const [
            ShellRosterEntry(
              pubkey: testViewer,
              role: ShellRosterRole.collaborator,
            ),
          ],
        ),
        hasRead: true,
      ),
    );
    expect(
      find.text('Collaborator — your keystrokes go to this terminal'),
      findsOneWidget,
    );

    await _pump(
      tester,
      head: const ShellAnnounceHead(terminal: null, hasRead: true),
    );
    expect(
      find.text('The owner closed or unshared this terminal'),
      findsOneWidget,
    );
  });
}

void inputBarTests() {
  ShellAnnounceHead collaboratorHead() => ShellAnnounceHead(
    terminal: testTerminal(
      roster: const [
        ShellRosterEntry(
          pubkey: testViewer,
          role: ShellRosterRole.collaborator,
        ),
      ],
    ),
    hasRead: true,
  );

  testWidgets('a viewer or member gets no input bar; a collaborator does', (
    tester,
  ) async {
    await _pump(tester, status: ShellObserverStatus.live);
    expect(find.byKey(const ValueKey('terminal-input-bar')), findsNothing);
    final readOnly = tester.widget<TerminalView>(find.byType(TerminalView));
    expect(readOnly.readOnly, isTrue);

    await _pump(
      tester,
      status: ShellObserverStatus.live,
      head: collaboratorHead(),
    );
    expect(find.byKey(const ValueKey('terminal-input-bar')), findsOneWidget);
    expect(
      find.text('Collaborator — your keystrokes go to this terminal'),
      findsOneWidget,
    );
    final typable = tester.widget<TerminalView>(find.byType(TerminalView));
    expect(typable.readOnly, isFalse);
    expect(typable.hardwareKeyboardOnly, isTrue);
  });

  testWidgets('the owner\'s own key may type from another device', (
    tester,
  ) async {
    await _pump(tester, status: ShellObserverStatus.live, viewer: testOwner);
    expect(find.byKey(const ValueKey('terminal-input-bar')), findsOneWidget);
  });

  testWidgets('a line is sent with Enter as one event; keys send their '
      'sequences; nothing is echoed locally', (tester) async {
    final h = await _pump(
      tester,
      status: ShellObserverStatus.live,
      head: collaboratorHead(),
    );
    await tester.enterText(
      find.byKey(const ValueKey('terminal-input-field')),
      'echo hi',
    );
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('terminal-input-send')));
    await tester.pump();
    expect(h.observer.sent.map(utf8.decode), ['echo hi\r']);
    final field = tester.widget<TextField>(
      find.byKey(const ValueKey('terminal-input-field')),
    );
    expect(field.controller!.text, isEmpty);
    // Enter keeps the keyboard: the field has focus again for the next line.
    expect(field.focusNode!.hasFocus, isTrue);
    final view = tester.widget<TerminalView>(find.byType(TerminalView));
    expect(view.terminal.buffer.getText().trim(), isEmpty);

    await tester.tap(find.byKey(const ValueKey('terminal-key-^C')));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('terminal-key-↑')));
    await tester.pump();
    expect(h.observer.sent.length, 3);
    expect(h.observer.sent[1], [0x03]);
    expect(utf8.decode(h.observer.sent[2]), '\x1b[A');
  });

  testWidgets('a refusal replaces the bar with the relay\'s words; a pause '
      'disables it and says so', (tester) async {
    final h = await _pump(
      tester,
      status: ShellObserverStatus.live,
      head: collaboratorHead(),
    );
    h.observer.setInputRefused('restricted: not a collaborator');
    await tester.pump();
    expect(
      find.byKey(const ValueKey('terminal-input-refused')),
      findsOneWidget,
    );
    expect(
      find.text('Input refused — restricted: not a collaborator'),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('terminal-input-field')), findsNothing);

    final paused = await _pump(
      tester,
      status: ShellObserverStatus.live,
      head: collaboratorHead(),
    );
    paused.observer.setInputPausedUntil(
      DateTime.now().add(const Duration(seconds: 9)),
    );
    await tester.pump();
    expect(find.byKey(const ValueKey('terminal-input-paused')), findsOneWidget);
    final field = tester.widget<TextField>(
      find.byKey(const ValueKey('terminal-input-field')),
    );
    expect(field.enabled, isFalse);
  });
}
