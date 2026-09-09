/**
 * What the handover surface says, decided from evidence and nothing else.
 *
 * Every predicate here reads a fact somebody signed: the accepted claim from
 * the 44228 chain, reachability from the coordination fold's lease evidence,
 * preservation from the checkpoint author's own `revision.preserved`. Nothing
 * is inferred from prose, from a status word, or from the presence of an
 * artifact — a `patch` artifact beside `preserved: "partial"` still means work
 * was left behind, and saying otherwise would be the comfortable guess this
 * product treats as a bug.
 *
 * Scope, in v1: a claim is **umbrella-wide** (`docs/HANDOVER_IMPL.md` §1). One
 * takeover hands over the whole session — every execution and assignment under
 * it — so every label this model feeds says "this session", never "this slice".
 */
import type { CoordinatedGeneration } from "@/shared/coordination/sessionCoordinationTypes";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";

/**
 * The three facts this model needs about an execution generation.
 *
 * Structurally satisfied by {@link CoordinatedGeneration} itself, so a caller
 * holding the shared coordination fold passes its rows unchanged; a surface
 * that only holds the resolver every session workspace already threads can
 * build the same three fields without a second relay read.
 */
export type CodingSessionHandoverGeneration = Pick<
  CoordinatedGeneration,
  "current" | "providerAuthorityPubkey" | "reachability" | "handover"
>;
import type {
  CodingSessionHandoverCheckpoint,
  CodingSessionHandoverContinuation,
  CodingSessionHandoverFold,
} from "./codingSessionHandoverFold";
import type { CodingSessionClaimState } from "./codingSessionMissionAuthority";

/** One thing a reader can open to check this panel's claim for themselves. */
export type CodingSessionHandoverEvidenceLink = {
  readonly label: string;
  readonly eventId: string;
};

/**
 * Why this execution is fenced, so the sentence can say the true thing.
 *
 * Three different facts, and one wording each: the claimant is somebody else
 * on this very body (`not-claimant`), this body is not the claimed one
 * (`other-body`), or the claim was voided and every body is frozen
 * (`voided`). `unknown` is the fourth honest answer: this app does not know
 * which body it is looking at, so it says that rather than "not fenced".
 */
export type CodingSessionHandoverFenceReason =
  | "voided"
  | "other-body"
  | "not-claimant"
  | "unknown";

/** The two labelled outcomes §0 refuses to conflate. */
export type CodingSessionHandoverOutcomeLabel =
  | "Native continuation"
  | "Reconstructed";

export type CodingSessionHandoverModel = {
  readonly claim: CodingSessionClaimState;
  /** When the claim was accepted, in unix seconds, or `null` if unknown. */
  readonly claimSince: number | null;
  /** When the claim was voided, in unix seconds, or `null` if unknown. */
  readonly claimVoidedAt: number | null;
  /** The provider authority pubkey the live claim names, or null. */
  readonly activeBody: string | null;
  readonly continuation: CodingSessionHandoverContinuation | null;
  readonly latestCheckpoint: CodingSessionHandoverCheckpoint | null;
  /**
   * Whether this viewer may run the claim-and-reconstruct flow.
   *
   * Founder or live operator, not the current claimant, a checkpoint to
   * reconstruct from, and evidence that the work is **not** already running:
   * the claimed body's current generation is not `provider_reachable`, or —
   * with no claim at all — none of the umbrella's current executions is.
   */
  readonly viewerMayContinue: boolean;
  /**
   * Whether this viewer may take the session back onto their own body.
   *
   * The same standing test without the checkpoint or the reachability test: a
   * fenced session with nothing to reconstruct can still be taken back, and
   * that act is what lifts the fence.
   */
  readonly viewerMayTakeBack: boolean;
  readonly viewerIsClaimant: boolean;
  /**
   * Whether the execution this viewer is looking at is fenced.
   *
   * True when the claim is voided (every body is fenced until somebody with
   * standing claims the session again) or when this execution's provider is
   * not the claimed body. Never inferred from reachability: an offline
   * provider is quiet, which is not the same as forbidden.
   */
  readonly thisExecutionFenced: boolean;
  /** Which fact fences it, or `null` when nothing does. */
  readonly fenceReason: CodingSessionHandoverFenceReason | null;
  /**
   * The fence a provider disclosed in its own 44223 (§3.1), when one did.
   *
   * The **chain is the primary source**; this is what a returning body
   * advertises about itself, and it can arrive before this client's chain read
   * catches up. A surface that shows it says where it came from.
   */
  readonly metadataFence: NonNullable<CoordinatedGeneration["handover"]> | null;
  /** The newest continuation that is not the active one, or null. */
  readonly priorContinuation: CodingSessionHandoverContinuation | null;
  /**
   * Checkpoints their own author replaced, newest first.
   *
   * History, and never the statement a reconstruction reads from: the author
   * said which one supersedes which, and a surface that offered an older one
   * would be reading a hash order the author overruled.
   */
  readonly supersededCheckpoints: readonly CodingSessionHandoverCheckpoint[];
  /** Whether the claimed body's current generation is provider-reachable. */
  readonly claimedBodyReachable: boolean;
  readonly retired: boolean;
  readonly retiredAt: number | null;
  readonly evidenceLinks: readonly CodingSessionHandoverEvidenceLink[];
  readonly outcomeLabel: CodingSessionHandoverOutcomeLabel | null;
  /**
   * What was not recovered, author's words first.
   *
   * When the checkpoint's `preserved` is `partial` or `none` this list always
   * opens with {@link CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE}, whatever
   * artifacts the checkpoint carries.
   */
  readonly missing: readonly string[];
  readonly recovered: readonly string[];
};

/**
 * This umbrella's generations, as the shared reachability read proves them.
 *
 * A generation the read never proved is `unverified`, never "nobody is
 * answering": the fence and the Continue action both hang off this, and
 * absence of evidence must not read as evidence.
 */
export function codingSessionHandoverGenerations(
  executions: readonly {
    activeGeneration: {
      providerAuthorityPubkey: string | null;
      commandTarget: CodingSessionCommandTarget | null;
    };
  }[],
  resolveReachability: (target: CodingSessionCommandTarget | null) => {
    known: boolean;
    reachable?: boolean;
  },
): CodingSessionHandoverGeneration[] {
  return executions.map((execution) => {
    const reachability = resolveReachability(
      execution.activeGeneration.commandTarget,
    );
    return {
      current: true,
      providerAuthorityPubkey:
        execution.activeGeneration.providerAuthorityPubkey ?? "",
      reachability:
        reachability.known && reachability.reachable === true
          ? "provider_reachable"
          : "unverified",
    };
  });
}

/** The sentence a `partial` or `none` preservation always renders. */
export const CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE =
  "Not all uncommitted work was preserved";

function uniqueLines(lines: readonly string[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const line of lines) {
    if (seen.has(line)) continue;
    seen.add(line);
    result.push(line);
  }
  return result;
}

/**
 * Derive the handover surface's model for one umbrella, as one viewer sees it.
 *
 * `generations` are that umbrella's generations from the shared coordination
 * fold — the only place in this app that answers "can this be reached right
 * now", and never substituted with metadata recency.
 */
export function deriveCodingSessionHandoverModel(input: {
  fold: CodingSessionHandoverFold;
  viewerPubkey: string | null;
  founderPubkey: string | null;
  /** Pubkeys holding a live operator grant on this umbrella. */
  operatorPubkeys: readonly string[];
  generations: readonly CodingSessionHandoverGeneration[];
  /** The provider authority pubkey of the execution this viewer is on. */
  thisExecutionProviderPubkey: string | null;
  /** When the umbrella was deleted, if a deletion was witnessed. */
  retiredAt?: number | null;
}): CodingSessionHandoverModel {
  const { fold } = input;
  const viewer = input.viewerPubkey?.toLowerCase() ?? null;
  const claim = fold.claim;
  const activeBody = claim.state === "active" ? claim.bodyPubkey : null;
  const viewerIsClaimant =
    claim.state === "active" && viewer !== null && claim.claimant === viewer;
  const hasStanding =
    viewer !== null &&
    (viewer === input.founderPubkey?.toLowerCase() ||
      input.operatorPubkeys.some((pubkey) => pubkey.toLowerCase() === viewer));

  const continuation =
    fold.continuations.find((row) => row.eventId === fold.activeContinuation) ??
    null;
  const latestCheckpoint =
    fold.checkpoints.find(
      (row) => row.eventId === fold.latestAuthorizedCheckpoint,
    ) ?? null;

  const claimedBodyReachable =
    activeBody !== null &&
    input.generations.some(
      (generation) =>
        generation.current &&
        generation.providerAuthorityPubkey === activeBody &&
        generation.reachability === "provider_reachable",
    );
  const anyCurrentReachable = input.generations.some(
    (generation) =>
      generation.current && generation.reachability === "provider_reachable",
  );

  // A retired umbrella offers nothing: not a takeover, not a reconstruction.
  // Nothing is republished to make a deleted session resumable (§3.2).
  const workIsElsewhere =
    claim.state === "active"
      ? !claimedBodyReachable
      : claim.state === "voided"
        ? true
        : !anyCurrentReachable;
  const viewerMayContinue =
    !fold.retired &&
    hasStanding &&
    !viewerIsClaimant &&
    latestCheckpoint !== null &&
    workIsElsewhere;
  const viewerMayTakeBack =
    !fold.retired &&
    hasStanding &&
    ((claim.state === "active" && !viewerIsClaimant) ||
      claim.state === "voided");

  // What a provider says about itself, as a fast path while the chain read
  // catches up. Current generations only: a fence disclosed by a generation
  // that is no longer current describes a past state.
  const metadataFence =
    input.generations.find(
      (generation) => generation.current && generation.handover,
    )?.handover ?? null;

  // The provider refuses a turn on three different grounds (§3), and this
  // says which one applies to the execution in front of this viewer. A
  // non-claimant operator on the claimed body is fenced too — the provider
  // refuses `operator != claim.claimant` — which is exactly the case a body
  // check alone would have called "not fenced".
  let fenceReason: CodingSessionHandoverFenceReason | null = null;
  if (!fold.retired) {
    if (claim.state === "voided") {
      fenceReason = "voided";
    } else if (claim.state === "active") {
      if (input.thisExecutionProviderPubkey === null) {
        fenceReason = "unknown";
      } else if (input.thisExecutionProviderPubkey !== claim.bodyPubkey) {
        fenceReason = "other-body";
      } else if (!viewerIsClaimant) {
        fenceReason = "not-claimant";
      }
    } else if (metadataFence !== null) {
      // The chain says nothing yet, but this execution's own provider says it
      // is fenced. Believed, and labelled as the provider's own statement.
      // A disclosed **voided** claim fences everyone, body or not: nobody
      // holds the session, which is precisely why it is frozen.
      fenceReason =
        metadataFence.state === "voided"
          ? "voided"
          : input.thisExecutionProviderPubkey === null
            ? "unknown"
            : input.thisExecutionProviderPubkey !== metadataFence.bodyPubkey
              ? "other-body"
              : viewer !== null && metadataFence.claimant === viewer
                ? null
                : "not-claimant";
    }
  }
  const thisExecutionFenced = fenceReason !== null;

  // Superseded and older continuations are history, not noise: "continued by
  // B until …" is what a returning participant is owed.
  const priorContinuation =
    [...fold.continuations]
      .reverse()
      .find((row) => row.eventId !== fold.activeContinuation) ?? null;

  const supersededCheckpoints = [...fold.checkpoints]
    .reverse()
    .filter((row) => row.standing === "superseded");

  const outcomeLabel: CodingSessionHandoverOutcomeLabel | null =
    continuation === null
      ? null
      : continuation.mode === "native-resume"
        ? "Native continuation"
        : "Reconstructed";

  const preserved = latestCheckpoint?.body.revision.preserved ?? null;
  const missing = uniqueLines([
    ...(preserved === "partial" || preserved === "none"
      ? [CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE]
      : []),
    ...(latestCheckpoint?.body.missing ?? []),
    ...(continuation?.missing ?? []),
  ]);
  const recovered = uniqueLines([...(continuation?.recovered ?? [])]);

  const evidenceLinks: CodingSessionHandoverEvidenceLink[] = [];
  if (claim.state === "active") {
    evidenceLinks.push({ label: "Claim", eventId: claim.acceptedEventId });
  } else if (claim.state === "voided") {
    evidenceLinks.push({
      label: "Voided claim",
      eventId: claim.last.acceptedEventId,
    });
    evidenceLinks.push({ label: "Voided by", eventId: claim.voidedBy });
  }
  if (latestCheckpoint) {
    evidenceLinks.push({
      label: "Checkpoint",
      eventId: latestCheckpoint.eventId,
    });
    for (const artifact of latestCheckpoint.body.artifacts) {
      if (artifact.kind === "patch") {
        evidenceLinks.push({ label: "Patch", eventId: artifact.eventId });
      }
    }
  }
  if (continuation) {
    evidenceLinks.push({
      label: "Continuation",
      eventId: continuation.eventId,
    });
  }

  return Object.freeze({
    claim,
    claimSince: fold.claimSince,
    claimVoidedAt: fold.claimVoidedAt,
    activeBody,
    continuation,
    latestCheckpoint,
    viewerMayContinue,
    viewerMayTakeBack,
    viewerIsClaimant,
    thisExecutionFenced,
    fenceReason,
    metadataFence,
    priorContinuation,
    supersededCheckpoints: Object.freeze(supersededCheckpoints),
    claimedBodyReachable,
    retired: fold.retired,
    retiredAt: input.retiredAt ?? null,
    evidenceLinks: Object.freeze(evidenceLinks),
    outcomeLabel,
    missing: Object.freeze(missing),
    recovered: Object.freeze(recovered),
  });
}
