import 'package:buzz/features/channels/channel.dart';
import 'package:buzz/features/channels/channel_actions_sheet.dart';
import 'package:buzz/features/channels/channel_detail_page.dart';
import 'package:buzz/features/channels/channel_management_provider.dart';
import 'package:buzz/features/channels/channel_messages_provider.dart';
import 'package:buzz/features/channels/channel_typing_provider.dart';
import 'package:buzz/features/channels/channels_provider.dart';
import 'package:buzz/features/coding_sessions/ui/coding_sessions_page.dart';
import 'package:buzz/features/profile/profile_provider.dart';
import 'package:buzz/features/profile/user_cache_provider.dart';
import 'package:buzz/features/profile/user_profile.dart';
import 'package:buzz/shared/mentions/agent_identity_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:hooks_riverpod/misc.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'fake_observer.dart';

/// D11(a)/(b): the two ways a member reaches the read-only observer.
///
/// Without these the feature is unreachable from the app and every other test
/// in this directory passes over a page nothing opens.
const _channelId = 'entry-channel';

final _channel = Channel(
  id: _channelId,
  name: 'general',
  channelType: 'stream',
  visibility: 'open',
  description: 'General discussion',
  createdBy: 'owner',
  createdAt: DateTime(2025),
  memberCount: 2,
  isMember: true,
);

/// A two-participant DM: the case `_showsMembersAction` returns false for, so
/// the header action has to stand on its own rather than ride along with
/// Members (D11a).
const _dmChannelId = 'entry-dm';

final _dmChannel = Channel(
  id: _dmChannelId,
  name: 'dm',
  channelType: 'dm',
  visibility: 'private',
  description: '',
  createdBy: 'me',
  createdAt: DateTime(2025),
  memberCount: 2,
  isMember: true,
  participants: const ['me', 'them'],
  participantPubkeys: const ['me', 'them'],
);

late SharedPreferences _prefs;

List<Override> _sharedOverrides(
  FakeObserverBinding binding, {
  String channelId = _channelId,
}) => <Override>[
  fakeObserverOverride(binding),
  currentPubkeyProvider.overrideWith((ref) => 'me'),
  channelMembersProvider(
    channelId,
  ).overrideWith((ref) async => const <ChannelMember>[]),
  agentOwnersProvider.overrideWithValue(
    const AsyncValue<Map<String, String>>.data(<String, String>{}),
  ),
  relayClientProvider.overrideWithValue(
    RelayClient(baseUrl: 'http://localhost:3000'),
  ),
  savedPrefsProvider.overrideWithValue(_prefs),
];

Widget _sheetApp(FakeObserverBinding binding) => ProviderScope(
  overrides: _sharedOverrides(binding),
  child: MaterialApp(
    theme: AppTheme.light(),
    home: Builder(
      builder: (context) => Scaffold(
        body: Center(
          child: TextButton(
            onPressed: () => showChannelActionsSheet(
              context: context,
              channel: _channel,
              isUnread: false,
            ),
            child: const Text('Open actions'),
          ),
        ),
      ),
    ),
  ),
);

Widget _detailApp(FakeObserverBinding binding, {Channel? channel}) {
  final resolved = channel ?? _channel;
  return ProviderScope(
    overrides: [
      ..._sharedOverrides(binding, channelId: resolved.id),
      channelMessagesProvider(
        resolved.id,
      ).overrideWith(() => _FakeMessagesNotifier(resolved.id)),
      channelTypingProvider(
        resolved.id,
      ).overrideWith(() => _FakeTypingNotifier(resolved.id)),
      channelDetailsProvider(
        resolved.id,
      ).overrideWith((ref) async => ChannelDetails.fromChannel(resolved)),
      channelsProvider.overrideWith(() => _FakeChannelsNotifier(resolved)),
      profileProvider.overrideWith(_FakeProfileNotifier.new),
      userCacheProvider.overrideWith(_FakeUserCacheNotifier.new),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: ChannelDetailPage(channel: resolved),
    ),
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    SharedPreferences.setMockInitialValues({});
    _prefs = await SharedPreferences.getInstance();
  });

  testWidgets('the channel actions sheet opens the observer', (tester) async {
    final binding = FakeObserverBinding(testSnapshot(sessions: const []));
    await tester.pumpWidget(_sheetApp(binding));
    await tester.tap(find.text('Open actions'));
    await tester.pumpAndSettle();

    expect(find.text('Coding sessions'), findsOneWidget);

    await tester.tap(find.text('Coding sessions'));
    await tester.pumpAndSettle();

    final page = tester.widget<CodingSessionsPage>(
      find.byType(CodingSessionsPage),
    );
    expect(page.channelId, _channelId);
    expect(page.channelName, 'general');
  });

  testWidgets('the channel header opens the observer beside Members', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot(sessions: const []));
    await tester.pumpWidget(_detailApp(binding));
    await tester.pump();

    final action = find.byKey(const ValueKey('channel-coding-sessions-action'));
    expect(action, findsOneWidget);
    // Beside Members, not instead of it.
    expect(find.byTooltip('View members'), findsOneWidget);

    await tester.tap(action);
    await tester.pumpAndSettle();

    final page = tester.widget<CodingSessionsPage>(
      find.byType(CodingSessionsPage),
    );
    expect(page.channelId, _channelId);
  });

  // A 2-participant DM has no Members action, and the coding-session action
  // used to be nested inside it — so the D11(a) header entry point was absent
  // exactly where the desktop's session DMs live.
  testWidgets('the header action is there in a two-person DM too', (
    tester,
  ) async {
    final binding = FakeObserverBinding(testSnapshot(sessions: const []));
    await tester.pumpWidget(_detailApp(binding, channel: _dmChannel));
    await tester.pump();

    expect(find.byTooltip('View members'), findsNothing);
    final action = find.byKey(const ValueKey('channel-coding-sessions-action'));
    expect(action, findsOneWidget);

    await tester.tap(action);
    await tester.pumpAndSettle();

    final page = tester.widget<CodingSessionsPage>(
      find.byType(CodingSessionsPage),
    );
    expect(page.channelId, _dmChannelId);
    expect(page.channelName, isNull);
  });
}

class _FakeMessagesNotifier extends ChannelMessagesNotifier {
  _FakeMessagesNotifier(super.channelId);

  @override
  AsyncValue<List<NostrEvent>> build() => const AsyncData([]);

  @override
  bool get hasLoadedMessages => true;

  @override
  bool get reachedOldest => true;
}

class _FakeTypingNotifier extends ChannelTypingNotifier {
  _FakeTypingNotifier(super.channelId);

  @override
  List<TypingEntry> build() => const [];
}

class _FakeChannelsNotifier extends ChannelsNotifier {
  _FakeChannelsNotifier(this.channel);

  final Channel channel;

  @override
  Future<List<Channel>> build() async => [channel];
}

class _FakeProfileNotifier extends ProfileNotifier {
  @override
  Future<UserProfile?> build() async =>
      const UserProfile(pubkey: 'me', displayName: 'Me');
}

class _FakeUserCacheNotifier extends UserCacheNotifier {
  @override
  Map<String, UserProfile> build() => const {};
}
