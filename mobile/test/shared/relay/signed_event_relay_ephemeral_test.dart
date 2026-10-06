import 'package:beekeeper/shared/relay/nostr_models.dart';
import 'package:beekeeper/shared/relay/poll_schedule.dart';
import 'package:beekeeper/shared/relay/signed_event_relay.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:nostr/nostr.dart' as nostr;

import '../../helpers/recording_relay_session.dart';

void main() {
  group('SignedEventRelay.sendEphemeral', () {
    test('signs with this key and hands the frame to the droppable path', () {
      final session = RecordingRelaySessionNotifier();
      final keys = nostr.Keys.generate();
      final relay = SignedEventRelay(session: session, nsec: keys.nsec);

      final sent = relay.sendEphemeral(
        kind: EventKind.presenceUpdate,
        content: 'online',
        tags: const [
          ['h', 'c1'],
        ],
      );

      expect(sent, isTrue);
      expect(session.operations, ['ephemeral']);
      final event = session.ephemeralEvents.single;
      expect(event.kind, EventKind.presenceUpdate);
      expect(event.pubkey, keys.public);
      expect(event.tags, [
        ['h', 'c1'],
      ]);
      expect(event.sig, isNotEmpty);
    });

    test('reports a drop and sends nothing without a key or when declined', () {
      final session = RecordingRelaySessionNotifier()..acceptEphemeral = false;
      expect(
        SignedEventRelay(
          session: session,
          nsec: null,
        ).sendEphemeral(kind: 20001, content: '', tags: const []),
        isFalse,
      );
      expect(session.operations, isEmpty);
      expect(
        SignedEventRelay(
          session: session,
          nsec: nostr.Keys.generate().nsec,
        ).sendEphemeral(kind: 20001, content: '', tags: const []),
        isFalse,
      );
      expect(session.operations, ['ephemeral']);
    });
  });

  group('alignedPollDelay', () {
    test('lands two starts at the same wall-clock tick', () {
      const period = Duration(seconds: 30);
      final first = DateTime.fromMillisecondsSinceEpoch(1_000_000);
      final later = first.add(const Duration(seconds: 7));
      final a = alignedPollDelay(
        key: 'k',
        period: period,
        pubkey: 'p',
        now: first,
      );
      final b = alignedPollDelay(
        key: 'k',
        period: period,
        pubkey: 'p',
        now: later,
      );
      expect(a, greaterThan(Duration.zero));
      expect(a, lessThanOrEqualTo(period));
      expect(first.add(a), later.add(b));
    });
  });
}
