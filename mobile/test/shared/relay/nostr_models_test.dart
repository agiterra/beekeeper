import 'package:flutter_test/flutter_test.dart';
import 'package:beekeeper/shared/relay/nostr_models.dart';

void main() {
  _projectTagTests();
  test('NostrFilter serializes and preserves authors', () {
    const filter = NostrFilter(
      kinds: [EventKind.readState],
      authors: ['pubkey-a'],
      tags: {
        '#t': ['read-state'],
      },
      since: 10,
      limit: 5,
    );

    expect(filter.toJson(), {
      'kinds': [EventKind.readState],
      'limit': 5,
      'authors': ['pubkey-a'],
      'since': 10,
      '#t': ['read-state'],
    });

    final copied = filter.copyWithSince(20);
    expect(copied.toJson(), {
      'kinds': [EventKind.readState],
      'limit': 5,
      'authors': ['pubkey-a'],
      'since': 20,
      '#t': ['read-state'],
    });
  });

  test('NostrFilter can serialize broad deletion subscriptions', () {
    const filter = NostrFilter(kinds: [EventKind.deletion], limit: 0);

    expect(filter.toJson(), {
      'kinds': [EventKind.deletion],
      'limit': 0,
    });
  });

  test('NostrFilter serializes relay query extensions', () {
    const filter = NostrFilter(
      kinds: EventKind.channelTimelineContentKinds,
      tags: {
        '#h': ['channel-id'],
      },
      limit: 50,
      extensions: {
        'top_level': true,
        'include_summaries': true,
        'include_aux': true,
        'before_id': 'cursor-id',
      },
    );

    expect(filter.toJson(), {
      'kinds': EventKind.channelTimelineContentKinds,
      'limit': 50,
      '#h': ['channel-id'],
      'top_level': true,
      'include_summaries': true,
      'include_aux': true,
      'before_id': 'cursor-id',
    });
  });

  test('channel unread activity kinds exclude non-message updates', () {
    expect(
      EventKind.channelMessageEventKinds,
      contains(EventKind.streamMessage),
    );
    expect(EventKind.channelMessageEventKinds, contains(EventKind.forumPost));
    expect(
      EventKind.channelMessageEventKinds,
      contains(EventKind.forumComment),
    );

    expect(
      EventKind.channelMessageEventKinds,
      isNot(contains(EventKind.reaction)),
    );
    expect(
      EventKind.channelMessageEventKinds,
      isNot(contains(EventKind.streamMessageEdit)),
    );
    expect(
      EventKind.channelMessageEventKinds,
      isNot(contains(EventKind.streamMessageDiff)),
    );
    expect(
      EventKind.channelMessageEventKinds,
      isNot(contains(EventKind.deletion)),
    );
    expect(
      EventKind.channelMessageEventKinds,
      isNot(contains(EventKind.systemMessage)),
    );
  });
}

const _owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';

NostrEvent _channelMetadata(List<List<String>> tags) => NostrEvent(
  id: _owner,
  pubkey: _owner,
  createdAt: 1,
  kind: 39000,
  tags: tags,
  content: '',
  sig: '$_owner$_owner',
);

void _projectTagTests() {
  group('ChannelData.projectRef', () {
    test('reads the relay-stamped project tag', () {
      final channel = ChannelData.fromEvent(
        _channelMetadata([
          ['d', 'chan-1'],
          ['name', 'sessions'],
          ['t', 'transport'],
          ['project', '30621:$_owner:beekeeper'],
        ]),
      );
      expect(channel.projectRef, '30621:$_owner:beekeeper');
      expect(channel.channelType, 'transport');
    });

    test('a channel with no project tag has no project', () {
      final channel = ChannelData.fromEvent(
        _channelMetadata([
          ['d', 'chan-1'],
          ['name', 'general'],
        ]),
      );
      expect(channel.projectRef, isNull);
    });

    test('a malformed project claim reads as no project, never as one', () {
      for (final bad in [
        '30617:$_owner:repo',
        '30621:$_owner:',
        '30621:ABC:slug',
        'beekeeper',
      ]) {
        final channel = ChannelData.fromEvent(
          _channelMetadata([
            ['d', 'chan-1'],
            ['project', bad],
          ]),
        );
        expect(channel.projectRef, isNull, reason: bad);
        expect(isProjectAddress(bad), isFalse, reason: bad);
      }
      expect(isProjectAddress('30621:$_owner:my.project-1'), isTrue);
    });
  });
}
