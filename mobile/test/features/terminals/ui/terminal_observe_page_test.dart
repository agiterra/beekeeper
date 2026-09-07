import 'dart:async';

import 'package:buzz/features/terminals/domain/terminals_domain.dart';
import 'package:buzz/features/terminals/state/shell_announce_head_provider.dart';
import 'package:buzz/features/terminals/state/shell_observer_provider.dart';
import 'package:buzz/features/terminals/ui/terminal_observe_page.dart';
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

  @override
  Future<void> resync() async {
    resyncCount++;
  }

  void setStatus(ShellObserverStatus status) =>
      state = state.copyWith(status: status);
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
        ],
        child: TerminalObservePage(terminal: testTerminal()),
      ),
    ),
  );
  await tester.pump();
  return (observer: observer, writes: writes);
}

void main() {
  testWidgets('shows whose terminal, the grid, the role and the status', (
    tester,
  ) async {
    await _pump(tester);
    expect(find.textContaining('’s terminal · 24x80'), findsOneWidget);
    expect(find.text('Member — read-only on this device'), findsOneWidget);
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
    await _pump(tester, viewer: testOwner);
    expect(
      find.text('Yours, from another device — read-only on this device'),
      findsOneWidget,
    );

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
      find.text('Collaborator — read-only on this device'),
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
