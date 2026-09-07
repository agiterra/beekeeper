import 'package:flutter/foundation.dart';

import 'coding_session_models.dart';
import 'coding_session_target.dart';
import 'coding_session_trust.dart';

/// How long a 24223 lease proves anything for.
const codingSessionLeaseTtl = Duration(seconds: 150);

/// One generation of one execution stream, with the facts folded over it.
@immutable
class CodingSessionExecution {
  final String channelId;
  final CodingSessionTarget target;

  /// Whose facts these are, and whether a create vouched for that signer.
  final CodingSessionAuthority authority;

  /// The newest reported status, or [CodingSessionStatus.unknown].
  final CodingSessionStatus status;

  /// When [status] was reported, in epoch seconds; `null` when no metadata
  /// was readable at all.
  final int? statusAt;

  /// The metadata record [status] came from, when there was one.
  final CodingSessionMetadata? metadata;

  /// The umbrella this execution belongs to, or `null` for none.
  final String? sessionRef;

  /// True when no higher generation of this stream exists in the read.
  final bool isCurrentGeneration;

  /// True when two distinct metadata payloads shared the newest second.
  ///
  /// Both were signed by the same authority and disagree; the newer-by-event-id
  /// rule still picks one to render, but the reader is told they collided.
  final bool statusConflict;

  /// Newest `created_at` of any fact for this execution, in epoch seconds.
  final int lastActivityAt;

  /// The 44221 command id that minted this generation, when one is readable.
  final String? commandId;

  const CodingSessionExecution({
    required this.channelId,
    required this.target,
    required this.authority,
    required this.status,
    required this.statusAt,
    required this.metadata,
    required this.sessionRef,
    required this.isCurrentGeneration,
    required this.statusConflict,
    required this.lastActivityAt,
    required this.commandId,
  });

  /// The signed `cs-target` key of this generation.
  String get targetKey => target.key;

  /// The generation-free identity of the stream this generation belongs to.
  String get executionKey => target.executionKey;

  /// The provider whose facts these are.
  String get signerPubkey => authority.pubkey;

  String? get runtime => metadata?.runtime;

  String? get model => metadata?.model;

  String? get agentRef => metadata?.agentRef;

  /// A short label for this execution: `runtime · model`, or the agent
  /// reference when the provider bound one.
  String get label => metadata?.label ?? target.driver;
}

/// The four ways an umbrella session's founder can resolve.
enum CodingSessionFounderResolution {
  /// A receipt-joined create named a genesis, and that genesis was readable.
  genesis,

  /// No create named a genesis; the earliest create's signer stands in.
  legacy,

  /// Two creates named different geneses. Neither wins.
  conflict,

  /// No create for this session was readable at all.
  unresolved,
}

/// Who founded an umbrella session, and how confidently.
@immutable
class CodingSessionFounder {
  /// The founder's pubkey; `null` for [CodingSessionFounderResolution.conflict]
  /// and [CodingSessionFounderResolution.unresolved].
  final String? pubkey;

  final CodingSessionFounderResolution resolution;

  /// The event id of the 44226 genesis the founder was read from; non-null
  /// exactly when [resolution] is [CodingSessionFounderResolution.genesis].
  ///
  /// A 44230 closure has to name this id, so a session whose founder was read
  /// any other way cannot be closed or reopened from this device — and says so
  /// rather than guessing at an anchor.
  final String? genesisRef;

  const CodingSessionFounder({
    required this.pubkey,
    required this.resolution,
    this.genesisRef,
  });

  /// The unresolved founder, used before any create is readable.
  static const unresolved = CodingSessionFounder(
    pubkey: null,
    resolution: CodingSessionFounderResolution.unresolved,
  );
}

/// The folded state of a whole umbrella session.
enum CodingSessionFoldedStatusKind {
  /// At least one execution is running.
  working,

  /// Nothing is running and at least one execution awaits input.
  waiting,

  /// Every execution is stopped.
  ended,

  /// Neither of the above: the most-recently-active non-stopped execution's
  /// own reported status stands.
  reported,

  /// Nothing readable to fold.
  unknown,
}

/// The result of [foldCodingSessionUmbrellaStatus].
@immutable
class CodingSessionFoldedStatus {
  final CodingSessionFoldedStatusKind kind;

  /// The underlying reported status the fold settled on, when there is one.
  final CodingSessionStatus? status;

  const CodingSessionFoldedStatus({required this.kind, this.status});

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is CodingSessionFoldedStatus &&
          kind == other.kind &&
          status == other.status;

  @override
  int get hashCode => Object.hash(kind, status);
}

/// Whether a provider is answering for the current generation, right now.
enum CodingSessionReachabilityKind {
  /// A live lease for the current generation, younger than the TTL.
  reachable,

  /// The leases were read and none proves anything: a live-sounding status
  /// must be shown as "No provider answering".
  noProviderAnswering,

  /// The leases have not been read, or two collided. Never rendered as
  /// "nobody answering" — an unknown read is not a negative answer.
  unknown,
}

/// The reachability verdict for one execution stream.
@immutable
class CodingSessionReachability {
  final CodingSessionReachabilityKind kind;

  /// Age of the lease the verdict rests on, when there was one.
  final Duration? leaseAge;

  /// Sequence of that lease, when there was one.
  final int? leaseSequence;

  /// True when two distinct leases tied at the highest sequence.
  final bool conflict;

  const CodingSessionReachability({
    required this.kind,
    this.leaseAge,
    this.leaseSequence,
    this.conflict = false,
  });

  /// The verdict before any lease query has come back.
  static const unknown = CodingSessionReachability(
    kind: CodingSessionReachabilityKind.unknown,
  );
}

/// One umbrella session: the executions that share a `sessionRef`, or the
/// generations of a single stream that claimed none.
@immutable
class CodingSessionUmbrella {
  final String channelId;

  /// Stable identity for routing; see [sessionRef] for the umbrella claim.
  final String key;

  /// The umbrella claim, or `null` for an implicit session.
  final String? sessionRef;

  /// Generations of this session, newest activity first.
  final List<CodingSessionExecution> executions;

  final CodingSessionFounder founder;

  /// The newest 44229 name, when one exists.
  final String? name;

  /// The newest 44227 goal, when one exists.
  final String? goal;

  /// True when the newest 44230 this observer may act on marks the session
  /// closed: signed by the founder of the genesis it names.
  final bool closed;

  final CodingSessionFoldedStatus status;

  /// Newest `created_at` of any fact in this session, in epoch seconds.
  final int lastActivityAt;

  const CodingSessionUmbrella({
    required this.channelId,
    required this.key,
    required this.sessionRef,
    required this.executions,
    required this.founder,
    required this.name,
    required this.goal,
    required this.closed,
    required this.status,
    required this.lastActivityAt,
  });

  /// What to call this session on screen.
  ///
  /// The newest signed name wins; failing that the newest execution's title or
  /// label. Never invented — the fallback is always something a provider or a
  /// member signed.
  String get displayName {
    final signed = name;
    if (signed != null && signed.trim().isNotEmpty) return signed;
    for (final execution in executions) {
      final title = execution.metadata?.title;
      if (title != null && title.trim().isNotEmpty) return title;
    }
    for (final execution in executions) {
      return execution.label;
    }
    return 'Coding session';
  }

  /// True when any execution's authority is the unverified fallback.
  bool get hasUnverifiedAuthority =>
      executions.any((execution) => !execution.authority.verified);
}

/// Resolve which generations exist, and fold each one's newest status.
///
/// A generation exists only when a lifecycle receipt with a
/// generation-creating status names it. The turn statuses are decoded and kept
/// for the transcript, but they never bring a generation into existence and
/// never decide its status — a `turn_refused` naming a stale generation must
/// not conjure one.
///
/// The newest 44223 wins for status: highest `created_at`, ties broken by the
/// *lower* event id. Distinct payloads sharing that second still render, with
/// [CodingSessionExecution.statusConflict] set.
List<CodingSessionExecution> resolveCodingSessionGenerations({
  required Iterable<CodingSessionReceipt> receipts,
  required Iterable<CodingSessionMetadata> metadata,
  Iterable<CodingSessionTranscriptEnvelope> transcripts = const [],
  Iterable<CodingSessionCreate> creates = const [],
  Map<String, CodingSessionAuthority> authorityByTarget = const {},
}) {
  final receiptList = receipts.toList();
  final metadataList = metadata.toList();
  final createsByCommandId = {
    for (final create in creates) create.commandId: create,
  };

  final targets = <String, CodingSessionTarget>{};
  final commandIdByTarget = <String, String>{};
  final commandRefByTarget = <String, CodingSessionEventRef>{};
  final channelByTarget = <String, String>{};
  for (final receipt in receiptList) {
    if (receipt.isTurnStage || !receipt.status.createsGeneration) continue;
    final target = receipt.session;
    if (target == null) continue;
    targets[target.key] = target;
    channelByTarget[target.key] = receipt.ref.channelId;
    // The earliest create-bearing receipt names the command that minted the
    // generation; a later resume receipt for the same target would be a
    // different command for the same key only if the provider replayed it.
    // "Earliest" is by the receipt's own signed order — `created_at`, ties
    // broken by the lower event id, the tie-break used everywhere else here —
    // never by relay-arrival order. Arrival order differs between devices
    // (history pages and live deliveries land in whatever order the relay
    // sends them), and the D8 lease command gate (`acceptedCommandId` below)
    // accepts or rejects leases by this command id, so reading it off arrival
    // order would let two devices holding the same events disagree about
    // whether a provider is answering.
    final incumbent = commandRefByTarget[target.key];
    if (incumbent == null || _isEarlier(receipt.ref, incumbent)) {
      commandRefByTarget[target.key] = receipt.ref;
      commandIdByTarget[target.key] = receipt.commandId;
    }
  }

  final currentGeneration = <String, int>{};
  for (final target in targets.values) {
    final key = target.executionKey;
    final incumbent = currentGeneration[key];
    if (incumbent == null || target.generation > incumbent) {
      currentGeneration[key] = target.generation;
    }
  }

  final executions = <CodingSessionExecution>[];
  for (final entry in targets.entries) {
    final targetKey = entry.key;
    final target = entry.value;
    final own = [
      for (final record in metadataList)
        if (record.target.key == targetKey) record,
    ];
    final newest = _newestMetadata(own);
    final conflict = _hasStatusConflict(own, newest);

    final stopReceipts = [
      for (final receipt in receiptList)
        if (!receipt.isTurnStage &&
            receipt.status == CodingSessionReceiptStatus.stopped &&
            receipt.session?.key == targetKey)
          receipt,
    ];
    final newestStop = stopReceipts.isEmpty
        ? null
        : stopReceipts.reduce(
            (left, right) =>
                left.ref.createdAt >= right.ref.createdAt ? left : right,
          );

    // A `stopped` receipt is a lifecycle fact of the same standing as
    // metadata: when it is the newer of the two, the execution is stopped even
    // if the last metadata still said running.
    var status = newest?.status ?? CodingSessionStatus.unknown;
    var statusAt = newest?.statusAt;
    if (newestStop != null &&
        (newest == null || newestStop.ref.createdAt >= newest.statusAt)) {
      status = CodingSessionStatus.stopped;
      statusAt = newestStop.ref.createdAt;
    }

    var lastActivityAt = 0;
    for (final record in own) {
      if (record.ref.createdAt > lastActivityAt) {
        lastActivityAt = record.ref.createdAt;
      }
    }
    for (final receipt in receiptList) {
      if (receipt.session?.key != targetKey) continue;
      if (receipt.ref.createdAt > lastActivityAt) {
        lastActivityAt = receipt.ref.createdAt;
      }
    }
    for (final envelope in transcripts) {
      if (envelope.target.key != targetKey) continue;
      if (envelope.ref.createdAt > lastActivityAt) {
        lastActivityAt = envelope.ref.createdAt;
      }
    }

    final commandId = commandIdByTarget[targetKey];
    final create = commandId == null ? null : createsByCommandId[commandId];
    final signerPubkey =
        authorityByTarget[targetKey]?.pubkey ??
        newest?.ref.signerPubkey ??
        create?.providerAuthorityPubkey ??
        '';
    executions.add(
      CodingSessionExecution(
        channelId: channelByTarget[targetKey] ?? '',
        target: target,
        authority:
            authorityByTarget[targetKey] ??
            CodingSessionAuthority(pubkey: signerPubkey, verified: false),
        status: status,
        statusAt: statusAt,
        metadata: newest,
        sessionRef: newest?.sessionRef ?? create?.sessionRef,
        isCurrentGeneration:
            currentGeneration[target.executionKey] == target.generation,
        statusConflict: conflict,
        lastActivityAt: lastActivityAt,
        commandId: commandId,
      ),
    );
  }
  executions.sort((left, right) {
    final byActivity = right.lastActivityAt.compareTo(left.lastActivityAt);
    return byActivity != 0
        ? byActivity
        : left.targetKey.compareTo(right.targetKey);
  });
  return List.unmodifiable(executions);
}

/// Group executions into umbrella sessions and resolve each one's founder,
/// name, goal and closed state.
///
/// [targetKeyByCommandId] is the receipt join from the trust gate: the
/// execution each create actually minted, as its own named provider reported
/// it. A create absent from that map was never answered, so it is evidence of
/// nothing here — D7 makes the *receipt-joined* create the founder's witness.
List<CodingSessionUmbrella> groupCodingSessionUmbrellas({
  required Iterable<CodingSessionExecution> executions,
  Iterable<CodingSessionCreate> creates = const [],
  Map<String, String> targetKeyByCommandId = const {},
  Map<String, CodingSessionGenesis> genesesByEventId = const {},
  Iterable<CodingSessionName> names = const [],
  Iterable<CodingSessionGoal> goals = const [],
  Iterable<CodingSessionClosure> closures = const [],
}) {
  // The umbrella a stream claimed, per stream. A generation whose metadata
  // echo has not landed yet claims nothing of its own, and reading that as
  // "belongs to no session" splits one session in two on screen — with no
  // name, no goal and no closed state on the half that resumed. The newest
  // generation that did claim an umbrella speaks for the generations that are
  // silent; a generation carrying a claim of its own always keeps it.
  final claimByExecutionKey = <String, CodingSessionExecution>{};
  for (final execution in executions) {
    if (execution.sessionRef == null) continue;
    final incumbent = claimByExecutionKey[execution.executionKey];
    if (incumbent == null ||
        execution.target.generation > incumbent.target.generation ||
        (execution.target.generation == incumbent.target.generation &&
            execution.lastActivityAt > incumbent.lastActivityAt)) {
      claimByExecutionKey[execution.executionKey] = execution;
    }
  }

  final grouped = <String, List<CodingSessionExecution>>{};
  final sessionRefByKey = <String, String?>{};
  for (final execution in executions) {
    // An execution that claimed no umbrella still groups with its own resumes:
    // generation 2 of a stream is the same session as generation 1, umbrella
    // claim or not.
    final claim =
        execution.sessionRef ??
        claimByExecutionKey[execution.executionKey]?.sessionRef;
    final key = claim ?? 'execution\u0000${execution.executionKey}';
    grouped.putIfAbsent(key, () => []).add(execution);
    sessionRefByKey[key] = claim;
  }

  final umbrellas = <CodingSessionUmbrella>[];
  for (final entry in grouped.entries) {
    final members = entry.value;
    final sessionRef = sessionRefByKey[entry.key];
    final channelId = members.first.channelId;
    final ownCreates = [
      for (final create in creates)
        if (_createBelongs(create, sessionRef, members, targetKeyByCommandId))
          create,
    ];
    final founder = resolveCodingSessionFounder(
      sessionRef: sessionRef,
      creates: ownCreates,
      genesesByEventId: genesesByEventId,
    );
    final name = sessionRef == null
        ? null
        : _newestByRef(
            names.where(
              (record) =>
                  record.sessionRef == sessionRef &&
                  record.ref.channelId == channelId,
            ),
            (record) => record.ref,
          );
    final goal = sessionRef == null
        ? null
        : _newestByRef(
            goals.where(
              (record) =>
                  record.sessionRef == sessionRef &&
                  record.ref.channelId == channelId,
            ),
            (record) => record.ref,
          );
    final closure = sessionRef == null
        ? null
        : _newestAuthorizedClosure(
            closures.where(
              (record) =>
                  record.sessionRef == sessionRef &&
                  record.ref.channelId == channelId,
            ),
            genesesByEventId,
          );
    var lastActivityAt = 0;
    for (final execution in members) {
      if (execution.lastActivityAt > lastActivityAt) {
        lastActivityAt = execution.lastActivityAt;
      }
    }
    umbrellas.add(
      CodingSessionUmbrella(
        channelId: channelId,
        key: entry.key,
        sessionRef: sessionRef,
        executions: List.unmodifiable(members),
        founder: founder,
        name: name?.content,
        goal: goal?.content,
        closed: closure?.closed ?? false,
        status: foldCodingSessionUmbrellaStatus(members),
        lastActivityAt: lastActivityAt,
      ),
    );
  }
  umbrellas.sort((left, right) {
    final byActivity = right.lastActivityAt.compareTo(left.lastActivityAt);
    return byActivity != 0 ? byActivity : left.key.compareTo(right.key);
  });
  return List.unmodifiable(umbrellas);
}

/// Resolve the founder of one umbrella from the creates that belong to it.
///
/// A create that names a genesis anchors the session to that exact 44226,
/// resolved by event id — the genesis's own session tag is never a *selector*,
/// but it still has to agree: a genesis that founded [sessionRef] founds this
/// umbrella, and a genesis that founded some other session founds nothing
/// here. Without that check a create naming a stranger's genesis would hand
/// this session the most confident founder label the UI has for someone who
/// founded nothing.
///
/// Two creates naming different geneses is a dispute the observer refuses to
/// settle.
CodingSessionFounder resolveCodingSessionFounder({
  required String? sessionRef,
  required Iterable<CodingSessionCreate> creates,
  Map<String, CodingSessionGenesis> genesesByEventId = const {},
}) {
  final records = creates.toList();
  if (records.isEmpty) return CodingSessionFounder.unresolved;
  final genesisRefs = {
    for (final create in records)
      if (create.genesisRef != null) create.genesisRef!,
  };
  if (genesisRefs.length > 1) {
    return const CodingSessionFounder(
      pubkey: null,
      resolution: CodingSessionFounderResolution.conflict,
    );
  }
  if (genesisRefs.length == 1) {
    final genesis = genesesByEventId[genesisRefs.single];
    // Unreadable, or readable and anchoring a different umbrella: either way
    // this session has no genesis-backed founder to show.
    if (genesis == null || genesis.sessionRef != sessionRef) {
      return CodingSessionFounder.unresolved;
    }
    return CodingSessionFounder(
      pubkey: genesis.founderPubkey,
      resolution: CodingSessionFounderResolution.genesis,
      genesisRef: genesis.ref.eventId,
    );
  }
  final earliest = records.reduce(
    (left, right) =>
        left.ref.createdAt < right.ref.createdAt ||
            (left.ref.createdAt == right.ref.createdAt &&
                left.ref.eventId.compareTo(right.ref.eventId) < 0)
        ? left
        : right,
  );
  return CodingSessionFounder(
    pubkey: earliest.ref.signerPubkey,
    resolution: CodingSessionFounderResolution.legacy,
  );
}

/// Fold a session's executions into one status.
///
/// Any running execution makes the session Working; failing that, any
/// execution waiting for input makes it Waiting. Ended is claimed only when
/// *every* execution is stopped — one live generation is enough to keep a
/// session alive. Otherwise the most-recently-active non-stopped execution
/// speaks for the session.
///
/// Only *current* generations vote. A superseded generation is the past of a
/// stream that is still running under a newer generation: providers do not
/// publish a `stopped` for the generation a resume replaced, so folding one in
/// would pin the session to Working for as long as the read remembers it, and
/// Ended could never be reached. When nothing in [executions] is marked
/// current — a partial read — every member votes rather than the session
/// reporting nothing at all.
CodingSessionFoldedStatus foldCodingSessionUmbrellaStatus(
  Iterable<CodingSessionExecution> executions,
) {
  final all = executions.toList();
  final current = [
    for (final execution in all)
      if (execution.isCurrentGeneration) execution,
  ];
  final members = current.isEmpty ? all : current;
  if (members.isEmpty) {
    return const CodingSessionFoldedStatus(
      kind: CodingSessionFoldedStatusKind.unknown,
    );
  }
  if (members.any(
    (execution) => execution.status == CodingSessionStatus.running,
  )) {
    return const CodingSessionFoldedStatus(
      kind: CodingSessionFoldedStatusKind.working,
      status: CodingSessionStatus.running,
    );
  }
  if (members.any(
    (execution) => execution.status == CodingSessionStatus.waitingForInput,
  )) {
    return const CodingSessionFoldedStatus(
      kind: CodingSessionFoldedStatusKind.waiting,
      status: CodingSessionStatus.waitingForInput,
    );
  }
  if (members.every((execution) => execution.status.isStopped)) {
    return const CodingSessionFoldedStatus(
      kind: CodingSessionFoldedStatusKind.ended,
      status: CodingSessionStatus.stopped,
    );
  }
  final live =
      [
        for (final execution in members)
          if (!execution.status.isStopped) execution,
      ]..sort((left, right) {
        final byActivity = right.lastActivityAt.compareTo(left.lastActivityAt);
        return byActivity != 0
            ? byActivity
            : left.targetKey.compareTo(right.targetKey);
      });
  return CodingSessionFoldedStatus(
    kind: CodingSessionFoldedStatusKind.reported,
    status: live.first.status,
  );
}

/// Decide whether a provider is answering for [currentTarget].
///
/// Only the highest-sequence live lease for the *current* generation, younger
/// than [ttl], proves reachability. Two distinct leases tied at that sequence
/// prove nothing and read as unknown rather than as a denial — the provider's
/// own sequence is supposed to break that tie, so a tie means the snapshot
/// cannot be trusted either way.
///
/// [acceptedCommandId] is the command whose lifecycle receipt minted this
/// generation ([CodingSessionExecution.commandId]). A lease carries the
/// command it was taken under in its `csl-command` tag, and the desktop drops
/// any lease naming a different one (`sessionCoordinationFold.ts`,
/// `lease.commandId !== authority.command.commandId`); this does the same, so
/// a 24223 that no accepted command backs cannot claim a provider is
/// answering. `null` switches that comparison off, and exists only for direct
/// callers of this function — nothing the channel view produces passes it.
/// [resolveCodingSessionGenerations] fills `commandIdByTarget` and `targets`
/// in the same loop body under the same guards, and builds executions only
/// from `targets`, so every [CodingSessionExecution.commandId] is non-null,
/// a D5-fallback target's included: the lease command gate above applies to
/// every execution the observer renders, not just the ones a create vouched
/// for.
///
/// [leasesRead] `== false` (no lease query has returned yet) always reads
/// unknown. An unknown read must never be rendered as "nobody answering".
CodingSessionReachability deriveCodingSessionReachability({
  required Iterable<CodingSessionLease> leases,
  required CodingSessionTarget currentTarget,
  required String? acceptedCommandId,
  required DateTime now,
  String? authorityPubkey,
  bool leasesRead = true,
  Duration ttl = codingSessionLeaseTtl,
}) {
  if (!leasesRead) return CodingSessionReachability.unknown;
  final own = [
    for (final lease in leases)
      if (lease.target.key == currentTarget.key &&
          (authorityPubkey == null ||
              lease.ref.signerPubkey == authorityPubkey) &&
          (acceptedCommandId == null || lease.commandId == acceptedCommandId))
        lease,
  ];
  if (own.isEmpty) {
    return const CodingSessionReachability(
      kind: CodingSessionReachabilityKind.noProviderAnswering,
    );
  }
  final highestSequence = own
      .map((lease) => lease.leaseSequence)
      .reduce((left, right) => left > right ? left : right);
  final highest = [
    for (final lease in own)
      if (lease.leaseSequence == highestSequence) lease,
  ];
  final distinct = {for (final lease in highest) lease.ref.eventId};
  if (distinct.length > 1) {
    return CodingSessionReachability(
      kind: CodingSessionReachabilityKind.unknown,
      leaseSequence: highestSequence,
      conflict: true,
    );
  }
  final winner = highest.first;
  final age = Duration(
    seconds: now.millisecondsSinceEpoch ~/ 1000 - winner.ref.createdAt,
  );
  final live = winner.state == CodingSessionLeaseState.live && age < ttl;
  return CodingSessionReachability(
    kind: live
        ? CodingSessionReachabilityKind.reachable
        : CodingSessionReachabilityKind.noProviderAnswering,
    leaseAge: age,
    leaseSequence: highestSequence,
  );
}

/// Whether [create] is evidence about this umbrella.
///
/// Three things have to hold, and each one is a way the observer has been
/// lied to before: the create's own named provider answered it with a
/// lifecycle receipt (so it minted a real execution), that execution is one of
/// this session's members, and the umbrella the create claimed is the one
/// being resolved. An unanswered create — anyone in the channel can publish
/// one — is evidence of nothing.
bool _createBelongs(
  CodingSessionCreate create,
  String? sessionRef,
  List<CodingSessionExecution> members,
  Map<String, String> targetKeyByCommandId,
) {
  final joinedTargetKey = targetKeyByCommandId[create.commandId];
  if (joinedTargetKey == null) return false;
  if (create.sessionRef != null && create.sessionRef != sessionRef) {
    return false;
  }
  return members.any((execution) => execution.targetKey == joinedTargetKey);
}

/// The newest 44230 this observer may act on for one umbrella.
///
/// A closure is evidence only through the genesis its own `cscl-genesis` tag
/// names, and only that genesis's signer — the session's founder — may close.
/// Reopening is a member act, so any signer's `open` counts once the genesis
/// resolves. Until it resolves neither action is safe: the named genesis may
/// anchor a different session entirely, and anyone in the channel can publish
/// a 44230. A closed badge over a session that is still running is the
/// observer asserting something nobody with the authority to end it signed.
CodingSessionClosure? _newestAuthorizedClosure(
  Iterable<CodingSessionClosure> closures,
  Map<String, CodingSessionGenesis> genesesByEventId,
) {
  final authorized = <CodingSessionClosure>[];
  for (final closure in closures) {
    final genesis = genesesByEventId[closure.genesisRef];
    if (genesis == null || genesis.sessionRef != closure.sessionRef) continue;
    if (closure.closed && closure.ref.signerPubkey != genesis.founderPubkey) {
      continue;
    }
    authorized.add(closure);
  }
  return _newestByRef(authorized, (record) => record.ref);
}

/// True when [candidate] comes before [incumbent] in signed order: lower
/// `created_at`, ties broken by the lower event id.
bool _isEarlier(
  CodingSessionEventRef candidate,
  CodingSessionEventRef incumbent,
) {
  final byTime = candidate.createdAt.compareTo(incumbent.createdAt);
  return byTime != 0
      ? byTime < 0
      : candidate.eventId.compareTo(incumbent.eventId) < 0;
}

CodingSessionMetadata? _newestMetadata(List<CodingSessionMetadata> records) {
  CodingSessionMetadata? newest;
  for (final record in records) {
    if (newest == null) {
      newest = record;
      continue;
    }
    if (record.ref.createdAt > newest.ref.createdAt) {
      newest = record;
      continue;
    }
    // Ties break to the lower event id, so every reader that saw the same two
    // events shows the same one.
    if (record.ref.createdAt == newest.ref.createdAt &&
        record.ref.eventId.compareTo(newest.ref.eventId) < 0) {
      newest = record;
    }
  }
  return newest;
}

/// Whether the newest second holds two metadata payloads that disagree.
///
/// The comparison is over [CodingSessionMetadata.canonicalPayload] — the
/// whole payload as the decoder validated it — because that is what the
/// desktop compares (`codingSessionTrustedIngress.ts`, distinct
/// `record.canonicalPayload` in the newest second). Two same-second events
/// differing only in, say, `projectRef` or `observedCommit` are still two
/// providers disagreeing about the session, and no list of field names
/// maintained here could be relied on to stay complete. Two byte-equal
/// payloads are one fact republished, never a conflict.
bool _hasStatusConflict(
  List<CodingSessionMetadata> records,
  CodingSessionMetadata? newest,
) {
  if (newest == null) return false;
  final sameSecond = [
    for (final record in records)
      if (record.ref.createdAt == newest.ref.createdAt) record,
  ];
  if (sameSecond.length < 2) return false;
  final payloads = {for (final record in sameSecond) record.canonicalPayload};
  return payloads.length > 1;
}

T? _newestByRef<T>(
  Iterable<T> records,
  CodingSessionEventRef Function(T record) refOf,
) {
  T? newest;
  for (final record in records) {
    if (newest == null) {
      newest = record;
      continue;
    }
    final candidate = refOf(record);
    final incumbent = refOf(newest);
    if (candidate.createdAt > incumbent.createdAt ||
        (candidate.createdAt == incumbent.createdAt &&
            candidate.eventId.compareTo(incumbent.eventId) > 0)) {
      newest = record;
    }
  }
  return newest;
}
