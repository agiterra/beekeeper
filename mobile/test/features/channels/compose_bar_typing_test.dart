import 'package:buzz/features/channels/channel.dart';
import 'package:buzz/features/channels/channel_management_provider.dart';
import 'package:buzz/features/channels/channels_provider.dart';
import 'package:buzz/features/channels/compose_bar.dart';
import 'package:buzz/features/channels/photo_library.dart';
import 'package:buzz/shared/custom_emoji/custom_emoji_provider.dart';
import 'package:buzz/shared/mentions/agent_identity_provider.dart';
import 'package:buzz/shared/relay/relay.dart';
import 'package:buzz/shared/theme/theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:image_picker/image_picker.dart';
import 'package:nostr/nostr.dart' as nostr;
import 'package:shared_preferences/shared_preferences.dart';

import '../../helpers/recording_relay_session.dart';

/// The typing indicator (kind:20002) is the cheapest frame the composer
/// sends and the first one worth losing: it goes out through
/// [RelaySessionNotifier.sendEphemeral], only while the session is
/// connected, and a dropped one does not start the 3 s throttle — the next
/// keystroke offers it again.
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late SharedPreferences prefs;

  setUp(() async {
    SharedPreferences.setMockInitialValues({});
    prefs = await SharedPreferences.getInstance();
  });

  Future<void> type(WidgetTester tester, String text) async {
    await tester.enterText(find.byType(TextField), text);
    await tester.pump();
  }

  testWidgets('typing sends one droppable kind:20002 per throttle window', (
    tester,
  ) async {
    final relay = _Relay(SessionStatus.connected);
    await tester.pumpWidget(_harness(prefs, relay));
    await _expandComposer(tester);

    await type(tester, 'h');
    expect(relay.operations, ['ephemeral']);
    final event = relay.ephemeralEvents.single;
    expect(event.kind, EventKind.typingIndicator);
    expect(event.tags, [
      ['h', 'channel-1'],
    ]);
    expect(event.sig, isNotEmpty);

    // Inside the throttle window the next keystroke sends nothing.
    await type(tester, 'he');
    expect(relay.operations, ['ephemeral']);
    expect(relay.published, isEmpty, reason: 'never a publish');
  });

  testWidgets('a dropped indicator is offered again on the next keystroke', (
    tester,
  ) async {
    final relay = _Relay(SessionStatus.connected)..acceptEphemeral = false;
    await tester.pumpWidget(_harness(prefs, relay));
    await _expandComposer(tester);

    await type(tester, 'h');
    await type(tester, 'he');

    // Two offers, both refused by the transport (gate active or lane dry):
    // the throttle never started, and nothing fell back to a publish.
    expect(relay.operations, ['ephemeral', 'ephemeral']);
    expect(relay.published, isEmpty);
  });

  testWidgets('nothing is sent while the session is not connected', (
    tester,
  ) async {
    final relay = _Relay(SessionStatus.reconnecting);
    await tester.pumpWidget(_harness(prefs, relay));
    await _expandComposer(tester);

    await type(tester, 'h');
    await type(tester, 'he');

    expect(relay.operations, isEmpty);
  });
}

Future<void> _expandComposer(WidgetTester tester) async {
  await tester.pumpAndSettle();
  if (find.byType(TextField).evaluate().isNotEmpty) return;
  await tester.tap(find.text('Message…'));
  await tester.pumpAndSettle();
}

Widget _harness(SharedPreferences prefs, _Relay relay) {
  final signer = nostr.Keys.generate();
  return ProviderScope(
    overrides: [
      customEmojiListProvider.overrideWithValue(const []),
      mediaUploadServiceProvider.overrideWithValue(
        MediaUploadService(
          baseUrl: 'https://relay.example',
          nsec: signer.nsec,
          pickGalleryImage: () async => null,
          pickGalleryVideo: () async => null,
        ),
      ),
      photoLibraryProvider.overrideWithValue(const _EmptyPhotoLibrary()),
      currentPubkeyProvider.overrideWith((ref) => signer.public),
      channelMembersProvider(
        'channel-1',
      ).overrideWith((ref) async => const <ChannelMember>[]),
      agentDirectoryProvider.overrideWith((ref) async => const []),
      agentOwnersProvider.overrideWith((ref) async => const <String, String>{}),
      relayClientProvider.overrideWithValue(
        RelayClient(baseUrl: 'http://localhost:3000'),
      ),
      relayConfigProvider.overrideWith(() => _FakeRelayConfig(signer.nsec)),
      relaySessionProvider.overrideWith(() => relay),
      savedPrefsProvider.overrideWithValue(prefs),
      channelsProvider.overrideWith(_FakeChannelsNotifier.new),
    ],
    child: MaterialApp(
      theme: AppTheme.light(),
      home: Scaffold(
        body: SafeArea(
          child: Align(
            alignment: Alignment.bottomCenter,
            child: ComposeBar(
              channelId: 'channel-1',
              onSend: (_, _, {mediaTags = const <List<String>>[]}) async {},
            ),
          ),
        ),
      ),
    ),
  );
}

class _Relay extends RecordingRelaySessionNotifier {
  _Relay(this._initial);

  final SessionStatus _initial;

  @override
  SessionState build() => SessionState(status: _initial);
}

class _FakeRelayConfig extends RelayConfigNotifier {
  _FakeRelayConfig(this.nsec);

  final String nsec;

  @override
  RelayConfig build() =>
      RelayConfig(baseUrl: 'http://localhost:3000', nsec: nsec);
}

class _FakeChannelsNotifier extends ChannelsNotifier {
  @override
  Future<List<Channel>> build() async => [
    Channel(
      id: 'channel-1',
      name: 'current',
      channelType: 'stream',
      visibility: 'open',
      description: '',
      createdBy: 'pubkey123',
      createdAt: DateTime(2024),
      memberCount: 2,
      isMember: true,
    ),
  ];

  @override
  List<ChannelMember> cachedMembersForChannel(String channelId) => const [];

  @override
  Future<void> refresh() async {}
}

class _EmptyPhotoLibrary implements PhotoLibrary {
  const _EmptyPhotoLibrary();

  @override
  Future<List<RecentPhoto>> loadRecentPhotos() async => const [];

  @override
  Future<List<XFile>> resolveSelectedPhotos(List<RecentPhoto> photos) async =>
      const [];
}
