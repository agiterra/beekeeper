import 'dart:convert';
import 'dart:io';

import 'package:buzz/shared/utils/fractional_rank.dart';
import 'package:flutter_test/flutter_test.dart';

/// The Dart rank binds to the same vectors as the Rust and TypeScript
/// implementations (`conformance/project-todo-fold/CONTRACT.md` § Ranks).
/// A rank one client mints must sort identically everywhere.
void main() {
  final vectors =
      jsonDecode(
            File(
              '../conformance/project-todo-fold/fixtures/rank-vectors.json',
            ).readAsStringSync(),
          )
          as Map<String, dynamic>;

  test('vectors carry the expected schema', () {
    expect(vectors['schema'], 'buzz-project-todo-rank-vectors/v1');
  });

  for (final raw in vectors['between'] as List<dynamic>) {
    final vector = raw as List<dynamic>;
    final after = vector[0] as String?;
    final before = vector[1] as String?;
    final expected = vector[2] as String;
    test('rankBetween($after, $before) == $expected', () {
      final minted = rankBetween(after, before);
      expect(minted, expected);
      expect(() => validateRank(minted), returnsNormally);
      if (after != null) expect(after.compareTo(minted) < 0, isTrue);
      if (before != null) expect(minted.compareTo(before) < 0, isTrue);
    });
  }

  for (final raw in vectors['invalid'] as List<dynamic>) {
    final rank = raw as String;
    test('validateRank refuses ${jsonEncode(rank)}', () {
      expect(() => validateRank(rank), throwsFormatException);
      expect(isValidRank(rank), isFalse);
    });
  }

  test('appends increment the integer and stay short', () {
    final ranks = <String>[];
    for (var i = 0; i < 1000; i++) {
      ranks.add(rankBetween(ranks.isEmpty ? null : ranks.last, null));
    }
    for (var i = 1; i < ranks.length; i++) {
      expect(ranks[i - 1].compareTo(ranks[i]) < 0, isTrue);
    }
    expect(ranks.every((r) => r.length <= 3), isTrue);
    expect(ranks.sublist(0, 3), ['a0', 'a1', 'a2']);
    expect(ranks[62], 'b00');

    var head = rankBetween(null, null);
    for (var i = 0; i < 200; i++) {
      final before = rankBetween(null, head);
      expect(before.compareTo(head) < 0, isTrue);
      head = before;
    }
    expect(head.length <= 3, isTrue);
  });

  test('inserting at one gap grows by one digit each time', () {
    var lo = 'a0';
    const hi = 'a1';
    for (var depth = 1; depth <= 20; depth++) {
      lo = rankBetween(lo, hi);
      expect(lo.compareTo(hi) < 0, isTrue);
      expect(lo.length <= 2 + depth, isTrue);
    }
  });

  test('rejects malformed or unordered bounds', () {
    expect(() => rankBetween('a1', 'a0'), throwsFormatException);
    expect(() => rankBetween('a0', 'a0'), throwsFormatException);
    expect(() => rankBetween('a0V0', null), throwsFormatException);
    expect(
      () => validateRank('a0${'z' * maxRankLength}'),
      throwsFormatException,
    );
    expect(
      () => validateRank('a0${'z' * (maxRankLength - 2)}'),
      returnsNormally,
    );
  });
}
