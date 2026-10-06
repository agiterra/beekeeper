import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionRunningGate } from "@/features/coding-sessions/lib/codingSessionObservationView";
import {
  codingSessionSubagentTitle,
  deriveCodingSessionSubagentPanel,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import { deriveCodingSessionBackgroundTasks } from "@/features/coding-sessions/lib/codingSessionTranscriptModelBackground";
import { resolveCodingSessionGenerationCallSettlement } from "@/features/coding-sessions/lib/codingSessionTranscriptModelSettlement";
import type {
  CodingSessionStatus,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  codingSessionMsUntilQuiet,
  codingSessionQuietMs,
  isCodingSessionTranscriptFresh,
} from "@/features/coding-sessions/lib/codingSessionWaitingLiveness";

/**
 * What a session is waiting on while its agent waits on its own work (SV-99):
 * the strip docked above the composer.
 *
 * Every line is read from evidence already on screen elsewhere, never from a
 * guess about what the agent is doing:
 *
 * - a **subagent** whose settled status is `running` — its Task/Agent call has
 *   no result and its turn is live by the provider's signed status (the same
 *   reading the stream row and the Agents panel give it) — on a seat whose
 *   status, after reachability demotion, is still working;
 * - a **background task** in the SV-78/SV-91 model's `running` state — a
 *   backgrounded command announced its id and nothing after it (notification,
 *   unprompted wake, continuity row) says it ended — on an execution whose
 *   status is still one a provider vouches for (working, waiting or idle);
 * - a **gate** whose signed start (SV-41) has no close, is not stale, and was
 *   read from a live subscription rather than a snapshot.
 *
 * Nothing outstanding is `null`: the strip is not drawn. An idle session with
 * nothing running therefore never shows one.
 *
 * The dot pulses only while some line is fresh: a gate start inside its stale
 * rule, or a transcript line whose execution published within the quiet
 * threshold. A transcript-backed strip over a quiet provider says "no update
 * for Nm" and holds still; reduced motion holds it still too.
 */

export type CodingSessionWaitingLine =
  | {
      kind: "subagent";
      key: string;
      executionKey: string;
      executionLabel: string;
      /** The subagent's Task description, bounded. */
      brief: string;
      /** The call's projected item id — the stream's anchor for it. */
      itemId: string;
    }
  | {
      kind: "background";
      key: string;
      executionKey: string;
      executionLabel: string;
      taskId: string;
      /** The backgrounded command's first line, or null when it named none. */
      brief: string | null;
      itemId: string;
    }
  | {
      kind: "gate";
      key: string;
      gate: string;
      startedAtMs: number;
      brief: string;
    };

export type CodingSessionWaiting = {
  lines: readonly CodingSessionWaitingLine[];
  subagents: number;
  backgroundTasks: number;
  gates: number;
  /** "Waiting on 2 subagents", "Waiting on a background task", "Gate running". */
  headline: string;
  /** One line of what: the first line's brief, or null when none says. */
  brief: string | null;
  /**
   * Silence of the freshest transcript-backed line's execution, floored to
   * minutes, when every such line is quiet; null when one is fresh, when
   * none has a known time, or when only gates are outstanding.
   */
  quietMs: number | null;
  /** Some line is fresh: the dot may pulse (motion preference aside). */
  fresh: boolean;
};

export type CodingSessionWaitingExecution = {
  executionKey: string;
  label: string;
  /** The seat-level status the roster shows (after reachability demotion). */
  status: CodingSessionWorkspaceStatus;
  generation: {
    generationId: string;
    status: CodingSessionStatus;
    transcript: readonly TranscriptItem[];
    lastTranscriptAt?: number | null;
  };
};

const MAX_BRIEF_CHARACTERS = 160;

/** Statuses a provider still vouches for; a background task may outlive a turn. */
function vouchesForBackgroundWork(status: CodingSessionWorkspaceStatus) {
  return (
    status.kind === "working" ||
    status.kind === "waiting" ||
    status.kind === "idle"
  );
}

/** The first non-blank line of `text`, bounded; null when there is none. */
export function codingSessionWaitingBriefLine(text: unknown): string | null {
  if (typeof text !== "string") return null;
  const line = text.split("\n").find((candidate) => candidate.trim());
  if (!line) return null;
  const trimmed = line.trim();
  return trimmed.length > MAX_BRIEF_CHARACTERS
    ? `${trimmed.slice(0, MAX_BRIEF_CHARACTERS - 1)}…`
    : trimmed;
}

/** What a backgrounded command ran: its `command`, else its description. */
function backgroundBrief(item: TranscriptItem): string | null {
  if (item.type !== "tool") return null;
  return (
    codingSessionWaitingBriefLine(item.args.command) ??
    codingSessionWaitingBriefLine(item.args.description) ??
    codingSessionWaitingBriefLine(item.title)
  );
}

/** Every outstanding line, subagents first, then background tasks, then gates. */
export function collectCodingSessionWaitingLines(input: {
  executions: readonly CodingSessionWaitingExecution[];
  runningGates: readonly CodingSessionRunningGate[];
}): CodingSessionWaitingLine[] {
  const subagents: CodingSessionWaitingLine[] = [];
  const background: CodingSessionWaitingLine[] = [];
  for (const execution of input.executions) {
    const { generation } = execution;
    if (!vouchesForBackgroundWork(execution.status)) continue;
    const settlementOf = resolveCodingSessionGenerationCallSettlement({
      activeGeneration: generation,
      generationId: generation.generationId,
      transcript: generation.transcript,
    });
    const panel = deriveCodingSessionSubagentPanel(
      [generation.transcript],
      (call) => settlementOf(call),
    );
    for (const row of panel.rows) {
      // A subagent runs inside a live turn: only a working seat has one. A
      // seat demoted for reachability vouches for nothing it ran.
      if (row.status !== "running" || execution.status.kind !== "working") {
        continue;
      }
      subagents.push({
        kind: "subagent",
        key: `subagent:${execution.executionKey}:${row.id}`,
        executionKey: execution.executionKey,
        executionLabel: execution.label,
        brief: codingSessionSubagentTitle(row.spawn.call),
        itemId: row.id,
      });
    }
    for (const [item, tasks] of deriveCodingSessionBackgroundTasks(
      generation.transcript,
    )) {
      for (const task of tasks) {
        if (task.state !== "running") continue;
        background.push({
          kind: "background",
          key: `background:${execution.executionKey}:${task.id}`,
          executionKey: execution.executionKey,
          executionLabel: execution.label,
          taskId: task.id,
          brief: backgroundBrief(item),
          itemId: item.id,
        });
      }
    }
  }
  const gates: CodingSessionWaitingLine[] = [];
  for (const gate of input.runningGates) {
    // A stale start reads "no result observed", and a snapshot cannot say
    // the gate is still going: neither is something to wait on.
    if (gate.stale || gate.notLive) continue;
    gates.push({
      kind: "gate",
      key: `gate:${gate.key}`,
      gate: gate.gate,
      startedAtMs: gate.startedAtMs,
      brief: gate.gate,
    });
  }
  return [...subagents, ...background, ...gates];
}

function plural(count: number, one: string, many: string): string {
  return count === 1 ? one : `${count} ${many}`;
}

/** The strip's headline for these counts; null when nothing is outstanding. */
export function formatCodingSessionWaitingHeadline(counts: {
  subagents: number;
  backgroundTasks: number;
  gates: number;
}): string | null {
  const on: string[] = [];
  if (counts.subagents > 0) {
    on.push(
      counts.subagents === 1 ? "1 subagent" : `${counts.subagents} subagents`,
    );
  }
  if (counts.backgroundTasks > 0) {
    on.push(
      plural(counts.backgroundTasks, "a background task", "background tasks"),
    );
  }
  const clauses: string[] = [];
  if (on.length > 0) clauses.push(`Waiting on ${on.join(" and ")}`);
  if (counts.gates > 0) {
    clauses.push(
      counts.gates === 1 ? "Gate running" : `${counts.gates} gates running`,
    );
  }
  return clauses.length > 0 ? clauses.join(" · ") : null;
}

/**
 * The strip's whole reading at `nowMs`, or null when nothing is outstanding.
 */
export function deriveCodingSessionWaiting(input: {
  executions: readonly CodingSessionWaitingExecution[];
  runningGates: readonly CodingSessionRunningGate[];
  nowMs: number;
}): CodingSessionWaiting | null {
  return summarizeCodingSessionWaiting({
    executions: input.executions,
    lines: collectCodingSessionWaitingLines(input),
    nowMs: input.nowMs,
  });
}

/**
 * {@link deriveCodingSessionWaiting} over lines already collected, so a view
 * can keep the transcript walk out of its clock tick.
 */
export function summarizeCodingSessionWaiting(input: {
  executions: readonly CodingSessionWaitingExecution[];
  lines: readonly CodingSessionWaitingLine[];
  nowMs: number;
}): CodingSessionWaiting | null {
  const { lines } = input;
  if (lines.length === 0) return null;
  let subagents = 0;
  let backgroundTasks = 0;
  let gates = 0;
  for (const line of lines) {
    if (line.kind === "subagent") subagents += 1;
    else if (line.kind === "background") backgroundTasks += 1;
    else gates += 1;
  }
  const headline = formatCodingSessionWaitingHeadline({
    subagents,
    backgroundTasks,
    gates,
  });
  if (headline === null) return null;

  const lastAtByExecution = new Map<string, number | null>();
  for (const execution of input.executions) {
    lastAtByExecution.set(
      execution.executionKey,
      execution.generation.lastTranscriptAt ?? null,
    );
  }
  let fresh = gates > 0;
  let newestQuietAt: number | null = null;
  for (const line of lines) {
    if (line.kind === "gate") continue;
    const lastAt = lastAtByExecution.get(line.executionKey) ?? null;
    if (isCodingSessionTranscriptFresh(lastAt, input.nowMs)) {
      fresh = true;
    } else if (lastAt !== null && Number.isFinite(lastAt)) {
      newestQuietAt =
        newestQuietAt === null ? lastAt : Math.max(newestQuietAt, lastAt);
    }
  }
  const quietMs =
    fresh || newestQuietAt === null
      ? null
      : codingSessionQuietMs(newestQuietAt, input.nowMs);

  return {
    lines,
    subagents,
    backgroundTasks,
    gates,
    headline,
    brief: lines.find((line) => line.brief)?.brief ?? null,
    quietMs,
    fresh,
  };
}

/**
 * Milliseconds until the last fresh transcript-backed line goes quiet, or
 * null when none is fresh — what the strip's one-shot timer waits so the dot
 * stops pulsing on time.
 */
export function codingSessionWaitingMsUntilQuiet(input: {
  executions: readonly CodingSessionWaitingExecution[];
  lines: readonly CodingSessionWaitingLine[];
  nowMs: number;
}): number | null {
  const keys = new Set<string>();
  for (const line of input.lines) {
    if (line.kind !== "gate") keys.add(line.executionKey);
  }
  let latest: number | null = null;
  for (const execution of input.executions) {
    if (!keys.has(execution.executionKey)) continue;
    const delay = codingSessionMsUntilQuiet(
      execution.generation.lastTranscriptAt,
      input.nowMs,
    );
    if (delay !== null && (latest === null || delay > latest)) latest = delay;
  }
  return latest;
}

/** One line of the info popover: what it is, and the evidence behind it. */
export function describeCodingSessionWaitingLine(
  line: CodingSessionWaitingLine,
): { title: string; evidence: string } {
  switch (line.kind) {
    case "subagent":
      return {
        title: `Subagent: ${line.brief}`,
        evidence: `Spawned by ${line.executionLabel}. Its call has no result yet and its turn is live.`,
      };
    case "background":
      return {
        title: line.brief
          ? `Background task: ${line.brief}`
          : `Background task ${line.taskId}`,
        evidence: `Started by ${line.executionLabel} (id ${line.taskId}). Nothing since says it ended.`,
      };
    case "gate":
      return {
        title: `Gate: ${line.gate}`,
        evidence:
          "The provider signed this gate's start and no close yet, inside its stale window.",
      };
  }
}
