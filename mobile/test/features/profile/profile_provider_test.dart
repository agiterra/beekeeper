import 'package:beekeeper/features/profile/profile_provider.dart';
import 'package:beekeeper/features/profile/user_profile.dart';
import 'package:beekeeper/shared/relay/relay.dart';
import 'package:beekeeper/shared/theme/theme.dart';
import 'package:fake_async/fake_async.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hooks_riverpod/hooks_riverpod.dart';
import 'package:nostr/nostr.dart' as nostr;
import 'package:shared_preferences/shared_preferences.dart';

import '../../helpers/recording_relay_session.dart';

void main() {
  test(
    'manual presence persists until Online restores automatic mode',
    () async {
      SharedPreferences.setMockInitialValues({});
      final prefs = await SharedPreferences.getInstance();
      var container = _buildContainer(prefs);

      expect(
        await container
            .read(presenceProvider.future)
            .timeout(
              const Duration(seconds: 2),
              onTimeout: () =>
                  throw StateError('initial presence did not resolve'),
            ),
        'online',
      );
      await container
          .read(presenceProvider.notifier)
          .setPresence('away')
          .timeout(
            const Duration(seconds: 2),
            onTimeout: () => throw StateError('setting Away did not resolve'),
          );
      expect(container.read(presenceProvider).value, 'away');
      expect(prefs.getString('buzz_presence_preference_aabb'), 'away');

      container.dispose();
      container = _buildContainer(prefs);
      addTearDown(container.dispose);
      expect(
        await container
            .read(presenceProvider.future)
            .timeout(
              const Duration(seconds: 2),
              onTimeout: () =>
                  throw StateError('stored presence did not resolve'),
            ),
        'away',
      );

      await container
          .read(presenceProvider.notifier)
          .setPresence('online')
          .timeout(
            const Duration(seconds: 2),
            onTimeout: () => throw StateError('setting Online did not resolve'),
          );
      expect(container.read(presenceProvider).value, 'online');
      expect(prefs.getString('buzz_presence_preference_aabb'), 'auto');
    },
  );

  group('heartbeat', () {
    test('beats are droppable ephemerals, never publishes', () async {
      SharedPreferences.setMockInitialValues({});
      final prefs = await SharedPreferences.getInstance();
      final relay = RecordingRelaySessionNotifier();
      fakeAsync((async) {
        final container = _buildHeartbeatContainer(prefs, relay);
        addTearDown(container.dispose);

        expect(
          _resolve(async, container.read(presenceProvider.future)),
          'online',
        );
        // The status change itself is a publish the notifier waits on.
        expect(relay.published.map((event) => event.kind), [20001]);
        expect(relay.published.single.content, 'online');

        async.elapse(const Duration(milliseconds: 120));
        final beats = relay.ephemeralEvents;
        expect(beats.length, greaterThanOrEqualTo(2));
        for (final beat in beats) {
          expect(beat.kind, EventKind.presenceUpdate);
          expect(beat.content, 'online');
          expect(beat.sig, isNotEmpty);
        }
        expect(relay.published, hasLength(1), reason: 'no beat is a publish');

        // Under the gate or a thin write lane the transport answers false and
        // sends nothing; the notifier must not fall back to a publish.
        relay.acceptEphemeral = false;
        final offered = relay.ephemeralEvents.length;
        async.elapse(const Duration(milliseconds: 80));
        expect(relay.ephemeralEvents.length, greaterThan(offered));
        expect(relay.published, hasLength(1));
      });
    });

    test('no beat while the session is not connected', () async {
      SharedPreferences.setMockInitialValues({});
      final prefs = await SharedPreferences.getInstance();
      final relay = RecordingRelaySessionNotifier();
      fakeAsync((async) {
        final container = _buildHeartbeatContainer(prefs, relay);
        addTearDown(container.dispose);

        expect(
          _resolve(async, container.read(presenceProvider.future)),
          'online',
        );
        relay.setConnected(false);
        async.elapse(const Duration(milliseconds: 120));

        expect(relay.ephemeralEvents, isEmpty);
        expect(relay.published, hasLength(1));
      });
    });
  });
}

/// Resolves [work] in virtual time and returns its value.
///
/// Both heartbeat tests run inside [fakeAsync] because the beat they assert
/// on is a 20 ms `Timer.periodic` (`_buildHeartbeatContainer`) whose first
/// beat is a randomly phased `Timer` (`PresenceNotifier._startHeartbeat`).
/// Waiting on it with a real `Future.delayed(120ms)` is a race the wall
/// clock decides — the same shape as the keepalive race in ledger item 208,
/// and one the full mobile suite's starved isolates lose. `async.elapse`
/// fires those timers deterministically; nothing on this path reads a raw
/// `DateTime.now()`, so virtual time is enough. Item 217.
T _resolve<T>(FakeAsync async, Future<T> work, {int rounds = 6}) {
  late T value;
  var done = false;
  Object? failure;
  work.then<void>((result) {
    value = result;
    done = true;
  }, onError: (Object error) => failure = error);
  for (var i = 0; i < rounds; i++) {
    async.elapse(Duration.zero);
  }
  if (failure != null) throw failure!;
  expect(done, isTrue, reason: 'the future never completed in virtual time');
  return value;
}

ProviderContainer _buildHeartbeatContainer(
  SharedPreferences prefs,
  RecordingRelaySessionNotifier relay,
) => ProviderContainer(
  overrides: [
    savedPrefsProvider.overrideWithValue(prefs),
    myPubkeyProvider.overrideWithValue('aabb'),
    profileProvider.overrideWith(_FakeProfileNotifier.new),
    relaySessionProvider.overrideWith(() => relay),
    relayConfigProvider.overrideWith(
      () => _FakeRelayConfig(nostr.Keys.generate().nsec),
    ),
    appLifecycleProvider.overrideWith(_ResumedLifecycle.new),
    presenceProvider.overrideWith(
      () =>
          PresenceNotifier(heartbeatInterval: const Duration(milliseconds: 20)),
    ),
  ],
);

class _FakeRelayConfig extends RelayConfigNotifier {
  _FakeRelayConfig(this.nsec);

  final String nsec;

  @override
  RelayConfig build() => RelayConfig(baseUrl: 'http://localhost:1', nsec: nsec);
}

ProviderContainer _buildContainer(SharedPreferences prefs) => ProviderContainer(
  overrides: [
    savedPrefsProvider.overrideWithValue(prefs),
    myPubkeyProvider.overrideWithValue('aabb'),
    profileProvider.overrideWith(_FakeProfileNotifier.new),
    relaySessionProvider.overrideWith(_DisconnectedRelaySession.new),
    appLifecycleProvider.overrideWith(_ResumedLifecycle.new),
  ],
);

class _FakeProfileNotifier extends ProfileNotifier {
  @override
  Future<UserProfile?> build() async =>
      const UserProfile(pubkey: 'aabb', displayName: 'Test');
}

class _DisconnectedRelaySession extends RelaySessionNotifier {
  @override
  SessionState build() =>
      const SessionState(status: SessionStatus.disconnected);
}

class _ResumedLifecycle extends AppLifecycleNotifier {
  @override
  AppLifecycleState build() => AppLifecycleState.resumed;
}
