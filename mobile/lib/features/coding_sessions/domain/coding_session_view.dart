import 'package:flutter/foundation.dart';

import '../../../shared/relay/nostr_models.dart';
import 'coding_session_fold.dart';
import 'coding_session_signature.dart';
import 'coding_session_transcript.dart';
import 'coding_session_transcript_item.dart';
import 'coding_session_trust.dart';

/// Raw events retained per generation before the oldest are evicted.
///
/// The cap applies per bucket, and `CodingSessionEventStore` gives the two
/// low-volume targeted kinds (44223 metadata, 24223 leases) a bucket of their
/// own, so a long transcript cannot evict the facts that give a generation its
/// status and its proof that a provider is answering.
const maxCodingSessionEventsPerGeneration = 2000;

/// The `limit` one history page asks for.
///
/// A page that comes back full means there is older history this observer did
/// not fetch, which the UI reports as "history truncated at 1000" rather than
/// presenting a partial transcript as a whole one.
const codingSessionHistoryPageLimit = 1000;

/// Everything one channel's coding-session read produced.
@immutable
class CodingSessionChannelView {
  final String channelId;

  /// Umbrella sessions, most recently active first.
  final List<CodingSessionUmbrella> sessions;

  /// The facts the trust gate accepted, kept so transcript and reachability
  /// can be derived per session without a second decode pass.
  final CodingSessionTrustedFacts facts;

  /// True when a lease query has come back for this channel.
  ///
  /// False makes every reachability read
  /// [CodingSessionReachabilityKind.unknown]: not knowing is not the same as
  /// nobody answering.
  final bool leasesRead;

  /// True when a history page came back full, so older facts exist unread.
  final bool historyTruncated;

  const CodingSessionChannelView({
    required this.channelId,
    required this.sessions,
    required this.facts,
    required this.leasesRead,
    required this.historyTruncated,
  });

  /// What the read had to throw away.
  CodingSessionReadCounts get counts => facts.counts;

  /// False when this device could not verify signatures at all.
  bool get signaturesVerified => facts.signaturesVerified;

  /// The session with [key], or `null`.
  CodingSessionUmbrella? sessionByKey(String key) {
    for (final session in sessions) {
      if (session.key == key) return session;
    }
    return null;
  }

  /// The session claiming [sessionRef], or `null`.
  CodingSessionUmbrella? sessionByRef(String sessionRef) {
    for (final session in sessions) {
      if (session.sessionRef == sessionRef) return session;
    }
    return null;
  }

  /// The transcript of [session], as interleaved per-execution blocks.
  List<CodingSessionTranscriptBlock> transcriptFor(
    CodingSessionUmbrella session,
  ) {
    final targetKeys = {
      for (final execution in session.executions) execution.targetKey,
    };
    final labels = {
      for (final execution in session.executions)
        execution.targetKey: execution.label,
    };
    return projectCodingSessionTranscript(
      facts.transcripts.where(
        (envelope) => targetKeys.contains(envelope.target.key),
      ),
      labelsByTargetKey: labels,
    );
  }

  /// Whether a provider is answering for [session]'s current generation.
  ///
  /// Reachability is a per-execution fact; a session with several executions
  /// is reachable when any current generation has a live lease, and unknown
  /// when no execution can answer either way.
  CodingSessionReachability reachabilityFor(
    CodingSessionUmbrella session, {
    required DateTime now,
  }) {
    var best = CodingSessionReachability.unknown;
    for (final execution in session.executions) {
      if (!execution.isCurrentGeneration) continue;
      final verdict = deriveCodingSessionReachability(
        leases: facts.leases,
        currentTarget: execution.target,
        acceptedCommandId: execution.commandId,
        authorityPubkey: execution.authority.pubkey,
        now: now,
        leasesRead: leasesRead,
      );
      if (verdict.kind == CodingSessionReachabilityKind.reachable) {
        return verdict;
      }
      if (best.kind == CodingSessionReachabilityKind.unknown &&
          verdict.kind == CodingSessionReachabilityKind.noProviderAnswering) {
        best = verdict;
      }
    }
    return best;
  }
}

/// Decode, gate and fold one channel's coding-session events in one pass.
///
/// This is the seam the provider layer builds on: hand it every coding-session
/// event the channel has produced and it returns the sessions, the transcript
/// source, and an honest account of everything it refused.
CodingSessionChannelView readCodingSessionChannel({
  required String channelId,
  required Iterable<NostrEvent> events,
  CodingSessionSignatureVerifier? verifier =
      const NostrPackageSignatureVerifier(),
  bool leasesRead = true,
  bool historyTruncated = false,
}) {
  final facts = applyCodingSessionTrustGate(
    channelId: channelId,
    events: events,
    verifier: verifier,
  );
  final executions = resolveCodingSessionGenerations(
    receipts: facts.receipts,
    metadata: facts.metadata,
    transcripts: facts.transcripts,
    creates: facts.creates,
    authorityByTarget: facts.authorityByTarget,
  );
  final sessions = groupCodingSessionUmbrellas(
    executions: executions,
    creates: facts.creates,
    targetKeyByCommandId: facts.targetKeyByCommandId,
    genesesByEventId: facts.genesesByEventId,
    names: facts.names,
    goals: facts.goals,
    closures: facts.closures,
  );
  return CodingSessionChannelView(
    channelId: channelId,
    sessions: sessions,
    facts: facts,
    leasesRead: leasesRead,
    historyTruncated: historyTruncated,
  );
}

/// Evict the oldest raw events for a generation past the retention cap.
///
/// Retention is per generation because a generation is the unit a transcript
/// is read in: trimming across generations would silently truncate the middle
/// of one session's history to make room for another's.
List<NostrEvent> retainNewestCodingSessionEvents(
  Iterable<NostrEvent> events, {
  int cap = maxCodingSessionEventsPerGeneration,
}) {
  final ordered = [...events]
    ..sort((left, right) {
      final byTime = left.createdAt.compareTo(right.createdAt);
      return byTime != 0 ? byTime : left.id.compareTo(right.id);
    });
  if (ordered.length <= cap) return List.unmodifiable(ordered);
  return List.unmodifiable(ordered.sublist(ordered.length - cap));
}
