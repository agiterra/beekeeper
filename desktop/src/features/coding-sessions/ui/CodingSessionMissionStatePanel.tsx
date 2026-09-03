import { Check, Circle, CircleDot, OctagonAlert } from "lucide-react";

import {
  codingSessionMissionAskedRelative,
  type CodingSessionMissionCanonicalStep,
  type CodingSessionMissionStateInput,
  type CodingSessionMissionWaitingModel,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { CODING_SESSION_COMPLETION_REFUSED_STATE_LINE } from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type { CodingSessionMissionLandModel } from "@/features/coding-sessions/lib/codingSessionMissionLand";
import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";
import { CodingSessionMissionLandControl } from "./CodingSessionMissionLandControl";

/** The four signed phases of one governed assignment, in order. */
const PHASES = ["assigned", "reported", "ruled", "acknowledged"] as const;

type MissionPhase = (typeof PHASES)[number];

/** Copy the deleted pinned transaction card owned. Moved here verbatim. */
const STATE_LABEL: Readonly<
  Record<CodingSessionMissionStateInput["kind"], string>
> = {
  completed: "Mission completed",
  blocked: "Mission blocked",
  "waiting-on-person": "Waiting on a person",
  "acknowledgement-required": "Acknowledgement required",
  stalled: "Mission stalled",
  running: "Mission running",
  conflict: "Mission state conflict",
  unknown: "Mission state unknown",
};

const STEP_PHASE: Readonly<
  Record<CodingSessionMissionCanonicalStep["type"], MissionPhase>
> = {
  assignment: "assigned",
  report: "reported",
  refutation: "ruled",
  disposition: "ruled",
  acknowledgement: "acknowledged",
};

/**
 * Resolve which phase the signed chain has actually reached.
 *
 * A phase is read off the last canonical step, never guessed from silence: a
 * state with no chain and no declared phase returns `null`, and the indicator
 * says the phase is not established rather than painting `assigned`.
 */
export function codingSessionMissionPhase(
  state: CodingSessionMissionStateInput,
): MissionPhase | null {
  if (state.kind === "running") return state.phase;
  if (state.kind === "completed") return "acknowledged";
  const chain =
    "canonicalChain" in state ? (state.canonicalChain ?? []) : undefined;
  const last = chain && chain.length > 0 ? chain[chain.length - 1] : undefined;
  if (last) return STEP_PHASE[last.type];
  if (state.kind === "acknowledgement-required") return "ruled";
  return null;
}

/**
 * Mission's state plane, in one line and one indicator.
 *
 * The chain list this replaces lived inside a card pinned above the stream and
 * restated, out of order, the same signed transactions the stream now renders
 * in place. What is left here is the part a rail is actually for: what state
 * the mission is in, how far the governed handoff has got, and — when the
 * answer is "blocked" — what a person has to do about it.
 */
/**
 * §1l's sentence for a completion the fold excluded `completion_not_verified`.
 *
 * The same code string `bee` prints, never a paraphrase: a founder reading two
 * different sentences for one exclusion has to work out whether they are the
 * same fact, and the answer must never be "it depends which surface".
 */
export const CODING_SESSION_COMPLETION_NOT_VERIFIED_SENTENCE =
  "Completed, but not verified: this session's policy requires a verifier's " +
  "ruling and no active verifier has ruled on the approved report. The " +
  "completion is not this mission's terminal until one does.";

/** §1l's disclosure when no 44245 reached this view. Unknown ≠ false. */
export const CODING_SESSION_NO_POLICY_RECORD_SENTENCE =
  "No policy record reached this view, so the fold read no verifier requirement.";

export function CodingSessionMissionStatePanel({
  completionNotVerified = false,
  land = null,
  nowMs = Date.now(),
  policyRecordKnown = true,
  state,
  waiting = null,
  completionRefusedNoVerifier = false,
}: {
  /**
   * Whether the Rust fold excluded a `mission.completed` with L7's
   * `completion_not_verified`. Read from the fold's own `excluded[]` code;
   * this panel never decides it (I6).
   */
  completionNotVerified?: boolean;
  /**
   * What the push path's rule says about landing this mission's commit, or
   * null when this surface did not ask.
   */
  land?: CodingSessionMissionLandModel | null;
  /**
   * Whether a kind:44245 record reached this view at all.
   *
   * False is disclosed rather than folded into "no requirement": unknown is
   * not the same fact as a policy that set none, and the fold that ran with
   * `verifierRequired: false` did so because nothing told it otherwise.
   */
  policyRecordKnown?: boolean;
  /** The clock, read once at render — never a timer (I1). */
  nowMs?: number;
  state: CodingSessionMissionStateInput;
  /**
   * The fold's waiting-on-a-person fact, already qualified by this surface's
   * own liveness (`isStateLine`). Null — the default — leaves this panel
   * byte-identical to what it rendered before the queue existed.
   */
  waiting?: CodingSessionMissionWaitingModel | null;
  /**
   * Whether this mission's `mission.completed` was excluded
   * `completion_not_verified` by the fold.
   *
   * A mission whose completion the fold refused is **not running**, and until
   * this prop existed the panel said `Mission running` over exactly that
   * (§1k, REVIEW-L7 F5). `false` — the default — leaves every other mission's
   * line byte-identical.
   */
  completionRefusedNoVerifier?: boolean;
}) {
  // §1k's line, and it takes the state line outright: the fold has refused
  // this mission's only terminal, so no other word for the state is true.
  const label = completionRefusedNoVerifier
    ? CODING_SESSION_COMPLETION_REFUSED_STATE_LINE
    : STATE_LABEL[state.kind];
  const phase = codingSessionMissionPhase(state);
  const phaseWord = phase ?? "phase not established";
  const asked =
    waiting === null
      ? null
      : codingSessionMissionAskedRelative(waiting.askedAtMs, nowMs);
  // §1g: with a timestamp the timeline already shows, append the age; with
  // none, append nothing — never `0m`.
  const waitingLine =
    waiting === null
      ? null
      : asked === null
        ? waiting.line
        : `${waiting.line} · asked ${asked}`;
  const placement = waiting?.placement ?? null;
  return (
    <div
      data-mission-state={state.kind}
      data-mission-waiting={placement ?? undefined}
      data-testid="mission-state-summary"
    >
      <p className="text-sm font-medium">
        {/* Three placements, one rule: the waiting fact never removes a fact.
            With no open lead turn a running mission is *only* waiting, so
            waiting is the state. With the lead working it is a second fact
            beside the state. And a mission that has **ended** does not stop
            having ended because somebody owes a ruling (F4) — there the
            terminal word keeps the line and the waiting fact is appended to
            it. Saying only one of two true things is how a rail lies by
            omission. */}
        {placement === "state-line" && waitingLine !== null
          ? waitingLine
          : placement === "appended" && waitingLine !== null
            ? `${label} · ${lowerFirst(waitingLine)}`
            : label}{" "}
        <span className="text-muted-foreground">· {phaseWord}</span>
      </p>
      {placement === "beside" && waitingLine !== null ? (
        <p
          className={cn(missionRowBodyClass(), "mt-1 font-medium")}
          data-testid="mission-state-waiting"
        >
          {waitingLine}
        </p>
      ) : null}
      <PhaseIndicator current={phase} label={label} />
      {state.kind === "blocked" ? (
        <div
          className={missionRowClass("attention", {
            tone: "critical",
            className: "mt-2",
          })}
          data-testid="mission-state-blocked"
        >
          <p className="flex items-center gap-2 text-xs font-medium text-destructive">
            <OctagonAlert aria-hidden className="size-3.5 shrink-0" />
            {state.summary}
          </p>
          <ul className="mt-1.5 list-disc space-y-1 pl-4 text-xs">
            {state.blockers.map((blocker) => (
              <li key={blocker}>{blocker}</li>
            ))}
          </ul>
          <p className={cn(missionRowBodyClass(), "mt-1.5 font-medium")}>
            Required action: {state.requiredAction}
          </p>
        </div>
      ) : null}
      {state.kind === "completed" ? (
        <div className="mt-1.5">
          <p className={missionRowBodyClass()}>{state.summary}</p>
          {state.landedShas.length > 0 ? (
            <p className={cn(missionRowMetaClass(), "mt-1 break-all")}>
              Landed{" "}
              {state.landedShas.map((sha, index) => (
                <span key={sha}>
                  {index > 0 ? ", " : null}
                  {/* Compact, with the exact sha one hover away — a 40-char
                      hash printed raw is a wall the rail cannot hold. */}
                  <code title={sha}>{compactSha(sha)}</code>
                </span>
              ))}
            </p>
          ) : null}
          {state.followUps.length > 0 ? (
            <p className={cn(missionRowMetaClass(), "mt-1")}>
              Follow-ups: {state.followUps.join(" · ")}
            </p>
          ) : null}
        </div>
      ) : null}
      {state.kind === "acknowledgement-required" ||
      state.kind === "waiting-on-person" ? (
        <p className={cn(missionRowBodyClass(), "mt-1.5 font-medium")}>
          Required action: {state.requiredAction}
          {state.heldOn ? ` · held on ${state.heldOn}` : null}
        </p>
      ) : null}
      {state.kind === "running" || state.kind === "stalled" ? (
        <p className={cn(missionRowBodyClass(), "mt-1.5")}>{state.detail}</p>
      ) : null}
      {state.kind === "conflict" ? (
        <p className={cn(missionRowBodyClass(), "mt-1.5")}>
          {state.eventIds.length} signed terminal records conflict; no terminal
          state is shown.
        </p>
      ) : null}
      {state.kind === "unknown" ? (
        <p className={cn(missionRowBodyClass(), "mt-1.5")}>
          {state.detail ??
            "No canonical typed transaction establishes the current state. Silence is not completion."}
        </p>
      ) : null}
      {/* L8.3: the fold's own `completion_not_verified`, in the same words the
          CLI prints. It is a *state* fact, not a terminal — the completion is
          on the wire and the fold refused it — so it sits under the state line
          rather than replacing it. */}
      {completionNotVerified ? (
        <p
          className={cn(missionRowBodyClass(), "mt-1.5 font-medium")}
          data-testid="mission-completion-not-verified"
        >
          {CODING_SESSION_COMPLETION_NOT_VERIFIED_SENTENCE}
        </p>
      ) : null}
      {policyRecordKnown ? null : (
        <p
          className={cn(missionRowMetaClass(), "mt-1")}
          data-testid="mission-policy-record-unknown"
        >
          {CODING_SESSION_NO_POLICY_RECORD_SENTENCE}
        </p>
      )}
      {land === null ? null : <CodingSessionMissionLandControl land={land} />}
    </div>
  );
}

/** `Waiting on …` → `waiting on …`, so it reads as a clause after the state word. */
function lowerFirst(value: string): string {
  return value.length === 0 ? value : value[0].toLowerCase() + value.slice(1);
}

function PhaseIndicator({
  current,
  label,
}: {
  current: MissionPhase | null;
  label: string;
}) {
  const currentIndex = current === null ? -1 : PHASES.indexOf(current);
  return (
    <ol
      aria-label={
        current === null
          ? `${label} — signed phase not established`
          : `${label} — current phase ${current}`
      }
      className="mt-2 flex flex-wrap items-center gap-x-1.5 gap-y-1"
      data-current-phase={current ?? "not-established"}
      data-testid="mission-state-phase-indicator"
    >
      {PHASES.map((phase, index) => {
        const done = currentIndex >= 0 && index < currentIndex;
        const active = index === currentIndex;
        const Icon = done ? Check : active ? CircleDot : Circle;
        return (
          <li
            className={cn(
              "inline-flex items-center gap-1 text-2xs",
              active
                ? "font-semibold text-foreground"
                : done
                  ? "text-emerald-600 dark:text-emerald-400"
                  : "text-muted-foreground",
            )}
            data-phase={phase}
            data-phase-state={done ? "done" : active ? "current" : "pending"}
            key={phase}
          >
            <Icon aria-hidden className="size-3 shrink-0" />
            <span>{phase}</span>
            {index < PHASES.length - 1 ? (
              <span aria-hidden className="ml-1 text-muted-foreground/60">
                →
              </span>
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

/** The rail's compact form for a landed commit sha; the exact value is the `title`. */
function compactSha(sha: string): string {
  return sha.length > 20 ? `${sha.slice(0, 8)}…${sha.slice(-6)}` : sha;
}
