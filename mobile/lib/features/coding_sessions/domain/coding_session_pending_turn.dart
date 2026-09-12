import 'package:flutter/foundation.dart';

import 'coding_session_fold.dart';
import 'coding_session_models.dart';

/// A turn this device sent and is still waiting to see answered.
///
/// A row exists from the moment the composer clears until a `turn_started`
/// (or `turn_injected`) receipt names its [commandId] (CREW_SESSIONS_PLAN
/// D4). It is keyed by the
/// execution's generation-free identity plus the command id, so a resume that
/// mints the next generation cannot orphan it, and it carries the raw
/// [draft] so a refusal can hand the exact words back.
@immutable
class CodingSessionPendingTurn {
  final String channelId;

  /// [CodingSessionTarget.executionKey] of the execution the turn was aimed at.
  final String executionKey;

  /// The generation the command's `target` named.
  final int generation;

  /// The `commandId` inside the signed payload.
  final String commandId;

  /// The text as published.
  final String text;

  /// The composer's raw draft before any trimming — what a refusal restores.
  final String draft;

  /// When the row was recorded, in epoch milliseconds.
  final int recordedAt;

  /// True once the relay acknowledged the event with `OK true`.
  ///
  /// Acceptance by the relay is not consent from the provider: the row stays
  /// pending until a receipt answers it.
  final bool published;

  const CodingSessionPendingTurn({
    required this.channelId,
    required this.executionKey,
    required this.generation,
    required this.commandId,
    required this.text,
    required this.draft,
    required this.recordedAt,
    required this.published,
  });

  /// The map key the store files this row under.
  String get key => storeKey(channelId, executionKey, commandId);

  /// How the store keys a row.
  static String storeKey(
    String channelId,
    String executionKey,
    String commandId,
  ) => '$channelId\u0000$executionKey\u0000$commandId';

  /// The same row, acknowledged by the relay.
  CodingSessionPendingTurn markPublished() => CodingSessionPendingTurn(
    channelId: channelId,
    executionKey: executionKey,
    generation: generation,
    commandId: commandId,
    text: text,
    draft: draft,
    recordedAt: recordedAt,
    published: true,
  );
}

/// Where a pending turn stands, read from its receipts.
enum CodingSessionPendingPhase {
  /// The relay has not acknowledged the event yet.
  sending,

  /// The relay accepted it; no provider receipt has arrived.
  published,

  /// The provider queued it for the next turn boundary.
  queued,

  /// The provider downgraded the requested delivery class and said why; the
  /// turn is still owed.
  degraded,

  /// A `turn_started` named it — the row is settled and leaves the list.
  started,

  /// A `turn_injected` named it: the words went into the turn already
  /// running. Settled exactly like [started] — nothing is owed.
  injected,

  /// A `turn_delivery_unknown` named it: a native steer was written to the
  /// runtime and nothing establishes whether it arrived. Terminal — the
  /// provider will not resend it — but not a failure: the words may already
  /// be inside the running turn, so they are *not* handed back to the
  /// composer. The row stays, says so, and the person dismisses it.
  deliveryUnknown,

  /// The provider refused it (`turn_refused`); the words go back to the
  /// composer.
  refused,

  /// The provider dropped it (`turn_dropped`); the words go back to the
  /// composer.
  dropped,
}

/// A pending turn plus the verdict its receipts give.
@immutable
class CodingSessionPendingTurnView {
  final CodingSessionPendingTurn turn;
  final CodingSessionPendingPhase phase;

  /// The receipt's `code: message`, for the phases that carry one.
  final String? detail;

  /// The newer current generation a refused turn could be re-addressed to,
  /// when the refusal was a generation problem and one exists.
  final int? readdressGeneration;

  const CodingSessionPendingTurnView({
    required this.turn,
    required this.phase,
    this.detail,
    this.readdressGeneration,
  });

  /// True once the row has nothing left to say and should leave the list.
  bool get settled =>
      phase == CodingSessionPendingPhase.started ||
      phase == CodingSessionPendingPhase.injected;

  /// True when the provider will not run the turn as sent.
  ///
  /// Deliberately excludes [CodingSessionPendingPhase.deliveryUnknown]: a
  /// failed row hands the words back to the composer, and an unknown delivery
  /// must not, because they may already be in the running turn.
  bool get failed =>
      phase == CodingSessionPendingPhase.refused ||
      phase == CodingSessionPendingPhase.dropped;

  /// True for the terminal answer that is neither settled nor failed: the
  /// person, not a receipt, retires this row.
  bool get deliveryUnknown =>
      phase == CodingSessionPendingPhase.deliveryUnknown;
}

/// Receipt codes that mean "the generation you named is gone", which is the
/// sender's cue to re-address the turn to the successor (D2 amended).
const codingSessionReaddressCodes = {'NO_LIVE_EXECUTION', 'STALE_GENERATION'};

/// Settle one pending turn against the receipts naming its command id.
///
/// Settlement is by `commandId` and receipt status only — never by matching
/// text (D4). Receipts are read newest-last in signed order; a `turn_started`
/// or `turn_injected` anywhere settles the row, a terminal refusal or drop
/// fails it, a `turn_delivery_unknown` parks it for the person to dismiss
/// (unless a later, definite receipt reconciles it), and a `turn_degraded` or
/// `turn_queued` describes it while it is still owed.
CodingSessionPendingTurnView settleCodingSessionPendingTurn(
  CodingSessionPendingTurn turn,
  Iterable<CodingSessionReceipt> receipts, {
  int? currentGeneration,
}) {
  final own =
      [
        for (final receipt in receipts)
          if (receipt.isTurnStage && receipt.commandId == turn.commandId)
            receipt,
      ]..sort((left, right) {
        final byTime = left.ref.createdAt.compareTo(right.ref.createdAt);
        return byTime != 0
            ? byTime
            : left.ref.eventId.compareTo(right.ref.eventId);
      });

  CodingSessionReceipt? degraded;
  CodingSessionReceipt? unknown;
  var queued = false;
  for (final receipt in own) {
    switch (receipt.status) {
      case CodingSessionReceiptStatus.turnStarted:
        return CodingSessionPendingTurnView(
          turn: turn,
          phase: CodingSessionPendingPhase.started,
        );
      case CodingSessionReceiptStatus.turnInjected:
        return CodingSessionPendingTurnView(
          turn: turn,
          phase: CodingSessionPendingPhase.injected,
        );
      case CodingSessionReceiptStatus.turnDeliveryUnknown:
        // Not returned here: a later `turn_dropped` (`STEER_NOT_DELIVERED`)
        // or `turn_injected` under the same command reconciles it, and those
        // are read in signed order after this one.
        unknown = receipt;
      case CodingSessionReceiptStatus.turnRefused:
      case CodingSessionReceiptStatus.turnDropped:
        final code = receipt.error?.code;
        final readdress =
            code != null &&
                codingSessionReaddressCodes.contains(code) &&
                currentGeneration != null &&
                currentGeneration > turn.generation
            ? currentGeneration
            : null;
        return CodingSessionPendingTurnView(
          turn: turn,
          phase: receipt.status == CodingSessionReceiptStatus.turnRefused
              ? CodingSessionPendingPhase.refused
              : CodingSessionPendingPhase.dropped,
          detail: _detail(receipt),
          readdressGeneration: readdress,
        );
      case CodingSessionReceiptStatus.turnDegraded:
        degraded = receipt;
      case CodingSessionReceiptStatus.turnQueued:
        queued = true;
      case CodingSessionReceiptStatus.interruptDelivered:
      case CodingSessionReceiptStatus.continuationRegistered:
      case CodingSessionReceiptStatus.created:
      case CodingSessionReceiptStatus.createdWithFailedInitialTurn:
      case CodingSessionReceiptStatus.failed:
      case CodingSessionReceiptStatus.resumed:
      case CodingSessionReceiptStatus.resumedWithoutContext:
      case CodingSessionReceiptStatus.stopped:
        break;
    }
  }
  if (unknown != null) {
    return CodingSessionPendingTurnView(
      turn: turn,
      phase: CodingSessionPendingPhase.deliveryUnknown,
      detail: _detail(unknown),
    );
  }
  if (degraded != null) {
    return CodingSessionPendingTurnView(
      turn: turn,
      phase: CodingSessionPendingPhase.degraded,
      detail: _detail(degraded),
    );
  }
  if (queued) {
    return CodingSessionPendingTurnView(
      turn: turn,
      phase: CodingSessionPendingPhase.queued,
    );
  }
  return CodingSessionPendingTurnView(
    turn: turn,
    phase: turn.published
        ? CodingSessionPendingPhase.published
        : CodingSessionPendingPhase.sending,
  );
}

String? _detail(CodingSessionReceipt receipt) {
  final error = receipt.error;
  if (error == null) return null;
  return error.message.trim().isEmpty
      ? error.code
      : '${error.code}: ${error.message}';
}

/// Why this device may, or may not, steer a session.
///
/// The relay admits a 44220 only from a session founder or a granted operator
/// (`ingest.rs`, `check_coding_session_membership`). This device folds
/// founders but not 44228 grants, so an operator's phone reads
/// [notFounder] until the relay accepts a command from it — the accepting
/// relay, not this fold, is the authority on grants.
enum CodingSessionSteerStanding {
  /// The umbrella's founder resolved to this device's key.
  founder,

  /// Not the founder, but the relay accepted a command from this key on this
  /// session already, so it holds a grant this device cannot read.
  acceptedOperator,

  /// The founder resolved to someone else, and nothing proves a grant.
  notFounder,

  /// The founder could not be resolved (conflict or no readable create), so
  /// nothing can be claimed either way.
  founderUnresolved,

  /// This device holds no signing key.
  noKey,
}

/// Decide [CodingSessionSteerStanding] for [session].
CodingSessionSteerStanding codingSessionSteerStanding({
  required CodingSessionUmbrella session,
  required String? myPubkey,
  required bool relayAcceptedMine,
}) {
  final mine = myPubkey?.toLowerCase();
  if (mine == null || mine.isEmpty) return CodingSessionSteerStanding.noKey;
  final founder = session.founder;
  switch (founder.resolution) {
    case CodingSessionFounderResolution.genesis:
    case CodingSessionFounderResolution.legacy:
      if (founder.pubkey?.toLowerCase() == mine) {
        return CodingSessionSteerStanding.founder;
      }
      return relayAcceptedMine
          ? CodingSessionSteerStanding.acceptedOperator
          : CodingSessionSteerStanding.notFounder;
    case CodingSessionFounderResolution.conflict:
    case CodingSessionFounderResolution.unresolved:
      return relayAcceptedMine
          ? CodingSessionSteerStanding.acceptedOperator
          : CodingSessionSteerStanding.founderUnresolved;
  }
}

/// True when [standing] lets the composer publish at all.
bool codingSessionMaySteer(CodingSessionSteerStanding standing) =>
    standing == CodingSessionSteerStanding.founder ||
    standing == CodingSessionSteerStanding.acceptedOperator;
