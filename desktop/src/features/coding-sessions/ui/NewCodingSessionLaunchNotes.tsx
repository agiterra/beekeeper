import { CircleAlert, CircleCheck, LoaderCircle } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import type { CodingSessionCrewLaunchStep } from "../lib/codingSessionCrewLaunch";
import {
  codingSessionGoalOverflowSentence,
  MAX_CODING_SESSION_GOAL_BYTES,
} from "../lib/codingSessionGoal";

/**
 * The two things the launch form still needs from the deleted *Team* tab: what
 * the goal field owes the person, and the signed sequence as it walks — today
 * the founding steps (channel, genesis, goal, name), and the step shape is the
 * crew launch's so the founded screen's Start can render its own with the
 * same row.
 *
 * `NewCodingSessionCrewTab.tsx` is gone. It held twelve exports, ten of which
 * lost their only production caller when the tab did, and its own test suite
 * kept all ten green so nothing would ever have flagged them (REVIEW-B3 F5) —
 * including `CodingSessionCrewRoster`, the roster that told the "four rows, one
 * live agent" lie D14 removed. These two are the survivors, in a file named
 * for what they are.
 */

/**
 * Show size feedback near the limit and disclose a failed goal publication.
 *
 * The cap is in UTF-8 bytes and the field counts characters, so feedback uses
 * the actual wire size. Ordinary short goals need no encoding explanation;
 * approaching the cap shows usage, and exceeding it names the refusal. An
 * over-cap goal used to launch a whole team and quietly publish no kind:44227
 * (item 103 finding 5; batch 2 review A2 F1). At the cap the counter *becomes*
 * the refusal, in the launch block's own words, so the two cannot drift. After
 * a founding, a goal that did not go out says so here rather than showing up
 * as an empty goal pill in the session, which reads as "nobody set one".
 */
export function CodingSessionLaunchGoalNotes({
  bytes,
  goalOutcome,
  overflow,
}: {
  /** The trimmed goal's size in UTF-8 bytes — the unit the cap is in. */
  bytes: number;
  /** What the last launch did about the goal, or null before one. */
  goalOutcome: { published: boolean; reason: string | null } | null;
  /** The overflow the shared helper reports, or null when the goal fits. */
  overflow: { bytes: number; cap: number } | null;
}) {
  return (
    <>
      {overflow === null ? (
        bytes >= MAX_CODING_SESSION_GOAL_BYTES * 0.8 ? (
          <p
            className="text-2xs text-muted-foreground"
            data-testid="new-coding-session-crew-goal-bytes"
            title={`${bytes.toLocaleString()} of ${MAX_CODING_SESSION_GOAL_BYTES.toLocaleString()} UTF-8 bytes.`}
          >
            {`${Math.round((bytes / MAX_CODING_SESSION_GOAL_BYTES) * 100)}% of the description limit used.`}
          </p>
        ) : null
      ) : (
        <p
          className="text-2xs text-destructive"
          data-testid="new-coding-session-crew-goal-bytes"
        >
          {codingSessionGoalOverflowSentence(overflow)}
        </p>
      )}
      {goalOutcome !== null && !goalOutcome.published ? (
        <p
          className="text-2xs text-destructive"
          data-testid="new-coding-session-crew-goal-unpublished"
          role="alert"
        >
          {`The session was founded, but its goal was not published: ${
            goalOutcome.reason ?? "the goal publish did not go out"
          }. Set it from the session's goal pill.`}
        </p>
      ) : null}
    </>
  );
}

/** The signed sequence, one row per step, with the failed one named. */
export function CodingSessionLaunchSteps({
  steps,
}: {
  steps: CodingSessionCrewLaunchStep[];
}) {
  return (
    <ol
      className="flex flex-col gap-1"
      data-testid="new-coding-session-crew-steps"
    >
      {steps.map((step) => (
        <li
          className={cn(
            "flex items-start gap-2 text-2xs",
            step.state === "failed"
              ? "text-destructive"
              : step.state === "done"
                ? "text-muted-foreground"
                : "text-muted-foreground/70",
          )}
          data-state={step.state}
          data-testid={`crew-step-${step.id}`}
          key={step.id}
        >
          {step.state === "running" ? (
            <LoaderCircle className="mt-0.5 size-3 shrink-0 animate-spin motion-reduce:animate-none" />
          ) : step.state === "done" ? (
            <CircleCheck className="mt-0.5 size-3 shrink-0" />
          ) : step.state === "failed" ? (
            <CircleAlert className="mt-0.5 size-3 shrink-0" />
          ) : (
            <span className="mt-0.5 size-3 shrink-0" />
          )}
          <span>
            {step.label}
            {step.detail ? ` — ${step.detail}` : ""}
          </span>
        </li>
      ))}
    </ol>
  );
}
