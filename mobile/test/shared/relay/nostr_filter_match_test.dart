import 'package:buzz/shared/relay/nostr_filter_match.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

/// Mirrors `crates/buzz-core/src/filter.rs` tests row for row so the two
/// matchers cannot drift apart silently: kind, author, since/until
/// inclusivity, id prefixes, `#x` any-of, multi-`#h`, and the empty clause.
void main() {
  const channelA = 'aaaaaaaa-0000-4000-8000-000000000000';
  const channelB = 'bbbbbbbb-0000-4000-8000-000000000000';
  final event = NostrEvent(
    id: 'abcdef0123456789',
    pubkey: 'alice',
    createdAt: 1000,
    kind: 9,
    tags: [
      ['h', channelB],
      ['e', 'target-1'],
      ['p', 'bob'],
    ],
    content: '',
    sig: '',
  );

  final rows = <String, (NostrFilter, bool)>{
    'kind matches': (const NostrFilter(kinds: [9]), true),
    'kind mismatch': (const NostrFilter(kinds: [1]), false),
    'kind in a set': (const NostrFilter(kinds: [1, 9, 40002]), true),
    'empty kinds matches nothing': (const NostrFilter(kinds: []), false),
    'author matches': (const NostrFilter(kinds: [9], authors: ['alice']), true),
    'author mismatch': (const NostrFilter(kinds: [9], authors: ['bob']), false),
    'author is exact, not a prefix': (
      const NostrFilter(kinds: [9], authors: ['ali']),
      false,
    ),
    'since in the past': (const NostrFilter(kinds: [9], since: 999), true),
    'since is inclusive': (const NostrFilter(kinds: [9], since: 1000), true),
    'since in the future': (const NostrFilter(kinds: [9], since: 1001), false),
    'until in the future': (const NostrFilter(kinds: [9], until: 1001), true),
    'until is inclusive': (const NostrFilter(kinds: [9], until: 1000), true),
    'until in the past': (const NostrFilter(kinds: [9], until: 999), false),
    'id exact': (
      const NostrFilter(kinds: [9], ids: ['abcdef0123456789']),
      true,
    ),
    'id prefix': (const NostrFilter(kinds: [9], ids: ['abcd']), true),
    'id mismatch': (const NostrFilter(kinds: [9], ids: ['ffff']), false),
    'ids any-of': (const NostrFilter(kinds: [9], ids: ['ffff', 'abc']), true),
    '#e tag matches': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#e': ['target-1'],
        },
      ),
      true,
    ),
    '#e tag mismatch': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#e': ['target-2'],
        },
      ),
      false,
    ),
    '#h single channel matches': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#h': [channelB],
        },
      ),
      true,
    ),
    '#h single channel mismatch': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#h': [channelA],
        },
      ),
      false,
    ),
    '#h multi-value matches any channel': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#h': [channelA, channelB],
        },
      ),
      true,
    ),
    'empty #h matches nothing': (
      const NostrFilter(kinds: [9], tags: {'#h': []}),
      false,
    ),
    'two tag clauses are AND-ed': (
      const NostrFilter(
        kinds: [9],
        tags: {
          '#h': [channelB],
          '#p': ['carol'],
        },
      ),
      false,
    ),
    'everything at once': (
      const NostrFilter(
        kinds: [9],
        authors: ['alice'],
        since: 1000,
        until: 1000,
        ids: ['abc'],
        tags: {
          '#h': [channelB],
          '#p': ['bob'],
        },
      ),
      true,
    ),
  };

  rows.forEach((name, row) {
    test(name, () => expect(nostrFilterMatches(row.$1, event), row.$2));
  });

  test('an event with no h tag is not excluded by an #h clause', () {
    // The relay falls back to its stored channel_id for these (reactions,
    // deletions); the client cannot, so it lets them through.
    final reaction = NostrEvent(
      id: 'r1',
      pubkey: 'alice',
      createdAt: 1,
      kind: 7,
      tags: [
        ['e', 'target-1'],
      ],
      content: '+',
      sig: '',
    );
    const filter = NostrFilter(
      kinds: [7],
      tags: {
        '#h': [channelA],
      },
    );
    expect(nostrFilterMatches(filter, reaction), isTrue);
  });

  test('filters in a list are OR-ed', () {
    expect(
      nostrFiltersMatch(const [
        NostrFilter(kinds: [1]),
        NostrFilter(kinds: [9]),
      ], event),
      isTrue,
    );
    expect(
      nostrFiltersMatch(const [
        NostrFilter(kinds: [1]),
        NostrFilter(kinds: [2]),
      ], event),
      isFalse,
    );
  });

  test('search filters are refused', () {
    expect(
      () => nostrFilterMatches(
        const NostrFilter(kinds: [9], search: 'hello'),
        event,
      ),
      throwsArgumentError,
    );
  });
}
