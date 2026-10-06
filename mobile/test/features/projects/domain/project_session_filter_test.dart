import 'package:beekeeper/features/projects/domain/project_session_filter.dart';
import 'package:flutter_test/flutter_test.dart';

const me = 'aa0011223344556677889900aabbccddeeff00112233445566778899aabbccdd';
const alice =
    '11111111222222223333333344444444555555556666666677777777dddddddd';

ProjectSessionFilterEntry _entry({
  String? founder = me,
  bool closed = false,
  int at = 1000,
}) => ProjectSessionFilterEntry(
  founderPubkey: founder,
  isClosed: closed,
  lastActivityAt: at,
);

ProjectSessionFilterResult<ProjectSessionFilterEntry> _run(
  List<ProjectSessionFilterEntry> entries,
  ProjectSessionFilter filter, {
  DateTime? now,
}) => filterProjectSessions(
  entries,
  filter,
  facts: (e) => e,
  myPubkey: me.toUpperCase(),
  now: now,
);

void main() {
  test(
    'mine keeps my sessions, hides others silently, counts unattributed',
    () {
      final result = _run([
        _entry(),
        _entry(founder: alice),
        _entry(founder: null),
      ], ProjectSessionFilter.defaults);
      expect(result.shown.length, 1);
      expect(result.hiddenUnattributed, 1);
      expect(result.hiddenByState, 0);
      expect(projectSessionUnattributedNote(1), contains('1 session without'));
      expect(projectSessionUnattributedNote(0), isNull);
    },
  );

  test('all shows everything, including the unattributed', () {
    final result = _run([
      _entry(founder: alice),
      _entry(founder: null),
    ], const ProjectSessionFilter(members: ProjectSessionMemberFilter.all));
    expect(result.shown.length, 2);
  });

  test('custom is a set of founders, case-insensitive', () {
    final result = _run(
      [_entry(), _entry(founder: alice), _entry(founder: null)],
      ProjectSessionFilter(
        members: ProjectSessionMembersCustom([alice.toUpperCase()]),
      ),
    );
    expect(result.shown.single.founderPubkey, alice);
    expect(result.hiddenUnattributed, 1);
  });

  test('a founded session survives the default range and My sessions '
      'through its genesis founder and founding-fact time', () {
    final now = DateTime(2026, 9, 9, 10);
    // What the fold hands the filter for a genesis-only umbrella: the genesis
    // signer as founder, the newest founding fact as its time.
    final founded = _entry(at: now.millisecondsSinceEpoch ~/ 1000 - 60);
    final result = _run([founded], ProjectSessionFilter.defaults, now: now);
    expect(result.shown, [founded]);
    expect(result.hiddenByState, 0);
    expect(result.hiddenUnattributed, 0);
    // A founded umbrella dated 0 — the pre-fold reading — would be hidden by
    // every bounded range, which is why the fold dates it from its facts.
    expect(
      _run(
        [_entry(at: 0)],
        const ProjectSessionFilter(range: ProjectSessionDateRange.month),
        now: now,
      ).hiddenByState,
      1,
    );
  });

  test('closed sessions hide only when the box is off, and are counted', () {
    final entries = [_entry(), _entry(closed: true)];
    expect(_run(entries, ProjectSessionFilter.defaults).shown.length, 2);
    final hidden = _run(entries, const ProjectSessionFilter(showClosed: false));
    expect(hidden.shown.length, 1);
    expect(hidden.hiddenByState, 1);
  });

  test('the date range is the local calendar, weeks start on Monday', () {
    // Wednesday 2026-09-09 10:00 local.
    final now = DateTime(2026, 9, 9, 10);
    int at(DateTime d) => d.millisecondsSinceEpoch ~/ 1000;
    final monday = DateTime(2026, 9, 7, 8);
    final lastSunday = DateTime(2026, 9, 6, 23);
    final yesterday = DateTime(2026, 9, 8, 12);
    final entries = [
      _entry(at: at(now)),
      _entry(at: at(yesterday)),
      _entry(at: at(monday)),
      _entry(at: at(lastSunday)),
    ];
    Set<int> shown(ProjectSessionDateRange range) => {
      for (final e in _run(
        entries,
        ProjectSessionFilter(range: range),
        now: now,
      ).shown)
        e.lastActivityAt,
    };
    expect(shown(ProjectSessionDateRange.today), {at(now)});
    expect(shown(ProjectSessionDateRange.yesterday), {at(yesterday)});
    expect(shown(ProjectSessionDateRange.week), {
      at(now),
      at(yesterday),
      at(monday),
    });
    expect(shown(ProjectSessionDateRange.month).length, 4);
    expect(
      _run(
        entries,
        const ProjectSessionFilter(range: ProjectSessionDateRange.today),
        now: now,
      ).hiddenByState,
      3,
    );
  });

  test('round-trips through JSON and tolerates the desktop\'s shapes', () {
    final filter = ProjectSessionFilter(
      members: ProjectSessionMembersCustom([alice]),
      showClosed: false,
      range: ProjectSessionDateRange.week,
    );
    expect(ProjectSessionFilter.fromJson(filter.toJson()), filter);
    // The desktop's first persisted shape was the bare member filter.
    expect(
      ProjectSessionFilter.fromJson({'mode': 'all'}),
      ProjectSessionFilter.defaults,
    );
    expect(
      ProjectSessionFilter.fromJson({
        'members': {'mode': 'all'},
        'showClosed': true,
        'showArchived': false,
        'range': {'kind': 'custom', 'from': '2026-09-01', 'to': null},
      }),
      const ProjectSessionFilter(members: ProjectSessionMemberFilter.all),
    );
    expect(
      ProjectSessionFilter.fromJson('garbage'),
      ProjectSessionFilter.defaults,
    );
    expect(filter.label, 'Custom · 1 · This week · open only');
    expect(ProjectSessionFilter.defaults.label, 'My sessions');
  });
}
