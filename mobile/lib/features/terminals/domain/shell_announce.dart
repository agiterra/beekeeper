import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';

/// The two roster roles a kind:30623 announce may grant (NIP-ST § Roster).
enum ShellRosterRole {
  /// May watch and type (kind:24312).
  collaborator('collaborator'),

  /// Watch-only, independent of project membership.
  viewer('viewer');

  const ShellRosterRole(this.wire);

  /// The exact string in the announce's arity-4 `p` tag.
  final String wire;

  /// Decode a wire role, or `null` for anything the relay would have refused.
  static ShellRosterRole? fromWire(String? value) {
    for (final role in ShellRosterRole.values) {
      if (role.wire == value) return role;
    }
    return null;
  }
}

/// One roster entry: `["p", <hex>, "", <role>]` on the owner's announce.
@immutable
class ShellRosterEntry {
  /// Lowercase 64-hex pubkey.
  final String pubkey;
  final ShellRosterRole role;

  const ShellRosterEntry({required this.pubkey, required this.role});

  @override
  bool operator ==(Object other) =>
      other is ShellRosterEntry && other.pubkey == pubkey && other.role == role;

  @override
  int get hashCode => Object.hash(pubkey, role);
}

/// The owner's grid, from the announce's `dims` tag (`<rows>x<cols>`).
@immutable
class ShellDims {
  final int rows;
  final int cols;

  const ShellDims({required this.rows, required this.cols});

  /// Parse `<rows>x<cols>`; `null` for any other shape.
  static ShellDims? parse(String? value) {
    if (value == null) return null;
    final match = RegExp(r'^(\d{1,3})x(\d{1,4})$').firstMatch(value);
    if (match == null) return null;
    final rows = int.parse(match.group(1)!);
    final cols = int.parse(match.group(2)!);
    if (rows <= 0 || cols <= 0) return null;
    return ShellDims(rows: rows, cols: cols);
  }

  @override
  String toString() => '${rows}x$cols';

  @override
  bool operator ==(Object other) =>
      other is ShellDims && other.rows == rows && other.cols == cols;

  @override
  int get hashCode => Object.hash(rows, cols);
}

/// A member's open shared terminal, read from its kind:30623 announce.
///
/// Mirrors the desktop's `RemoteTerminal` (`useProjectTerminals.ts`): the
/// roster is advisory for display — the relay and the owner's host enforce
/// the real grants — and nothing here claims the terminal is *live*; that is
/// only known once frames arrive.
@immutable
class RemoteTerminal {
  final String sessionId;

  /// Lowercase 64-hex owner pubkey — the announce's signer.
  final String ownerPubkey;
  final String title;

  /// The `30621:<owner>:<d>` project address the terminal is shared under.
  final String projectRef;
  final ShellDims? dims;
  final List<ShellRosterEntry> roster;

  /// The announce's `created_at`, so a stale `open` head can be shown as such.
  final int announcedAt;

  const RemoteTerminal({
    required this.sessionId,
    required this.ownerPubkey,
    required this.title,
    required this.projectRef,
    required this.dims,
    required this.roster,
    required this.announcedAt,
  });

  /// The `(owner, sessionId)` identity of the announce.
  String get key => '$ownerPubkey $sessionId';

  /// The roster role [pubkey] holds, or `null` for none.
  ShellRosterRole? roleOf(String? pubkey) {
    final wanted = pubkey?.toLowerCase();
    if (wanted == null) return null;
    for (final entry in roster) {
      if (entry.pubkey == wanted) return entry.role;
    }
    return null;
  }

  /// True when [pubkey] may type: the owner, or a roster collaborator.
  bool mayType(String? pubkey) =>
      pubkey != null &&
      (pubkey.toLowerCase() == ownerPubkey ||
          roleOf(pubkey) == ShellRosterRole.collaborator);
}

final _hex64 = RegExp(r'^[0-9a-f]{64}$');

String? _singleTag(NostrEvent event, String name) {
  String? found;
  for (final tag in event.tags) {
    if (tag.length < 2 || tag[0] != name) continue;
    if (found != null) return null;
    found = tag[1];
  }
  return found;
}

/// Parse the roster from an announce's arity-4 `p` tags.
///
/// Malformed entries — wrong arity, non-hex or uppercase pubkey, unknown role,
/// a duplicate — are skipped: display code must never invent a grant from a
/// tag the relay would have rejected (`useProjectTerminals.ts:40-53`).
List<ShellRosterEntry> rosterFromAnnounce(NostrEvent event) {
  final roster = <ShellRosterEntry>[];
  final seen = <String>{};
  for (final tag in event.tags) {
    if (tag.length < 4 || tag[0] != 'p') continue;
    final pubkey = tag[1];
    if (!_hex64.hasMatch(pubkey)) continue;
    final role = ShellRosterRole.fromWire(tag[3]);
    if (role == null) continue;
    if (!seen.add(pubkey)) continue;
    roster.add(ShellRosterEntry(pubkey: pubkey, role: role));
  }
  return roster;
}

/// Read one announce as a [RemoteTerminal], or `null` when it is not a
/// well-formed *open* one.
RemoteTerminal? remoteTerminalFromEvent(NostrEvent event) {
  if (event.kind != EventKind.shellSession) return null;
  if (_singleTag(event, 'status') != 'open') return null;
  final sessionId = _singleTag(event, 'd');
  if (sessionId == null || sessionId.isEmpty) return null;
  final projectRef = _singleTag(event, 'a');
  if (projectRef == null || !isProjectAddress(projectRef)) return null;
  final owner = event.pubkey.toLowerCase();
  if (!_hex64.hasMatch(owner)) return null;
  final title = _singleTag(event, 'title');
  return RemoteTerminal(
    sessionId: sessionId,
    ownerPubkey: owner,
    title: title == null || title.trim().isEmpty ? 'terminal' : title.trim(),
    projectRef: projectRef,
    dims: ShellDims.parse(_singleTag(event, 'dims')),
    roster: rosterFromAnnounce(event),
    announcedAt: event.createdAt,
  );
}

/// Every open terminal in [events], newest head per `(owner, d)`.
///
/// A closed head replaces an open one for the same address: the announce is
/// addressable, so the newest is the only truth. [excludeOwner] drops this
/// device's own sessions — a phone hosts none, but the paired desktop shares
/// its key, and observing your own terminal from the phone is exactly the
/// owner-typing-from-another-device case NIP-ST allows, so callers choose.
List<RemoteTerminal> remoteTerminalsFromEvents(
  Iterable<NostrEvent> events, {
  String? excludeOwner,
}) {
  final heads = <String, NostrEvent>{};
  for (final event in events) {
    if (event.kind != EventKind.shellSession) continue;
    final d = _singleTag(event, 'd');
    if (d == null) continue;
    final key = '${event.pubkey.toLowerCase()} $d';
    final incumbent = heads[key];
    if (incumbent == null ||
        event.createdAt > incumbent.createdAt ||
        (event.createdAt == incumbent.createdAt &&
            event.id.compareTo(incumbent.id) < 0)) {
      heads[key] = event;
    }
  }
  final excluded = excludeOwner?.toLowerCase();
  final terminals = <RemoteTerminal>[];
  for (final head in heads.values) {
    final terminal = remoteTerminalFromEvent(head);
    if (terminal == null) continue;
    if (excluded != null && terminal.ownerPubkey == excluded) continue;
    terminals.add(terminal);
  }
  terminals.sort((left, right) {
    final byTitle = left.title.toLowerCase().compareTo(
      right.title.toLowerCase(),
    );
    return byTitle != 0 ? byTitle : left.key.compareTo(right.key);
  });
  return terminals;
}

/// Group [terminals] by project address, preserving their order.
Map<String, List<RemoteTerminal>> terminalsByProject(
  Iterable<RemoteTerminal> terminals,
) {
  final grouped = <String, List<RemoteTerminal>>{};
  for (final terminal in terminals) {
    grouped.putIfAbsent(terminal.projectRef, () => []).add(terminal);
  }
  return grouped;
}
