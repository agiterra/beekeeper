import type { ReactNode } from "react";
import {
  Check,
  Circle,
  CircleDashed,
  CircleDot,
  Flag,
  Minus,
  OctagonAlert,
  TriangleAlert,
  X,
} from "lucide-react";

import {
  codingSessionSeatAuthorityCopy,
  type CodingSessionSeatAuthority,
  type CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type {
  CodingSessionMissionDisclosureInput,
  CodingSessionMissionGoalModel,
  CodingSessionMissionInspectorModel,
  CodingSessionMissionInspectorSection,
  CodingSessionMissionPlanStep,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import { cn } from "@/shared/lib/cn";
import { codingSessionUnseatedReportDetail } from "./CodingSessionMissionDeliveryBadge";
import { CodingSessionMissionDeliveryList } from "./CodingSessionMissionDeliveryList";
import {
  codingSessionGoalRejectionSentence,
  codingSessionPrivateContextLine,
  isCodingSessionPrivateContextMarker,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { CodingSessionMissionDecisionQueue } from "./CodingSessionMissionDecisionQueue";
import { CodingSessionMissionStatePanel } from "./CodingSessionMissionStatePanel";

export type CodingSessionMissionInspectorProps = {
  model: CodingSessionMissionInspectorModel;
  variant: "panel" | "drawer";
  focusedExecutionKey: string | null;
  loading?: boolean;
  errorMessage?: string | null;
  /**
   * Team-wake delivery evidence. `undefined` is not `[]`: it means no
   * projection was supplied, and while evidence is loading the section says
   * `Wake delivery unknown` rather than "none observed".
   */
  deliveries?: readonly CodingSessionTeamWakeDelivery[];
  /** Seat authority per execution, from the accepted 44228 chain. */
  seatAuthorities?: readonly CodingSessionSeatAuthority[];
  /** Report event ids the Rust fold listed under `unseatedReports`. */
  unseatedReportEventIds?: readonly string[];
  /** The founder's goal edit control, rendered under Current goal. */
  goalEditor?: ReactNode;
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
  deliveries,
  errorMessage = null,
  goalEditor,
  model,
  variant,
  focusedExecutionKey,
  loading = false,
  onFocusParticipant,
  onOpenFileTrace,
  onRefresh,
  seatAuthorities,
  unseatedReportEventIds,
}: CodingSessionMissionInspectorProps) {
  const authorityByExecution = new Map(
    (seatAuthorities ?? []).map((authority) => [
      authority.executionKey,
      authority,
    ]),
  );
  const unseatedReports = new Set(unseatedReportEventIds ?? []);
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
          title="Current goal"
          truncations={truncationsFor(model, "goal")}
        >
          <Goal goal={model.goal} />
          {goalEditor ? <div className="mt-2">{goalEditor}</div> : null}
        </InspectorSection>

        <InspectorSection
          title="Mission state"
          truncations={truncationsFor(model, "mission-state")}
        >
          <MissionStateAndLiveness model={model} />
          <CodingSessionMissionStatePanel
            state={model.missionState}
            waiting={model.waiting}
          />
          {model.missionState.kind !== "unknown" &&
          model.missionState.kind !== "conflict" ? (
            <SignedSource eventId={model.missionState.sourceEventId} />
          ) : null}
          {model.missionState.kind === "conflict"
            ? model.missionState.eventIds.map((eventId) => (
                <SignedSource eventId={eventId} key={eventId} />
              ))
            : null}
        </InspectorSection>

        {/* Item 105's first UI consumer. Beside Mission state rather than
            inside it: the state is what the mission *is*, and the queue is the
            list of rulings it is holding — two facts, two sections. */}
        <InspectorSection title="Decisions">
          <CodingSessionMissionDecisionQueue
            decisions={model.decisions}
            decisionsKnown={model.decisionsKnown}
            decisionsTruncated={model.decisionsTruncated}
            decisionsTruncatedNotice={model.decisionsTruncatedNotice}
          />
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
                const authority =
                  authorityByExecution.get(participant.executionKey) ??
                  (seatAuthorities === undefined && loading
                    ? unknownSeatAuthority(participant.executionKey)
                    : null);
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
                          {participant.disposition}
                        </span>
                      </span>
                    </button>
                    {authority ? (
                      <SeatAuthorityDetail authority={authority} />
                    ) : null}
                  </li>
                );
              })}
            </ul>
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
              {model.files.map((file) => {
                // Finding 24: a seat whose host withheld the paths reports the
                // vault's content-addressed marker in their place. It is a
                // receipt, not a path, and printing it as one told a reader
                // their files were called `[elided private context: 183 bytes,
                // sha256:…]`.
                const privateContext = isCodingSessionPrivateContextMarker(
                  file.path,
                );
                return (
                  <li className="min-w-0" key={file.path}>
                    {!privateContext ? (
                      <code className="block break-all text-xs text-foreground">
                        {file.path}
                      </code>
                    ) : (
                      <p
                        className="text-xs text-foreground"
                        data-testid="mission-file-private-context"
                      >
                        {codingSessionPrivateContextLine(file.editCount)}
                      </p>
                    )}
                    <p className="mt-0.5 text-2xs text-muted-foreground">
                      {file.observed ? "Observed file edit" : null}
                      {file.observed && file.reportedBy.length > 0
                        ? " · "
                        : null}
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
                );
              })}
            </ul>
          )}
        </InspectorSection>

        <InspectorSection
          title="Structured tests"
          truncations={truncationsFor(model, "tests")}
        >
          {model.tests.length === 0 ? (
            // SURFACES D5: the refusal names what it is refusing. No wire kind
            // reports tests today, and a seat writing "3/3 passing" in its turn
            // is prose — this panel will not count it.
            <EmptyCopy>
              <span className="block">No test report yet</span>
              <span className="block">
                Nothing on the wire reports tests. A seat's written report is
                prose in its turn — Beekeeper will not count it.
              </span>
            </EmptyCopy>
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
          title="Reports"
          truncations={truncationsFor(model, "reports")}
        >
          {model.reports.length === 0 ? (
            <EmptyCopy>No structured seat reports published.</EmptyCopy>
          ) : (
            <ul className="space-y-2">
              {model.reports.map((report) => (
                <li
                  data-testid="mission-report-row"
                  data-unseated={
                    unseatedReports.has(report.sourceEventId)
                      ? "true"
                      : undefined
                  }
                  key={report.sourceEventId}
                >
                  <p className="text-xs">{report.summary}</p>
                  <p className="mt-0.5 text-2xs text-muted-foreground">
                    Reported by {report.authorLabel}
                    {unseatedReports.has(report.sourceEventId) ? (
                      <span
                        className="ml-1 inline-flex items-center gap-1 align-middle text-amber-700 dark:text-amber-300"
                        title={codingSessionUnseatedReportDetail()}
                      >
                        <Flag aria-hidden className="size-3 shrink-0" />
                        {"· unseated"}
                      </span>
                    ) : null}
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
          <div className="space-y-3">
            <div data-testid="mission-integrity-delivery">
              <h4 className="mb-1.5 text-2xs font-semibold text-muted-foreground">
                Delivery
              </h4>
              <CodingSessionMissionDeliveryList
                deliveries={deliveries}
                loading={loading}
              />
            </div>
            <div data-testid="mission-integrity-records">
              <h4 className="mb-1.5 text-2xs font-semibold text-muted-foreground">
                Rejected and conflicting records
              </h4>
              <Integrity model={model} />
            </div>
          </div>
        </InspectorSection>
      </div>
    </aside>
  );
}

/** Signed terminal state → the one word this line leads with. */
const MISSION_STATE_WORD: Readonly<
  Record<CodingSessionMissionInspectorModel["missionState"]["kind"], string>
> = {
  completed: "Completed",
  blocked: "Blocked",
  "waiting-on-person": "Waiting on a person",
  "acknowledgement-required": "Acknowledgement required",
  stalled: "Stalled",
  running: "Running",
  conflict: "State conflict",
  unknown: "State unknown",
};

/**
 * The record and the room, on one line: `Blocked (signed) · 2 seats live`.
 *
 * On the 2026-09-01 run a lead used `mission.blocked` four times as a note —
 * there is no note verb — and the rail read Blocked, in red, while two seats
 * went on working for another twenty minutes. Both halves were true; showing
 * only the first made the surface lie about the session.
 *
 * `(signed)` is a claim about provenance and appears only when a signed record
 * establishes the state; `unknown` carries no such record and says so. The
 * liveness clause is the roster's own W1 count, never prose parsed out of the
 * blocker body, and it never prints `0 seats live` — a count of nothing is
 * spelled out in words instead.
 */
function MissionStateAndLiveness({
  model,
}: {
  model: CodingSessionMissionInspectorModel;
}) {
  const state = model.missionState;
  const word = MISSION_STATE_WORD[state.kind];
  const provenance = state.kind === "unknown" ? "" : " (signed)";
  const live = model.participants.filter(
    (participant) => participant.status.kind === "working",
  ).length;
  const liveness =
    model.participants.length === 0
      ? "seat liveness not projected"
      : live === 0
        ? "no seat is working"
        : `${live} seat${live === 1 ? "" : "s"} live`;
  return (
    <p
      className="mb-1.5 text-xs font-medium"
      data-live-seats={model.participants.length === 0 ? "unknown" : live}
      data-mission-state={state.kind}
      data-testid="mission-state-and-liveness"
    >
      {word}
      {provenance} <span className="text-muted-foreground">· {liveness}</span>
    </p>
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
  // Critique A1: a record this surface refused says so, and says which
  // identity it could not bind — `No accepted mission goal published` over a
  // goal that is on the wire is a claim about the wire made from the outcome
  // of a local join.
  //
  // Live run 3's own miss was **not reproduced** through the real readers
  // (REVIEW-L2 F8): the published goal passes every one of them. This branch
  // removes the class; the remaining suspect is a reader that has not resolved
  // yet, whose sentence is L4's to add. Until it does, `absent` below still
  // covers two facts, and that is stated rather than hidden.
  if (goal.kind === "rejected") {
    return (
      <p
        className="text-xs text-amber-700 dark:text-amber-300"
        data-goal-state="rejected"
        data-testid="mission-goal-rejected"
        role="status"
      >
        {codingSessionGoalRejectionSentence(goal.disagreements)}
      </p>
    );
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

function PlanSteps({
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

/**
 * Seat authority detail, with the exact repair command when a seat was created
 * but never granted.
 *
 * `maySteer` masks this gap everywhere else: the hire host publishes
 * `grant-operator` for the actor, which satisfies the fold's `may_lead` check,
 * so an ungranted seat looks governed until a refutation or an `activeSeats`
 * listing disagrees. This is the persistent surface that says otherwise.
 */
function SeatAuthorityDetail({
  authority,
}: {
  authority: CodingSessionSeatAuthority;
}) {
  const copy = codingSessionSeatAuthorityCopy[authority.kind];
  return (
    <div
      className="mt-1 pl-2.5"
      data-kind={authority.kind}
      data-testid="mission-team-seat-authority"
    >
      <p
        className={cn(
          "flex items-center gap-1 text-2xs",
          authority.kind === "created-ungranted"
            ? "text-amber-700 dark:text-amber-300"
            : "text-muted-foreground",
        )}
      >
        {copy.badge ? <Flag aria-hidden className="size-3 shrink-0" /> : null}
        {authority.detail}
      </p>
      {authority.remedy ? (
        <code className="mt-1 block break-all text-2xs text-muted-foreground">
          {authority.remedy}
        </code>
      ) : null}
    </div>
  );
}

/** The honest placeholder while the authority projection has not arrived. */
function unknownSeatAuthority(
  executionKey: string,
): CodingSessionSeatAuthority {
  return {
    executionKey,
    actorPubkey: null,
    role: null,
    kind: "unknown",
    grantEventId: null,
    detail: codingSessionSeatAuthorityCopy.unknown.detail,
    remedy: null,
  };
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

function SignedSource({
  authorLabel,
  eventId,
}: {
  authorLabel?: string;
  eventId: string;
}) {
  return (
    <details className="mt-1 text-2xs text-muted-foreground">
      <summary className="w-fit cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
        Signed source
      </summary>
      <dl className="mt-1 space-y-1">
        {authorLabel ? (
          <div>
            <dt className="font-medium">Author</dt>
            <dd>
              <code className="block break-all">{authorLabel}</code>
            </dd>
          </div>
        ) : null}
        <div>
          <dt className="font-medium">Event</dt>
          <dd>
            <code className="block break-all">{eventId}</code>
          </dd>
        </div>
      </dl>
    </details>
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
