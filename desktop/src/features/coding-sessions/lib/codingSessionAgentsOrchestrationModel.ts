/**
 * The Agents surface's orchestration view as data (SV-40): one card per
 * mission, its phase pipeline, each seat's live activity, and the direct
 * spawns.
 *
 * T3 Code's `AgentsPanel` (`a2ca89aa10:apps/web/src/components/AgentsPanel.tsx`)
 * and `deriveAgentPanelModel` (`packages/client-runtime/src/state/
 * subagentRuntime.ts` at the same commit) are the reference: a workflow card
 * with `N/M settled`, a phase rail of one dot per member, collapsible phase
 * sections that open by default while running, and a direct-spawns list. What
 * Beekeeper has instead of T3's in-process workflow coordinator is the wire:
 *
 * - **Phases** are seat roles. A seat's role is its kind-44223 metadata
 *   `role`; a kind-44244 `assignment`'s signed `assigneeRole` adds a phase
 *   nobody holds yet; a kind-44249 `work.declared` whose criteria no
 *   assignment owes adds a phase that carries the word `declared`.
 * - **Order** is the Route rail's road order (`codingSessionRouteModel.ts`,
 *   §9.2): the lead first, then the rest in hire order (the umbrella's own
 *   execution order). Phases with no seat follow, assigned before declared.
 * - **Agents** are executions, coloured by **state** only (DB12): running
 *   primary, done success, failed destructive, waiting amber. Identity
 *   accents never colour a state dot. A seat waits when its provider signed
 *   `waiting_for_input`, or when an open `decision.request` (DB8) is its own
 *   question or holds up an assignment it was given.
 * - **Activity** is read off the transcript (kind 44225): the newest open
 *   tool item, the `result` items' `usage` for tokens, the tool items for the
 *   tool count. Model is the 44223 metadata `model`. Kind 44200 turn
 *   metrics are not read: they are encrypted to the agent's owner, carry no
 *   channel, and the coding-session provider does not publish them.
 *
 * Pure, and fed relay facts only. Nothing here asks whether this machine runs
 * a seat, so a teammate on another computer derives the same cards from the
 * same events. A value the wire does not carry is `null` and reads
 * "not reported", never `0`.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { ProjectWorkDeclaration } from "@/shared/api/tauriProjectWork";

import type { CodingSessionMissionTransactionInput } from "./codingSessionMissionContracts";
import type { CodingSessionMissionDecisionInput } from "./codingSessionMissionDecisions";
import {
  type CodingSessionSettledSubagentStatus,
  type CodingSessionSubagentPanel,
  type CodingSessionSubagentRow,
  formatCodingSessionSubagentTokens,
} from "./codingSessionSubagents";
import type {
  CodingSessionExecution,
  CodingSessionStatus,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";

/** One execution with its signed status and the status the view shows. */
export type CodingSessionAgentsOrchestrationExecution = {
  execution: CodingSessionExecution;
  wireStatus: CodingSessionStatus;
  status: CodingSessionWorkspaceStatus;
};

/** A seat's state, in the hues DB12 allows. */
export type CodingSessionAgentState =
  | "running"
  | "waiting"
  | "idle"
  | "done"
  | "failed"
  | "stopped"
  | "unknown";

const SETTLED_STATES: ReadonlySet<CodingSessionAgentState> = new Set([
  "done",
  "failed",
  "stopped",
]);

const ACTIVE_STATES: ReadonlySet<CodingSessionAgentState> = new Set([
  "running",
  "waiting",
  "idle",
]);

/** The words a state reads as, for a screen reader and a tooltip. */
export const CODING_SESSION_AGENT_STATE_LABEL: Readonly<
  Record<CodingSessionAgentState, string>
> = {
  running: "Working",
  waiting: "Waiting on a person",
  idle: "Idle",
  done: "Completed",
  failed: "Failed",
  stopped: "Stopped",
  unknown: "Status unknown",
};

/** An agent's state in words, naming what a waiting seat waits on. */
export function codingSessionAgentStateWord(
  agent: Pick<CodingSessionOrchestrationAgent, "state" | "waitingOn">,
): string {
  if (agent.waitingOn === "ruling") return "Waiting on a ruling";
  if (agent.waitingOn === "input") return "Waiting for input";
  return CODING_SESSION_AGENT_STATE_LABEL[agent.state];
}

/** Token usage from the transcript's turn results, or nothing reported. */
export type CodingSessionAgentTokens =
  | {
      kind: "reported";
      /** Σ input + output over the turn results that reported either. */
      total: number;
      /** True when some turn result reported no usage, or only half of it. */
      partial: boolean;
      reportedTurns: number;
      turns: number;
    }
  | { kind: "not-reported" };

export type CodingSessionOrchestrationAgent = {
  executionKey: string;
  label: string;
  role: string | null;
  state: CodingSessionAgentState;
  /**
   * Why a waiting seat waits: its provider signed `waiting_for_input`, or an
   * open ruling (DB8) is its question or holds its assignment. Null otherwise.
   */
  waitingOn: "input" | "ruling" | null;
  settled: boolean;
  /** The newest open tool call's name while the seat is live; else null. */
  currentTool: string | null;
  /** 44223 `model`, or null when the metadata named none. */
  model: string | null;
  tokens: CodingSessionAgentTokens;
  /** Tool items in this seat's transcript: what this client has seen. */
  toolCount: number;
  /**
   * A settled seat's outcome line (T3 `agentActivityText`, settled branch):
   * a failed seat's newest error result, or a completed seat's newest result,
   * first line only. Null while the seat is live or when nothing applies.
   */
  outcome: CodingSessionAgentOutcome | null;
};

/** What a settled seat ended with, in one line. */
export type CodingSessionAgentOutcome = {
  text: string;
  failed: boolean;
};

/** Where a phase comes from, strongest first. */
export type CodingSessionPhaseOrigin = "seat" | "assigned" | "declared";

/**
 * `done` is every agent completed; `failed` is every agent settled with one
 * failed; `settled` is every agent settled with one stopped and none failed.
 * T3 calls all three "done" with a check; a failure under a green check would
 * hide it, so Beekeeper keeps them apart.
 */
export type CodingSessionPhaseState =
  | "pending"
  | "running"
  | "done"
  | "failed"
  | "settled"
  | "unknown";

export type CodingSessionOrchestrationPhase = {
  key: string;
  title: string;
  origin: CodingSessionPhaseOrigin;
  state: CodingSessionPhaseState;
  agents: CodingSessionOrchestrationAgent[];
  activeCount: number;
  /** Every agent whose newest state is terminal: done, failed or stopped. */
  settledCount: number;
  /** Of those, the ones that failed, and the ones that stopped. */
  failedCount: number;
  stoppedCount: number;
  unknownCount: number;
};

export type CodingSessionOrchestrationCard = {
  key: string;
  title: string;
  phases: CodingSessionOrchestrationPhase[];
  /** Seats with no role: listed under the card, never dropped. */
  unphased: CodingSessionOrchestrationAgent[];
  /** Agents whose newest state is terminal. */
  settled: number;
  /** Agents whose newest state is failed. */
  failed: number;
  total: number;
  live: boolean;
};

export type CodingSessionOrchestrationSpawn = {
  id: string;
  /**
   * The call's `toolCallId`, which its subagent's items name as their
   * `parentToolId`: the key its own page opens by (SV-80). `null` when the
   * producer sent none, and then no page can open.
   */
  parentToolId: string | null;
  title: string;
  type: string | null;
  /** The panel row's status, read through the spawn's turn settlement. */
  status: CodingSessionSettledSubagentStatus;
  durationMs: number | null;
  /** Settled: the result's first line; live: the latest thing it did. */
  preview: string | null;
  model: string | null;
  totalTokens: number | null;
  toolCount: number | null;
};

export type CodingSessionAgentsOrchestrationModel = {
  cards: CodingSessionOrchestrationCard[];
  spawns: CodingSessionOrchestrationSpawn[];
  /** Whether kind-44249 work coverage reached this view at all. */
  declaredWorkRead: boolean;
  /** Whether the kind-44244 assignments reached this view at all. */
  assignmentsRead: boolean;
};

export type CodingSessionAgentsOrchestrationInput = {
  /** The mission's name: the session title. */
  title: string;
  /** Stable card key: the umbrella key. */
  missionKey: string;
  /** Every execution, in umbrella (hire) order. */
  executions: readonly CodingSessionAgentsOrchestrationExecution[];
  subagents: CodingSessionSubagentPanel;
  /** Signed 44244 rows, or null when they were not read here. */
  transactions: readonly CodingSessionMissionTransactionInput[] | null;
  /** Folded 44249 declarations, or null when coverage was not read here. */
  declaredWork: readonly ProjectWorkDeclaration[] | null;
  /**
   * The team fold's `decisions[]` (DB8), or null when not read. Only open
   * rows matter here; mapping one to a seat needs `transactions` too.
   */
  decisions?: readonly CodingSessionMissionDecisionInput[] | null;
  resolveActorName: (pubkey: string) => string | null | undefined;
};

/**
 * A seat's state from its signed status, corrected by reachability.
 *
 * A terminal word the provider signed is a fact whatever the reachability
 * says; a live word is believed only when the workspace still shows it live,
 * so a seat whose provider stopped answering reads unknown, never running.
 */
export function codingSessionAgentState(
  entry: Pick<
    CodingSessionAgentsOrchestrationExecution,
    "status" | "wireStatus"
  >,
): CodingSessionAgentState {
  switch (entry.wireStatus) {
    case "completed":
      return "done";
    case "failed":
      return "failed";
    case "stopped":
    case "interrupted":
      return "stopped";
    default:
      break;
  }
  switch (entry.status.kind) {
    case "working":
      return "running";
    case "waiting":
      return "waiting";
    case "idle":
    case "founded":
      return "idle";
    case "ended":
      return "stopped";
    default:
      return "unknown";
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function finite(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0
    ? value
    : null;
}

/** Turn-result token usage across the given transcripts. */
export function readCodingSessionAgentTokens(
  transcripts: readonly (readonly TranscriptItem[])[],
): CodingSessionAgentTokens {
  let turns = 0;
  let reportedTurns = 0;
  let total = 0;
  let partial = false;
  for (const transcript of transcripts) {
    for (const item of transcript) {
      if (item.type !== "lifecycle" || item.title !== "Turn result") continue;
      turns += 1;
      const usage: unknown = (item as { usage?: unknown }).usage;
      const input = isRecord(usage) ? finite(usage.inputTokens) : null;
      const output = isRecord(usage) ? finite(usage.outputTokens) : null;
      if (input === null && output === null) {
        partial = true;
        continue;
      }
      if (input === null || output === null) partial = true;
      reportedTurns += 1;
      total += (input ?? 0) + (output ?? 0);
    }
  }
  return reportedTurns === 0
    ? { kind: "not-reported" }
    : { kind: "reported", total, partial, reportedTurns, turns };
}

/** The newest tool call that has not finished, by its start. */
export function newestOpenToolName(
  transcripts: readonly (readonly TranscriptItem[])[],
): string | null {
  let newest: { at: number; name: string } | null = null;
  for (const transcript of transcripts) {
    for (const item of transcript) {
      if (item.type !== "tool") continue;
      if (item.status !== "executing" && item.status !== "pending") continue;
      if (item.completedAt) continue;
      const at = Date.parse(item.startedAt || item.timestamp) || 0;
      if (newest === null || at >= newest.at) {
        newest = { at, name: item.toolName || item.title };
      }
    }
  }
  return newest?.name ?? null;
}

/**
 * A settled seat's outcome from its newest turn result (T3 shows a settled
 * row's error, else its result). A failed seat with no error result says so
 * rather than show a success line under a red dot.
 */
export function readCodingSessionAgentOutcome(
  state: CodingSessionAgentState,
  transcripts: readonly (readonly TranscriptItem[])[],
): CodingSessionAgentOutcome | null {
  if (state !== "done" && state !== "failed") return null;
  const wanted = state === "failed";
  let text: string | null = null;
  for (const transcript of transcripts) {
    for (const item of transcript) {
      if (item.type !== "lifecycle" || item.title !== "Turn result") continue;
      if ((item.renderClass === "error") !== wanted) continue;
      text = firstLine(item.text) ?? text;
    }
  }
  if (text !== null) return { text, failed: wanted };
  return wanted ? { text: "No failure detail reported", failed: true } : null;
}

function countTools(transcripts: readonly (readonly TranscriptItem[])[]) {
  let count = 0;
  for (const transcript of transcripts) {
    for (const item of transcript) if (item.type === "tool") count += 1;
  }
  return count;
}

function executionTranscripts(
  execution: CodingSessionExecution,
): (readonly TranscriptItem[])[] {
  return [
    ...execution.priorGenerations.map((record) => record.transcript),
    execution.activeGeneration.transcript,
  ];
}

function normalizeRole(role: string | null | undefined): string | null {
  const trimmed = role?.trim().toLowerCase() ?? "";
  return trimmed.length > 0 ? trimmed : null;
}

/** `builder` → `Builder`, `code-review` → `Code review`. */
export function codingSessionPhaseTitle(role: string): string {
  const words = role.replace(/[-_]+/g, " ").trim();
  return words.length === 0
    ? role
    : words.charAt(0).toUpperCase() + words.slice(1);
}

/**
 * The seats an open ruling holds (DB8), by lower-cased actor pubkey: the
 * asker of each unanswered `decision.request`, and the assignee of every
 * assignment it names in `blocks`. Both need the signed 44244 rows; without
 * them no seat is marked, and the view already says assignments were not read.
 */
export function codingSessionSeatsHeldByRulings(
  decisions: readonly CodingSessionMissionDecisionInput[] | null | undefined,
  transactions: readonly CodingSessionMissionTransactionInput[] | null,
): ReadonlySet<string> {
  const held = new Set<string>();
  if (!decisions || !transactions) return held;
  const rows = new Map(
    transactions.map((row) => [row.sourceEventId.toLowerCase(), row]),
  );
  for (const decision of decisions) {
    if (decision.answerId !== null || decision.answeredBy !== null) continue;
    const request = rows.get(decision.requestId.toLowerCase());
    if (request?.type === "decision.request") {
      held.add(request.authorPubkey.toLowerCase());
    }
    for (const blocked of decision.blocks) {
      const assignment = rows.get(blocked.toLowerCase());
      if (assignment?.type === "assignment" && assignment.counterpartyPubkey) {
        held.add(assignment.counterpartyPubkey.toLowerCase());
      }
    }
  }
  return held;
}

function deriveAgent(
  entry: CodingSessionAgentsOrchestrationExecution,
  resolveActorName: CodingSessionAgentsOrchestrationInput["resolveActorName"],
  heldByRuling: ReadonlySet<string>,
): CodingSessionOrchestrationAgent {
  const record = entry.execution.activeGeneration;
  const transcripts = executionTranscripts(entry.execution);
  const signed = codingSessionAgentState(entry);
  // A ruling holds a seat only while the seat is still in play: a settled
  // seat's signed terminal word, and an unreachable seat's unknown, stand.
  const ruled =
    record.agentRef !== null &&
    heldByRuling.has(record.agentRef.toLowerCase()) &&
    (signed === "running" || signed === "idle" || signed === "waiting");
  const state: CodingSessionAgentState = ruled ? "waiting" : signed;
  const live = state === "running" || state === "waiting";
  const named = record.agentRef ? resolveActorName(record.agentRef) : null;
  return {
    executionKey: entry.execution.executionKey,
    label: named || record.title || record.label,
    role: normalizeRole(record.role),
    state,
    waitingOn: ruled ? "ruling" : state === "waiting" ? "input" : null,
    settled: SETTLED_STATES.has(state),
    currentTool: live ? newestOpenToolName(transcripts) : null,
    model: record.model?.trim() ? record.model.trim() : null,
    tokens: readCodingSessionAgentTokens(transcripts),
    toolCount: countTools(transcripts),
    outcome: readCodingSessionAgentOutcome(state, transcripts),
  };
}

function phaseState(
  agents: readonly CodingSessionOrchestrationAgent[],
): Pick<
  CodingSessionOrchestrationPhase,
  | "state"
  | "activeCount"
  | "settledCount"
  | "failedCount"
  | "stoppedCount"
  | "unknownCount"
> {
  const count = (test: (agent: CodingSessionOrchestrationAgent) => boolean) =>
    agents.filter(test).length;
  const activeCount = count((agent) => ACTIVE_STATES.has(agent.state));
  const settledCount = count((agent) => agent.settled);
  const failedCount = count((agent) => agent.state === "failed");
  const stoppedCount = count((agent) => agent.state === "stopped");
  const unknownCount = agents.length - activeCount - settledCount;
  const state: CodingSessionPhaseState =
    agents.length === 0
      ? "pending"
      : activeCount > 0
        ? "running"
        : settledCount < agents.length
          ? "unknown"
          : failedCount > 0
            ? "failed"
            : stoppedCount > 0
              ? "settled"
              : "done";
  return {
    state,
    activeCount,
    settledCount,
    failedCount,
    stoppedCount,
    unknownCount,
  };
}

/** A declaration's display name: its plan file's slug. */
function declarationTitle(declaration: ProjectWorkDeclaration): string {
  const file = declaration.planRef.path.split("/").pop() ?? "";
  const slug = file.replace(/\.md$/i, "");
  return slug.length > 0 ? codingSessionPhaseTitle(slug) : "Declared work";
}

function firstLine(text: string | null | undefined): string | null {
  const line = (text ?? "").split("\n").find((candidate) => candidate.trim());
  return line ? line.trim() : null;
}

/**
 * One direct spawn, in T3's preview rule (`subagentDetailPreview`): a settled
 * spawn leads with its result's first line, a live one with what it is doing.
 */
export function deriveCodingSessionOrchestrationSpawn(
  row: CodingSessionSubagentRow,
): CodingSessionOrchestrationSpawn {
  const settled = row.status !== "running";
  const result = row.spawn.call.result
    ? firstLine(row.spawn.call.result)
    : null;
  return {
    id: row.id,
    parentToolId: row.spawn.call.toolCallId ?? null,
    title: row.title,
    type: row.type,
    status: row.status,
    durationMs: row.durationMs,
    preview: settled ? (result ?? row.latest) : (row.latest ?? result),
    model: row.model,
    totalTokens: row.totalTokens,
    toolCount: row.toolCount,
  };
}

/** The whole orchestration view for one session. */
export function deriveCodingSessionAgentsOrchestration(
  input: CodingSessionAgentsOrchestrationInput,
): CodingSessionAgentsOrchestrationModel {
  type Draft = {
    key: string;
    title: string;
    origin: CodingSessionPhaseOrigin;
    rank: number;
    at: number;
    agents: CodingSessionOrchestrationAgent[];
  };
  const phases = new Map<string, Draft>();
  const unphased: CodingSessionOrchestrationAgent[] = [];
  const heldByRuling = codingSessionSeatsHeldByRulings(
    input.decisions,
    input.transactions,
  );

  input.executions.forEach((entry, index) => {
    const agent = deriveAgent(entry, input.resolveActorName, heldByRuling);
    if (agent.role === null) {
      unphased.push(agent);
      return;
    }
    const existing = phases.get(agent.role);
    if (existing) {
      existing.agents.push(agent);
      return;
    }
    phases.set(agent.role, {
      key: `role:${agent.role}`,
      title: codingSessionPhaseTitle(agent.role),
      origin: "seat",
      // Route order (§9.2): the lead first, the rest in hire order.
      rank: agent.role === "lead" ? -1 : index,
      at: 0,
      agents: [agent],
    });
  });

  // An assignment names a role before any seat may hold it.
  for (const row of input.transactions ?? []) {
    if (row.type !== "assignment") continue;
    const role = normalizeRole(row.assigneeRole);
    if (role === null || phases.has(role)) continue;
    phases.set(role, {
      key: `role:${role}`,
      title: codingSessionPhaseTitle(role),
      origin: "assigned",
      rank: Number.MAX_SAFE_INTEGER - 1,
      at: row.createdAt,
      agents: [],
    });
  }

  // A declaration no assignment owes yet exists only because it was declared.
  (input.declaredWork ?? []).forEach((declaration, index) => {
    if (declaration.state === "superseded") return;
    const bound = declaration.criteria.some(
      (criterion) => criterion.assignmentRefs.length > 0,
    );
    if (bound) return;
    const key = `declared:${declaration.workId}`;
    if (phases.has(key)) return;
    phases.set(key, {
      key,
      title: declarationTitle(declaration),
      origin: "declared",
      rank: Number.MAX_SAFE_INTEGER,
      at: index,
      agents: [],
    });
  });

  const ordered = [...phases.values()].sort(
    (left, right) => left.rank - right.rank || left.at - right.at,
  );
  const phaseRows: CodingSessionOrchestrationPhase[] = ordered.map((draft) => ({
    key: draft.key,
    title: draft.title,
    origin: draft.origin,
    agents: draft.agents,
    ...phaseState(draft.agents),
  }));

  const cards: CodingSessionOrchestrationCard[] = [];
  if (phaseRows.length > 0) {
    const everyone = [
      ...phaseRows.flatMap((phase) => phase.agents),
      ...unphased,
    ];
    cards.push({
      key: input.missionKey,
      title: input.title,
      phases: phaseRows,
      unphased,
      settled: everyone.filter((agent) => agent.settled).length,
      failed: everyone.filter((agent) => agent.state === "failed").length,
      total: everyone.length,
      live: everyone.some((agent) => ACTIVE_STATES.has(agent.state)),
    });
  }

  return {
    cards,
    spawns: input.subagents.rows.map(deriveCodingSessionOrchestrationSpawn),
    declaredWorkRead: input.declaredWork !== null,
    assignmentsRead: input.transactions !== null,
  };
}

/** `159k tok`, `≥ 12.4k tok`, or `tokens not reported` — never `0 tok`. */
export function formatCodingSessionAgentTokens(
  tokens: CodingSessionAgentTokens,
): string {
  if (tokens.kind === "not-reported") return "tokens not reported";
  const figure = formatCodingSessionSubagentTokens(tokens.total);
  return tokens.partial ? `≥ ${figure} tok` : `${figure} tok`;
}

/** Where the token figure came from, for its tooltip. */
export function describeCodingSessionAgentTokens(
  tokens: CodingSessionAgentTokens,
): string {
  if (tokens.kind === "not-reported") {
    return "No turn result in this seat's transcript reported token usage.";
  }
  return `Input + output tokens from ${tokens.reportedTurns} of ${tokens.turns} turn results in the transcript.`;
}

/** `N tool` / `N tools`: tool calls this client has seen in the transcript. */
export function formatCodingSessionToolCount(count: number | null): string {
  if (count === null) return "tools not reported";
  return `${count} ${count === 1 ? "tool" : "tools"}`;
}

/** The model chip text; `model not reported` when the metadata named none. */
export function formatCodingSessionAgentModel(model: string | null): string {
  return model ?? "model not reported";
}

/** A phase header's count line, in T3's words plus the honest extras. */
export function formatCodingSessionPhaseCounts(
  phase: CodingSessionOrchestrationPhase,
): string {
  if (phase.agents.length === 0) {
    if (phase.origin === "declared") return "declared · not assigned";
    if (phase.origin === "assigned") return "assigned · no seat yet";
    return "pending";
  }
  // "done" is completed only; a failure or a stop is counted in its own word.
  const done = phase.settledCount - phase.failedCount - phase.stoppedCount;
  const parts =
    phase.activeCount > 0
      ? [`${phase.activeCount} active`, `${done} done`]
      : [];
  if (phase.activeCount === 0 && done > 0) parts.push(`${done} done`);
  if (phase.failedCount > 0) parts.push(`${phase.failedCount} failed`);
  if (phase.stoppedCount > 0) parts.push(`${phase.stoppedCount} stopped`);
  if (phase.unknownCount > 0) parts.push(`${phase.unknownCount} unknown`);
  return parts.join(" · ");
}

/** The spawn row's meta line: model, tokens and tools, unknowns said. */
export function formatCodingSessionSpawnMeta(
  spawn: CodingSessionOrchestrationSpawn,
): string {
  return [
    formatCodingSessionAgentModel(spawn.model),
    spawn.totalTokens === null
      ? "tokens not reported"
      : `${formatCodingSessionSubagentTokens(spawn.totalTokens)} tok`,
    formatCodingSessionToolCount(spawn.toolCount),
  ].join(" · ");
}
