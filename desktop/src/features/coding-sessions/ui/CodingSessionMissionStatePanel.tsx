import { Check, Circle, CircleDot, OctagonAlert } from "lucide-react";

import type {
  CodingSessionMissionCanonicalStep,
  CodingSessionMissionStateInput,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  missionRowBodyClass,
  missionRowClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import { cn } from "@/shared/lib/cn";

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
export function CodingSessionMissionStatePanel({
  state,
}: {
  state: CodingSessionMissionStateInput;
}) {
  const label = STATE_LABEL[state.kind];
  const phase = codingSessionMissionPhase(state);
  const phaseWord = phase ?? "phase not established";
  return (
    <div data-mission-state={state.kind} data-testid="mission-state-summary">
      <p className="text-sm font-medium">
        {label} <span className="text-muted-foreground">· {phaseWord}</span>
      </p>
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
    </div>
  );
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
