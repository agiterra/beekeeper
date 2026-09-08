import 'package:buzz/shared/relay/poll_schedule.dart';
import 'package:buzz/shared/relay/relay_closed_retry.dart';
import 'package:buzz/shared/relay/relay_reconnect_policy.dart';
import 'package:buzz/shared/relay/relay_closed_policy.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('fnv1a32', () {
    test('matches the reference vectors', () {
      expect(fnv1a32(''), 0x811c9dc5);
      expect(fnv1a32('a'), 0xe40c292c);
      expect(fnv1a32('foobar'), 0xbf9cf968);
    });
  });

  group('phaseOffset', () {
    const period = Duration(seconds: 30);

    test('is deterministic for the same pubkey and key', () {
      final a = phaseOffset('terminals', period, pubkey: 'p1', random: 0.5);
      final b = phaseOffset('terminals', period, pubkey: 'p1', random: 0.5);
      expect(a, b);
    });

    test('differs across keys and across pubkeys', () {
      final base = phaseOffset('terminals', period, pubkey: 'p1', random: 0.5);
      expect(
        phaseOffset('projects', period, pubkey: 'p1', random: 0.5),
        isNot(base),
      );
      expect(
        phaseOffset('terminals', period, pubkey: 'p2', random: 0.5),
        isNot(base),
      );
    });

    test('jitter moves the phase by at most 10 % of the period', () {
      final centre = phaseOffset('k', period, pubkey: 'p', random: 0.5);
      final low = phaseOffset('k', period, pubkey: 'p', random: 0.0);
      final high = phaseOffset('k', period, pubkey: 'p', random: 0.999);
      final tenPercent = period * 0.1;
      int wrapped(Duration d) =>
          (d - centre).inMicroseconds.abs() % period.inMicroseconds;
      expect(wrapped(low), lessThanOrEqualTo(tenPercent.inMicroseconds));
      expect(wrapped(high), lessThanOrEqualTo(tenPercent.inMicroseconds));
    });

    test('always lands inside the period', () {
      for (var i = 0; i < 200; i++) {
        final offset = phaseOffset(
          'key-$i',
          period,
          pubkey: 'pub-${i % 7}',
          random: (i % 10) / 10,
        );
        expect(offset, greaterThanOrEqualTo(Duration.zero));
        expect(offset, lessThan(period));
      }
    });

    test('a zero period yields no offset', () {
      expect(
        phaseOffset('k', Duration.zero, pubkey: 'p', random: 0.3),
        Duration.zero,
      );
    });
  });

  group('jitteredReconnectDelayMs', () {
    test('spreads the delay across [0.75, 1.25) of the base', () {
      expect(jitteredReconnectDelayMs(1000, 0.0), 750);
      expect(jitteredReconnectDelayMs(1000, 0.5), 1000);
      expect(jitteredReconnectDelayMs(1000, 0.999), 1250);
      expect(jitteredReconnectDelayMs(30000, 0.0), 22500);
    });
  });

  group('closedRetryDelayMs', () {
    test('doubles per attempt and saturates at 30 s', () {
      for (var attempt = 0; attempt < 100; attempt++) {
        expect(
          closedRetryBackoffMs(attempt),
          attempt >= 5 ? 30000 : 1000 * (1 << attempt),
        );
      }
    });

    test('a rate-limited close waits at least the gate window', () {
      expect(
        closedRetryDelayMs(
          attempt: 0,
          closedClass: RelayClosedClass.rateLimited,
          message: 'rate-limited: quota exceeded; retry in 4s',
          gateRemainingMs: 3990,
        ),
        3990,
      );
    });

    test('a rate-limited close with no gate uses the relay hint', () {
      expect(
        closedRetryDelayMs(
          attempt: 0,
          closedClass: RelayClosedClass.rateLimited,
          message: 'rate-limited: quota exceeded; retry in 4s',
          gateRemainingMs: 0,
        ),
        4000,
      );
      expect(
        closedRetryDelayMs(
          attempt: 0,
          closedClass: RelayClosedClass.rateLimited,
          message: 'rate-limited: no hint',
          gateRemainingMs: 0,
        ),
        10000,
      );
    });

    test('backoff wins over a shorter gate', () {
      expect(
        closedRetryDelayMs(
          attempt: 4,
          closedClass: RelayClosedClass.rateLimited,
          message: 'rate-limited: quota exceeded; retry in 1s',
          gateRemainingMs: 1000,
        ),
        16000,
      );
    });

    test('a retryable close ignores the gate', () {
      expect(
        closedRetryDelayMs(
          attempt: 1,
          closedClass: RelayClosedClass.retryable,
          message: 'error: transient',
          gateRemainingMs: 9000,
        ),
        2000,
      );
    });
  });
}
