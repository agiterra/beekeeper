import * as React from "react";
import {
  Bot,
  Check,
  ChevronDown,
  CircleStop,
  LoaderCircle,
  ShieldQuestion,
  X,
} from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type CodingSessionSettledSubagentStatus,
  type CodingSessionSubagentSpawn,
  codingSessionSubagentTitle,
  formatCodingSessionSubagentGroupLabel,
  settleCodingSessionSubagentSpawns,
  summarizeCodingSessionSubagentStatuses,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import type { CodingSessionTranscriptEntry } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  ACTIVITY_ROW_DETAIL_INSET_CLASS,
  ACTIVITY_ROW_ICON_CLASS,
  ACTIVITY_ROW_LABEL_CLASS,
  ACTIVITY_ROW_LINE_CLASS,
} from "@/features/agents/ui/AgentSessionToolItem/ToolItemRowClasses";
import { cn } from "@/shared/lib/cn";
import { Markdown } from "@/shared/ui/markdown";
import { CodingSessionTurnSettlementContext } from "./CodingSessionTranscriptItem";
import { requestCodingSessionUmbrellaItemReveal } from "./CodingSessionUmbrellaTimelineWindow";

type SubagentsEntry = Extract<
  CodingSessionTranscriptEntry,
  { kind: "subagents" }
>;

/** Renders one of a subagent's own items with the stream's row components. */
export type CodingSessionSubagentChildRenderer = (
  item: TranscriptItem,
) => React.ReactNode;

/** A spawn as read through its turn's settlement (`settleCodingSessionSubagentSpawns`). */
type SettledSubagentSpawn = Omit<CodingSessionSubagentSpawn, "status"> & {
  status: CodingSessionSettledSubagentStatus;
};

/**
 * The whole group reads running while any spawn runs, then failed, then
 * status unknown, then stopped, and done only when every spawn finished.
 */
function groupStatus(
  spawns: readonly SettledSubagentSpawn[],
): CodingSessionSettledSubagentStatus {
  const statuses = spawns.map((spawn) => spawn.status);
  if (statuses.includes("running")) return "running";
  if (statuses.includes("failed")) return "failed";
  if (statuses.includes("unknown")) return "unknown";
  return statuses.includes("stopped") ? "stopped" : "done";
}

export function CodingSessionSubagentStatusIcon({
  status,
}: {
  status: CodingSessionSettledSubagentStatus;
}) {
  if (status === "running") {
    return (
      <LoaderCircle
        aria-label="Running"
        className="size-3.5 shrink-0 animate-spin text-muted-foreground motion-reduce:animate-none"
      />
    );
  }
  if (status === "unknown") {
    // Nothing on screen says whether the turn is still going (the provider is
    // unreachable): no spinner, which would claim live work, and no verdict.
    return (
      <ShieldQuestion
        aria-label="Status unknown"
        className="size-3.5 shrink-0 text-muted-foreground"
      />
    );
  }
  if (status === "stopped") {
    // Muted, unlike failed: the turn ended before the call answered, which is
    // not the subagent reporting an error.
    return (
      <CircleStop
        aria-label="Stopped"
        className="size-3.5 shrink-0 text-muted-foreground"
      />
    );
  }
  if (status === "failed") {
    return (
      <X aria-label="Failed" className="size-3.5 shrink-0 text-destructive" />
    );
  }
  // Done is the quiet state: said, but dimmed below the label, so the row
  // reads as the other settled activity rows do.
  return (
    <Check
      aria-label="Done"
      className="size-3.5 shrink-0 text-muted-foreground/60"
    />
  );
}

/**
 * A Task/Agent spawn in the lead's stream: one compact row, with the
 * subagent's own items behind it. Several spawns in a row share the row.
 * The items are built only while the row is open.
 *
 * SV-06: where the workspace has an Agents surface (`onOpenAgentsSurface`),
 * clicking the row opens it, as T3 Code's subagent row does; the chevron at
 * its right still expands the steps in place. Without one, the row itself
 * expands. Either way the row stays a `<details>`, so "Show in
 * conversation" from the Agents panel can still open it here.
 */
export function CodingSessionSubagentEntry({
  disclosureId,
  entry,
  onOpenAgentsSurface = null,
  onOpenChange,
  open,
  renderChild,
}: {
  disclosureId: string;
  entry: SubagentsEntry;
  /** Opens the workspace's Agents surface; `null` when there is none. */
  onOpenAgentsSurface?: (() => void) | null;
  onOpenChange: (id: string, open: boolean) => void;
  open: boolean;
  renderChild: CodingSessionSubagentChildRenderer;
}) {
  // A Task call that never answered reads "running" in the transcript even
  // after its turn is known to be over (or cannot be known); read the spawns
  // through the same settlement the turn's tool rows use, so this row never
  // shows a spinner or "1 working" beside rows that read settled.
  const settlement = React.useContext(CodingSessionTurnSettlementContext);
  const spawns = settleCodingSessionSubagentSpawns(entry.spawns, settlement);
  const status = groupStatus(spawns);
  const label =
    spawns === entry.spawns
      ? entry.label
      : formatCodingSessionSubagentGroupLabel(spawns);
  // Attribution: every spawn's own description, on the row's tooltip, so a
  // "Ran 1 subagent" row still says which one without being opened.
  const spawnTitles = spawns
    .map((spawn) => codingSessionSubagentTitle(spawn.call))
    .join("\n");
  const opensSurface = onOpenAgentsSurface !== null;
  // SV-06, T3's `summarizeSubagentStatuses`: "Kicked off 3 subagents" says
  // how many started, not how many are still working, so the counts ride
  // along — always as the row's description, and visibly until every spawn
  // is done ("2 working · 1 done", "1 done · 1 failed").
  const statusSummary = summarizeCodingSessionSubagentStatuses(spawns);
  return (
    <details
      className="group/subagents"
      // Named on the row itself because the spawns below are built only while
      // it is open; `revealCodingSessionSubagentInStream` finds it by these.
      data-subagent-call-ids={spawns.map((spawn) => spawn.call.id).join(" ")}
      data-opens-surface={opensSurface ? "agents" : undefined}
      data-status={status}
      data-testid="coding-session-subagents"
      onToggle={(event) => onOpenChange(disclosureId, event.currentTarget.open)}
      open={open}
    >
      {/* SV-01/SV-06: the activity-row pattern — full width, a soft fill on
          hover, a 16px muted robot and a dimmed label. The group's status
          icon stays on the row in every state; a failure keeps its colour. */}
      {/* biome-ignore lint/a11y/noStaticElementInteractions: a <summary> is natively interactive (Enter/Space activate it as a click); the handler only redirects that activation to the Agents surface. */}
      <summary
        aria-description={statusSummary || undefined}
        aria-label={opensSurface ? `${label} — open in Agents` : label}
        className={cn(
          "group/row cursor-pointer list-none",
          ACTIVITY_ROW_LINE_CLASS,
        )}
        onClick={
          opensSurface
            ? (event) => {
                event.preventDefault();
                onOpenAgentsSurface?.();
              }
            : undefined
        }
        title={spawnTitles || undefined}
      >
        <Bot aria-hidden className={ACTIVITY_ROW_ICON_CLASS} />
        <span className={ACTIVITY_ROW_LABEL_CLASS}>{label}</span>
        {status !== "done" && statusSummary ? (
          <span
            className="shrink-0 text-muted-foreground/70"
            data-testid="coding-session-subagents-status-summary"
          >
            · {statusSummary}
          </span>
        ) : null}
        <CodingSessionSubagentStatusIcon status={status} />
        {opensSurface ? (
          <button
            aria-expanded={open}
            aria-label={open ? "Hide steps here" : "Show steps here"}
            className="ms-auto inline-flex shrink-0 cursor-pointer items-center rounded-sm p-0.5 text-muted-foreground/0 transition hover:bg-accent/60 focus-visible:text-muted-foreground/70 group-hover/row:text-muted-foreground/70 group-open/subagents:text-muted-foreground/70"
            data-testid="coding-session-subagents-inline-toggle"
            onClick={(event) => {
              // The row opens the surface; this alone toggles in place.
              event.preventDefault();
              event.stopPropagation();
              onOpenChange(disclosureId, !open);
            }}
            title={open ? "Hide steps here" : "Show steps here"}
            type="button"
          >
            <ChevronDown
              aria-hidden
              className="size-3.5 transition group-open/subagents:rotate-180"
            />
          </button>
        ) : (
          <ChevronDown className="ms-auto size-3.5 shrink-0 text-muted-foreground/0 transition group-hover/row:text-muted-foreground/70 group-open/subagents:rotate-180 group-open/subagents:text-muted-foreground/70" />
        )}
      </summary>
      {open ? (
        // T3's inline subagent detail sits under the label, not in a rail.
        <div
          className={cn(
            ACTIVITY_ROW_DETAIL_INSET_CLASS,
            "mt-1 flex flex-col gap-3",
          )}
        >
          {spawns.map((spawn) => (
            <CodingSessionSubagentSpawnDetail
              key={spawn.call.id}
              renderChild={renderChild}
              // Always named: "Ran 1 subagent" says how many, not which.
              showHeader
              spawn={spawn}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}

/**
 * One subagent's own items, then its report. Shared by the stream row and the
 * Agents panel, each passing its own item renderer.
 */
export function CodingSessionSubagentSpawnDetail({
  renderChild,
  showHeader,
  spawn,
}: {
  renderChild: CodingSessionSubagentChildRenderer;
  showHeader: boolean;
  spawn: SettledSubagentSpawn;
}) {
  const { status } = spawn;
  const report =
    status === "running" || status === "stopped" || status === "unknown"
      ? ""
      : spawn.call.result.trim();
  return (
    <div
      className="flex min-w-0 flex-col gap-1"
      data-subagent-call-id={spawn.call.id}
      data-testid="coding-session-subagent-spawn"
    >
      {showHeader ? (
        <p className="flex min-w-0 items-center gap-2 text-xs font-medium text-foreground/85">
          <CodingSessionSubagentStatusIcon status={status} />
          <span className="truncate">
            {codingSessionSubagentTitle(spawn.call)}
          </span>
        </p>
      ) : null}
      {spawn.children.length > 0 ? (
        <div className="flex min-w-0 flex-col gap-1">
          {spawn.children.map((child) => (
            <div className="min-w-0" key={child.id}>
              {renderChild(child)}
            </div>
          ))}
        </div>
      ) : (
        // An older provider dropped a subagent's items rather than publish
        // them; the call cannot tell that apart from a subagent that did
        // nothing, so it says only what it knows.
        <p className="text-xs text-muted-foreground">
          {status === "running" || status === "unknown"
            ? "No steps published yet."
            : "No steps of this subagent were published."}
        </p>
      )}
      {status === "stopped" ? (
        <p className="text-xs text-muted-foreground">
          Stopped — its turn ended before the subagent returned a result.
        </p>
      ) : null}
      {status === "unknown" ? (
        <p className="text-xs text-muted-foreground">
          Status unknown — no result yet, and nothing says whether its turn is
          still running.
        </p>
      ) : null}
      {report ? (
        <div
          className={cn(
            "mt-1 rounded-md px-3 py-2",
            status === "failed" ? "bg-destructive/5" : "bg-muted/40",
          )}
          data-testid="coding-session-subagent-report"
        >
          <p className="text-2xs font-medium text-muted-foreground">
            {status === "failed" ? "Failed" : "Report"}
          </p>
          <Markdown className="text-sm leading-6" content={report} />
        </div>
      ) : null}
    </div>
  );
}

/**
 * Open and scroll to a spawn's row in the conversation, if it is mounted.
 * When it is not (its turn sits above the umbrella timeline's "Load earlier"),
 * ask the umbrella timeline to widen its window to that turn; `true` means
 * the timeline took the request. Otherwise — e.g. a virtualized-out row in the
 * one-seat view — `false`, and the caller keeps its own inline view.
 */
export function revealCodingSessionSubagentInStream(
  callItemId: string,
): boolean {
  if (typeof document === "undefined") return false;
  // The group row names its spawns, because a closed row has not built them.
  const group = document.querySelector(
    `[data-testid="coding-session-transcript"] details[data-subagent-call-ids~="${CSS.escape(callItemId)}"]`,
  );
  if (!(group instanceof HTMLDetailsElement)) {
    return requestCodingSessionUmbrellaItemReveal(callItemId);
  }
  // Setting `open` fires `toggle`, which the row already mirrors into the
  // transcript's disclosure state; that state builds the spawns.
  group.open = true;
  group.scrollIntoView({ behavior: "smooth", block: "nearest" });
  return true;
}
