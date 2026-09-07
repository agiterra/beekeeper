import 'package:flutter/foundation.dart';

/// Whose sessions a project shows (desktop `projectSessionFilter.ts`).
///
/// Attribution is by founder, the human who signed the session's kind 44226
/// genesis. A session with no resolved founder cannot be attributed; `mine`
/// and `custom` hide it and report the count, so a project never silently
/// drops work.
@immutable
sealed class ProjectSessionMemberFilter {
  const ProjectSessionMemberFilter();

  static const mine = ProjectSessionMembersMine();
  static const all = ProjectSessionMembersAll();

  Map<String, Object?> toJson();

  static ProjectSessionMemberFilter fromJson(Object? value) {
    if (value is! Map) return mine;
    switch (value['mode']) {
      case 'all':
        return all;
      case 'custom':
        final raw = value['pubkeys'];
        final pubkeys = <String>{};
        if (raw is List) {
          for (final entry in raw) {
            if (entry is String) pubkeys.add(entry.toLowerCase());
          }
        }
        return ProjectSessionMembersCustom(List.unmodifiable(pubkeys));
      default:
        return mine;
    }
  }
}

/// Only sessions this device's key founded.
class ProjectSessionMembersMine extends ProjectSessionMemberFilter {
  const ProjectSessionMembersMine();

  @override
  Map<String, Object?> toJson() => const {'mode': 'mine'};

  @override
  bool operator ==(Object other) => other is ProjectSessionMembersMine;

  @override
  int get hashCode => 1;
}

/// Every session, attributed or not.
class ProjectSessionMembersAll extends ProjectSessionMemberFilter {
  const ProjectSessionMembersAll();

  @override
  Map<String, Object?> toJson() => const {'mode': 'all'};

  @override
  bool operator ==(Object other) => other is ProjectSessionMembersAll;

  @override
  int get hashCode => 2;
}

/// Sessions founded by any of [pubkeys] (lowercase).
class ProjectSessionMembersCustom extends ProjectSessionMemberFilter {
  final List<String> pubkeys;

  const ProjectSessionMembersCustom(this.pubkeys);

  @override
  Map<String, Object?> toJson() => {'mode': 'custom', 'pubkeys': pubkeys};

  @override
  bool operator ==(Object other) =>
      other is ProjectSessionMembersCustom &&
      listEquals(other.pubkeys, pubkeys);

  @override
  int get hashCode => Object.hash(3, Object.hashAll(pubkeys));
}

/// A last-activity window on the local calendar, `any` by default — hiding
/// old work silently is not a default. Weeks start on Monday.
enum ProjectSessionDateRange {
  any('any', 'Any time'),
  today('today', 'Today'),
  yesterday('yesterday', 'Yesterday'),
  week('week', 'This week'),
  month('month', 'This month');

  const ProjectSessionDateRange(this.wire, this.label);

  final String wire;
  final String label;

  static ProjectSessionDateRange fromWire(Object? value) {
    for (final range in values) {
      if (range.wire == value) return range;
    }
    return any;
  }

  /// `[start, end)` in epoch seconds against [now]; `null` for no bound.
  ({int? start, int? end}) resolve(DateTime now) {
    final today = DateTime(now.year, now.month, now.day);
    int secs(DateTime date) => date.millisecondsSinceEpoch ~/ 1000;
    DateTime addDays(DateTime date, int days) =>
        DateTime(date.year, date.month, date.day + days);
    switch (this) {
      case any:
        return (start: null, end: null);
      case ProjectSessionDateRange.today:
        return (start: secs(today), end: secs(addDays(today, 1)));
      case yesterday:
        return (start: secs(addDays(today, -1)), end: secs(today));
      case week:
        final monday = addDays(today, -((today.weekday + 6) % 7));
        return (start: secs(monday), end: secs(addDays(monday, 7)));
      case month:
        final first = DateTime(today.year, today.month, 1);
        final next = DateTime(today.year, today.month + 1, 1);
        return (start: secs(first), end: secs(next));
    }
  }
}

/// Which coding sessions a project shows, on three independent axes.
///
/// The desktop also keeps an "archived" axis; this app's closure reader
/// knows only `closed`/`open`, so there is no archived state to filter and
/// none is invented.
@immutable
class ProjectSessionFilter {
  final ProjectSessionMemberFilter members;
  final bool showClosed;
  final ProjectSessionDateRange range;

  const ProjectSessionFilter({
    this.members = ProjectSessionMemberFilter.mine,
    this.showClosed = true,
    this.range = ProjectSessionDateRange.any,
  });

  static const defaults = ProjectSessionFilter();

  ProjectSessionFilter copyWith({
    ProjectSessionMemberFilter? members,
    bool? showClosed,
    ProjectSessionDateRange? range,
  }) => ProjectSessionFilter(
    members: members ?? this.members,
    showClosed: showClosed ?? this.showClosed,
    range: range ?? this.range,
  );

  Map<String, Object?> toJson() => {
    'members': members.toJson(),
    'showClosed': showClosed,
    'range': {'kind': range.wire},
  };

  /// Parse a persisted value of unknown vintage; anything odd is the default.
  static ProjectSessionFilter fromJson(Object? value) {
    if (value is! Map) return defaults;
    final rangeRaw = value['range'];
    return ProjectSessionFilter(
      members: ProjectSessionMemberFilter.fromJson(value['members']),
      showClosed: value['showClosed'] is bool
          ? value['showClosed'] as bool
          : true,
      range: ProjectSessionDateRange.fromWire(
        rangeRaw is Map ? rangeRaw['kind'] : null,
      ),
    );
  }

  /// The trigger's one-line summary of the whole filter.
  String get label {
    final parts = [
      switch (members) {
        ProjectSessionMembersMine() => 'My sessions',
        ProjectSessionMembersAll() => 'All sessions',
        ProjectSessionMembersCustom(pubkeys: final keys) =>
          'Custom · ${keys.length}',
      },
      if (range != ProjectSessionDateRange.any) range.label,
      if (!showClosed) 'open only',
    ];
    return parts.join(' · ');
  }

  @override
  bool operator ==(Object other) =>
      other is ProjectSessionFilter &&
      other.members == members &&
      other.showClosed == showClosed &&
      other.range == range;

  @override
  int get hashCode => Object.hash(members, showClosed, range);
}

/// What the filter needs to know about one session.
@immutable
class ProjectSessionFilterEntry {
  /// Lowercase founder pubkey, or `null` when unresolved.
  final String? founderPubkey;
  final bool isClosed;

  /// Newest activity, epoch seconds.
  final int lastActivityAt;

  const ProjectSessionFilterEntry({
    required this.founderPubkey,
    required this.isClosed,
    required this.lastActivityAt,
  });
}

/// The sessions that pass, and honest counts of the ones that did not.
@immutable
class ProjectSessionFilterResult<T> {
  final List<T> shown;

  /// Hidden only because their founder is unknown.
  final int hiddenUnattributed;

  /// Hidden by the closed box or the date range.
  final int hiddenByState;

  const ProjectSessionFilterResult({
    required this.shown,
    required this.hiddenUnattributed,
    required this.hiddenByState,
  });
}

/// Apply every axis (desktop `filterProjectSessions`).
ProjectSessionFilterResult<T> filterProjectSessions<T>(
  Iterable<T> entries,
  ProjectSessionFilter filter, {
  required ProjectSessionFilterEntry Function(T entry) facts,
  required String? myPubkey,
  DateTime? now,
}) {
  final allowed = switch (filter.members) {
    ProjectSessionMembersAll() => null,
    ProjectSessionMembersMine() => {
      if (myPubkey != null && myPubkey.isNotEmpty) myPubkey.toLowerCase(),
    },
    ProjectSessionMembersCustom(pubkeys: final keys) => {
      for (final key in keys) key.toLowerCase(),
    },
  };
  final window = filter.range.resolve(now ?? DateTime.now());
  final shown = <T>[];
  var hiddenUnattributed = 0;
  var hiddenByState = 0;
  for (final entry in entries) {
    final fact = facts(entry);
    if (fact.isClosed && !filter.showClosed) {
      hiddenByState += 1;
      continue;
    }
    if (window.start != null || window.end != null) {
      final at = fact.lastActivityAt;
      if ((window.start != null && at < window.start!) ||
          (window.end != null && at >= window.end!)) {
        hiddenByState += 1;
        continue;
      }
    }
    if (allowed == null) {
      shown.add(entry);
      continue;
    }
    final founder = fact.founderPubkey?.toLowerCase();
    if (founder == null) {
      hiddenUnattributed += 1;
      continue;
    }
    if (allowed.contains(founder)) shown.add(entry);
  }
  return ProjectSessionFilterResult(
    shown: shown,
    hiddenUnattributed: hiddenUnattributed,
    hiddenByState: hiddenByState,
  );
}

/// The disclosure line for sessions the current filter cannot attribute.
String? projectSessionUnattributedNote(int count) {
  if (count <= 0) return null;
  final plural = count != 1;
  return '$count ${plural ? 'sessions' : 'session'} without a known '
      'initiator ${plural ? 'are' : 'is'} hidden — choose All sessions to '
      'see ${plural ? 'them' : 'it'}.';
}
