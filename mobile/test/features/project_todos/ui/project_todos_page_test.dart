import 'package:buzz/features/profile/user_cache_provider.dart';
import 'package:buzz/features/profile/user_profile.dart';
import 'package:buzz/features/project_todos/domain/project_todo_op.dart';
import 'package:buzz/features/project_todos/state/project_todo_actions.dart';
import 'package:buzz/features/project_todos/state/project_todos_provider.dart';
import 'package:buzz/features/project_todos/ui/project_todos_page.dart';
import 'package:buzz/features/projects/state/projects_provider.dart';
import 'package:buzz/shared/mentions/agent_identity_provider.dart';
import 'package:buzz/shared/relay/relay_provider.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../projects/ui/fake_projects.dart';
import '../fake_project_todos.dart';

/// The page over a fixed fold: what it shows, and which op each control
/// asks for. The fold itself is pinned by the conformance vectors; these
/// tests are about rendering and intent.
void main() {
  late FakeProjectTodoActions actions;

  Future<void> pump(
    WidgetTester tester, {
    required ProjectTodosRead read,
    Map<String, UserProfile> users = const {},
    Set<String> agents = const {},
    String? initialListId,
  }) async {
    actions = FakeProjectTodoActions();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          projectTodosProvider.overrideWith(
            () => FakeProjectTodosNotifier(todoAddress, read),
          ),
          projectTodoActionsProvider.overrideWith((ref, address) => actions),
          projectsProvider.overrideWith(
            () => FakeProjectsNotifier(testProjectsRead([testProject()])),
          ),
          userCacheProvider.overrideWith(() => FakeUserCacheNotifier(users)),
          knownAgentPubkeysProvider.overrideWith((ref) => agents),
          myPubkeyProvider.overrideWithValue(todoViewer),
        ],
        child: MaterialApp(
          theme: AppTheme.light(),
          home: ProjectTodosPage(
            address: todoAddress,
            initialListId: initialListId,
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  final launch = testTodoList(
    open: [
      testTodoItem('a1', text: 'Write the NIP', rank: 'a0'),
      testTodoItem(
        'a2',
        text: 'Ship the relay',
        rank: 'a1',
        assignee: todoAgent,
        due: '2020-01-01',
      ),
      testTodoItem(
        'a3',
        text: 'Tell Andy',
        rank: 'a2',
        assignee: todoOwner,
        due: '2999-12-31',
      ),
    ],
    completed: [
      testTodoItem(
        'd1',
        text: 'Pick a kind',
        done: true,
        completedAt: 500,
        completedBy: todoOwner,
      ),
      testTodoItem(
        'd2',
        text: 'Write the contract',
        done: true,
        completedAt: 400,
        completedBy: todoViewer,
      ),
    ],
  );

  testWidgets('shows open items in rank order, then completed most recent '
      'first, with due and assignee chips', (tester) async {
    await pump(
      tester,
      read: testTodosRead(lists: [launch]),
      users: const {
        todoOwner: UserProfile(pubkey: todoOwner, displayName: 'Brian'),
        todoAgent: UserProfile(
          pubkey: todoAgent,
          displayName: 'Tank',
          ownerPubkey: todoOwner,
        ),
      },
    );

    expect(find.text('Beekeeper · To-do'), findsOneWidget);
    expect(find.text('Open · 3'), findsOneWidget);
    expect(find.text('Completed · 2'), findsOneWidget);

    double top(String key) => tester.getTopLeft(find.byKey(ValueKey(key))).dy;
    expect(
      top('todo-open-${itemId('a1')}') < top('todo-open-${itemId('a2')}'),
      isTrue,
    );
    expect(
      top('todo-open-${itemId('a2')}') < top('todo-open-${itemId('a3')}'),
      isTrue,
    );
    expect(
      top('todo-open-${itemId('a3')}') < top('todo-done-${itemId('d1')}'),
      isTrue,
    );
    expect(
      top('todo-done-${itemId('d1')}') < top('todo-done-${itemId('d2')}'),
      isTrue,
    );

    // The agent assignee gets the bot glyph; the person gets a name.
    final agentChip = find.byKey(ValueKey('todo-assignee-${itemId('a2')}'));
    expect(
      find.descendant(
        of: agentChip,
        matching: find.byKey(const ValueKey('todo-assignee-bot')),
      ),
      findsOneWidget,
    );
    expect(find.text('Tank'), findsOneWidget);
    final personChip = find.byKey(ValueKey('todo-assignee-${itemId('a3')}'));
    expect(
      find.descendant(
        of: personChip,
        matching: find.byKey(const ValueKey('todo-assignee-bot')),
      ),
      findsNothing,
    );
    expect(find.text('Brian'), findsOneWidget);

    // A past due date on an open item is overdue, in the error colour.
    final overdue = find.byKey(ValueKey('todo-due-${itemId('a2')}'));
    expect(
      find.descendant(
        of: overdue,
        matching: find.byKey(const ValueKey('todo-due-overdue')),
      ),
      findsOneWidget,
    );
    expect(find.textContaining('overdue'), findsOneWidget);
    final overdueText = tester.widget<Text>(
      find.descendant(of: overdue, matching: find.byType(Text)),
    );
    expect(overdueText.style?.color, AppTheme.light().colorScheme.error);
    final upcoming = find.byKey(ValueKey('todo-due-${itemId('a3')}'));
    expect(
      find.descendant(
        of: upcoming,
        matching: find.byKey(const ValueKey('todo-due-upcoming')),
      ),
      findsOneWidget,
    );
    expect(find.text('Dec 31, 2999'), findsOneWidget);

    // Completed rows are struck through and checked.
    final doneText = tester.widget<Text>(find.text('Pick a kind'));
    expect(doneText.style?.decoration, TextDecoration.lineThrough);
    final doneBox = tester.widget<Checkbox>(
      find.byKey(ValueKey('todo-checkbox-${itemId('d1')}')),
    );
    expect(doneBox.value, isTrue);
    expect(find.byKey(const ValueKey('todo-notice-ignored')), findsNothing);
    expect(find.byKey(const ValueKey('todo-notice-truncated')), findsNothing);
  });

  testWidgets('the checkbox asks for item.done; undoing a completed item '
      'asks for done=false', (tester) async {
    await pump(tester, read: testTodosRead(lists: [launch]));
    await tester.tap(find.byKey(ValueKey('todo-checkbox-${itemId('a1')}')));
    await tester.pumpAndSettle();
    expect(actions.calls, ['setDone $listA ${itemId('a1')} true']);

    await tester.tap(find.byKey(ValueKey('todo-checkbox-${itemId('d2')}')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'setDone $listA ${itemId('d2')} false');
  });

  testWidgets('the add field asks for item.add on the shown list', (
    tester,
  ) async {
    await pump(tester, read: testTodosRead(lists: [launch]));
    await tester.enterText(
      find.byKey(const ValueKey('todo-add-field')),
      '  Land it  ',
    );
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pumpAndSettle();
    expect(actions.calls, ['addItem $listA Land it']);
    expect(
      tester
          .widget<TextField>(find.byKey(const ValueKey('todo-add-field')))
          .controller
          ?.text,
      '',
    );

    // Blank input asks for nothing.
    await tester.enterText(find.byKey(const ValueKey('todo-add-field')), '  ');
    await tester.tap(find.byKey(const ValueKey('todo-add-submit')));
    await tester.pumpAndSettle();
    expect(actions.calls.length, 1);
  });

  testWidgets('a refusal is shown verbatim, and the controls stay', (
    tester,
  ) async {
    await pump(tester, read: testTodosRead(lists: [launch]));
    actions.failure = Exception(
      'restricted: viewers may not write project to-dos',
    );
    await tester.tap(find.byKey(ValueKey('todo-checkbox-${itemId('a1')}')));
    await tester.pumpAndSettle();
    expect(
      find.text('restricted: viewers may not write project to-dos'),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('todo-add-field')), findsOneWidget);
  });

  testWidgets('ignored and truncated reads are disclosed', (tester) async {
    await pump(
      tester,
      read: testTodosRead(lists: [launch], ignored: 2, truncated: true),
    );
    expect(find.byKey(const ValueKey('todo-notice-ignored')), findsOneWidget);
    expect(
      find.textContaining('2 changes could not be applied'),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('todo-notice-truncated')), findsOneWidget);
    expect(find.byKey(const ValueKey('todo-notice-error')), findsNothing);
  });

  testWidgets('a read error is disclosed verbatim', (tester) async {
    await pump(
      tester,
      read: testTodosRead(
        lists: const [],
        error: 'To-do history read failed: Exception: timed out',
      ),
    );
    expect(
      find.text('To-do history read failed: Exception: timed out'),
      findsOneWidget,
    );
  });

  testWidgets('no lists: an empty state with a New list door; the sheet '
      'offers Project or Personal and a pin switch, and asks for '
      'list.create with the choice', (tester) async {
    await pump(tester, read: testTodosRead(lists: const []));
    expect(find.byKey(const ValueKey('todo-empty')), findsOneWidget);
    expect(find.text('No to-do lists yet'), findsOneWidget);
    expect(find.byKey(const ValueKey('todo-list-picker')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('todo-empty-new-list')));
    await tester.pumpAndSettle();
    // The choice, with its one-line explanations and the "fixed" warning.
    expect(find.byKey(const ValueKey('todo-new-list-project')), findsOneWidget);
    expect(
      find.byKey(const ValueKey('todo-new-list-personal')),
      findsOneWidget,
    );
    expect(
      find.text('Every project member reads and edits it.'),
      findsOneWidget,
    );
    expect(
      find.text('Only you. The relay withholds it from everyone else.'),
      findsOneWidget,
    );
    expect(
      find.text('This cannot be changed later; make a new list instead.'),
      findsOneWidget,
    );
    // Defaults: project, pinned.
    expect(
      tester
          .widget<SwitchListTile>(
            find.byKey(const ValueKey('todo-new-list-pinned')),
          )
          .value,
      isTrue,
    );
    await tester.enterText(
      find.byKey(const ValueKey('todo-list-title-field')),
      'Launch',
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-list-title-save')));
    await tester.pumpAndSettle();
    expect(actions.calls, ['createList Launch project pinned=true']);

    // Personal and unpinned, when chosen.
    await tester.tap(find.byKey(const ValueKey('todo-empty-new-list')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-new-list-personal')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-new-list-pinned')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const ValueKey('todo-list-title-field')),
      'Mine',
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-list-title-save')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'createList Mine personal pinned=false');
  });

  testWidgets('a personal list shows a lock and a pinned one a pin in the '
      'picker; the menu asks for list.pinned', (tester) async {
    final mine = testTodoList(
      id: listB,
      title: 'Mine',
      visibility: TodoVisibility.personal,
      pinned: true,
      createdAt: 200,
    );
    await pump(tester, read: testTodosRead(lists: [launch, mine]));
    // Launch is shown first: project, unpinned — no glyphs.
    expect(find.byKey(ValueKey('todo-list-personal-$listA')), findsNothing);
    expect(find.byKey(ValueKey('todo-list-pinned-$listA')), findsNothing);

    await tester.tap(find.byKey(const ValueKey('todo-list-picker')));
    await tester.pumpAndSettle();
    expect(find.byKey(ValueKey('todo-list-personal-$listB')), findsWidgets);
    expect(find.byKey(ValueKey('todo-list-pinned-$listB')), findsWidgets);
    expect(find.byIcon(LucideIcons.lock), findsWidgets);
    await tester.tap(find.text('Mine').last);
    await tester.pumpAndSettle();
    // The closed picker keeps the glyphs on the selected personal list.
    expect(find.byKey(ValueKey('todo-list-personal-$listB')), findsOneWidget);
    expect(find.byKey(ValueKey('todo-list-pinned-$listB')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('todo-list-menu')));
    await tester.pumpAndSettle();
    expect(find.text('Unpin from project tree'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('todo-menu-pin')));
    await tester.pumpAndSettle();
    expect(actions.calls, ['setListPinned $listB false']);

    // Back on the unpinned list, the menu offers to pin.
    await tester.tap(find.byKey(const ValueKey('todo-list-picker')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Launch').last);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-list-menu')));
    await tester.pumpAndSettle();
    expect(find.text('Pin to project tree'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('todo-menu-pin')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'setListPinned $listA true');
  });

  testWidgets('initialListId selects that list first', (tester) async {
    final second = testTodoList(
      id: listB,
      title: 'Later',
      createdAt: 200,
      open: [testTodoItem('b1', listId: listB, text: 'Second list item')],
    );
    await pump(
      tester,
      read: testTodosRead(lists: [launch, second]),
      initialListId: listB,
    );
    expect(find.text('Second list item'), findsOneWidget);
    expect(find.text('Write the NIP'), findsNothing);
  });

  testWidgets('the picker switches lists; archived lists hide until shown', (
    tester,
  ) async {
    final second = testTodoList(
      id: listB,
      title: 'Later',
      createdAt: 200,
      open: [testTodoItem('b1', listId: listB, text: 'Second list item')],
    );
    final archived = testTodoList(
      id: 'cccccccccccccccccccccccccccccccc',
      title: 'Old',
      archived: true,
      createdAt: 300,
    );
    await pump(tester, read: testTodosRead(lists: [launch, second, archived]));
    expect(find.text('Write the NIP'), findsOneWidget);
    expect(find.text('Second list item'), findsNothing);

    await tester.tap(find.byKey(const ValueKey('todo-list-picker')));
    await tester.pumpAndSettle();
    expect(find.text('Old (archived)'), findsNothing);
    await tester.tap(find.text('Later').last);
    await tester.pumpAndSettle();
    expect(find.text('Second list item'), findsOneWidget);
    expect(find.text('Write the NIP'), findsNothing);

    await tester.tap(find.byKey(const ValueKey('todo-list-menu')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-menu-show-archived')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('todo-list-picker')));
    await tester.pumpAndSettle();
    expect(find.text('Old (archived)'), findsOneWidget);
    await tester.tap(find.text('Old (archived)').last);
    await tester.pumpAndSettle();
    expect(
      find.text('Nothing here yet. Add the first item above.'),
      findsOneWidget,
    );

    // Archive actions ask for list.archived on the shown list.
    await tester.tap(find.byKey(const ValueKey('todo-list-menu')));
    await tester.pumpAndSettle();
    expect(find.text('Unarchive list'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('todo-menu-archive')));
    await tester.pumpAndSettle();
    expect(actions.calls, ['setListArchived ${archived.id} false']);
  });

  testWidgets('tapping a row opens the edit sheet; its controls ask for one '
      'op each', (tester) async {
    await pump(
      tester,
      read: testTodosRead(lists: [launch]),
      users: const {
        todoAgent: UserProfile(
          pubkey: todoAgent,
          displayName: 'Tank',
          ownerPubkey: todoOwner,
        ),
      },
    );
    await tester.tap(find.text('Ship the relay'));
    await tester.pumpAndSettle();
    expect(find.text('Edit item'), findsOneWidget);
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('todo-item-assignee')),
        matching: find.byIcon(LucideIcons.bot),
      ),
      findsOneWidget,
    );
    expect(find.text('Tank'), findsWidgets);

    // Text: the save control is inert until the draft changes.
    final save = find.byKey(const ValueKey('todo-item-text-save'));
    expect(tester.widget<IconButton>(save).onPressed, isNull);
    await tester.enterText(
      find.byKey(const ValueKey('todo-item-text-field')),
      'Ship the relay today',
    );
    await tester.pumpAndSettle();
    await tester.tap(save);
    await tester.pumpAndSettle();
    expect(actions.calls, [
      'setText $listA ${itemId('a2')} Ship the relay today',
    ]);

    // Due: clearing asks for due=null.
    await tester.tap(find.byKey(const ValueKey('todo-item-due-clear')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'setDue $listA ${itemId('a2')} null');

    // Assignee: the sheet lists people and agents; "Unassigned" clears.
    await tester.tap(find.byKey(const ValueKey('todo-item-assignee')));
    await tester.pumpAndSettle();
    expect(find.text('Assign to'), findsOneWidget);
    expect(find.text('Agents'), findsOneWidget);
    expect(find.text('People'), findsOneWidget);
    expect(
      find.byKey(const ValueKey('todo-assignee-option-$todoViewer')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(const ValueKey('todo-assignee-option-none')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'setAssignee $listA ${itemId('a2')} null');

    // Remove asks once, then for item.remove.
    await tester.tap(find.byKey(const ValueKey('todo-item-remove')));
    await tester.pumpAndSettle();
    expect(find.text('Remove this item?'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('todo-item-remove-confirm')));
    await tester.pumpAndSettle();
    expect(actions.calls.last, 'removeItem $listA ${itemId('a2')}');
  });

  testWidgets('a drag asks for one item.rank move', (tester) async {
    await pump(tester, read: testTodosRead(lists: [launch]));
    final handle = find.byKey(ValueKey('todo-drag-${itemId('a1')}'));
    final target = tester.getCenter(
      find.byKey(ValueKey('todo-open-${itemId('a3')}')),
    );
    final gesture = await tester.startGesture(tester.getCenter(handle));
    await tester.pump(const Duration(milliseconds: 100));
    await gesture.moveTo(target + const Offset(0, 20));
    await tester.pump(const Duration(milliseconds: 100));
    await gesture.up();
    await tester.pumpAndSettle();
    // Where exactly the drop lands among the lower rows depends on the
    // gesture; that it is one move of the first row downward, in
    // ReorderableListView terms, is the contract (the rank math is pinned
    // in the actions test).
    expect(actions.calls.length, 1);
    expect(
      actions.calls.single,
      anyOf('moveItem $listA 0 2', 'moveItem $listA 0 3'),
    );
  });

  testWidgets('pull to refresh asks the notifier to read again', (
    tester,
  ) async {
    await pump(tester, read: testTodosRead(lists: [launch]));
    await tester.fling(find.text('Open · 3'), const Offset(0, 400), 1200);
    await tester.pumpAndSettle();
    final notifier =
        ProviderScope.containerOf(
              tester.element(find.byType(ProjectTodosPage)),
            ).read(projectTodosProvider(todoAddress).notifier)
            as FakeProjectTodosNotifier;
    expect(notifier.refreshCount, 1);
  });
}
