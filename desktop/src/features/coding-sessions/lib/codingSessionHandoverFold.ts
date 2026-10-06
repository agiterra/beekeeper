/**
 * The TypeScript twin of `crates/beekeeper-core/src/coding_session_handover_fold.rs`.
 *
 * Pinned to that fold's own fixture
 * (`codingSessionHandover.fixture.json`, written by
 * `the_typescript_fixture_is_this_folds_real_output`), so a rule the Rust fold
 * changes and this one does not is a failing test rather than a panel that
 * quietly disagrees with the CLI and the provider.
 *
 * Standing is **decided here, by the reader**, from the accepted 44228 chain —
 * a 44247 record carries none of its own. Anybody who can write to the channel
 * can publish a checkpoint; only the chain says whose checkpoint may be
 * reconstructed from. A record that fails a standing test is listed as
 * `unauthorized` and disclosed under `excluded`, because "somebody said this
 * and it does not count" and "nobody said anything" are different answers.
 *
 * **An envelope failure fails the whole set**, exactly as Rust does it and for
 * the reason its header gives: these records decide what somebody else
 * reconstructs work from, so a fold that dropped one and answered anyway could
 * show "no checkpoint" over a checkpoint that exists, and a person would then
 * reconstruct from an older revision believing it was the newest. Signatures
 * are checked first, with the app's own `hasValidSignature`, because the
 * signature is what makes an author an author.
 *
 * Deletion precedence (§1, §3.2): a retired genesis lists every record it
 * holds and answers `null` for the latest authorized checkpoint, `null` for
 * the active continuation and `no-claim` for the claim.
 */
import type { RelayEvent } from "@/shared/api/types";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  CODING_SESSION_NO_CLAIM,
  type CodingSessionClaimState,
} from "./codingSessionMissionAuthority";
import {
  decodeCodingSessionHandoverEvent,
  type CodingSessionCheckpointBody,
  type CodingSessionContinuationMode,
  type CodingSessionHandoverTarget,
} from "./codingSessionHandoverWire";

/**
 * Whether the chain gives this record's author the standing it needs.
 *
 * Three values, not two: `superseded` is a continuation that acted on a claim
 * which is no longer in force — history rather than error ("continued by B
 * until …"), and collapsing it into `unauthorized` would call somebody's
 * legitimate work a violation.
 */
export type CodingSessionHandoverStanding =
  | "authorized"
  | "unauthorized"
  | "superseded";

/**
 * Apply the author's own statement of order (`prevCheckpointRef`).
 *
 * An **authorized** checkpoint naming another supersedes it only when the
 * target is in this fold, was written by the **same author**, and is itself
 * authorized. An unknown id, another author's checkpoint, a continuation's id,
 * an unauthorized target or a self-reference has no effect at all, and the
 * reference stays on the entry verbatim for a reader to make of what it will.
 *
 * **No clock is consulted.** The reference *is* the ordering — that is the
 * whole point of the field: three checkpoints in one second were previously
 * ordered by a hash, and a reconstruction started from the older statement.
 */
function applyCheckpointSupersession(
  checkpoints: CodingSessionHandoverCheckpoint[],
  excluded: CodingSessionHandoverExclusion[],
): void {
  const supersessions: Array<[string, string]> = [];
  for (const namer of checkpoints) {
    if (namer.standing !== "authorized") continue;
    const targetId = namer.body.prevCheckpointRef;
    if (targetId === null || targetId === namer.eventId) continue;
    const namesAnAuthorizedCheckpointOfTheSameAuthor = checkpoints.some(
      (target) =>
        target.eventId === targetId &&
        target.author === namer.author &&
        target.standing === "authorized",
    );
    if (namesAnAuthorizedCheckpointOfTheSameAuthor) {
      supersessions.push([targetId, namer.eventId]);
    }
  }
  for (const [targetId, namerId] of supersessions) {
    const index = checkpoints.findIndex((entry) => entry.eventId === targetId);
    if (index < 0) continue;
    checkpoints[index] = {
      ...checkpoints[index],
      standing: "superseded",
      supersededBy: namerId,
    };
    excluded.push({
      eventId: targetId,
      reason: `checkpoint replaced by ${namerId}, which its own author wrote as the next one: a reconstruction starts from the newest statement, not the newest timestamp`,
    });
  }
}

/** One checkpoint, with the standing its author held when it was written. */
export type CodingSessionHandoverCheckpoint = {
  readonly eventId: string;
  readonly author: string;
  readonly createdAt: number;
  readonly standing: CodingSessionHandoverStanding;
  /**
   * The authorized checkpoint that named this one in its `prevCheckpointRef`,
   * when one did — the link a surface follows to say "replaced by …".
   *
   * `null` on everything else, including a checkpoint whose author simply
   * never wrote another.
   */
  readonly supersededBy: string | null;
  readonly body: CodingSessionCheckpointBody;
};

/** One continuation, with the standing its claim gives it. */
export type CodingSessionHandoverContinuation = {
  readonly eventId: string;
  readonly author: string;
  readonly createdAt: number;
  readonly claimRef: string;
  readonly mode: CodingSessionContinuationMode;
  readonly target: CodingSessionHandoverTarget;
  readonly checkpointRef: string | null;
  readonly recovered: readonly string[];
  readonly missing: readonly string[];
  readonly note: string | null;
  readonly standing: CodingSessionHandoverStanding;
};

/** One record the fold refused, and the reason it refused it. */
export type CodingSessionHandoverExclusion = {
  readonly eventId: string;
  readonly reason: string;
};

/**
 * One grant or seat as the chain knows it, with the time it took effect.
 *
 * A caller that can only see *current* grants supplies those; standing then
 * under-claims for a checkpoint written under a grant that has since been
 * revoked, and never over-claims. That direction is deliberate: the cost of
 * under-claiming is a disclosed "unauthorized" row a person can check, and the
 * cost of over-claiming is reconstructing from a stranger's statement.
 */
export type CodingSessionHandoverStandingEntry = {
  readonly pubkey: string;
  readonly acceptedAt: number;
};

/** The chain context one umbrella's handover records are read against. */
export type CodingSessionHandoverContext = {
  readonly founderPubkey: string;
  /** Live operator grants (`grant-operator`), with their acceptance times. */
  readonly grants: readonly CodingSessionHandoverStandingEntry[];
  /** Governed seats of this umbrella, with their acceptance times. */
  readonly seats: readonly (CodingSessionHandoverStandingEntry & {
    readonly role: string;
  })[];
  /** The §1 claim state of the same chain. */
  readonly claim: CodingSessionClaimState;
  /** When the claim in force was accepted, in unix seconds, or `null`. */
  readonly claimSince?: number | null;
  /** When the claim was voided, in unix seconds, or `null`. */
  readonly claimVoidedAt?: number | null;
  /** Whether this umbrella's genesis has been deleted. */
  readonly retired: boolean;
};

/** Either the whole fold, or the one defect that stopped it. */
export type CodingSessionHandoverFoldResult =
  | { readonly ok: true; readonly value: CodingSessionHandoverFold }
  | { readonly ok: false; readonly error: string };

/** The fold's complete answer about one umbrella's handover records. */
export type CodingSessionHandoverFold = {
  readonly checkpoints: readonly CodingSessionHandoverCheckpoint[];
  readonly latestAuthorizedCheckpoint: string | null;
  readonly continuations: readonly CodingSessionHandoverContinuation[];
  readonly activeContinuation: string | null;
  readonly claim: CodingSessionClaimState;
  /** When the claim in force was accepted, or `null` when nobody knows. */
  readonly claimSince: number | null;
  /** When the claim was voided, or `null`. */
  readonly claimVoidedAt: number | null;
  readonly retired: boolean;
  readonly excluded: readonly CodingSessionHandoverExclusion[];
};

function heldStandingAt(
  entries: readonly CodingSessionHandoverStandingEntry[],
  pubkey: string,
  at: number,
): boolean {
  return entries.some(
    (entry) => entry.pubkey === pubkey && entry.acceptedAt <= at,
  );
}

/**
 * Fold one umbrella's 44247 records against its accepted authority chain.
 *
 * Returns the defect rather than throwing it: the surface renders "these
 * records could not be read, and here is why" instead of an empty panel over
 * records that plainly exist.
 */
export function foldCodingSessionHandover(input: {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  events: readonly RelayEvent[];
  context: CodingSessionHandoverContext;
}): CodingSessionHandoverFoldResult {
  const excluded: CodingSessionHandoverExclusion[] = [];
  const checkpoints: CodingSessionHandoverCheckpoint[] = [];
  const continuations: CodingSessionHandoverContinuation[] = [];
  const { context } = input;
  const retired = context.retired;
  // Standing is judged against the claim the **chain** carries, and only then
  // is the reported claim cleared for a retired genesis — the order Rust folds
  // in (`coding_session_handover_fold.rs`). Clearing first would call a
  // claimant's own continuation "not the claim in force" and file a deletion
  // under a superseded-history reason, which is a different sentence about a
  // different fact.
  const claim = retired ? CODING_SESSION_NO_CLAIM : context.claim;
  const activeClaim = context.claim.state === "active" ? context.claim : null;

  const seen = new Set<string>();
  const ordered = [...input.events];
  for (const event of ordered) {
    if (seen.has(event.id)) {
      return {
        ok: false,
        error: `duplicate coding-session handover event ${event.id}: the same record was supplied twice, and a fold that counted it twice would double a continuation`,
      };
    }
    seen.add(event.id);
  }
  ordered.sort(
    (left, right) =>
      left.created_at - right.created_at ||
      (left.id < right.id ? -1 : left.id > right.id ? 1 : 0),
  );

  for (const event of ordered) {
    // The signature is the author, so it is checked before anybody is named
    // one.
    if (!hasValidSignature(event)) {
      return {
        ok: false,
        error: `coding-session handover ${event.id} has an invalid signature`,
      };
    }
    const decoded = decodeCodingSessionHandoverEvent({
      event,
      channelRef: input.channelRef,
      sessionRef: input.sessionRef,
      genesisRef: input.genesisRef,
    });
    if (!decoded.ok) {
      return {
        ok: false,
        error: `coding-session handover ${event.id} is invalid: ${decoded.reason}`,
      };
    }
    const record = decoded.value;
    if (record.type === "checkpoint") {
      const authorized =
        record.author === context.founderPubkey ||
        heldStandingAt(context.grants, record.author, record.createdAt) ||
        heldStandingAt(context.seats, record.author, record.createdAt);
      if (!authorized) {
        excluded.push({
          eventId: record.eventId,
          reason: `checkpoint by ${record.author} is not used for reconstruction: that pubkey was neither the founder nor a live operator nor an active seat of this umbrella when it was written`,
        });
      }
      checkpoints.push({
        eventId: record.eventId,
        author: record.author,
        createdAt: record.createdAt,
        standing: authorized ? "authorized" : "unauthorized",
        supersededBy: null,
        body: record.body,
      });
      continue;
    }
    let standing: CodingSessionHandoverStanding;
    if (
      activeClaim !== null &&
      activeClaim.acceptedEventId === record.body.claimRef
    ) {
      if (activeClaim.claimant === record.author) {
        standing = "authorized";
      } else {
        standing = "unauthorized";
        excluded.push({
          eventId: record.eventId,
          reason: `continuation by ${record.author} names the claim in force, but that claim is held by ${activeClaim.claimant}`,
        });
      }
    } else {
      standing = "superseded";
      excluded.push({
        eventId: record.eventId,
        reason: `continuation acted on claim ${record.body.claimRef}, which is not the claim in force: it is history, not the current state of this session`,
      });
    }
    continuations.push({
      eventId: record.eventId,
      author: record.author,
      createdAt: record.createdAt,
      claimRef: record.body.claimRef,
      mode: record.body.mode,
      target: record.body.target,
      checkpointRef: record.body.checkpointRef,
      recovered: record.body.recovered,
      missing: record.body.missing,
      note: record.body.note,
      standing,
    });
  }

  // Before the retirement branch, exactly as Rust orders it: a superseded
  // checkpoint carries its own reason, so the deletion line below is added
  // only to records that carry none.
  applyCheckpointSupersession(checkpoints, excluded);

  if (retired) {
    // Listed, never acted on. Every record of a retired umbrella carries its
    // reason so a surface says "deleted" rather than showing an empty panel
    // over records that plainly exist.
    for (const eventId of [
      ...checkpoints.map((entry) => entry.eventId),
      ...continuations.map((entry) => entry.eventId),
    ]) {
      if (excluded.some((exclusion) => exclusion.eventId === eventId)) continue;
      excluded.push({
        eventId,
        reason:
          "this session was deleted: nothing here is reconstructed or resumed",
      });
    }
    return {
      ok: true,
      value: Object.freeze({
        checkpoints: Object.freeze(checkpoints),
        latestAuthorizedCheckpoint: null,
        continuations: Object.freeze(continuations),
        activeContinuation: null,
        claim,
        claimSince: null,
        claimVoidedAt: null,
        retired: true,
        excluded: Object.freeze(excluded),
      }),
    };
  }

  const latestAuthorizedCheckpoint =
    [...checkpoints].reverse().find((entry) => entry.standing === "authorized")
      ?.eventId ?? null;
  const activeContinuation =
    [...continuations]
      .reverse()
      .find((entry) => entry.standing === "authorized")?.eventId ?? null;

  return {
    ok: true,
    value: Object.freeze({
      checkpoints: Object.freeze(checkpoints),
      latestAuthorizedCheckpoint,
      continuations: Object.freeze(continuations),
      activeContinuation,
      claim,
      claimSince: context.claimSince ?? null,
      claimVoidedAt: context.claimVoidedAt ?? null,
      retired: false,
      excluded: Object.freeze(excluded),
    }),
  };
}
