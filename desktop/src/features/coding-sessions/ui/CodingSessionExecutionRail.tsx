import * as React from "react";
import { Bot, Users } from "lucide-react";

import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionModelSummary,
  formatCodingSessionRuntimeLabel,
} from "@/features/coding-sessions/lib/codingSessionLabels";
import {
  codingSessionDispositionWord,
  listCodingSessionUmbrellaParticipants,
  type CodingSessionActorNameResolver,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { deriveCodingSessionExecutionStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { UNKNOWN_CODING_SESSION_REACHABILITY } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionSubagentPanel } from "@/features/coding-sessions/lib/codingSessionSubagents";
import { markdownToPlainText } from "@/features/projects/lib/projectsViewHelpers";
import { cn } from "@/shared/lib/cn";
import { CodingSessionSubagentsSection } from "./CodingSessionSubagentsPanel";

type ExecutionRailTab = "overview" | `execution:${string}`;

/**
 * The Agents surface: who is participating in this session, per signed
 * execution records. Content only — panel chrome (host tab strip, close
 * control, width, sheet behavior) belongs to the surface host. The internal
 * tabs here navigate *within* the surface (overview vs one execution).
 */
export function CodingSessionExecutionRail({
  actorNames,
  canSteer = false,
  machine,
  resolveReachability = UNKNOWN_CODING_SESSION_REACHABILITY,
  subagents = null,
  umbrella,
}: {
  /**
   * Names for the umbrella's seated actors, resolved by the surface that
   * owns a query client. Absent, a seat labels itself by its role alone —
   * never by a name it had to invent.
   */
  actorNames?: CodingSessionActorNameResolver;
  /**
   * Whether this viewer may prompt executions. It chooses which of W1's two
   * waiting strings a waiting seat reads, and nothing else.
   */
  canSteer?: boolean;
  /**
   * What each seat needs to say which machine runs it (SV-24). Absent, a
   * seat names its provider by key and never claims this computer.
   */
  machine?: CodingSessionExecutionMachineContext;
  /**
   * Reachability for this channel, threaded down from the surface that owns
   * the coordination read. Without it every seat is `{known:false}` — which
   * demotes nothing, because absence of evidence is not evidence.
   */
  resolveReachability?: CodingSessionReachabilityResolver;
  /**
   * The Task/Agent subagents the session's seats spawned (ledger 308), listed
   * under the participants on the overview. Absent or empty, nothing renders.
   */
  subagents?: CodingSessionSubagentPanel | null;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const participants = React.useMemo(
    () =>
      listCodingSessionUmbrellaParticipants(umbrella, actorNames).filter(
        (participant) => participant.kind === "execution",
      ),
    [actorNames, umbrella],
  );
  // W1 once per execution: the rows, the detail card and the footer tally
  // all read this map, so the panel cannot answer the same question twice.
  const statuses = React.useMemo(() => {
    const resolved = new Map<string, CodingSessionWorkspaceStatus>();
    for (const execution of umbrella.executions) {
      resolved.set(
        execution.executionKey,
        deriveCodingSessionExecutionStatus(
          execution,
          resolveReachability(execution.activeGeneration.commandTarget),
        ),
      );
    }
    return resolved;
  }, [resolveReachability, umbrella]);
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
            canSteer={canSteer}
            machine={machine}
            execution={selectedExecution}
            panelId={`${panelId}-${selectedExecution.executionKey}`}
            status={statuses.get(selectedExecution.executionKey)}
          />
        ) : (
          <Overview
            actorNames={actorNames}
            canSteer={canSteer}
            machine={machine}
            executions={umbrella.executions}
            panelId={`${panelId}-overview`}
            statuses={statuses}
            subagents={subagents}
          />
        )}
      </div>

      <ExecutionRailFooter
        canSteer={canSteer}
        executions={umbrella.executions}
        statuses={statuses}
      />
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
  canSteer,
  machine,
  executions,
  panelId,
  statuses,
  subagents,
}: {
  actorNames?: CodingSessionActorNameResolver;
  canSteer: boolean;
  machine?: CodingSessionExecutionMachineContext;
  executions: CodingSessionExecution[];
  panelId: string;
  statuses: ReadonlyMap<string, CodingSessionWorkspaceStatus>;
  subagents: CodingSessionSubagentPanel | null;
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
            canSteer={canSteer}
            execution={execution}
            key={execution.executionKey}
            machine={machine}
            status={statuses.get(execution.executionKey)}
          />
        ))}
      </div>
      {subagents && subagents.rows.length > 0 ? (
        <div className="mt-4">
          <CodingSessionSubagentsSection panel={subagents} />
        </div>
      ) : null}
    </section>
  );
}

function ExecutionDetail({
  actorNames,
  canSteer,
  machine,
  execution,
  panelId,
  status,
}: {
  actorNames?: CodingSessionActorNameResolver;
  canSteer: boolean;
  machine?: CodingSessionExecutionMachineContext;
  execution: CodingSessionExecution;
  panelId: string;
  status: CodingSessionWorkspaceStatus | undefined;
}) {
  const record = execution.activeGeneration;
  const latest = latestActivity(record.transcript);
  return (
    <section
      aria-label={`${executionName(execution, actorNames)} execution`}
      id={panelId}
      role="tabpanel"
    >
      <ExecutionCard
        actorNames={actorNames}
        canSteer={canSteer}
        execution={execution}
        machine={machine}
        status={status}
      />
      <div className="mt-4 border-t border-border/60 pt-4">
        <p className="text-3xs font-semibold tracking-[0.12em] text-muted-foreground uppercase">
          Latest activity
        </p>
        <p
          className="mt-2 text-xs leading-5 text-foreground/90"
          data-testid="coding-session-execution-latest-activity"
        >
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
  canSteer,
  execution,
  machine,
  status,
}: {
  actorNames?: CodingSessionActorNameResolver;
  canSteer: boolean;
  execution: CodingSessionExecution;
  machine?: CodingSessionExecutionMachineContext;
  status: CodingSessionWorkspaceStatus | undefined;
}) {
  const record = execution.activeGeneration;
  const resolved = status ?? {
    kind: "unknown" as const,
    label: "Status unknown" as const,
  };
  const word = codingSessionDispositionWord(resolved, canSteer);
  const runsOn = codingSessionExecutionMachine(execution, {
    localProviderPubkey: machine?.localProviderPubkey,
    resolveName: machine?.resolveName ?? actorNames,
  });
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
            <span
              className={cn(
                "shrink-0 text-2xs font-medium",
                executionStatusTone(resolved),
              )}
              data-testid="coding-session-execution-status"
            >
              {word}
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
          <p
            className="mt-0.5 truncate text-2xs text-muted-foreground"
            data-local={runsOn.local ? "true" : "false"}
            data-testid="coding-session-execution-machine"
            title={runsOn.title}
          >
            {runsOn.label}
          </p>
          {record.transcript.length > 0 ? (
            <p
              className="mt-2 truncate text-2xs text-muted-foreground"
              data-testid="coding-session-execution-card-activity"
            >
              {latestActivity(record.transcript)}
            </p>
          ) : null}
        </div>
      </div>
    </article>
  );
}

/**
 * The footer tally, in W1's own words.
 *
 * It used to recount the raw 44223 status — a second reader of the same
 * question, which is how `1 working` came to sit under a strip reading `no
 * provider answering` (WALK-2026-08-29 finding 1). It now tallies the very
 * statuses the rows above it render, so the row and the count cannot
 * disagree, and `All idle` is gone with them: a tally of the words is never
 * a claim the rows contradict.
 */
function ExecutionRailFooter({
  canSteer,
  executions,
  statuses,
}: {
  canSteer: boolean;
  executions: CodingSessionExecution[];
  statuses: ReadonlyMap<string, CodingSessionWorkspaceStatus>;
}) {
  const tally = React.useMemo(() => {
    const counts = new Map<string, number>();
    for (const execution of executions) {
      const status = statuses.get(execution.executionKey);
      if (status === undefined) continue;
      const word = codingSessionDispositionWord(status, canSteer);
      counts.set(word, (counts.get(word) ?? 0) + 1);
    }
    return [...counts.entries()]
      .sort(([left], [right]) => footerWordRank(left) - footerWordRank(right))
      .map(([word, count]) => `${count} ${word}`)
      .join(" · ");
  }, [canSteer, executions, statuses]);
  return (
    <div
      className="flex shrink-0 items-center gap-2 border-t border-border/60 px-4 py-3 text-2xs text-muted-foreground"
      data-testid="coding-session-execution-rail-footer"
    >
      <Users className="size-3.5" />
      <span>{tally}</span>
    </div>
  );
}

/** Activity first, then the states a person can act on, then the rest. */
const FOOTER_WORD_ORDER = [
  "live",
  "waiting for you",
  "waiting for an operator",
  "idle",
  "released",
];

function footerWordRank(word: string): number {
  const rank = FOOTER_WORD_ORDER.indexOf(word);
  return rank === -1 ? FOOTER_WORD_ORDER.length : rank;
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

/**
 * Which machine runs an execution (SV-24): the provider that signs its facts.
 *
 * `on this computer` only when this computer's provider key is that signer;
 * otherwise the provider by the name this client already gives it, or its
 * first eight hex. While this computer's own key is unknown, no seat claims
 * to be here.
 */
export type CodingSessionExecutionMachineContext = {
  /**
   * This computer's provider key: `null` when it runs no provider,
   * `undefined` while that is unknown.
   */
  localProviderPubkey: string | null | undefined;
  /** The name this client gives a provider key, when it has one. */
  resolveName?: CodingSessionActorNameResolver;
};

export function codingSessionExecutionMachine(
  execution: CodingSessionExecution,
  context: CodingSessionExecutionMachineContext,
): { label: string; title: string; local: boolean } {
  const { localProviderPubkey } = context;
  const record = execution.activeGeneration;
  const provider = (
    record.providerAuthorityPubkey ??
    execution.signerPubkey ??
    ""
  )
    .trim()
    .toLowerCase();
  if (provider === "") {
    return {
      label: "provider not named",
      title: "No provider key signed this execution's facts.",
      local: false,
    };
  }
  const local = localProviderPubkey?.trim().toLowerCase() ?? null;
  if (local !== null && local !== "" && local === provider) {
    return {
      label: "on this computer",
      title: `This computer's provider (${provider.slice(0, 8)}) runs this execution.`,
      local: true,
    };
  }
  const name = context.resolveName?.(provider)?.trim() || null;
  const short = provider.slice(0, 8);
  return {
    label: name ? `on ${name}` : `on provider ${short}`,
    title:
      localProviderPubkey === undefined
        ? `Runs on the provider ${name ?? short} (${short}). Whether that is this computer was not read.`
        : `Runs on the provider ${name ?? short} (${short}), not on this computer.`,
    local: false,
  };
}

/**
 * Colour for a resolved W1 status. Exported for test.
 *
 * The word itself comes from `codingSessionDispositionWord`; this only says
 * how loudly to print it. An `unknown` a signed lifecycle put there — or that
 * the lease demoted — is attention-worthy; a merely unread one is not.
 */
export function executionStatusTone(
  status: CodingSessionWorkspaceStatus,
): string {
  if (status.kind === "working") return "text-blue-500";
  if (status.kind === "waiting") return "text-amber-500";
  if (status.kind === "unknown" && status.attention) return "text-destructive";
  return "text-muted-foreground";
}

/**
 * The newest attributable thing a seat did, as one line of plain text.
 * Exported for test.
 *
 * An assistant message is markdown; this line is not a markdown surface, so
 * the syntax is stripped and runs of whitespace collapsed (SV-95), as the
 * project activity feed does — "**Failed commands:**" reads as the words.
 */
export function latestActivity(
  transcript: CodingSessionExecution["activeGeneration"]["transcript"],
): string | null {
  const latest = [...transcript].reverse().find((item) => {
    if (item.type === "message") return item.role === "assistant";
    return item.type === "tool" || item.type === "plan";
  });
  if (!latest) return null;
  if (latest.type === "message") {
    return markdownToPlainText(latest.text).replace(/\s+/g, " ").trim() || null;
  }
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
