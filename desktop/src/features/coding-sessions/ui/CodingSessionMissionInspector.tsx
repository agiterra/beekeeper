import type { ReactNode } from "react";
import { Flag } from "lucide-react";

import {
  codingSessionSeatAuthorityCopy,
  COMPLETION_NOT_VERIFIED_CODE,
  type CodingSessionSeatAuthority,
  type CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type {
  CodingSessionMissionGoalModel,
  CodingSessionMissionInspectorModel,
  CodingSessionMissionInspectorSection,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  CODING_SESSION_GOAL_UNRESOLVED,
  codingSessionGoalErrorSentence,
  type CodingSessionGoalReader,
} from "@/features/coding-sessions/lib/codingSessionGoal";
import {
  CODING_SESSION_NO_OPEN_HOLDS,
  codingSessionOpenHoldStatusLine,
  codingSessionOpenHoldWaitLine,
  codingSessionOpenHoldsTruncation,
  type CodingSessionMissionOpenHold,
  type CodingSessionMissionOpenHolds,
} from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import { useCodingSessionOpenHolds } from "./CodingSessionUmbrellaWorkspaceModel";
import { codingSessionParticipantAccent } from "@/features/coding-sessions/lib/codingSessionParticipantAccent";
import { cn } from "@/shared/lib/cn";

import { AcceptedPlan, PlanSteps } from "./CodingSessionMissionPlanSection";
import {
  EmptyCopy,
  SignedSource,
} from "./CodingSessionMissionInspectorPrimitives";
import { codingSessionUnseatedReportDetail } from "./CodingSessionMissionDeliveryBadge";
import { CodingSessionMissionDeliveryList } from "./CodingSessionMissionDeliveryList";
import {
  codingSessionGoalRejectionSentence,
  codingSessionPrivateContextLine,
  isCodingSessionPrivateContextMarker,
} from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import {
  CODING_SESSION_OBSERVATION_EMPTY,
  type CodingSessionObservationGateView,
} from "@/features/coding-sessions/lib/codingSessionObservationView";
import {
  CodingSessionGateRows,
  CodingSessionTestIcon as TestIcon,
} from "./CodingSessionGateRows";
import { CodingSessionMissionDecisionQueue } from "./CodingSessionMissionDecisionQueue";
import { Integrity } from "./CodingSessionMissionInspectorIntegrity";
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
  /**
   * This session's folded kind-44246 gate rows, observed first.
   *
   * `undefined` is not `[]`: it means no observation fold reached this view,
   * which is unknown rather than "no gate ran". The card says so.
   */
  gateRows?: readonly CodingSessionObservationGateView[];
  /** The founder's goal edit control, rendered under Current goal. */
  goalEditor?: ReactNode;
  /**
   * What the goal reader can currently say (A1).
   *
   * Defaults to `resolved` so any caller that has not adopted it renders
   * exactly today's sentences; the surfaces that *have* one hand it over.
   */
  goalReader?: CodingSessionGoalReader;
  /**
   * Open assignments and reports, from `deriveCodingSessionMissionOpenHolds`.
   *
   * A5: `Assignment open 3m 3s · no report yet` existed in exactly one place —
   * the Route rail's legend — where it named the holder and never the waiter,
   * carried no clock, and vanished below a ~1,590 px window. It belongs on the
   * one roster that is complete and survives every width.
   *
   * Each hold renders under **the seat that waits**, not the party that holds
   * (REVIEW-L4 F2). The waiter is the one with a seat in the ordinary case —
   * a seat files a report and the founder owes the verdict — so anchoring on
   * the holder put the commonest hold in the mission nowhere.
   */
  openHolds?: CodingSessionMissionOpenHolds;
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
  gateRows = [],
  goalEditor,
  goalReader = { kind: "resolved" },
  openHolds,
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
  // The prop wins when a caller (or a test) supplies one; otherwise the
  // workspace's provider does. See `CodingSessionOpenHoldsContext` for why the
  // holds travel as context rather than through the surface hook.
  const contextOpenHolds = useCodingSessionOpenHolds();
  const holds = openHolds ?? contextOpenHolds;
  const seatLabels = new Set(model.participants.map((one) => one.label));
  const unseatedHolds = holds.holds.filter(
    (hold) => hold.waiterLabel === null || !seatLabels.has(hold.waiterLabel),
  );
  const holdsTruncation = codingSessionOpenHoldsTruncation(holds.omitted);
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
          <Goal goal={model.goal} reader={goalReader} />
          {/* A1's compounding half: `Set goal` publishes a *second* 44227, and
              the UI's answer to a record it failed to read must never be to
              make the record worse. The control appears only when the reader
              settled and the selection bound a goal to this mission. */}
          {goalEditor && codingSessionGoalIsEditable(goalReader, model.goal) ? (
            <div className="mt-2">{goalEditor}</div>
          ) : null}
        </InspectorSection>

        <InspectorSection
          title="Mission state"
          truncations={truncationsFor(model, "mission-state")}
        >
          <MissionStateAndLiveness model={model} />
          <CodingSessionMissionStatePanel
            completionRefusedNoVerifier={model.integrity.rejectedReasons.some(
              (reason) => reason.code === COMPLETION_NOT_VERIFIED_CODE,
            )}
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
                    <OpenHolds
                      holds={holds.holds.filter(
                        (hold) => hold.waiterLabel === participant.label,
                      )}
                    />
                  </li>
                );
              })}
            </ul>
          )}
          {/* A5: a hold whose waiter resolves to no seat on this roster has
              nowhere on it to live. It renders here under its own heading —
              never as an unlabelled third line under whichever seat happened
              to be last, which is how it reads as that seat's (F2). */}
          {unseatedHolds.length > 0 ? (
            <div className="mt-3">
              <h4 className="text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
                Open, held off the roster
              </h4>
              <OpenHolds holds={unseatedHolds} />
            </div>
          ) : null}
          {holdsTruncation === null ? null : (
            <p
              className="mt-2 text-2xs text-amber-700 dark:text-amber-300"
              data-testid="mission-open-holds-truncated"
              role="status"
            >
              {holdsTruncation}
            </p>
          )}
          {holds.holds.length === 0 && model.participants.length > 0 ? (
            <p className="mt-2 text-2xs text-muted-foreground">
              {CODING_SESSION_NO_OPEN_HOLDS}
            </p>
          ) : null}
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
          {/* A3, closed. This card used to say "Nothing on the wire reports
              tests" — true when written, false since kind 44246 landed. It now
              renders the session's own signed **gate rows**, the same component
              the Audit tab uses over the same fold, so the two surfaces cannot
              disagree about whether a gate passed (live-run finding 26). The
              44244 `report.tests[]` entries it also holds are counted beside
              them, never merged into them: a claim inside a report and a signed
              gate row are different facts. */}
          <CodingSessionGateRows
            emptyCopy={
              <>
                <span className="block">
                  {CODING_SESSION_OBSERVATION_EMPTY.gates}
                </span>
                <span className="block text-2xs">
                  {model.tests.length === 0
                    ? "Kind 44246 carries a gate’s name, its outcome and the command that produced it. None has been published for this session."
                    : `Kind 44246 carries a gate’s name, its outcome and the command that produced it. None has been published for this session; ${model.tests.length} test result${model.tests.length === 1 ? " is" : "s are"} claimed inside signed reports below.`}
                </span>
              </>
            }
            rows={gateRows}
            testId="coding-session-inspector-gates"
          />
          {model.tests.length > 0 ? (
            <ul
              aria-label="Structured test results"
              className="mt-2 space-y-2"
              data-testid="coding-session-inspector-report-tests"
            >
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
                        {result.outcome} · claimed in a report by{" "}
                        {result.authorLabel}
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
          ) : null}
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

/**
 * May the founder publish a goal from here?
 *
 * Only over a settled reader that bound a record — or found none. A reader
 * still in flight, one that errored, and a record this surface refused on
 * identity are all states where publishing a second 44227 would bury the
 * first.
 *
 * The `rejected` member arrives with the goal *selection* (lane L2). With L2
 * merged it narrows properly; the structural read this replaced existed only
 * to keep the gate complete while the two halves of A1 sat in two lanes.
 */
function codingSessionGoalIsEditable(
  reader: CodingSessionGoalReader,
  goal: CodingSessionMissionGoalModel,
): boolean {
  return reader.kind === "resolved" && goal.kind !== "rejected";
}

function Goal({
  goal,
  reader,
}: {
  goal: CodingSessionMissionGoalModel;
  reader: CodingSessionGoalReader;
}) {
  // The reader speaks before the record does: with nothing read yet there is
  // no honest sentence about what the wire holds.
  if (reader.kind === "unresolved") {
    return (
      <EmptyCopy>
        <span data-testid="mission-goal-unresolved">
          {CODING_SESSION_GOAL_UNRESOLVED}
        </span>
      </EmptyCopy>
    );
  }
  if (reader.kind === "errored") {
    return (
      <p
        className="text-xs text-amber-700 dark:text-amber-300"
        data-testid="mission-goal-errored"
      >
        {codingSessionGoalErrorSentence(reader.message)}
      </p>
    );
  }
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

/**
 * The open-hold lines for one seat, or for the holds no seat answers.
 *
 * Two lines, both from `codingSessionMissionOpenHolds` — this component writes
 * no prose of its own, which is what keeps the Inspector's sentence and the
 * rail head's the same sentence rather than two that agree today.
 */
function OpenHolds({
  holds,
}: {
  holds: readonly CodingSessionMissionOpenHold[];
}) {
  if (holds.length === 0) return null;
  return (
    <>
      {holds.map((hold) => {
        const status = codingSessionOpenHoldStatusLine(hold);
        return (
          <p
            className="mt-0.5 pl-2.5 text-2xs text-muted-foreground"
            data-testid="mission-open-hold"
            key={`${hold.sourceEventId ?? "undated"}:${hold.holding}`}
          >
            <span className="block">{codingSessionOpenHoldWaitLine(hold)}</span>
            {status === null ? null : <span className="block">{status}</span>}
          </p>
        );
      })}
    </>
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
