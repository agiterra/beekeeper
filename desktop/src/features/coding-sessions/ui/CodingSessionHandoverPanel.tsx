import * as React from "react";

import { formatCoordinationAge } from "@/shared/coordination/sessionCoordinationFormat";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";
import { CODING_SESSION_READDRESS_LABEL } from "../lib/codingSessionTurnRefusal";
import type { CodingSessionHandoverModel } from "../lib/codingSessionHandoverModel";

/**
 * Who owns this session's work, and what a viewer may do about it.
 *
 * Three things this panel refuses to do. It never says "continued" without
 * saying **how** — native continuation and reconstruction are different
 * outcomes and the label distinguishes them. It never renders a claim without
 * the evidence a reader can open. And it never softens a partial
 * preservation: if the checkpoint's author measured that not everything was
 * preserved, the missing line says so however many artifacts are attached.
 *
 * Scope, said out loud in every label: a claim is umbrella-wide (§1). Handing
 * over means handing over the **whole session** — every execution and every
 * assignment under it — so the action's own words carry that consequence
 * rather than leaving a person to discover it after the fact.
 *
 * Colour never carries a fact alone: every state that tints also names itself
 * in a word ("Fenced", "Handover voided", "Deleted"), and every control is a
 * button with text rather than an icon a screen reader has to guess at.
 */
export function CodingSessionHandoverPanel({
  model,
  resolveName,
  onContinue,
  onTakeBack,
  onOpenEvidence,
  busy = null,
  errorMessage = null,
  noticeMessage = null,
  nowMs = Date.now(),
  workdirField = null,
  continueBlockedReason = null,
  capped = [],
}: {
  model: CodingSessionHandoverModel;
  /** Display name for a pubkey; falls back to the canonical truncation. */
  resolveName?: (pubkey: string) => string | null;
  /** Runs the claim + reconstruction. Absent when this viewer may not. */
  onContinue?: () => void;
  /** Publishes a takeover onto this viewer's own body. No reconstruction. */
  onTakeBack?: () => void;
  onOpenEvidence?: (eventId: string) => void;
  /** Which action is in flight, so its button says what it is doing. */
  busy?: "continue" | "take-back" | null;
  errorMessage?: string | null;
  /**
   * An outcome nobody can state yet — not a refusal.
   *
   * Rendered under its own word, because "Refused" over a create that may
   * still be sitting in the relay would be the app asserting the one thing it
   * does not know.
   */
  noticeMessage?: string | null;
  nowMs?: number;
  /**
   * The checkout picker this reconstruction will run in.
   *
   * A slot rather than a field of its own: the app already has one workdir
   * picker, with its own remembered paths per channel and project, and a
   * second one here would be a second answer to "where does my code live".
   */
  workdirField?: React.ReactNode;
  /**
   * Why the action cannot run yet, in the words a person can act on.
   *
   * Rendered *in place of* the button, so a control is never shown doing
   * nothing: "choose a checkout directory" is a prerequisite, not a failure.
   */
  continueBlockedReason?: string | null;
  /** Which reads came back at their ceiling, so the panel says it is partial. */
  capped?: readonly string[];
}) {
  const headingId = React.useId();
  const name = React.useCallback(
    (pubkey: string) => resolveName?.(pubkey) ?? truncatePubkey(pubkey),
    [resolveName],
  );
  // `at` is a signed epoch-second stamp; the age is measured from it, never
  // from arrival or render time.
  const age = (at: number | null) => {
    // Unknown is said, never rounded to "just now": a claim folded from
    // receipts alone has no timestamp, and a surface that guessed one would
    // be inventing the very fact a reader is checking.
    if (at === null) return "at an unrecorded time";
    const value = formatCoordinationAge(
      Math.max(0, Math.floor(nowMs / 1_000) - at),
    );
    return value === "just now" ? "just now" : `${value} ago`;
  };

  if (model.retired) {
    return (
      <section
        aria-labelledby={headingId}
        className="rounded-xl border border-border/60 bg-muted/30 px-3 py-2 text-sm"
        data-testid="coding-session-handover"
      >
        <h3 className="sr-only" id={headingId}>
          Session handover
        </h3>
        <p data-testid="coding-session-handover-retired">
          <span className="font-medium">Deleted</span>
          {model.retiredAt === null
            ? " — this session was deleted. Nothing here can be continued."
            : ` on ${new Date(model.retiredAt * 1_000).toLocaleDateString()} — this session was deleted. Nothing here can be continued.`}
        </p>
      </section>
    );
  }

  const claim = model.claim;
  const status = statusSentence(model, name, age);
  const fenced = model.thisExecutionFenced;
  // History, kept: a continuation whose claim has moved on still says who
  // carried this session and until when.
  const historical =
    model.priorContinuation !== null &&
    model.priorContinuation.eventId !== model.continuation?.eventId
      ? model.priorContinuation
      : null;

  return (
    <section
      aria-labelledby={headingId}
      className={cn(
        "rounded-xl border px-3 py-2 text-sm",
        fenced
          ? "border-amber-500/30 bg-amber-500/10"
          : "border-border/60 bg-muted/30",
      )}
      data-testid="coding-session-handover"
    >
      <h3 className="sr-only" id={headingId}>
        Session handover
      </h3>
      <p
        className="text-foreground"
        data-testid="coding-session-handover-status"
      >
        {status}
      </p>
      {model.outcomeLabel === null ? null : (
        <p
          className="mt-1 text-xs text-muted-foreground"
          data-testid="coding-session-handover-outcome"
        >
          <span className="font-medium text-foreground">
            {model.outcomeLabel}
          </span>
          {model.outcomeLabel === "Reconstructed"
            ? " — a new execution joined this session from a checkpoint. The original execution's own context stayed on its machine."
            : " — the original execution was resumed where it already ran."}
        </p>
      )}
      {model.recovered.length === 0 ? null : (
        <ul className="mt-1 text-xs text-muted-foreground">
          {model.recovered.map((line) => (
            <li key={line}>Recovered: {line}</li>
          ))}
        </ul>
      )}
      {model.missing.length === 0 ? null : (
        <ul
          className="mt-1 text-xs text-foreground"
          data-testid="coding-session-handover-missing"
        >
          {model.missing.map((line) => (
            <li key={line}>Missing: {line}</li>
          ))}
        </ul>
      )}
      {model.evidenceLinks.length === 0 ? null : (
        <p
          className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-2xs text-muted-foreground"
          data-testid="coding-session-handover-evidence"
        >
          {model.evidenceLinks.map((link) => (
            <button
              className="underline underline-offset-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              key={`${link.label}:${link.eventId}`}
              onClick={() => onOpenEvidence?.(link.eventId)}
              type="button"
            >
              {link.label} {truncatePubkey(link.eventId)}
            </button>
          ))}
        </p>
      )}
      {model.postClaimNextAction === null ? null : (
        <p
          className="mt-2 text-xs text-foreground"
          data-liveness={model.postClaimNextAction.liveness}
          data-testid="coding-session-handover-next-action"
        >
          {model.postClaimNextAction.sentence}
          {model.postClaimNextAction.liveness === "live" ? null : (
            <>
              {" "}
              Use <span className="font-medium">Reconnect</span> in the
              composer, or{" "}
              <span className="font-medium">
                “{CODING_SESSION_READDRESS_LABEL}”
              </span>{" "}
              for a turn it already owes you.
            </>
          )}
        </p>
      )}
      {fenced ? (
        <p
          className="mt-2 text-xs text-foreground"
          data-fence={model.fenceReason ?? ""}
          data-testid="coding-session-handover-fenced"
        >
          {fenceSentence(model, name)}
        </p>
      ) : null}
      {model.supersededCheckpoints.length === 0 ? null : (
        <p
          className="mt-1 text-2xs text-muted-foreground"
          data-testid="coding-session-handover-superseded"
        >
          {model.supersededCheckpoints.length === 1
            ? "An earlier checkpoint was replaced by its own author's next one"
            : `${model.supersededCheckpoints.length} earlier checkpoints were replaced by their own author's next one`}
          ; this reconstruction reads the newest statement, not the newest
          timestamp.
        </p>
      )}
      {historical === null ? null : (
        <p
          className="mt-1 text-xs text-muted-foreground"
          data-testid="coding-session-handover-history"
        >
          Continued by {name(historical.author)} until{" "}
          {new Date(historical.createdAt * 1_000).toLocaleDateString()}, under
          an earlier claim.
        </p>
      )}
      {model.claimedBodyReachable && claim.state === "active" ? (
        <p className="mt-2 text-xs text-muted-foreground">
          The claimed execution is reachable — continue from the composer rather
          than reconstructing it.
        </p>
      ) : null}
      {model.viewerMayContinue && !onContinue ? (
        <div className="mt-2">
          {workdirField === null ? null : (
            <div className="mb-2" data-testid="coding-session-handover-workdir">
              {workdirField}
            </div>
          )}
          <p
            className="text-xs text-foreground"
            data-testid="coding-session-handover-blocked"
          >
            <span className="font-medium">Not yet</span> —{" "}
            {continueBlockedReason ??
              "this computer has no provider to continue this session on."}
          </p>
        </div>
      ) : null}
      {model.viewerMayContinue && onContinue ? (
        <div className="mt-2">
          {workdirField === null ? null : (
            <div className="mb-2" data-testid="coding-session-handover-workdir">
              {workdirField}
            </div>
          )}
          <Button
            data-testid="coding-session-handover-continue"
            disabled={busy !== null}
            onClick={onContinue}
            size="sm"
            type="button"
          >
            {busy === "continue"
              ? "Taking over this session…"
              : "Continue this session's work"}
          </Button>
          <p className="mt-1 text-xs text-muted-foreground">
            This hands over the whole session: every execution and assignment
            under it is fenced until you release or someone takes it back.
          </p>
        </div>
      ) : null}
      {/* One action at a time: continuing *is* taking over, plus the
          reconstruction. Offering both would be two buttons for one act. */}
      {model.viewerMayTakeBack && onTakeBack && !model.viewerMayContinue ? (
        <div className="mt-2">
          <Button
            data-testid="coding-session-handover-take-back"
            disabled={busy !== null}
            onClick={onTakeBack}
            size="sm"
            type="button"
            variant="outline"
          >
            {busy === "take-back"
              ? "Taking this session back…"
              : claim.state === "voided"
                ? "Take over this session"
                : "Take this session back"}
          </Button>
          <p className="mt-1 text-xs text-muted-foreground">
            This hands over the whole session: every execution and assignment
            under it is fenced until you release or someone takes it back.
          </p>
        </div>
      ) : null}
      {capped.length === 0 ? null : (
        <p
          className="mt-2 text-2xs text-muted-foreground"
          data-testid="coding-session-handover-capped"
        >
          This read of {capped.join(" and ")} came back at its limit, so what is
          above may be part of the history rather than all of it.
        </p>
      )}
      {noticeMessage === null ? null : (
        <p
          className="mt-2 text-xs text-foreground"
          data-testid="coding-session-handover-notice"
        >
          <span className="font-medium">Unsettled</span> — {noticeMessage}
        </p>
      )}
      {errorMessage === null ? null : (
        <p
          className="mt-2 text-xs text-foreground"
          data-testid="coding-session-handover-error"
        >
          <span className="font-medium">Refused</span> — {errorMessage}
        </p>
      )}
    </section>
  );
}

/**
 * The fence, in the words of the fact that causes it.
 *
 * Never "held elsewhere" for a voided claim — nobody holds it, which is
 * exactly why it is frozen — and never silence when this app cannot tell which
 * body it is looking at.
 */
function fenceSentence(
  model: CodingSessionHandoverModel,
  name: (pubkey: string) => string,
): React.ReactNode {
  const claimant =
    model.claim.state === "active"
      ? model.claim.claimant
      : model.claim.state === "voided"
        ? model.claim.last.claimant
        : (model.metadataFence?.claimant ?? null);
  const disclosed = model.claim.state === "no-claim" && model.metadataFence;
  const source = disclosed
    ? " (as this execution's own provider reports it)"
    : "";
  if (model.fenceReason === "unknown") {
    return (
      <>
        <span className="font-medium">Fence unknown</span> — this session is
        held{claimant === null ? "" : ` by ${name(claimant)}`}, and this app
        cannot tell whether the execution in front of you is the claimed body.
      </>
    );
  }
  if (model.fenceReason === "voided") {
    return (
      <>
        <span className="font-medium">Fenced</span> — every execution of this
        session is frozen, including this one, until someone with standing takes
        it over{source}.
      </>
    );
  }
  if (model.fenceReason === "other-body") {
    return (
      <>
        <span className="font-medium">Fenced</span> — this execution
        {model.activeBody === null
          ? ""
          : ` is not ${truncatePubkey(model.activeBody)}`}
        , the body holding this session, so its turns will be refused{source}.
      </>
    );
  }
  return (
    <>
      <span className="font-medium">Fenced</span> —{" "}
      {claimant === null ? "somebody else" : name(claimant)} holds this session;
      your turns on this execution will be refused until you take it back
      {source}.
    </>
  );
}

/**
 * The one sentence at the top, built from the claim state and nothing else.
 *
 * `none` and `voided` are deliberately different sentences: "nobody has taken
 * this over" and "somebody took it over and then lost standing, so it stays
 * fenced" are different facts, and a reader who is told the first when the
 * second is true will keep trying to steer an execution that cannot act.
 */
function statusSentence(
  model: CodingSessionHandoverModel,
  name: (pubkey: string) => string,
  age: (at: number | null) => string,
): React.ReactNode {
  const claim = model.claim;
  if (claim.state === "voided") {
    return (
      <>
        <span className="font-medium">Handover voided</span> —{" "}
        {name(claim.last.claimant)} lost standing {age(model.claimVoidedAt)}.
        Every execution of this session stays fenced until someone with standing
        takes it over.
      </>
    );
  }
  if (claim.state === "active") {
    return (
      <>
        <span className="font-medium">Active</span> — {name(claim.claimant)}{" "}
        holds this session on {truncatePubkey(claim.bodyPubkey)}, since{" "}
        {age(model.claimSince)}.
      </>
    );
  }
  return (
    <>
      <span className="font-medium">No handover</span> — nobody has taken this
      session over.
    </>
  );
}
