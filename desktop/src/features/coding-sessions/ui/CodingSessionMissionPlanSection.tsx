import {
  Check,
  Circle,
  CircleDashed,
  CircleDot,
  Minus,
  TriangleAlert,
  X,
} from "lucide-react";

import type {
  CodingSessionMissionInspectorModel,
  CodingSessionMissionPlanStep,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { cn } from "@/shared/lib/cn";

import {
  EmptyCopy,
  SignedSource,
} from "./CodingSessionMissionInspectorPrimitives";

/**
 * The Inspector's plan renderers: the accepted plan and its steps.
 *
 * Split out of `CodingSessionMissionInspector.tsx`, which passed the
 * repository's 1,000-line ceiling as L4 added the goal reader's states and the
 * open-hold lines. The rule is to split the file, never to raise the limit.
 * Nothing about the plan's markup or copy changed in the move; the plan
 * sections are also the block no other lane in this batch is editing, which is
 * what makes them the safe cut.
 */
export function AcceptedPlan({
  plan,
}: {
  plan: CodingSessionMissionInspectorModel["acceptedPlan"];
}) {
  if (plan.kind !== "available") {
    return (
      <div>
        <p
          className={cn(
            "text-xs",
            plan.kind === "conflict"
              ? "text-amber-700 dark:text-amber-300"
              : "text-muted-foreground",
          )}
        >
          {plan.label}
        </p>
        {plan.sourceEventIds.map((eventId) => (
          <SignedSource eventId={eventId} key={eventId} />
        ))}
      </div>
    );
  }
  return (
    <div>
      {plan.steps.length > 0 ? (
        <PlanSteps
          ariaLabel="Accepted mission plan"
          showProvenance
          steps={plan.steps}
        />
      ) : (
        <EmptyCopy>{plan.label}</EmptyCopy>
      )}
    </div>
  );
}

export function PlanSteps({
  ariaLabel,
  showProvenance = false,
  steps,
}: {
  ariaLabel: string;
  showProvenance?: boolean;
  steps: readonly CodingSessionMissionPlanStep[];
}) {
  return (
    <ol aria-label={ariaLabel} className="mt-2 space-y-1.5">
      {steps.map((step, index) => (
        <li className="flex gap-2 text-xs" key={step.id}>
          <PlanStepStatus status={step.status} />
          <div className="min-w-0 flex-1">
            <span className="mr-1 text-muted-foreground tabular-nums">
              {index + 1}.
            </span>
            {step.text}
            {showProvenance && step.authorLabel && step.sourceEventId ? (
              <div className="mt-1">
                <p className="text-2xs text-muted-foreground">
                  Accepted from {visibleSourceAuthor(step.authorLabel)}
                </p>
                {step.sourceCreatedAt !== null && step.sourceIndex !== null ? (
                  <p className="text-2xs text-muted-foreground">
                    Signed criterion source index {step.sourceIndex} ·{" "}
                    <time
                      dateTime={new Date(
                        step.sourceCreatedAt * 1000,
                      ).toISOString()}
                    >
                      {new Date(step.sourceCreatedAt * 1000).toLocaleString(
                        [],
                        { dateStyle: "medium", timeStyle: "short" },
                      )}
                    </time>
                  </p>
                ) : null}
                <SignedSource
                  authorLabel={
                    isRawSourceIdentifier(step.authorLabel)
                      ? step.authorLabel
                      : undefined
                  }
                  eventId={step.sourceEventId}
                />
              </div>
            ) : null}
          </div>
        </li>
      ))}
    </ol>
  );
}

function PlanStepStatus({
  status,
}: {
  status: CodingSessionMissionPlanStep["status"];
}) {
  const Icon =
    status === "completed"
      ? Check
      : status === "in_progress"
        ? CircleDot
        : status === "blocked" || status === "failed"
          ? TriangleAlert
          : status === "cancelled"
            ? X
            : status === "unknown"
              ? CircleDashed
              : status === null
                ? Minus
                : Circle;
  const label =
    status === null ? "status not reported" : status.replace("_", " ");
  return (
    <span
      className={cn(
        "mt-0.5 inline-flex shrink-0 items-center gap-1 text-2xs",
        status === "completed" && "text-emerald-600 dark:text-emerald-400",
        status === "in_progress" && "text-primary",
        (status === "blocked" || status === "failed") &&
          "text-amber-700 dark:text-amber-300",
        status === "unknown" && "text-amber-700 dark:text-amber-300",
        (status === null || status === "pending" || status === "cancelled") &&
          "text-muted-foreground",
      )}
      data-plan-status={status ?? "not-reported"}
    >
      <Icon aria-hidden className="size-3.5" />
      <span>{label}</span>
    </span>
  );
}

function visibleSourceAuthor(authorLabel: string): string {
  return isRawSourceIdentifier(authorLabel)
    ? `${authorLabel.slice(0, 8)}…${authorLabel.slice(-6)}`
    : authorLabel;
}

function isRawSourceIdentifier(value: string): boolean {
  return /^[0-9a-f]{64}$/i.test(value);
}
