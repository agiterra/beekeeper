import * as React from "react";
import { Bot, Users } from "lucide-react";

import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionModelSummary,
  formatCodingSessionRuntimeLabel,
} from "@/features/coding-sessions/lib/codingSessionLabels";
import {
  listCodingSessionUmbrellaParticipants,
  type CodingSessionActorNameResolver,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";

type ExecutionRailTab = "overview" | `execution:${string}`;

/**
 * The Agents surface: who is participating in this session, per signed
 * execution records. Content only — panel chrome (host tab strip, close
 * control, width, sheet behavior) belongs to the surface host. The internal
 * tabs here navigate *within* the surface (overview vs one execution).
 */
export function CodingSessionExecutionRail({
  actorNames,
  umbrella,
}: {
  /**
   * Names for the umbrella's seated actors, resolved by the surface that
   * owns a query client. Absent, a seat labels itself by its role alone —
   * never by a name it had to invent.
   */
  actorNames?: CodingSessionActorNameResolver;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const participants = React.useMemo(
    () =>
      listCodingSessionUmbrellaParticipants(umbrella, actorNames).filter(
        (participant) => participant.kind === "execution",
      ),
    [actorNames, umbrella],
  );
  const [activeTab, setActiveTab] =
    React.useState<ExecutionRailTab>("overview");
  const panelId = React.useId();
  const tabRefs = React.useRef<Array<HTMLButtonElement | null>>([]);
  const tabs = React.useMemo<
    Array<{
      id: ExecutionRailTab;
      label: string;
      key: string;
      hint: string | null;
    }>
  >(
    () => [
      { id: "overview", label: "All agents", key: "overview", hint: null },
      ...participants.map((participant, index) => ({
        id: `execution:${participant.executionKey}` as const,
        label: disambiguatedExecutionLabel(participants, index),
        key: participant.executionKey,
        // A seat's tab reads "<agent> · <role>"; what it is running is the
        // hover, so the tab strip stays about who rather than about what.
        hint: executionRuntimeHint(participant.execution),
      })),
    ],
    [participants],
  );
  React.useEffect(() => {
    if (!tabs.some((tab) => tab.id === activeTab)) setActiveTab("overview");
  }, [activeTab, tabs]);
  const selectedExecution =
    activeTab === "overview"
      ? null
      : (umbrella.executions.find(
          (execution) =>
            execution.executionKey === activeTab.slice("execution:".length),
        ) ?? null);

  return (
    <div
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid="coding-session-execution-rail"
    >
      <div className="flex h-12 shrink-0 items-center border-b border-border/60 px-3">
        <div
          aria-label="Execution views"
          className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
          onKeyDown={(event) => {
            const current = tabs.findIndex((tab) => tab.id === activeTab);
            let next = current;
            if (event.key === "ArrowRight") next = (current + 1) % tabs.length;
            else if (event.key === "ArrowLeft")
              next = (current - 1 + tabs.length) % tabs.length;
            else if (event.key === "Home") next = 0;
            else if (event.key === "End") next = tabs.length - 1;
            else return;
            event.preventDefault();
            const tab = tabs[next];
            if (tab) {
              setActiveTab(tab.id);
              tabRefs.current[next]?.focus();
            }
          }}
          role="tablist"
        >
          {tabs.map((tab, index) => (
            <ExecutionTab
              active={activeTab === tab.id}
              controlsId={`${panelId}-${tab.key}`}
              hint={tab.hint}
              key={tab.key}
              label={tab.label}
              onClick={() => setActiveTab(tab.id)}
              ref={(node) => {
                tabRefs.current[index] = node;
              }}
            />
          ))}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-3">
        {selectedExecution ? (
          <ExecutionDetail
            actorNames={actorNames}
            execution={selectedExecution}
            panelId={`${panelId}-${selectedExecution.executionKey}`}
          />
        ) : (
          <Overview
            actorNames={actorNames}
            executions={umbrella.executions}
            panelId={`${panelId}-overview`}
          />
        )}
      </div>

      <ExecutionRailFooter executions={umbrella.executions} />
    </div>
  );
}

function ExecutionTab({
  active,
  controlsId,
  hint = null,
  label,
  onClick,
  ref,
}: {
  active: boolean;
  controlsId: string;
  /** Secondary detail (runtime · model) shown on hover, when there is one. */
  hint?: string | null;
  label: string;
  onClick: () => void;
  ref: React.Ref<HTMLButtonElement>;
}) {
  return (
    <button
      aria-controls={controlsId}
      aria-selected={active}
      className={cn(
        "shrink-0 rounded-md px-2.5 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground",
        active && "bg-muted text-foreground",
      )}
      onClick={onClick}
      ref={ref}
      role="tab"
      tabIndex={active ? 0 : -1}
      title={hint ?? undefined}
      type="button"
    >
      {label}
    </button>
  );
}

function Overview({
  actorNames,
  executions,
  panelId,
}: {
  actorNames?: CodingSessionActorNameResolver;
  executions: CodingSessionExecution[];
  panelId: string;
}) {
  return (
    <section aria-label="Execution overview" id={panelId} role="tabpanel">
      <p className="pb-2 text-3xs font-semibold tracking-[0.12em] text-muted-foreground uppercase">
        Participants
      </p>
      <div className="divide-y divide-border/60">
        {executions.map((execution) => (
          <ExecutionCard
            actorNames={actorNames}
            execution={execution}
            key={execution.executionKey}
          />
        ))}
      </div>
    </section>
  );
}

function ExecutionDetail({
  actorNames,
  execution,
  panelId,
}: {
  actorNames?: CodingSessionActorNameResolver;
  execution: CodingSessionExecution;
  panelId: string;
}) {
  const record = execution.activeGeneration;
  const latest = latestActivity(record.transcript);
  return (
    <section
      aria-label={`${executionName(execution, actorNames)} execution`}
      id={panelId}
      role="tabpanel"
    >
      <ExecutionCard actorNames={actorNames} execution={execution} />
      <div className="mt-4 border-t border-border/60 pt-4">
        <p className="text-3xs font-semibold tracking-[0.12em] text-muted-foreground uppercase">
          Latest activity
        </p>
        <p className="mt-2 text-xs leading-5 text-foreground/90">
          {latest ?? "No attributable activity yet."}
        </p>
      </div>
      <dl className="mt-4 grid gap-3 border-t border-border/60 pt-4 text-xs">
        <ExecutionFact label="Generation" value={record.label} />
        <ExecutionFact label="Runtime" value={runtimeLabel(execution)} />
        {record.model ? (
          <ExecutionFact
            label="Model"
            value={formatCodingSessionModelSummary(record.model)}
          />
        ) : null}
        <ExecutionFact
          label="Last activity"
          value={formatLastActivity(record.lastEventAt)}
        />
      </dl>
      <p className="mt-4 px-1 text-2xs leading-4 text-muted-foreground">
        Spawned agents will appear here when the session publishes signed spawn
        relationships.
      </p>
    </section>
  );
}

function ExecutionCard({
  actorNames,
  execution,
}: {
  actorNames?: CodingSessionActorNameResolver;
  execution: CodingSessionExecution;
}) {
  const record = execution.activeGeneration;
  const status = executionStatus(record.status);
  return (
    <article className="py-3" data-testid="coding-session-execution-card">
      <div className="flex items-start gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
          <Bot className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <p className="min-w-0 flex-1 truncate text-xs font-semibold">
              {executionName(execution, actorNames)}
            </p>
            <span className={cn("shrink-0 text-2xs font-medium", status.tone)}>
              {status.label}
            </span>
          </div>
          <p className="mt-0.5 truncate text-2xs text-muted-foreground">
            {[
              runtimeLabel(execution),
              record.model
                ? formatCodingSessionModelSummary(record.model)
                : null,
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
          {record.transcript.length > 0 ? (
            <p className="mt-2 truncate text-2xs text-muted-foreground">
              {latestActivity(record.transcript)}
            </p>
          ) : null}
        </div>
      </div>
    </article>
  );
}

function ExecutionRailFooter({
  executions,
}: {
  executions: CodingSessionExecution[];
}) {
  const working = executions.filter((execution) =>
    ["running", "starting"].includes(execution.activeGeneration.status),
  ).length;
  const waiting = executions.filter(
    (execution) => execution.activeGeneration.status === "waiting_for_input",
  ).length;
  return (
    <div className="flex shrink-0 items-center gap-2 border-t border-border/60 px-4 py-3 text-2xs text-muted-foreground">
      <Users className="size-3.5" />
      <span>{working > 0 ? `${working} working` : "All idle"}</span>
      {waiting > 0 ? <span>· {waiting} waiting</span> : null}
    </div>
  );
}

function ExecutionFact({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-2xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 wrap-break-word">{value}</dd>
    </div>
  );
}

/**
 * What this execution is called on its card.
 *
 * A seated execution is the agent and its role; an unseated one is its
 * runtime, exactly as before. Either way the runtime and model stay visible
 * on the line beneath — demoted, never dropped.
 */
function executionName(
  execution: CodingSessionExecution,
  actorNames?: CodingSessionActorNameResolver,
): string {
  const record = execution.activeGeneration;
  if (record.agentRef && record.role) {
    return formatCodingSessionExecutionLabel({
      agentRef: record.agentRef,
      role: record.role,
      agentDisplayName: actorNames?.(record.agentRef) ?? null,
      runtime: null,
      model: null,
    }).primary;
  }
  return formatCodingSessionRuntimeLabel(
    record.runtime ?? record.provider ?? "Execution",
  );
}

function runtimeLabel(execution: CodingSessionExecution): string {
  const record = execution.activeGeneration;
  return formatCodingSessionRuntimeLabel(
    record.runtime ?? record.provider ?? "Unknown runtime",
  );
}

/** Runtime and model for a seated execution, or null when it leads with them. */
function executionRuntimeHint(
  execution: CodingSessionExecution,
): string | null {
  const record = execution.activeGeneration;
  return formatCodingSessionExecutionLabel({
    agentRef: record.agentRef,
    role: record.role,
    runtime: record.runtime ?? record.commandTarget?.driver ?? null,
    model: record.model,
  }).secondary;
}

function shortExecutionLabel(label: string): string {
  return label.split(" · ")[0]?.trim() || label;
}

function disambiguatedExecutionLabel(
  participants: ReturnType<typeof listCodingSessionUmbrellaParticipants>,
  index: number,
): string {
  const participant = participants[index];
  if (participant?.kind !== "execution") return "Agent";
  const base = shortExecutionLabel(participant.label);
  const matches = participants.filter(
    (candidate) =>
      candidate.kind === "execution" &&
      shortExecutionLabel(candidate.label) === base,
  );
  if (matches.length < 2) return base;
  return `${base} ${matches.indexOf(participant) + 1}`;
}

/** Map a signed execution status to its rail label. Exported for test. */
export function executionStatus(status: string): {
  label: string;
  tone: string;
} {
  if (status === "running" || status === "starting") {
    return { label: "Working", tone: "text-blue-500" };
  }
  if (status === "waiting_for_input") {
    return { label: "Waiting", tone: "text-amber-500" };
  }
  // `idle` is the provider's most common resting status — omitting it here
  // rendered every waiting execution as "Status unknown" while the header
  // beside it read Idle.
  if (["idle", "completed", "stopped", "interrupted"].includes(status)) {
    return { label: "Idle", tone: "text-muted-foreground" };
  }
  if (status === "failed" || status === "disconnected") {
    return { label: "Needs attention", tone: "text-destructive" };
  }
  return { label: "Status unknown", tone: "text-muted-foreground" };
}

function latestActivity(
  transcript: CodingSessionExecution["activeGeneration"]["transcript"],
): string | null {
  const latest = [...transcript].reverse().find((item) => {
    if (item.type === "message") return item.role === "assistant";
    return item.type === "tool" || item.type === "plan";
  });
  if (!latest) return null;
  if (latest.type === "message") return latest.text.trim() || null;
  if (latest.type === "tool") {
    return latest.descriptor.preview
      ? `${latest.descriptor.label} · ${latest.descriptor.preview}`
      : latest.descriptor.label;
  }
  if (latest.type === "plan") return latest.title || "Updated the plan";
  return null;
}

function formatLastActivity(value: string): string {
  const date = new Date(value);
  return Number.isFinite(date.getTime())
    ? date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" })
    : value;
}
