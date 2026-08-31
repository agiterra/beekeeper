import {
  Check,
  Circle,
  CircleDashed,
  CircleDot,
  Minus,
  OctagonAlert,
  TriangleAlert,
  X,
} from "lucide-react";

import { renderCodingSessionContextLoad } from "@/features/coding-sessions/lib/codingSessionContextLoad";
import type {
  CodingSessionMissionDisclosureInput,
  CodingSessionMissionGoalModel,
  CodingSessionMissionInspectorModel,
  CodingSessionMissionInspectorSection,
  CodingSessionMissionPlanStep,
  CodingSessionMissionStateInput,
  CodingSessionMissionUsageInput,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import { cn } from "@/shared/lib/cn";

export type CodingSessionMissionInspectorProps = {
  model: CodingSessionMissionInspectorModel;
  variant: "panel" | "drawer";
  focusedExecutionKey: string | null;
  loading?: boolean;
  errorMessage?: string | null;
  onFocusParticipant?: (executionKey: string | null) => void;
  /** Finalizer-owned bridge into Trace when observed-file provenance is absent. */
  onOpenFileTrace?: (path: string) => void;
  onRefresh?: () => void;
};

/**
 * Mission's state plane. `panel` is the wide-workspace aside; `drawer` is the
 * same scrollable content for a finalizer-owned narrow Sheet/Dialog. This
 * component deliberately does not mount or control either surrounding lens.
 */
export function CodingSessionMissionInspector({
  errorMessage = null,
  model,
  variant,
  focusedExecutionKey,
  loading = false,
  onFocusParticipant,
  onOpenFileTrace,
  onRefresh,
}: CodingSessionMissionInspectorProps) {
  return (
    <aside
      aria-label="Mission inspector"
      className={cn(
        "flex h-full min-h-0 flex-col bg-background text-foreground",
        variant === "panel" && "w-full",
        variant === "drawer" && "w-full",
      )}
      data-testid="coding-session-mission-inspector"
      data-variant={variant}
    >
      <header className="shrink-0 border-b border-border/60 px-4 py-3">
        <p className="text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
          Mission
        </p>
        <h2 className="mt-0.5 text-sm font-semibold">Inspector</h2>
      </header>

      {loading ? (
        <p
          className="border-b border-border/60 bg-muted/20 px-4 py-2 text-xs text-muted-foreground"
          role="status"
        >
          Loading signed Mission evidence…
        </p>
      ) : null}
      {errorMessage ? (
        <div
          className="border-b border-destructive/35 bg-destructive/10 px-4 py-2"
          role="alert"
        >
          <p className="text-xs text-destructive">{errorMessage}</p>
          {onRefresh ? (
            <button
              className="mt-1 rounded-sm text-2xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={onRefresh}
              type="button"
            >
              Retry signed evidence
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-8">
        <InspectorSection
          title="Goal"
          truncations={truncationsFor(model, "goal")}
        >
          <Goal goal={model.goal} />
        </InspectorSection>

        <InspectorSection
          title="Mission state"
          truncations={truncationsFor(model, "mission-state")}
        >
          <MissionState state={model.missionState} />
        </InspectorSection>

        <InspectorSection
          title="Accepted plan"
          truncations={truncationsFor(model, "accepted-plan")}
        >
          <AcceptedPlan plan={model.acceptedPlan} />
        </InspectorSection>

        <InspectorSection
          title="Seat-reported plans"
          truncations={truncationsFor(model, "seat-plans")}
        >
          {model.seatPlans.length === 0 ? (
            <EmptyCopy>No seat has published a signed plan.</EmptyCopy>
          ) : (
            <div className="space-y-3">
              {model.seatPlans.map((plan) => (
                <div data-testid="mission-seat-plan" key={plan.executionKey}>
                  <div className="flex items-baseline justify-between gap-2">
                    <p className="text-xs font-medium">{plan.ownerLabel}</p>
                    {plan.state !== "absent" ? (
                      <span className="text-2xs text-muted-foreground tabular-nums">
                        {plan.completedCount}/{plan.steps.length}
                      </span>
                    ) : null}
                  </div>
                  {plan.explanation ? (
                    <p className="mt-1 text-xs text-muted-foreground">
                      {plan.explanation}
                    </p>
                  ) : null}
                  {plan.steps.length === 0 ? (
                    <EmptyCopy>
                      No signed plan published by this seat.
                    </EmptyCopy>
                  ) : (
                    <PlanSteps
                      ariaLabel={`${plan.ownerLabel} seat-reported plan`}
                      steps={plan.steps}
                    />
                  )}
                  {plan.sourceEventId ? (
                    <SignedSource eventId={plan.sourceEventId} />
                  ) : null}
                </div>
              ))}
            </div>
          )}
        </InspectorSection>

        <InspectorSection
          title="Changes"
          truncations={truncationsFor(model, "changes")}
        >
          <Changes model={model} />
        </InspectorSection>

        <InspectorSection
          title="Files"
          truncations={truncationsFor(model, "files")}
        >
          {model.files.length === 0 ? (
            <EmptyCopy>No observed or seat-reported files.</EmptyCopy>
          ) : (
            <ul aria-label="Mission files" className="space-y-2">
              {model.files.map((file) => (
                <li className="min-w-0" key={file.path}>
                  <code className="block break-all text-xs text-foreground">
                    {file.path}
                  </code>
                  <p className="mt-0.5 text-2xs text-muted-foreground">
                    {file.observed ? "Observed file edit" : null}
                    {file.observed && file.reportedBy.length > 0 ? " · " : null}
                    {file.reportedBy.length > 0
                      ? `Reported by ${file.reportedBy.map((source) => source.authorLabel).join(", ")}`
                      : null}
                    {file.editCount !== null
                      ? ` · ${file.editCount} ${file.editCount === 1 ? "edit" : "edits"}`
                      : null}
                  </p>
                  {file.observed
                    ? file.observedSourceEventIds.map((eventId) => (
                        <SignedSource eventId={eventId} key={eventId} />
                      ))
                    : null}
                  {file.observed && !file.observedSourceKnown ? (
                    <div className="mt-1">
                      <p className="text-2xs text-amber-700 dark:text-amber-300">
                        Observed source is unavailable in this projection.
                      </p>
                      {onOpenFileTrace ? (
                        <button
                          className="mt-1 rounded-sm text-2xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          onClick={() => onOpenFileTrace(file.path)}
                          type="button"
                        >
                          Open Trace for source evidence
                        </button>
                      ) : (
                        <p className="mt-1 text-2xs text-muted-foreground">
                          Trace action requires final integration.
                        </p>
                      )}
                    </div>
                  ) : null}
                  {file.reportedBy.map((source) => (
                    <div key={source.sourceEventId}>
                      <p className="mt-1 text-2xs text-muted-foreground">
                        {source.authorLabel} report source
                      </p>
                      <SignedSource eventId={source.sourceEventId} />
                    </div>
                  ))}
                </li>
              ))}
            </ul>
          )}
        </InspectorSection>

        <InspectorSection
          title="Structured tests"
          truncations={truncationsFor(model, "tests")}
        >
          {model.tests.length === 0 ? (
            <EmptyCopy>No structured test results published.</EmptyCopy>
          ) : (
            <ul aria-label="Structured test results" className="space-y-2">
              {model.tests.map((result) => (
                <li
                  className="rounded-lg border border-border/60 bg-muted/15 p-2.5"
                  key={result.id}
                >
                  <div className="flex items-start gap-2">
                    <TestIcon outcome={result.outcome} />
                    <div className="min-w-0 flex-1">
                      <p className="text-xs font-medium">{result.name}</p>
                      <code className="mt-1 block break-all text-2xs text-muted-foreground">
                        {result.command}
                      </code>
                      <p className="mt-1 text-2xs text-muted-foreground">
                        {result.outcome} · reported by {result.authorLabel}
                      </p>
                      {result.evidence ? (
                        <p className="mt-1 text-xs">{result.evidence}</p>
                      ) : (
                        <p className="mt-1 text-2xs text-muted-foreground">
                          No evidence string published.
                        </p>
                      )}
                      <SignedSource eventId={result.sourceEventId} />
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </InspectorSection>

        <InspectorSection
          title="Team"
          truncations={truncationsFor(model, "team")}
        >
          {model.participants.length === 0 ? (
            <EmptyCopy>No signed session seats projected.</EmptyCopy>
          ) : (
            <ul aria-label="Mission team" className="space-y-1.5">
              {model.participants.map((participant) => {
                const focused =
                  participant.executionKey === focusedExecutionKey;
                const accent = codingSessionParticipantAccent(
                  participant.executionKey,
                );
                return (
                  <li key={participant.executionKey}>
                    <button
                      aria-label={`${focused ? "Show all participants" : `Focus ${participant.label}`} — ${participant.disposition}`}
                      aria-pressed={focused}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                        focused
                          ? cn(accent.border, accent.soft)
                          : "border-border/60 hover:bg-muted/35",
                      )}
                      data-testid="mission-team-participant"
                      onClick={() =>
                        onFocusParticipant?.(
                          focused ? null : participant.executionKey,
                        )
                      }
                      type="button"
                    >
                      <span
                        aria-hidden
                        className={cn(
                          "size-2 shrink-0 rounded-full",
                          accent.dot,
                        )}
                      />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-xs font-medium">
                          {participant.label}
                        </span>
                        <span className="block truncate text-2xs text-muted-foreground">
                          {participant.disposition} · context{" "}
                          {renderCodingSessionContextLoad(
                            participant.contextLoad,
                          )}
                        </span>
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </InspectorSection>

        <InspectorSection
          title="Context"
          truncations={truncationsFor(model, "context")}
        >
          {model.contextFacts.length === 0 ? (
            <EmptyCopy>
              No report has published assignment or Git context.
            </EmptyCopy>
          ) : (
            <dl className="space-y-2">
              {model.contextFacts.map((fact) => (
                <div key={fact.id}>
                  <dt className="text-2xs font-medium text-muted-foreground">
                    {fact.label} · {fact.authorLabel}
                  </dt>
                  <dd>
                    <code className="break-all text-xs">{fact.value}</code>
                  </dd>
                  <SignedSource eventId={fact.sourceEventId} />
                </div>
              ))}
            </dl>
          )}
          <Usage usage={model.usage} />
        </InspectorSection>

        <InspectorSection
          title="Reports"
          truncations={truncationsFor(model, "reports")}
        >
          {model.reports.length === 0 ? (
            <EmptyCopy>No structured seat reports published.</EmptyCopy>
          ) : (
            <ul className="space-y-2">
              {model.reports.map((report) => (
                <li key={report.sourceEventId}>
                  <p className="text-xs">{report.summary}</p>
                  <p className="mt-0.5 text-2xs text-muted-foreground">
                    Reported by {report.authorLabel}
                  </p>
                  <SignedSource eventId={report.sourceEventId} />
                </li>
              ))}
            </ul>
          )}
        </InspectorSection>

        <InspectorSection
          title="Integrity"
          truncations={truncationsFor(model, "integrity")}
        >
          <Integrity model={model} />
        </InspectorSection>
      </div>
    </aside>
  );
}

function InspectorSection({
  children,
  title,
  truncations = [],
}: {
  children: React.ReactNode;
  title: string;
  truncations?: readonly CodingSessionMissionInspectorModel["truncations"][number][];
}) {
  return (
    <section className="border-b border-border/50 py-4 last:border-b-0">
      <h3 className="mb-2 text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
        {title}
      </h3>
      {children}
      {truncations.map((truncation) => (
        <p
          className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
          key={truncation.id}
          role="status"
        >
          {truncation.notice}
        </p>
      ))}
    </section>
  );
}

function truncationsFor(
  model: CodingSessionMissionInspectorModel,
  section: CodingSessionMissionInspectorSection,
) {
  return model.truncations.filter((item) => item.section === section);
}

function EmptyCopy({ children }: { children: React.ReactNode }) {
  return <p className="text-xs text-muted-foreground">{children}</p>;
}

function Goal({ goal }: { goal: CodingSessionMissionGoalModel }) {
  if (goal.kind === "absent") {
    return <EmptyCopy>No accepted mission goal published.</EmptyCopy>;
  }
  if (goal.kind === "conflict") {
    return (
      <div>
        <p className="text-xs text-amber-700 dark:text-amber-300">
          Goal unavailable — conflicting signed records.
        </p>
        {goal.eventIds.map((eventId) => (
          <SignedSource eventId={eventId} key={eventId} />
        ))}
      </div>
    );
  }
  return (
    <div>
      <p className="text-sm">{goal.text}</p>
      <p className="mt-1 text-2xs text-muted-foreground">
        Published by {goal.authorLabel}
      </p>
      <SignedSource eventId={goal.sourceEventId} />
    </div>
  );
}

function AcceptedPlan({
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
      {plan.authorLabel ? (
        <p className="text-2xs text-muted-foreground">
          Accepted from {plan.authorLabel}
        </p>
      ) : null}
      {plan.steps.length > 0 ? (
        <PlanSteps ariaLabel="Accepted mission plan" steps={plan.steps} />
      ) : (
        <EmptyCopy>{plan.label}</EmptyCopy>
      )}
      {plan.sourceEventIds[0] ? (
        <SignedSource eventId={plan.sourceEventIds[0]} />
      ) : null}
    </div>
  );
}

function PlanSteps({
  ariaLabel,
  steps,
}: {
  ariaLabel: string;
  steps: readonly CodingSessionMissionPlanStep[];
}) {
  return (
    <ol aria-label={ariaLabel} className="mt-2 space-y-1.5">
      {steps.map((step, index) => (
        <li className="flex gap-2 text-xs" key={step.id}>
          <PlanStepStatus status={step.status} />
          <span className="min-w-0 flex-1">
            <span className="mr-1 text-muted-foreground tabular-nums">
              {index + 1}.
            </span>
            {step.text}
          </span>
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

function Changes({ model }: { model: CodingSessionMissionInspectorModel }) {
  const changes = model.changes;
  if (changes.state === "none") {
    return <EmptyCopy>No signed edit activity observed.</EmptyCopy>;
  }
  if (changes.state === "unnamed") {
    return (
      <p className="text-xs">
        {changes.unreportedEditCount}{" "}
        {changes.unreportedEditCount === 1 ? "edit" : "edits"} observed; file
        names were not reported.
      </p>
    );
  }
  return (
    <div>
      <p className="text-xs tabular-nums">
        {changes.namedEditCount} named{" "}
        {changes.namedEditCount === 1 ? "edit" : "edits"}
        {changes.additions !== null && changes.deletions !== null
          ? ` · +${changes.additions} −${changes.deletions}`
          : " · line totals not fully reported"}
      </p>
      {changes.unreportedEditCount > 0 ? (
        <p className="mt-1 text-xs text-amber-700 dark:text-amber-300">
          Plus {changes.unreportedEditCount}{" "}
          {changes.unreportedEditCount === 1 ? "edit" : "edits"} with no
          reported file name.
        </p>
      ) : null}
    </div>
  );
}

function TestIcon({ outcome }: { outcome: "passed" | "failed" | "not-run" }) {
  const Icon = outcome === "passed" ? Check : outcome === "failed" ? X : Minus;
  return (
    <Icon
      aria-label={outcome}
      className={cn(
        "mt-0.5 size-3.5 shrink-0",
        outcome === "passed" && "text-emerald-600 dark:text-emerald-400",
        outcome === "failed" && "text-destructive",
        outcome === "not-run" && "text-muted-foreground",
      )}
    />
  );
}

function MissionState({ state }: { state: CodingSessionMissionStateInput }) {
  const presentation = missionStatePresentation(state);
  return (
    <div>
      <div className="flex items-center gap-2">
        <span
          aria-hidden
          className={cn("size-2 rounded-full", presentation.dot)}
        />
        <p className="text-sm font-medium">{presentation.label}</p>
      </div>
      {presentation.detail ? (
        <p className="mt-1.5 text-xs text-muted-foreground">
          {presentation.detail}
        </p>
      ) : null}
      {state.kind !== "unknown" && state.kind !== "conflict" ? (
        <SignedSource eventId={state.sourceEventId} />
      ) : null}
      {state.kind === "conflict"
        ? state.eventIds.map((eventId) => (
            <SignedSource eventId={eventId} key={eventId} />
          ))
        : null}
    </div>
  );
}

function missionStatePresentation(state: CodingSessionMissionStateInput): {
  label: string;
  detail: string | null;
  dot: string;
} {
  switch (state.kind) {
    case "running":
      return {
        label: "Running",
        detail: state.detail,
        dot: "bg-emerald-500",
      };
    case "acknowledgement-required":
      return {
        label: "Acknowledgement required",
        detail: state.heldOn
          ? `${state.requiredAction} · held on ${state.heldOn}`
          : state.requiredAction,
        dot: "bg-amber-500",
      };
    case "waiting-on-person":
      return {
        label: "Waiting on a person",
        detail: state.heldOn
          ? `${state.requiredAction} · held on ${state.heldOn}`
          : state.requiredAction,
        dot: "bg-amber-500",
      };
    case "stalled":
      return { label: "Stalled", detail: state.detail, dot: "bg-amber-500" };
    case "blocked":
      return {
        label: "Blocked",
        detail: [
          state.summary,
          ...state.blockers,
          `Required: ${state.requiredAction}`,
        ].join(" · "),
        dot: "bg-amber-500",
      };
    case "completed":
      return {
        label: "Completed",
        detail: [
          state.summary,
          state.landedShas.length > 0
            ? `Landed ${state.landedShas.join(", ")}`
            : null,
          state.followUps.length > 0
            ? `Follow-ups: ${state.followUps.join(" · ")}`
            : null,
        ]
          .filter(Boolean)
          .join(" · "),
        dot: "bg-emerald-500",
      };
    case "conflict":
      return {
        label: "Conflicting mission state",
        detail: "Signed terminal records disagree; no terminal state is shown.",
        dot: "bg-amber-500",
      };
    case "unknown":
      return {
        label: "Mission state unknown",
        detail: state.detail,
        dot: "bg-muted-foreground/50",
      };
  }
}

function Usage({ usage }: { usage: CodingSessionMissionUsageInput | null }) {
  if (!usage) {
    return (
      <div className="mt-3 border-t border-border/50 pt-3">
        <p className="text-2xs font-medium text-muted-foreground">Usage</p>
        <EmptyCopy>Terminal usage not reported.</EmptyCopy>
      </div>
    );
  }
  const fields = [
    ["Input", formatCount(usage.inputTokens)],
    ["Output", formatCount(usage.outputTokens)],
    ["Total", formatCount(usage.totalTokens)],
    ["Tools", formatCount(usage.toolCalls)],
    ["Cost", usage.costUsd === null ? null : `$${usage.costUsd.toFixed(2)}`],
  ].filter((field): field is [string, string] => field[1] !== null);
  return (
    <div className="mt-3 border-t border-border/50 pt-3">
      <p className="text-2xs font-medium text-muted-foreground">Usage</p>
      <dl className="mt-1 grid grid-cols-2 gap-x-3 gap-y-1">
        {fields.map(([label, value]) => (
          <div
            className="flex items-baseline justify-between gap-2"
            key={label}
          >
            <dt className="text-2xs text-muted-foreground">{label}</dt>
            <dd className="text-xs tabular-nums">{value}</dd>
          </div>
        ))}
      </dl>
      <SignedSource eventId={usage.sourceEventId} />
    </div>
  );
}

function Integrity({ model }: { model: CodingSessionMissionInspectorModel }) {
  const { integrity } = model;
  const clean =
    integrity.rejectedEventCount === 0 &&
    !integrity.rejectionsTruncated &&
    integrity.rejectedReasons.length === 0 &&
    integrity.conflicts.length === 0;
  if (clean) {
    return (
      <EmptyCopy>No rejected or conflicting transaction records.</EmptyCopy>
    );
  }
  return (
    <div className="space-y-3">
      {integrity.rejectedEventCount === null ||
      integrity.rejectedEventCount > 0 ||
      integrity.rejectedReasons.length > 0 ? (
        <div className="rounded-lg border border-amber-500/45 bg-amber-500/10 p-2.5">
          <p className="flex items-center gap-2 text-xs font-medium text-amber-700 dark:text-amber-300">
            <OctagonAlert aria-hidden className="size-3.5" />
            {integrity.rejectedEventCount === null
              ? "Rejected event total unavailable after the safety bound"
              : `${integrity.rejectedEventCount} rejected ${integrity.rejectedEventCount === 1 ? "event" : "events"}`}
          </p>
          {integrity.rejectionsTruncated ? (
            <p className="mt-1 text-2xs text-muted-foreground">
              Showing {integrity.rejectedReasons.length} rejected events;
              additional unique count unavailable after the safety bound.
            </p>
          ) : null}
          {integrity.rejectedReasons.length > 0 ? (
            <DisclosureList items={integrity.rejectedReasons} />
          ) : (
            <p className="mt-1 text-2xs text-muted-foreground">
              The trusted decoder reported no bounded reason detail.
            </p>
          )}
        </div>
      ) : null}
      {integrity.conflicts.length > 0 ? (
        <div className="rounded-lg border border-amber-500/45 bg-amber-500/10 p-2.5">
          <p className="flex items-center gap-2 text-xs font-medium text-amber-700 dark:text-amber-300">
            <TriangleAlert aria-hidden className="size-3.5" />
            Conflicting signed records
          </p>
          <DisclosureList items={integrity.conflicts} />
        </div>
      ) : null}
    </div>
  );
}

function DisclosureList({
  items,
}: {
  items: readonly CodingSessionMissionDisclosureInput[];
}) {
  return (
    <ul className="mt-2 space-y-2">
      {items.map((item) => (
        <li
          className="text-xs"
          key={`${item.code}:${item.summary}:${item.eventIds.join(":")}`}
        >
          <p>
            <code className="text-2xs">{item.code}</code> · {item.summary}
          </p>
          {item.eventIds.map((eventId) => (
            <SignedSource eventId={eventId} key={eventId} />
          ))}
        </li>
      ))}
    </ul>
  );
}

function SignedSource({ eventId }: { eventId: string }) {
  return (
    <details className="mt-1 text-2xs text-muted-foreground">
      <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        Signed source
      </summary>
      <code className="mt-1 block break-all">{eventId}</code>
    </details>
  );
}

function formatCount(value: number | null): string | null {
  return value === null ? null : value.toLocaleString();
}
