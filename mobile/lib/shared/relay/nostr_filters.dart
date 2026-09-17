import 'nostr_models.dart';

/// Canonical [NostrFilter] constructors for common Buzz queries.
///
/// Centralising filter shapes keeps relay queries consistent across providers
/// and makes kind/tag conventions easy to audit.
abstract final class NostrFilters {
  /// Channels where I'm a member (kind:39002 with `#p` = my pubkey).
  static NostrFilter myChannels(String myPk) => NostrFilter(
    kinds: [39002],
    tags: {
      '#p': [myPk],
    },
    limit: 500,
  );

  /// Channel metadata for the given channel IDs.
  static NostrFilter channelMetadata(List<String> ids) =>
      NostrFilter(kinds: [39000], tags: {'#d': ids}, limit: ids.length);

  /// Members list for a single channel.
  static NostrFilter channelMembers(String channelId) => NostrFilter(
    kinds: [39002],
    tags: {
      '#d': [channelId],
    },
    limit: 1,
  );

  /// A single user's profile (kind:0).
  static NostrFilter profile(String pubkey) =>
      NostrFilter(kinds: [0], authors: [pubkey], limit: 1);

  /// Batch user profiles (kind:0) for multiple pubkeys.
  static NostrFilter profilesBatch(List<String> pubkeys) =>
      NostrFilter(kinds: [0], authors: pubkeys, limit: pubkeys.length);

  /// Channel messages (all event kinds that appear in channels).
  static NostrFilter messages(
    String channelId, {
    int limit = 200,
    int? until,
  }) => NostrFilter(
    kinds: EventKind.channelEventKinds,
    tags: {
      '#h': [channelId],
    },
    limit: limit,
    until: until,
  );

  /// Reactions (kind:7) on a specific event.
  static NostrFilter reactions(String eventId) => NostrFilter(
    kinds: [7],
    tags: {
      '#e': [eventId],
    },
  );

  /// Canvas event for a channel.
  static NostrFilter canvas(String channelId) => NostrFilter(
    kinds: [40100],
    tags: {
      '#h': [channelId],
    },
    limit: 1,
  );

  /// Workflows (kind:30620) in a channel.
  static NostrFilter workflows(String channelId) => NostrFilter(
    kinds: [30620],
    tags: {
      '#h': [channelId],
    },
  );

  /// DM channels where I'm a participant.
  static NostrFilter dmList(String myPk) => NostrFilter(
    kinds: [39000],
    tags: {
      '#t': ['dm'],
      '#p': [myPk],
    },
  );

  /// Latest per-viewer hidden-DM snapshot (kind:30622, `#p` = my pubkey).
  static NostrFilter hiddenDms(String myPk) => NostrFilter(
    kinds: [EventKind.dmVisibility],
    tags: {
      '#p': [myPk],
    },
    limit: 1,
  );

  /// Forum posts (kind:45001) in a channel.
  static NostrFilter forumPosts(
    String channelId, {
    int limit = 50,
    int? until,
  }) => NostrFilter(
    kinds: [45001],
    tags: {
      '#h': [channelId],
    },
    limit: limit,
    until: until,
  );

  /// Replies in a forum thread (root event id + channel scope).
  static NostrFilter forumThread(String rootId, String channelId) =>
      NostrFilter(
        kinds: [9, 45003],
        tags: {
          '#e': [rootId],
          '#h': [channelId],
        },
      );

  /// NIP-50 message search, optionally scoped to a channel.
  static NostrFilter searchMessages(
    String query, {
    String? channelId,
    int limit = 20,
  }) => NostrFilter(
    kinds: [9, 40002, 45001, 45003],
    tags: channelId != null
        ? {
            '#h': [channelId],
          }
        : const {},
    search: query,
    limit: limit,
  );

  /// Global user search over kind:0 profiles (NIP-50 via the HTTP bridge).
  ///
  /// `search_mode: "prefix"` is a Buzz bridge-only extension: every caller is
  /// a typeahead surface, so a partially typed name must match ("rac" →
  /// "raccoon"). Mirrors desktop's `build_user_search_filter`
  /// (desktop/src-tauri/src/commands/profile.rs). Bridge-only — send through
  /// `queryRelay`, not a WebSocket REQ.
  static NostrFilter searchUsers(String query, {int limit = 50}) => NostrFilter(
    kinds: [0],
    search: query,
    limit: limit,
    extensions: const {'search_mode': 'prefix'},
  );

  /// Deletions (kind:5) targeting event IDs.
  static NostrFilter deletionsByTargetIds(
    List<String> ids, {
    List<String>? authors,
  }) => NostrFilter(
    kinds: [EventKind.deletion],
    authors: authors,
    tags: {'#e': ids},
    limit: ids.length,
  );

  /// User notes (kind:1) for the global Pulse timeline.
  static NostrFilter globalNotes({int limit = 50, int? until}) =>
      NostrFilter(kinds: [EventKind.note], limit: limit, until: until);

  /// Notes by a set of authors for Pulse timelines.
  static NostrFilter notesTimeline(
    List<String> pubkeys, {
    int limit = 200,
    int? until,
  }) => NostrFilter(
    kinds: [EventKind.note],
    authors: pubkeys,
    limit: limit,
    until: until,
  );

  /// Reactions authored by a user.
  static NostrFilter userReactions(String pubkey, {int limit = 200}) =>
      NostrFilter(kinds: [EventKind.reaction], authors: [pubkey], limit: limit);

  /// Reactions targeting notes.
  static NostrFilter noteReactions(List<String> noteIds) => NostrFilter(
    kinds: [EventKind.reaction],
    tags: {'#e': noteIds},
    limit: 500,
  );

  /// Fetch notes by ids.
  static NostrFilter notesByIds(List<String> ids) =>
      NostrFilter(kinds: [EventKind.note], ids: ids, limit: ids.length);

  /// User notes (kind:1) for a single author.
  static NostrFilter userNotes(String pubkey, {int limit = 20, int? until}) =>
      NostrFilter(
        kinds: [EventKind.note],
        authors: [pubkey],
        limit: limit,
        until: until,
      );

  /// Contact list (kind:3) for a user.
  static NostrFilter contactList(String pubkey) =>
      NostrFilter(kinds: [EventKind.contactList], authors: [pubkey], limit: 1);

  /// Relay membership list (kind:13534).
  static NostrFilter relayMembers() =>
      const NostrFilter(kinds: [EventKind.relayMembership], limit: 1);

  /// Agent profiles (kind:10100).
  static NostrFilter agentProfiles() =>
      const NostrFilter(kinds: [10100], limit: 100);

  /// User status (NIP-38, kind:30315).
  static NostrFilter userStatus(String pubkey) =>
      NostrFilter(kinds: [30315], authors: [pubkey], limit: 1);

  // --- Coding sessions (read-only observer) --------------------------------
  //
  // Every coding-session filter carries explicit `kinds` and `#h`: the relay's
  // p-gate answers 403 to a filter without kinds, and the coding-session read
  // is always scoped to one channel. None of them narrow by `authors` —
  // authority is open (channel membership), and which signer may be trusted
  // for a given target is decided after decode by the trust gate, not by the
  // relay query.

  /// History read for the three per-generation fact kinds (44223/44224/44225).
  static NostrFilter codingSessionFacts(String channelId, {int limit = 1000}) =>
      NostrFilter(
        kinds: const [
          EventKind.codingSessionMetadata,
          EventKind.codingSessionLifecycleReceipt,
          EventKind.codingSessionTranscript,
        ],
        tags: {
          '#h': [channelId],
        },
        limit: limit,
      );

  /// Live subscription twin of [codingSessionFacts] (`limit: 0`).
  static NostrFilter codingSessionFactsLive(String channelId) => NostrFilter(
    kinds: const [
      EventKind.codingSessionMetadata,
      EventKind.codingSessionLifecycleReceipt,
      EventKind.codingSessionTranscript,
    ],
    tags: {
      '#h': [channelId],
    },
    limit: 0,
  );

  /// The provider catalogs (44222) advertised in one channel — what a
  /// create's provider picker lists. Newest first; a provider re-advertises
  /// on every revision, so a small window holds every live signer.
  static NostrFilter codingSessionProviderCatalogs(
    String channelId, {
    int limit = 50,
  }) => NostrFilter(
    kinds: const [EventKind.codingSessionProviderCatalog],
    tags: {
      '#h': [channelId],
    },
    limit: limit,
  );

  /// Creation evidence: one filter per kind (44221, 44224, 44226).
  ///
  /// Deliberately three filters rather than one three-kind filter: a single
  /// filter shares one `limit` across the kinds, so a chatty receipt stream
  /// would starve the creates and geneses that authority resolution needs.
  static List<NostrFilter> codingSessionCreates(
    String channelId, {
    int limit = 1000,
  }) => [
    for (final kind in const [
      EventKind.codingSessionLifecycleCommand,
      EventKind.codingSessionLifecycleReceipt,
      EventKind.codingSessionGenesis,
    ])
      NostrFilter(
        kinds: [kind],
        tags: {
          '#h': [channelId],
        },
        limit: limit,
      ),
  ];

  /// Live twin of [codingSessionCreates] for the two kinds
  /// [codingSessionFactsLive] leaves out: creates (44221) and geneses
  /// (44226), `limit: 0`. One two-kind filter is fine live — there is no page
  /// for a chattier kind to starve — and it is what lets an observer see a
  /// create published *after* its history read: this device's own, whose
  /// refusal must settle the pending row, and another device's, whose
  /// session must appear without a reload.
  static NostrFilter codingSessionCreatesLive(String channelId) => NostrFilter(
    kinds: const [
      EventKind.codingSessionLifecycleCommand,
      EventKind.codingSessionGenesis,
    ],
    tags: {
      '#h': [channelId],
    },
    limit: 0,
  );

  /// Umbrella-session display names (kind:44229).
  static NostrFilter codingSessionNames(String channelId, {int limit = 1000}) =>
      NostrFilter(
        kinds: const [EventKind.codingSessionName],
        tags: {
          '#h': [channelId],
        },
        limit: limit,
      );

  /// Umbrella-session goals (kind:44227).
  static NostrFilter codingSessionGoals(String channelId, {int limit = 1000}) =>
      NostrFilter(
        kinds: const [EventKind.codingSessionGoal],
        tags: {
          '#h': [channelId],
        },
        limit: limit,
      );

  /// Umbrella-session closures (kind:44230).
  static NostrFilter codingSessionClosures(
    String channelId, {
    int limit = 1000,
  }) => NostrFilter(
    kinds: const [EventKind.codingSessionClosure],
    tags: {
      '#h': [channelId],
    },
    limit: limit,
  );

  /// Provider leases (kind:24223), the only proof of reachability.
  ///
  /// Never paginated: a lease older than its 150 s TTL proves nothing, so a
  /// second page of expired leases would only cost bytes.
  static NostrFilter codingSessionLeases(String channelId) => NostrFilter(
    kinds: const [EventKind.codingSessionLease],
    tags: {
      '#h': [channelId],
    },
    limit: 1000,
  );

  // --- Projects (NIP-MP) ------------------------------------------------------

  /// Every project head the relay will show this reader (kind:30621).
  ///
  /// The relay withholds private projects the reader is not admitted to, so
  /// an unfiltered read is already scoped to what may be seen.
  static NostrFilter projects({int limit = 200}) =>
      NostrFilter(kinds: const [EventKind.project], limit: limit);

  /// Live twin of [projects]: heads published from now on.
  static NostrFilter projectsLive(int sinceSeconds) => NostrFilter(
    kinds: const [EventKind.project],
    since: sinceSeconds,
    limit: 100,
  );

  /// Deletions (kind:5) that may tombstone a project head.
  static NostrFilter projectTombstones({int limit = 500}) =>
      NostrFilter(kinds: const [EventKind.deletion], limit: limit);

  /// The relay-signed roster projection (kind:39010) for one project address.
  static NostrFilter projectRoster(String projectAddress) => NostrFilter(
    kinds: const [EventKind.projectRoster],
    tags: {
      '#d': [projectAddress],
    },
    limit: 1,
  );

  // --- Project to-dos (NIP-TD) ------------------------------------------------

  /// One history page of to-do ops (kind:44248) for a project coordinate,
  /// newest first. The relay pages at 1000; a caller walks older pages with
  /// [until] (inclusive, so it dedupes by id) until a page comes back short.
  static NostrFilter projectTodoOps(
    String projectAddress, {
    int limit = 500,
    int? until,
  }) => NostrFilter(
    kinds: const [EventKind.projectTodoOp],
    tags: {
      '#a': [projectAddress],
    },
    limit: limit,
    until: until,
  );

  /// Live twin of [projectTodoOps]: ops from [sinceSeconds] on. NIP-TD says
  /// `since = now - 900`, because a peer may legally stamp up to 900 s in
  /// the past; the reader dedupes by id.
  static NostrFilter projectTodoOpsLive(
    String projectAddress,
    int sinceSeconds,
  ) => NostrFilter(
    kinds: const [EventKind.projectTodoOp],
    tags: {
      '#a': [projectAddress],
    },
    since: sinceSeconds,
    limit: 100,
  );

  // --- Shared terminals (NIP-ST) ---------------------------------------------

  /// Every shared-terminal announce (kind:30623) the relay will show me.
  ///
  /// Announces of private projects I am not admitted to are withheld unless I
  /// am on the session's roster, so this is already the set I may watch.
  static NostrFilter shellSessions({int limit = 500}) =>
      NostrFilter(kinds: const [EventKind.shellSession], limit: limit);

  /// Live twin of [shellSessions]: republished heads from now on.
  static NostrFilter shellSessionsLive(int sinceSeconds) => NostrFilter(
    kinds: const [EventKind.shellSession],
    since: sinceSeconds,
    limit: 100,
  );

  /// The current announce head of one terminal, by its owner and session id.
  ///
  /// Pinned to `authors` so a member's forged head for someone else's session
  /// never reaches the roster check.
  static NostrFilter shellSessionHead(String ownerPubkey, String sessionId) =>
      NostrFilter(
        kinds: const [EventKind.shellSession],
        authors: [ownerPubkey],
        tags: {
          '#d': [sessionId],
        },
        limit: 1,
      );

  /// The frame stream (kind:24311) of one terminal.
  ///
  /// The `authors` constraint is what makes frame spoofing by other members
  /// ineffective by construction (NIP-ST § Client behavior). Frames are
  /// ephemeral, so `since` only matters for the relay's in-memory tail.
  static NostrFilter shellFrames(
    String ownerPubkey,
    String sessionId, {
    required int sinceSeconds,
  }) => NostrFilter(
    kinds: const [EventKind.shellFrame],
    authors: [ownerPubkey],
    tags: {
      '#d': [sessionId],
    },
    since: sinceSeconds,
    limit: 500,
  );

  /// Roster reads: authority transitions (44228) and relay receipts (40099).
  static List<NostrFilter> codingSessionRoster(
    String channelId, {
    int limit = 500,
  }) => [
    for (final kind in const [
      EventKind.codingSessionAuthorityTransition,
      EventKind.relayReceipt,
    ])
      NostrFilter(
        kinds: [kind],
        tags: {
          '#h': [channelId],
        },
        limit: limit,
      ),
  ];
}
