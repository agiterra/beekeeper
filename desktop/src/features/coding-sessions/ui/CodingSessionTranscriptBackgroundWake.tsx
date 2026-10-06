import { BellRing } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { ACTIVITY_ROW_LINE_CLASS } from "@/features/agents/ui/AgentSessionToolItem/ToolItemRowClasses";
import {
  type CodingSessionTurnAutonomousWake,
  parseCodingSessionTaskNotifications,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import { formatCodingSessionBlockTime } from "./CodingSessionTranscriptRhythm";

/**
 * The row standing in for a `<task-notification>` prompt (SV-77/SV-78).
 *
 * The agent's runtime wrote that message, not a person, so it never renders
 * as a prompt bubble. Opening an autonomous turn it reads "Woke on background
 * task <id>"; inside a turn someone prompted it reads "Background task <id>
 * reported". The notification's own status word (`completed`, `failed`,
 * `killed`) and summary follow — its words, never a verdict of ours.
 */
export function CodingSessionBackgroundWakeRow({
  item,
  opensTurn,
}: {
  item: Extract<TranscriptItem, { type: "message" }>;
  /** The notification is the turn's first entry: nobody prompted it. */
  opensTurn: boolean;
}) {
  const notifications = parseCodingSessionTaskNotifications(item.text);
  const ids = notifications.map((notification) => notification.taskId);
  const subject = `background ${ids.length === 1 ? "task" : "tasks"} ${ids.join(", ")}`;
  const lead = opensTurn
    ? `Woke on ${subject}`
    : `${subject.charAt(0).toUpperCase()}${subject.slice(1)} reported`;
  const statuses = [
    ...new Set(
      notifications
        .map((notification) => notification.status)
        .filter((status): status is string => Boolean(status)),
    ),
  ];
  const summary =
    notifications.length === 1 ? (notifications[0]?.summary ?? null) : null;
  const time = formatCodingSessionBlockTime(item.timestamp);
  return (
    <div
      className={cn(
        ACTIVITY_ROW_LINE_CLASS,
        "gap-1.5 px-1 text-muted-foreground",
      )}
      data-opens-turn={opensTurn ? "" : undefined}
      data-task-ids={ids.join(" ")}
      data-testid="coding-session-background-wake"
    >
      <BellRing aria-hidden className="size-3.5 shrink-0" />
      <span className="shrink-0">{lead}</span>
      {statuses.length > 0 ? (
        <span
          className="shrink-0 text-muted-foreground/70"
          data-testid="coding-session-background-wake-status"
        >
          · {statuses.join(", ")}
        </span>
      ) : null}
      {summary ? (
        <span className="min-w-0 truncate text-muted-foreground/70">
          · {summary}
        </span>
      ) : null}
      {time ? (
        <time
          className="ms-auto shrink-0 ps-3 text-xs tabular-nums"
          dateTime={item.timestamp}
          title={time.title}
        >
          {time.label}
        </time>
      ) : null}
    </div>
  );
}

/**
 * The quiet line opening a turn nobody prompted (SV-93), read from the
 * provider's `autonomous_turn…` status rows — the only evidence on the wire,
 * since claude-agent-acp 0.84.0 never forwards the `<task-notification>`
 * prompt that {@link CodingSessionBackgroundWakeRow} would render.
 *
 * "Woke on its own" is the provider's claim and nothing more; "· background
 * task" joins it only once a row names a task notification as the cause. No
 * task id: the rows name none, and guessing which task it was would be ours.
 */
export function CodingSessionAutonomousWakeRow({
  wake,
}: {
  wake: CodingSessionTurnAutonomousWake;
}) {
  const time = formatCodingSessionBlockTime(wake.timestamp);
  return (
    <div
      className={cn(
        ACTIVITY_ROW_LINE_CLASS,
        "gap-1.5 px-1 text-muted-foreground",
      )}
      data-cause={wake.cause ?? undefined}
      data-testid="coding-session-autonomous-wake"
    >
      <BellRing aria-hidden className="size-3.5 shrink-0" />
      <span className="shrink-0">Woke on its own</span>
      {wake.cause === "background-task" ? (
        <span
          className="shrink-0 text-muted-foreground/70"
          data-testid="coding-session-autonomous-wake-cause"
        >
          · background task
        </span>
      ) : null}
      {time ? (
        <time
          className="ms-auto shrink-0 ps-3 text-xs tabular-nums"
          dateTime={wake.timestamp}
          title={time.title}
        >
          {time.label}
        </time>
      ) : null}
    </div>
  );
}
