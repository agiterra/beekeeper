import 'dart:convert';

import 'package:flutter/foundation.dart';

/// Nostr event kind constants.
///
/// Keep in sync with `desktop/src/shared/constants/kinds.ts`.
abstract final class EventKind {
  static const note = 1;
  static const contactList = 3;
  static const deletion = 5;
  static const reaction = 7;

  /// Kind:9030 event requesting that the relay add a community member.
  static const relayAdminAddMember = 9030;

  /// Kind:13534 event containing the current relay-community membership.
  static const relayMembership = 13534;
  static const streamMessage = 9;
  static const nip29DeleteEvent = 9005;
  static const presenceUpdate = 20001;
  static const typingIndicator = 20002;
  static const auth = 22242;
  static const agentObserverFrame = 24200;
  static const huddleReaction = 24810;
  static const readState = 30078;
  static const eventReminder = 30300;
  static const userStatus = 30315;
  static const dmVisibility = 30622;
  static const streamMessageV2 = 40002;
  static const channelThreadSummary = 39005;
  static const channelWindowBounds = 39006;
  static const streamMessageEdit = 40003;
  static const streamMessageDiff = 40008;
  static const systemMessage = 40099;
  static const jobRequest = 43001;
  static const jobAccepted = 43002;
  static const jobProgress = 43003;
  static const jobResult = 43004;
  static const jobCancel = 43005;
  static const jobError = 43006;
  static const forumPost = 45001;
  static const forumComment = 45003;
  static const huddleStarted = 48100;
  static const huddleParticipantJoined = 48101;
  static const huddleParticipantLeft = 48102;
  static const huddleEnded = 48103;

  // --- Projects (NIP-MP) ------------------------------------------------------

  /// Kind:30621 project announcement (addressable, `d` = slug; address is
  /// `30621:<owner-hex>:<d>`). Channels hang off it two ways: the head's own
  /// `channel` tags, and the `project` tag the relay stamps onto each
  /// channel's kind:39000 metadata.
  static const project = 30621;

  /// Kind:39010 relay-signed project roster projection (`d` = the project
  /// address).
  static const projectRoster = 39010;

  // --- Project to-dos (NIP-TD) ------------------------------------------------
  // Keep in sync with `crates/beekeeper-core/src/kind.rs` and
  // `desktop/src/shared/constants/kinds.ts`. See `docs/nips/NIP-TD.md`.

  /// Kind:44248 project to-do op: one field-level edit to a shared,
  /// project-scoped to-do list. Regular, append-only, scoped by a canonical
  /// `a` coordinate (`30621:<owner>:<dtag>`); never carries an `h` tag.
  static const projectTodoOp = 44248;

  // --- Agents-repository drafts (NIP-AD) ------------------------------------
  // Keep in sync with `crates/beekeeper-core/src/kind.rs` and
  // `desktop/src/shared/constants/kinds.ts`. See `docs/nips/NIP-AD.md`.

  /// Kind:44250 agents-repository draft op: one proposed change to one file
  /// of the project's agents repository, or the committer's record that
  /// named drafts landed on `main`. Same project-scoped gate as 44248.
  static const agentsRepoDraftOp = 44250;

  /// Kind:44251 project artifact pin op (NIP-AR): which documents, plans and
  /// folders of the project's agents repository show in every member's
  /// sidebar, and in what order. Its own kind rather than another 44250 op,
  /// because 44250's fold is a per-path draft chain a `commit.record` closes
  /// and a pin must never be closed by a commit.
  static const projectArtifactPinOp = 44251;

  /// Kind:30624 project pack source (NIP-PK): the repository the project's
  /// roles, plans and manifests are staged from; `d` = the project
  /// coordinate.
  static const projectPackSource = 30624;

  /// Kind:30618 repository ref state, relay-signed on every push; `d` = the
  /// repository id. What tells a reader `main` moved.
  static const repoState = 30618;

  // --- Shared terminals (NIP-ST) ---------------------------------------------
  // Keep in sync with `crates/beekeeper-core/src/kind.rs` and
  // `desktop/src/shared/constants/kinds.ts`. See `docs/nips/NIP-ST.md`.

  /// Kind:30623 shared-terminal announce (addressable, `d` = session id).
  /// Owner-signed; carries the project address in `a`, `title`, `status`
  /// (`open`|`closed`), `dims` (`<rows>x<cols>`) and the per-session roster as
  /// arity-4 `p` tags (`collaborator`|`viewer`). Deliberately no cwd.
  static const shellSession = 30623;

  /// Kind:24310 watch (ephemeral, observer → owner): `{"action":"watch"|"stop"
  /// |"resync"}`, sent on open and every 15 s while a terminal is on screen.
  static const shellWatch = 24310;

  /// Kind:24311 frame (ephemeral, owner → observers): base64 raw terminal
  /// bytes with `t` (`tail`|`snap`|`diff`|`resize`|`end`), `seq`, `epoch`.
  static const shellFrame = 24311;

  /// Kind:24312 input (ephemeral, roster collaborator → owner): base64 raw
  /// input bytes for the owner's PTY, ≤ 8 KiB of base64 per event.
  static const shellInput = 24312;

  // --- Shared observation (NIP-SW, NIP-SP, NIP-SDV) --------------------------
  // Keep in sync with `crates/beekeeper-core/src/kind.rs` and
  // `desktop/src/shared/constants/kinds.ts`. Mobile neither reads nor writes
  // these yet; the integers are mirrored so the tables cannot drift.

  /// Kind:24320 surface watch (ephemeral, member → producer), shared by the
  /// preview and device surfaces (`surface` tag).
  static const surfaceWatch = 24320;

  /// Kind:24321 surface frame (ephemeral, producer → session channel): a
  /// base64 JPEG, accepted only from the announced producer.
  static const surfaceFrame = 24321;

  /// Kind:30626 session preview announce (addressable, `d` = sessionRef).
  static const sessionPreviewAnnounce = 30626;

  /// Kind:44253 surface snapshot record (preview or device).
  static const surfaceSnapshot = 44253;

  /// Kind:44254 session device command.
  static const sessionDeviceCommand = 44254;

  /// Kind:44255 provider-signed session device record.
  static const sessionDeviceRecord = 44255;

  // --- Coding sessions (44220-44231, 44244-44247, 44252, 24223) -------------
  // Keep in sync with `desktop/src/shared/constants/kinds.ts`. Mobile reads
  // the fact kinds and, since 2026-09-07, also publishes the member-signed
  // command kinds (44220 turns, 44221 stop, 44227/44229/44230 goal, name,
  // closure) from `features/coding_sessions/domain/coding_session_commands.dart`.
  // It still never signs a provider fact, and it cannot create a session:
  // custody of agent keys is desktop-host-local (CREW_SESSIONS_PLAN D6).

  /// Kind:44220 governed turn command addressed to a coding-session provider.
  static const codingSessionCommand = 44220;

  /// Kind:44221 lifecycle command (`session.create` / `resume` / `stop`).
  static const codingSessionLifecycleCommand = 44221;

  /// Kind:44222 provider catalog of runtimes and models it can serve.
  static const codingSessionProviderCatalog = 44222;

  /// Kind:44223 per-generation provider metadata (status, runtime, model).
  static const codingSessionMetadata = 44223;

  /// Kind:44224 lifecycle/turn receipt answering a 44220 or 44221.
  static const codingSessionLifecycleReceipt = 44224;

  /// Kind:44225 transcript envelope carrying one transcript item.
  static const codingSessionTranscript = 44225;

  /// Kind:44226 umbrella-session genesis; its signer is the founder.
  static const codingSessionGenesis = 44226;

  /// Kind:44227 umbrella-session goal (addressable by `d` = sessionRef).
  static const codingSessionGoal = 44227;

  /// Kind:44228 authority transition within a session roster.
  static const codingSessionAuthorityTransition = 44228;

  /// Kind:44229 umbrella-session display name (addressable by `d`).
  static const codingSessionName = 44229;

  /// Kind:44230 umbrella-session closure marker (addressable by `d`).
  static const codingSessionClosure = 44230;

  /// Kind:44231 turn checkpoint (NIP-CSCK) — the provider-measured working
  /// tree at the end of one turn: tree and commit SHAs, the transcript range
  /// it covers, and the files the turn changed, or why none could be read.
  ///
  /// Mobile neither writes nor reads the record yet; the integer is mirrored
  /// here because this table and
  /// `desktop/src/shared/constants/kinds.ts` must not drift (CLAUDE.md).
  static const codingSessionCheckpoint = 44231;

  /// Kind:44244 signed, append-only team transaction within a session.
  static const codingSessionTeamTransaction = 44244;

  /// Kind:44245 session policy (NIP-CSP), addressable by `d` = sessionRef.
  ///
  /// Mobile neither writes nor reads the record yet; the integer is mirrored
  /// here because this table and
  /// `desktop/src/shared/constants/kinds.ts` must not drift (CLAUDE.md).
  static const codingSessionPolicy = 44245;

  /// Kind:44246 observation (NIP-CSOB) — a checkpoint, a gate row, a finding,
  /// or a phase's own measured span.
  ///
  /// Mobile neither writes nor reads the record yet; the integer is mirrored
  /// here because this table and
  /// `desktop/src/shared/constants/kinds.ts` must not drift (CLAUDE.md).
  static const codingSessionObservation = 44246;

  /// Kind:44247 handover (NIP-CSH) — a durable checkpoint of the work, or the
  /// record of a claimant continuing it.
  ///
  /// Mobile neither writes nor reads the record yet; the integer is mirrored
  /// here because this table and
  /// `desktop/src/shared/constants/kinds.ts` must not drift (CLAUDE.md).
  static const codingSessionHandover = 44247;

  /// Kind:44252 provider-signed generated session title (NIP-CSG § Generated
  /// title).
  ///
  /// A model's words, signed by the provider that ran the founder's first
  /// turn — never a person's name, which stays kind:44229. Readers rank the
  /// two with `resolveCodingSessionDisplayName`
  /// (`features/coding_sessions/domain/coding_session_title.dart`), the
  /// shared rule pinned by `conformance/session-display-name/`.
  static const codingSessionGeneratedTitle = 44252;

  /// Kind:24223 ephemeral provider lease proving the provider is reachable.
  static const codingSessionLease = 24223;

  /// Kind:40099 relay receipt / system message.
  ///
  /// Same integer as [systemMessage]; named here for the coding-session roster
  /// read, which asks the relay for its receipts alongside kind:44228.
  static const relayReceipt = systemMessage;

  /// Event kinds that represent user-visible channel messages.
  static const channelMessageEventKinds = [
    streamMessage, // 9
    streamMessageV2, // 40002
    forumPost, // 45001
    forumComment, // 45003
  ];

  /// Event kinds that represent channel activity (messages, edits, reactions,
  /// deletions, system events). Matches the desktop's `CHANNEL_EVENT_KINDS`.
  static const channelEventKinds = [
    deletion, // 5
    reaction, // 7
    nip29DeleteEvent, // 9005 — Beekeeper-native deletion
    ...channelMessageEventKinds,
    40001, // legacy pre-migration stream messages
    streamMessageEdit, // 40003
    streamMessageDiff, // 40008
    systemMessage, // 40099
    huddleStarted, // 48100 — visible huddle session row
    huddleParticipantJoined, // 48101 — huddle lifecycle metadata
    huddleParticipantLeft, // 48102 — huddle lifecycle metadata
    huddleEnded, // 48103 — visible huddle ended row
  ];

  /// Auxiliary timeline kinds that overlay or hide existing rows.
  static const channelAuxEventKinds = [
    deletion,
    reaction,
    nip29DeleteEvent,
    streamMessageEdit,
  ];

  /// Visible content kinds requested by the NIP-CW channel-window path.
  static const channelTimelineContentKinds = [
    streamMessage,
    streamMessageV2,
    streamMessageDiff,
    systemMessage,
    jobRequest,
    jobAccepted,
    jobProgress,
    jobResult,
    jobCancel,
    jobError,
    huddleStarted,
  ];
}

/// A Nostr event as defined by NIP-01.
@immutable
class NostrEvent {
  final String id;
  final String pubkey;
  final int createdAt;
  final int kind;
  final List<List<String>> tags;
  final String content;
  final String sig;

  const NostrEvent({
    required this.id,
    required this.pubkey,
    required this.createdAt,
    required this.kind,
    required this.tags,
    required this.content,
    required this.sig,
  });

  factory NostrEvent.fromJson(Map<String, dynamic> json) {
    return NostrEvent(
      id: json['id'] as String,
      pubkey: json['pubkey'] as String,
      createdAt: json['created_at'] as int,
      kind: json['kind'] as int,
      tags: (json['tags'] as List<dynamic>)
          .map((t) => (t as List<dynamic>).map((e) => e as String).toList())
          .toList(),
      content: json['content'] as String,
      sig: json['sig'] as String,
    );
  }

  Map<String, dynamic> toJson() => {
    'id': id,
    'pubkey': pubkey,
    'created_at': createdAt,
    'kind': kind,
    'tags': tags,
    'content': content,
    'sig': sig,
  };

  /// Get the first value for a given tag key.
  String? getTagValue(String key) {
    for (final tag in tags) {
      if (tag.isNotEmpty && tag[0] == key && tag.length > 1) {
        return tag[1];
      }
    }
    return null;
  }

  /// The channel/group ID from the `h` tag (NIP-29).
  String? get channelId => getTagValue('h');

  /// Extract thread parent and root IDs from `e` tags.
  ///
  /// Matches the desktop's `getThreadReference` logic:
  /// - Tags with marker `"reply"` identify the direct parent.
  /// - Tags with marker `"root"` identify the thread root.
  /// - If no markers are present, falls back to null (top-level message).
  ({String? parentId, String? rootId}) get threadReference {
    final eTags = [
      for (final tag in tags)
        if (tag.length >= 2 && tag[0] == 'e') tag,
    ];

    if (eTags.isEmpty) return (parentId: null, rootId: null);

    // Find tagged root and reply markers (desktop convention).
    List<String>? rootTag;
    List<String>? replyTag;
    for (final tag in eTags) {
      if (tag.length >= 4) {
        if (tag[3] == 'root') rootTag = tag;
        if (tag[3] == 'reply') replyTag = tag;
      }
    }

    if (replyTag == null) return (parentId: null, rootId: null);

    final parentId = replyTag[1];
    final rootId = rootTag?[1] ?? parentId;
    return (parentId: parentId, rootId: rootId);
  }

  /// The parent event ID from the `e` tag.
  String? get parentEventId => threadReference.parentId;

  @override
  bool operator ==(Object other) =>
      identical(this, other) || other is NostrEvent && id == other.id;

  @override
  int get hashCode => id.hashCode;
}

/// A NIP-01 subscription filter.
@immutable
class NostrFilter {
  final List<int> kinds;
  final List<String>? authors;

  /// Specific event IDs (NIP-01 single-event lookup).
  final List<String>? ids;
  final int limit;
  final int? since;
  final int? until;

  /// NIP-50 full-text search query.
  final String? search;

  /// Tag filters, e.g. `{'#h': ['channel-id']}`.
  final Map<String, List<String>> tags;

  /// Beekeeper relay bridge filter extensions (for example NIP-CW `top_level`).
  final Map<String, Object?> extensions;

  const NostrFilter({
    required this.kinds,
    this.authors,
    this.ids,
    this.limit = 100,
    this.since,
    this.until,
    this.search,
    this.tags = const {},
    this.extensions = const {},
  });

  /// Return a copy with an updated `since` value.
  NostrFilter copyWithSince(int since) => NostrFilter(
    kinds: kinds,
    authors: authors,
    ids: ids,
    limit: limit,
    since: since,
    until: until,
    search: search,
    tags: tags,
    extensions: extensions,
  );

  Map<String, dynamic> toJson() {
    final json = <String, dynamic>{'kinds': kinds, 'limit': limit};
    if (authors != null) json['authors'] = authors;
    if (ids != null) json['ids'] = ids;
    if (since != null) json['since'] = since;
    if (until != null) json['until'] = until;
    if (search != null) json['search'] = search;
    for (final entry in tags.entries) {
      json[entry.key] = entry.value;
    }
    for (final entry in extensions.entries) {
      json[entry.key] = entry.value;
    }
    return json;
  }
}

/// Parsed kind:0 user profile metadata.
@immutable
class ProfileData {
  final String pubkey;
  final String? displayName;
  final String? avatarUrl;
  final String? about;
  final String? nip05;

  const ProfileData({
    required this.pubkey,
    this.displayName,
    this.avatarUrl,
    this.about,
    this.nip05,
  });

  factory ProfileData.fromEvent(NostrEvent event) {
    Map<String, dynamic> meta = {};
    try {
      final decoded = jsonDecode(event.content);
      if (decoded is Map<String, dynamic>) meta = decoded;
    } catch (_) {}
    return ProfileData(
      pubkey: event.pubkey,
      displayName:
          (meta['display_name'] as String?) ?? (meta['name'] as String?),
      avatarUrl: meta['picture'] as String?,
      about: meta['about'] as String?,
      nip05: meta['nip05'] as String?,
    );
  }
}

final _projectAddress = RegExp(r'^30621:[0-9a-f]{64}:\S+$');

/// True when [value] is a well-formed NIP-MP project address
/// (`30621:<lowercase 64-hex owner>:<slug>`).
///
/// The relay only requires the slug to be a single non-empty `d` tag
/// (`validate_project_envelope`), so this checks shape, not vocabulary.
bool isProjectAddress(String value) => _projectAddress.hasMatch(value);

/// Parsed kind:39000 channel metadata.
@immutable
class ChannelData {
  final String id;
  final String name;
  final String channelType;
  final String visibility;
  final String description;
  final String? topic;
  final List<String> participantPubkeys;
  final int? ttlSeconds;
  final DateTime? ttlDeadline;
  final bool isArchived;

  /// The project this channel belongs to, as a `30621:<owner>:<d>` address.
  ///
  /// Read from the `["project", …]` tag the relay stamps onto the metadata it
  /// signs (`crates/beekeeper-relay/src/handlers/side_effects.rs`); `null` for a
  /// channel no project claims. Only the relay's own back-reference — a
  /// project head's forward `channel` tags are unioned in by the reader.
  final String? projectRef;

  const ChannelData({
    required this.id,
    required this.name,
    required this.channelType,
    required this.visibility,
    required this.description,
    this.topic,
    this.participantPubkeys = const [],
    this.ttlSeconds,
    this.ttlDeadline,
    this.isArchived = false,
    this.projectRef,
  });

  factory ChannelData.fromEvent(NostrEvent event) {
    final id = event.getTagValue('d') ?? '';
    final name = event.getTagValue('name') ?? '';
    // Prefer explicit ["t", type]; fall back to ["hidden"] => dm, else "stream".
    // The fallback exists for relays that haven't been upgraded to emit the
    // explicit type tag yet.
    final explicitType = event.getTagValue('t');
    final hasHidden = event.tags.any((t) => t.isNotEmpty && t[0] == 'hidden');
    final channelType = explicitType ?? (hasHidden ? 'dm' : 'stream');
    // Prefer explicit ["public"]; fall back to NIP-29 absence-of-"private".
    final hasPublic = event.tags.any((t) => t.isNotEmpty && t[0] == 'public');
    final hasPrivate = event.tags.any((t) => t.isNotEmpty && t[0] == 'private');
    final visibility = hasPublic
        ? 'open'
        : hasPrivate
        ? 'private'
        : 'open';
    final description = event.getTagValue('about') ?? '';
    final topic = event.getTagValue('topic');
    final participants = [
      for (final t in event.tags)
        if (t.length >= 2 && t[0] == 'p') t[1],
    ];
    final ttlRaw = event.getTagValue('ttl');
    final ttlSeconds = ttlRaw != null ? int.tryParse(ttlRaw) : null;
    final ttlDeadlineRaw = event.getTagValue('ttl_deadline');
    final ttlDeadline = ttlDeadlineRaw != null
        ? DateTime.tryParse(ttlDeadlineRaw)
        : null;
    // Relay republishes kind:39000 with `["archived", "true"]` when a channel
    // is archived (including the auto-archive emitted by the TTL reaper). The
    // tag value "false" is also accepted server-side, so only treat "true" as
    // archived — anything else (missing tag, "false", unexpected value) means
    // active.
    final isArchived = event.getTagValue('archived') == 'true';
    // A project address is `30621:<64-hex owner>:<slug>`; anything else in the
    // tag is a malformed claim and reads as no project, never as one.
    final projectRaw = event.getTagValue('project');
    final projectRef = projectRaw != null && isProjectAddress(projectRaw)
        ? projectRaw
        : null;
    return ChannelData(
      id: id,
      name: name,
      channelType: channelType,
      visibility: visibility,
      description: description,
      topic: topic,
      participantPubkeys: participants,
      ttlSeconds: ttlSeconds,
      ttlDeadline: ttlDeadline,
      isArchived: isArchived,
      projectRef: projectRef,
    );
  }
}

/// A single member entry parsed from a kind:39002 members event.
@immutable
class MemberEntry {
  final String pubkey;
  final String role;

  const MemberEntry({required this.pubkey, required this.role});
}

/// Parse a kind:39002 members event into the list of `(pubkey, role)` entries.
///
/// NIP-29 members tags follow the shape `["p", <pubkey>, <relay>, <role>]`.
List<MemberEntry> membersFromEvent(NostrEvent event) {
  return [
    for (final t in event.tags)
      if (t.length >= 2 && t[0] == 'p')
        MemberEntry(pubkey: t[1], role: t.length >= 4 ? t[3] : 'member'),
  ];
}

/// Parse a Beekeeper command response from the relay's OK message content.
///
/// Command kinds (e.g. 41010, 30620, 46020) return `"response:{...}"` in the
/// OK message. Returns `null` if the message is not a command response or the
/// JSON is invalid.
Map<String, dynamic>? parseCommandResponse(String message) {
  // Try the spec format first: "response:{...}".
  const prefix = 'response:';
  if (message.startsWith(prefix)) {
    try {
      final decoded = jsonDecode(message.substring(prefix.length));
      if (decoded is Map<String, dynamic>) return decoded;
    } catch (_) {}
    return null;
  }
  // Fallback: raw JSON object (older relays, backward compat).
  try {
    final decoded = jsonDecode(message);
    if (decoded is Map<String, dynamic>) return decoded;
  } catch (_) {}
  return null;
}
