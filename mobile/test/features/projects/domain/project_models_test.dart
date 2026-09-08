import 'package:buzz/features/projects/domain/project_models.dart';
import 'package:buzz/shared/relay/nostr_models.dart';
import 'package:flutter_test/flutter_test.dart';

const owner =
    'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const alice =
    '11111111222222223333333344444444555555556666666677777777dddddddd';

NostrEvent _head({
  String dtag = 'beekeeper',
  String? name = 'Beekeeper',
  String pubkey = owner,
  int createdAt = 100,
  String id = 'h1',
  List<List<String>> extra = const [],
}) => NostrEvent(
  id: id,
  pubkey: pubkey,
  createdAt: createdAt,
  kind: 30621,
  tags: [
    ['d', dtag],
    if (name != null) ['name', name],
    ...extra,
  ],
  content: 'A project',
  sig: '',
);

NostrEvent _tombstone(String address, {String pubkey = owner}) => NostrEvent(
  id: 't1',
  pubkey: pubkey,
  createdAt: 300,
  kind: 5,
  tags: [
    ['a', address],
  ],
  content: '',
  sig: '',
);

ProjectChannel _channel(
  String id, {
  String name = '',
  String type = 'stream',
  int? lastActivityAt,
}) => ProjectChannel(
  id: id,
  name: name,
  channelType: type,
  isMember: true,
  lastActivityAt: lastActivityAt,
);

void main() {
  test('reads a head: address, name fallback, channels, members, access', () {
    final project = projectFromEvent(
      _head(
        name: null,
        extra: [
          ['channel', 'c1'],
          ['channel', 'c1'],
          ['channel', 'c2'],
          ['p', alice, '', 'collaborator'],
          ['p', owner, '', 'owner'],
          ['p', 'not-hex'],
          ['buzz-access', 'private'],
          ['description', 'Described'],
        ],
      ),
    )!;
    expect(project.address, '30621:$owner:beekeeper');
    expect(project.name, 'beekeeper');
    expect(project.description, 'Described');
    expect(project.channelIds, ['c1', 'c2']);
    expect(project.memberPubkeys, [alice]);
    expect(project.isPrivate, isTrue);
    expect(projectFromEvent(_head(dtag: '')), isNull);
  });

  test('the newest head per slot wins and a tombstone removes it', () {
    final projects = projectsFromEvents(
      [
        _head(createdAt: 100, id: 'old', name: 'Old'),
        _head(createdAt: 200, id: 'new', name: 'Beekeeper'),
        _head(dtag: 'zeta', name: 'Zeta', id: 'z'),
        _head(dtag: 'gone', name: 'Gone', id: 'g'),
      ],
      [
        _tombstone('30621:$owner:gone'),
        // Someone else's kind:5 deletes nothing of the owner's.
        _tombstone('30621:$owner:zeta', pubkey: alice),
      ],
    );
    expect(projects.map((p) => p.name), ['Beekeeper', 'Zeta']);
  });

  test('a channel belongs by forward tag or by the relay back-reference', () {
    final project = projectFromEvent(
      _head(
        extra: [
          ['channel', 'c1'],
        ],
      ),
    )!;
    expect(
      channelBelongsToProject(
        project: project,
        channelId: 'c1',
        channelProjectRef: null,
      ),
      isTrue,
    );
    expect(
      channelBelongsToProject(
        project: project,
        channelId: 'c9',
        channelProjectRef: project.address,
      ),
      isTrue,
    );
    expect(
      channelBelongsToProject(
        project: project,
        channelId: 'c9',
        channelProjectRef: '30621:$alice:other',
      ),
      isFalse,
    );
  });

  test('the sessions channel: transport, else the canonical name, else the '
      'busiest, else none', () {
    final project = projectFromEvent(_head(name: '  My  Project '))!;
    expect(projectSessionsChannelName('  My  Project '), 'My Project sessions');
    expect(
      pickProjectSessionsChannel(project, [
        _channel('a', name: 'general'),
        _channel('t', name: 'anything', type: 'transport'),
        _channel('n', name: 'my project sessions'),
      ])!.id,
      't',
    );
    expect(
      pickProjectSessionsChannel(project, [
        _channel('a', name: 'general', lastActivityAt: 900),
        _channel('n', name: 'MY  project   Sessions'),
      ])!.id,
      'n',
    );
    expect(
      pickProjectSessionsChannel(project, [
        _channel('a', name: 'general', lastActivityAt: 900),
        _channel('b', name: 'dev', lastActivityAt: 950),
      ])!.id,
      'b',
    );
    expect(
      pickProjectSessionsChannel(project, [_channel('a', name: 'general')]),
      isNull,
    );
  });

  test('among several transports the one with the newest session activity '
      'wins, then the lowest id', () {
    final project = projectFromEvent(_head(name: 'P'))!;
    final transports = [
      _channel('t3', type: 'transport'),
      _channel('t2', type: 'transport'),
      _channel('t1', type: 'transport'),
    ];
    // No activity anywhere: deterministic across members by id.
    expect(pickProjectSessionsChannel(project, transports)!.id, 't1');
    // The desktop's rule: the channel the sessions actually live in.
    expect(
      pickProjectSessionsChannel(
        project,
        transports,
        sessionActivityByChannel: {'t3': 900, 't2': 950},
      )!.id,
      't2',
    );
    // A channel's own last-message time never outranks session activity.
    expect(
      pickProjectSessionsChannel(
        project,
        [
          _channel('t9', type: 'transport', lastActivityAt: 5000),
          _channel('t1', type: 'transport'),
        ],
        sessionActivityByChannel: {'t1': 10},
      )!.id,
      't1',
    );
  });
}
