import 'package:buzz/features/coding_sessions/domain/coding_sessions_domain.dart';
import 'package:buzz/features/projects/domain/project_children.dart';
import 'package:buzz/features/projects/domain/project_models.dart';
import 'package:flutter_test/flutter_test.dart';

import '../../coding_sessions/ui/fake_observer.dart';
import '../ui/fake_projects.dart';

ProjectSessionRow _session(
  String key, {
  CodingSessionFoldedStatusKind kind = CodingSessionFoldedStatusKind.reported,
  bool closed = false,
  int at = 1000,
}) => ProjectSessionRow(
  session: testUmbrella(
    key: key,
    sessionRef: null,
    name: key,
    closed: closed,
    status: CodingSessionFoldedStatus(kind: kind),
    lastActivityAt: at,
  ),
  channelId: 'c',
  channelName: 'sessions',
);

ProjectChannel _channel(String name, {String type = 'stream'}) =>
    ProjectChannel(id: name, name: name, channelType: type, isMember: true);

void main() {
  test('rows rank sessions, channels, forums, terminals; sessions by '
      'activity and channels by name; the transport is not a row', () {
    final rows = buildProjectChildren(
      sessions: [
        _session('idle', at: 900),
        _session('closed', closed: true, at: 5000),
        _session('working', kind: CodingSessionFoldedStatusKind.working, at: 1),
        _session('newer-idle', at: 950),
      ],
      channels: [
        _channel('zeta'),
        _channel('alpha'),
        _channel('board', type: 'forum'),
        _channel('transport', type: 'transport'),
      ],
      terminals: [testTerminal(title: 'shell')],
    );
    expect(rows.map((r) => r.label), [
      'working',
      'newer-idle',
      'idle',
      'closed',
      'alpha',
      'zeta',
      'board',
      'shell',
    ]);
    expect(rows.map((r) => r.key).toSet().length, rows.length);
    expect(projectSessionFounders([_session('a'), _session('b')]), [
      testSignerPubkey,
    ]);
  });
}
