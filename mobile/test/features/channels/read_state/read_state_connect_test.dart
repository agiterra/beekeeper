import 'package:beekeeper/shared/community/community_provider.dart';
import 'package:beekeeper/shared/read_state/read_state_provider.dart';
import 'package:beekeeper/shared/relay/relay.dart';
import 'package:beekeeper/shared/theme/theme_provider.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:nostr/nostr.dart' as nostr;
import 'package:shared_preferences/shared_preferences.dart';

import '../../../helpers/recording_relay_session.dart';

/// What a relay connect costs the read-state sync: one coalesced fetch and
/// one live subscription — not one pair from a notifier rebuild plus another
/// pair from the reconnect listener, which is what watching the session
/// state used to produce on every disconnected → connecting → connected
/// step.
void main() {
  late ProviderContainer container;
  late nostr.Keys keys;

  Future<void> settle() async {
    for (var i = 0; i < 8; i++) {
      await Future<void>.delayed(Duration.zero);
    }
  }

  Future<_Relay> pump(SessionStatus initial) async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();
    keys = nostr.Keys.generate();
    final relay = _Relay(initial)..completeHistory(const []);
    container = ProviderContainer(
      overrides: [
        savedPrefsProvider.overrideWithValue(prefs),
        relayConfigProvider.overrideWith(() => _FakeRelayConfig(keys.nsec)),
        relaySessionProvider.overrideWith(() => relay),
        activeCommunityProvider.overrideWith((ref) async => null),
        appLifecycleProvider.overrideWith(_FakeAppLifecycle.new),
      ],
    );
    addTearDown(container.dispose);
    // Resolve the community first so the notifier builds exactly once.
    await container.read(activeCommunityProvider.future);
    container.read(readStateProvider);
    await settle();
    return relay;
  }

  int count(RecordingRelaySessionNotifier relay, String op) =>
      relay.operations.where((entry) => entry == op).length;

  test('a connect costs one coalesced fetch and one subscribe', () async {
    final relay = await pump(SessionStatus.disconnected);
    expect(count(relay, 'query1'), 0, reason: 'nothing to read while down');
    expect(count(relay, 'subscribe'), 0);
    expect(count(relay, 'fetch'), 0);

    relay.setConnected(true);
    await settle();

    expect(count(relay, 'query1'), 1);
    expect(count(relay, 'subscribe'), 1);
    expect(count(relay, 'fetch'), 0, reason: 'the read left the socket');
    final filter = relay.coalescedQueryGroups.single.single;
    expect(filter.kinds, [EventKind.readState]);
    expect(filter.authors, [keys.public]);
    expect(filter.tags['#t'], ['read-state']);
    expect(filter.since, isNotNull);
  });

  test(
    'building while connected reads once; a reconnect reads once more',
    () async {
      final relay = await pump(SessionStatus.connected);
      expect(count(relay, 'query1'), 1);
      expect(count(relay, 'subscribe'), 1);

      relay.setConnected(false);
      await settle();
      relay.setConnected(true);
      await settle();

      expect(count(relay, 'query1'), 2);
      expect(count(relay, 'subscribe'), 2);
    },
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
  RelayConfig build() => RelayConfig(baseUrl: 'http://localhost:1', nsec: nsec);
}

class _FakeAppLifecycle extends AppLifecycleNotifier {
  @override
  AppLifecycleState build() => AppLifecycleState.resumed;
}
