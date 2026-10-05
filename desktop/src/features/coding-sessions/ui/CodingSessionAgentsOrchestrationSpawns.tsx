import * as React from "react";

import {
  type CodingSessionOrchestrationSpawn,
  formatCodingSessionSpawnMeta,
} from "@/features/coding-sessions/lib/codingSessionAgentsOrchestrationModel";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";

import {
  CodingSessionSubagentStatusIcon,
  revealCodingSessionSubagentInStream,
} from "./CodingSessionSubagentEntry";

/**
 * The orchestration view's "Direct spawns" (T3 `AgentsPanel`, direct agents):
 * the Task/Agent subagents the session's seats ran, each with its type,
 * duration, the first line of its result (or, while it runs, what it is
 * doing), and model · tokens · tools with every unknown said as such.
 */
export function CodingSessionAgentsOrchestrationSpawns({
  spawns,
}: {
  spawns: readonly CodingSessionOrchestrationSpawn[];
}) {
  if (spawns.length === 0) return null;
  return (
    <section
      aria-label="Direct spawns"
      className="flex flex-col"
      data-testid="coding-session-agents-direct-spawns"
    >
      <p className="px-1.5 pt-1 text-2xs font-medium tracking-wider text-muted-foreground uppercase">
        Direct spawns
      </p>
      {spawns.map((spawn) => (
        <SpawnRow key={spawn.id} spawn={spawn} />
      ))}
    </section>
  );
}

function SpawnRow({ spawn }: { spawn: CodingSessionOrchestrationSpawn }) {
  const [notInView, setNotInView] = React.useState(false);
  return (
    <button
      className="flex w-full min-w-0 flex-col gap-0.5 rounded-md px-1.5 py-1 text-left hover:bg-accent/40"
      data-status={spawn.status}
      data-testid="coding-session-agents-spawn-row"
      onClick={() =>
        setNotInView(!revealCodingSessionSubagentInStream(spawn.id))
      }
      title={
        notInView
          ? "Not on screen in the conversation — scroll to its turn"
          : "Show in conversation"
      }
      type="button"
    >
      <span className="flex w-full min-w-0 items-center gap-2">
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            spawn.status === "running" && "bg-primary",
            spawn.status === "done" && "bg-emerald-500",
            spawn.status === "failed" && "bg-destructive",
            spawn.status === "stopped" && "bg-muted-foreground/60",
            // Neither live nor settled: a hollow dot, no colour claim.
            spawn.status === "unknown" && "border border-muted-foreground/60",
          )}
        />
        <span className="min-w-0 truncate text-sm font-medium">
          {spawn.title}
        </span>
        {spawn.type ? (
          <span className="max-w-28 shrink-0 truncate rounded-sm border border-border/60 px-1 font-mono text-3xs text-muted-foreground">
            {spawn.type}
          </span>
        ) : null}
        <span className="ml-auto flex shrink-0 items-center gap-1 font-mono text-2xs tabular-nums text-muted-foreground/80">
          {spawn.durationMs !== null ? (
            <span data-testid="coding-session-agents-spawn-duration">
              {formatCodingSessionDuration(spawn.durationMs)}
            </span>
          ) : spawn.status === "running" ||
            spawn.status === "unknown" ? null : (
            // Settled with no measured span: said, never shown as `0s`.
            <span>duration not reported</span>
          )}
          <CodingSessionSubagentStatusIcon status={spawn.status} />
        </span>
      </span>
      {spawn.preview ? (
        <span
          className={cn(
            "block w-full truncate text-xs",
            spawn.status === "failed"
              ? "text-destructive"
              : "text-muted-foreground",
          )}
          data-testid="coding-session-agents-spawn-preview"
        >
          {spawn.preview}
        </span>
      ) : null}
      <span className="block w-full truncate font-mono text-2xs text-muted-foreground/70">
        {formatCodingSessionSpawnMeta(spawn)}
      </span>
    </button>
  );
}
