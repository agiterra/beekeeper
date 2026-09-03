import {
  buildFileEditDiff,
  type FileEditDiff,
} from "@/features/agents/ui/agentSessionFileEditDiff";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { getToolString } from "@/features/agents/ui/agentSessionUtils";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { hasRedactionMarker } from "@/shared/lib/redactionMarker";

export type CodingSessionTurnCompletion = {
  durationMs: number | null;
  costUsd: number | null;
  outcome: string | null;
  timestamp: string;
  state: "completed" | "failed" | "interrupted";
};

export type CodingSessionTranscriptEntry =
  | { kind: "item"; item: TranscriptItem }
  | {
      kind: "tool-group";
      id: string;
      label: string;
      items: Extract<TranscriptItem, { type: "tool" }>[];
    };

export type CodingSessionTranscriptTurn = {
  kind: "turn";
  id: string;
  entries: CodingSessionTranscriptEntry[];
  changedFiles: CodingSessionChangedFile[];
  diagnostics: TranscriptItem[];
  completion: CodingSessionTurnCompletion | null;
  isWorking: boolean;
  startedAt: string | null;
};

export type CodingSessionChangedFile = {
  path: string;
  filename: string;
  additions: number | null;
  deletions: number | null;
  diffs: CodingSessionChangedFileDiff[];
  editCount: number;
};

export type CodingSessionChangedFileDiff = FileEditDiff & {
  id: string;
};

export type CodingSessionTranscriptStandalone = {
  kind: "standalone";
  id: string;
  entry: CodingSessionTranscriptEntry;
};

export type CodingSessionTranscriptBlock =
  | CodingSessionTranscriptTurn
  | CodingSessionTranscriptStandalone;

export type CodingSessionTranscriptModel = {
  blocks: CodingSessionTranscriptBlock[];
  diagnostics: TranscriptItem[];
};

/**
 * Lifecycle rows that belong in the diagnostics rail rather than the reading
 * order. "Content dropped" is deliberately absent: a dropped item is something
 * the reader must see in place, not a telemetry line.
 *
 * "Status" covers the provider's generic status slugs. The continuity slugs
 * are deliberately NOT among them: `buildStatusLifecycleItem` gives those the
 * `CODING_SESSION_CONTINUITY_TITLE` title instead, precisely so this gate
 * misses them and they stay in the reading order.
 */
const DIAGNOSTIC_LIFECYCLE_TITLES = new Set([
  "Account Info",
  "Context compact boundary",
  "Context compacted",
  "Context Window Updated",
  "Context cleared",
  "Status",
  "System Init",
  "Unrecognized transcript event",
  "Unrecognized transcript item",
]);

type MutableTurn = {
  id: string;
  items: TranscriptItem[];
};

export function deriveCodingSessionTranscriptModel(
  transcript: TranscriptItem[],
  options: { isWorking: boolean },
): CodingSessionTranscriptModel {
  const rawOrdered: Array<MutableTurn | TranscriptItem> = [];
  const turnsById = new Map<string, MutableTurn>();

  for (const item of transcript) {
    if (isSystemInitMetadata(item)) continue;

    if (!item.turnId) {
      rawOrdered.push(item);
      continue;
    }

    let turn = turnsById.get(item.turnId);
    if (!turn) {
      turn = { id: item.turnId, items: [] };
      turnsById.set(item.turnId, turn);
      rawOrdered.push(turn);
    }
    turn.items.push(item);
  }

  const ordered = coalesceStandaloneSettledTurns(rawOrdered);
  const lastTurn = [...ordered]
    .reverse()
    .find((candidate): candidate is MutableTurn => isMutableTurn(candidate));
  const blocks: CodingSessionTranscriptBlock[] = [];
  const diagnostics: TranscriptItem[] = [];

  for (const candidate of ordered) {
    if (isMutableTurn(candidate)) {
      const turn = deriveTurn(
        candidate,
        options.isWorking && candidate === lastTurn,
      );
      if (
        turn.entries.length > 0 ||
        turn.diagnostics.length > 0 ||
        turn.completion ||
        turn.isWorking
      ) {
        blocks.push(turn);
      }
      continue;
    }

    if (isDiagnosticItem(candidate)) {
      if (!isCeremonialDiagnostic(candidate)) diagnostics.push(candidate);
      continue;
    }
    blocks.push({
      kind: "standalone",
      id: candidate.id,
      entry: { kind: "item", item: candidate },
    });
  }

  return { blocks, diagnostics };
}

/**
 * Reconstructs only presentation-safe settled turns when the projection has
 * no authoritative provider turn id.
 *
 * Historical Hive streams can begin after a user prompt has already crossed
 * the bridge, leaving a same-generation sequence such as:
 *
 *   system init -> assistant text -> context telemetry -> terminal result
 *
 * All four entries are otherwise standalone, so the terminal result repeats
 * the assistant prose as a second visible row. A signed terminal event is the
 * one safe boundary available here: buffer only unscoped items from the same
 * session generation, promote them to one display turn when that terminal
 * arrives, and leave unterminated buffers exactly as standalone items. Never
 * cross a generation or an existing explicit turn.
 */
function coalesceStandaloneSettledTurns(
  ordered: Array<MutableTurn | TranscriptItem>,
): Array<MutableTurn | TranscriptItem> {
  const coalesced: Array<MutableTurn | TranscriptItem> = [];

  for (let index = 0; index < ordered.length; index += 1) {
    const candidate = ordered[index];
    if (!candidate) continue;
    if (isMutableTurn(candidate)) {
      coalesced.push(candidate);
      continue;
    }

    const sessionId = candidate.sessionId?.trim() ?? "";
    if (!sessionId) {
      coalesced.push(candidate);
      continue;
    }

    const items = [candidate];
    let terminalIndex = isTurnTerminal(candidate) ? index : -1;
    let cursor = index + 1;
    while (terminalIndex < 0 && cursor < ordered.length) {
      const next = ordered[cursor];
      if (
        !next ||
        isMutableTurn(next) ||
        next.sessionId?.trim() !== sessionId
      ) {
        break;
      }
      items.push(next);
      if (isTurnTerminal(next)) terminalIndex = cursor;
      cursor += 1;
    }

    if (terminalIndex >= 0) {
      const terminal = items.at(-1);
      if (!terminal) {
        coalesced.push(candidate);
        continue;
      }
      coalesced.push({
        id: `settled:${terminal.id}`,
        items,
      });
      index = terminalIndex;
    } else {
      coalesced.push(candidate);
    }
  }

  return coalesced;
}

function deriveTurn(
  turn: MutableTurn,
  canBeWorking: boolean,
): CodingSessionTranscriptTurn {
  const visible: TranscriptItem[] = [];
  const diagnostics: TranscriptItem[] = [];
  const assistantResultEchoes = new Set(
    turn.items.flatMap((item) =>
      item.type === "message" && item.role === "assistant"
        ? [normalizeContent(item.text)]
        : [],
    ),
  );
  let completion: CodingSessionTurnCompletion | null = null;

  for (const item of turn.items) {
    if (isTurnResult(item)) {
      // Structured `durationMs`/`costUsd` on the item are authoritative. The
      // regex parse remains only for already-published events whose builders
      // baked the metrics into the display text; on a structured item the
      // text carries no suffixes, so parsing it is a harmless trim.
      const result = parseTurnResult(item.text);
      completion = {
        durationMs:
          typeof item.durationMs === "number"
            ? item.durationMs
            : result.durationMs,
        costUsd:
          typeof item.costUsd === "number" ? item.costUsd : result.costUsd,
        outcome: item.outcome?.trim() || null,
        timestamp: item.timestamp,
        state: isErrorItem(item) ? "failed" : "completed",
      };

      if (isErrorItem(item)) {
        visible.push({
          ...item,
          text: result.body || item.text,
        });
      } else if (
        result.body &&
        !isCeremonialCompletionValue(result.body) &&
        !assistantResultEchoes.has(normalizeContent(result.body))
      ) {
        visible.push({
          id: `${item.id}:assistant-result`,
          type: "message",
          renderClass: "message",
          role: "assistant",
          title: "Assistant",
          text: result.body,
          timestamp: item.timestamp,
          turnId: item.turnId,
          sessionId: item.sessionId,
          channelId: item.channelId,
          bridgeSource: item.bridgeSource,
        });
      }
      continue;
    }

    if (isInterrupted(item)) {
      completion = {
        durationMs: null,
        costUsd: null,
        outcome: "interrupted",
        timestamp: item.timestamp,
        state: "interrupted",
      };
      continue;
    }

    if (isDiagnosticItem(item)) {
      if (isErrorItem(item)) {
        visible.push(item);
      } else if (!isCeremonialDiagnostic(item)) {
        diagnostics.push(item);
      }
      continue;
    }

    visible.push(item);
  }

  return {
    kind: "turn",
    id: turn.id,
    entries: groupAdjacentTools(visible),
    changedFiles: deriveCodingSessionChangedFiles(turn.items),
    diagnostics,
    completion,
    isWorking: canBeWorking && completion === null,
    startedAt: deriveTurnStartedAt(turn.items),
  };
}

/**
 * What the Observed-changes surface learned from a transcript.
 *
 * `files` is what can be named. `unreportedEditCount` is the honest remainder:
 * edits the transcript *does* record, for which no producer published a path.
 * The two are kept apart deliberately — folding the remainder into `files`
 * would invent a filename, and dropping it renders presence as absence, which
 * is what the 2026-08-29 walk found the surface doing over sixteen real edits.
 */
export type CodingSessionObservedChanges = {
  files: CodingSessionChangedFile[];
  unreportedEditCount: number;
};

/**
 * Fold a transcript's completed edits into per-file changes.
 *
 * The files half of {@link deriveCodingSessionObservedChanges}; kept as its own
 * export because the per-turn model only ever renders named files.
 */
export function deriveCodingSessionChangedFiles(
  items: TranscriptItem[],
): CodingSessionChangedFile[] {
  return deriveCodingSessionObservedChanges(items).files;
}

/**
 * Fold a transcript's completed edits into per-file changes *and* a count of
 * the edits that named no file.
 *
 * An item counts as an edit when the classifier recognized it as a file edit
 * **or** when the producer published ACP's own `edit` discriminant. The second
 * test is what makes real sessions work: claude-agent-acp names its editor
 * `Edit` — and, while the tool's arguments are still streaming, `Preparing
 * file…` — and no name rule in the classifier matches either, so a session's
 * every edit classified as "generic" and the fold never saw it.
 *
 * A path is taken from the diff, then the tool's arguments, then the
 * producer's published `edit.paths`, then the descriptor's object. An edit that
 * yields none is counted in `unreportedEditCount` rather than dropped.
 */
export function deriveCodingSessionObservedChanges(
  items: TranscriptItem[],
): CodingSessionObservedChanges {
  type MutableChangedFile = {
    path: string;
    filename: string;
    diffs: CodingSessionChangedFileDiff[];
    editCount: number;
    countedEditCount: number;
  };
  const files = new Map<string, MutableChangedFile>();
  let unreportedEditCount = 0;

  for (const item of items) {
    if (
      item.type !== "tool" ||
      item.isError ||
      item.status !== "completed" ||
      !isObservedFileEdit(item)
    ) {
      continue;
    }

    const diff = buildFileEditDiff(item, item.descriptor);
    const path =
      diff?.path ??
      getToolString(item.args, [
        "path",
        "file",
        "file_path",
        "filePath",
        "target_file",
      ]) ??
      item.editPaths?.[0] ??
      item.descriptor.object ??
      null;
    const normalizedPath = path ? normalizeChangedFilePath(path) : "";
    if (!normalizedPath) {
      unreportedEditCount += 1;
      continue;
    }
    const key = normalizedPath;
    const existing = files.get(key) ?? {
      path: normalizedPath,
      filename: changedFileBasename(normalizedPath),
      diffs: [],
      editCount: 0,
      countedEditCount: 0,
    };
    existing.editCount += 1;
    if (diff) {
      existing.diffs.push({ ...diff, id: item.id, path: normalizedPath });
      existing.countedEditCount += 1;
    }
    files.set(key, existing);
  }

  return {
    files: [...files.values()].map((file) => {
      const allEditsCounted =
        file.editCount > 0 && file.countedEditCount === file.editCount;
      return {
        path: file.path,
        filename: file.filename,
        additions: allEditsCounted
          ? file.diffs.reduce((total, diff) => total + diff.additions, 0)
          : null,
        deletions: allEditsCounted
          ? file.diffs.reduce((total, diff) => total + diff.deletions, 0)
          : null,
        diffs: file.diffs,
        editCount: file.editCount,
      };
    }),
    unreportedEditCount,
  };
}

/**
 * Whether a completed tool item is a file edit — by classification, or by the
 * producer's published ACP discriminant.
 */
function isObservedFileEdit(
  item: Extract<TranscriptItem, { type: "tool" }>,
): boolean {
  return (
    item.descriptor.renderClass === "file-edit" || item.toolKind === "edit"
  );
}

/**
 * Normalize a candidate path, or reject it as no path at all.
 *
 * Found live 2026-09-01 at 12:36: `FILES` printed
 * `[elided private context: 183 bytes, sha256:b35397…]` in the path slot and
 * `CHANGES` counted it as a second *named* edit. The provider redacts a host
 * path before signing, and the marker it leaves behind is the provider saying
 * "I had this and chose not to publish it" — which is precisely an edit with
 * no reported file name, not a file with a 90-character name.
 *
 * The empty string is the caller's existing signal for that, so the marker
 * takes the `unreportedEditCount` branch the field's own doc comment
 * describes. The marker's shape is owned by `shared/lib/redactionMarker` and
 * read from there; a second regex here would be a second definition of the
 * privacy contract, which is how the two readers drift apart.
 *
 * The predicate is a *contains* test on purpose: a marker anywhere in the
 * candidate makes the whole candidate unusable as a path. A real path that
 * merely contains the word `elided` matches nothing and is untouched.
 */
function normalizeChangedFilePath(path: string): string {
  if (hasRedactionMarker(path)) return "";
  return path
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\.\/+/, "");
}

function changedFileBasename(path: string): string {
  return path.split("/").at(-1) || path;
}

function deriveTurnStartedAt(items: TranscriptItem[]): string | null {
  const prompt = items.find(
    (item) => item.type === "message" && item.role === "user",
  );
  if (prompt) return prompt.timestamp;

  const first = items[0];
  if (!first) return null;
  if (first.type === "tool") return first.startedAt || first.timestamp;
  return first.timestamp;
}

function groupAdjacentTools(
  items: TranscriptItem[],
): CodingSessionTranscriptEntry[] {
  const narrativeItems = coalescePlanSnapshots(items);
  const visibleToolTail = 3;
  const entries: CodingSessionTranscriptEntry[] = [];

  for (let index = 0; index < narrativeItems.length; index += 1) {
    const item = narrativeItems[index];
    if (
      !isCompletedSuccessfulTool(item) ||
      deriveCodingSessionTaskModel([item]) !== null
    ) {
      entries.push({ kind: "item", item });
      continue;
    }

    const tools = [item];
    let cursor = index + 1;
    while (cursor < narrativeItems.length) {
      const candidate = narrativeItems[cursor];
      if (
        !candidate ||
        !isCompletedSuccessfulTool(candidate) ||
        deriveCodingSessionTaskModel([candidate]) !== null
      )
        break;
      tools.push(candidate);
      cursor += 1;
    }

    const previousTools = tools.slice(0, -visibleToolTail);
    const recentTools = tools.slice(-visibleToolTail);
    if (previousTools.length > 0) {
      entries.push({
        kind: "tool-group",
        id: `tools:${tools[0].id}`,
        label: formatToolGroupLabel(previousTools),
        items: previousTools,
      });
    }
    entries.push(
      ...recentTools.map(
        (tool): CodingSessionTranscriptEntry => ({ kind: "item", item: tool }),
      ),
    );
    index = cursor - 1;
  }

  return entries;
}

/** Keep a plan's first narrative position while replacing it with its latest snapshot. */
function coalescePlanSnapshots(items: TranscriptItem[]): TranscriptItem[] {
  const planIndexes = items.flatMap((item, index) =>
    isRenderablePlanSnapshot(item) ? [index] : [],
  );
  if (planIndexes.length < 2) return items;

  const firstPlanIndex = planIndexes[0];
  const latestPlan = items[planIndexes.at(-1) ?? firstPlanIndex];
  const planIndexSet = new Set(planIndexes);
  return items.flatMap((item, index) => {
    if (index === firstPlanIndex) return [latestPlan];
    return planIndexSet.has(index) ? [] : [item];
  });
}

function isRenderablePlanSnapshot(item: TranscriptItem): boolean {
  if (deriveCodingSessionTaskModel([item]) === null) return false;
  return item.type === "plan" || isCompletedSuccessfulTool(item);
}

function formatToolGroupLabel(
  tools: Extract<TranscriptItem, { type: "tool" }>[],
): string {
  const classes = new Set(
    tools.map((tool) => tool.descriptor?.renderClass ?? tool.renderClass),
  );
  const count = tools.length;
  if (classes.size === 1 && classes.has("shell")) {
    return `Ran ${count} ${count === 1 ? "command" : "commands"}`;
  }
  if (classes.size === 1 && classes.has("file-read")) {
    return `Read ${count} ${count === 1 ? "file" : "files"}`;
  }
  if (classes.size === 1 && classes.has("file-edit")) {
    return `Edited ${count} ${count === 1 ? "file" : "files"}`;
  }
  return `Ran ${count} tool ${count === 1 ? "call" : "calls"}`;
}

function isMutableTurn(
  value: MutableTurn | TranscriptItem,
): value is MutableTurn {
  return "items" in value;
}

export function isCompletedSuccessfulCodingSessionTool(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "tool" }> {
  return isCompletedSuccessfulTool(item);
}

function isCompletedSuccessfulTool(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "tool" }> {
  return (
    item.type === "tool" &&
    !item.isError &&
    item.status === "completed" &&
    item.renderClass !== "error"
  );
}

function isTurnResult(
  item: TranscriptItem,
): item is Extract<TranscriptItem, { type: "lifecycle" }> {
  return item.type === "lifecycle" && item.title === "Turn result";
}

function isInterrupted(item: TranscriptItem): boolean {
  return item.type === "lifecycle" && item.title === "Interrupted";
}

function isTurnTerminal(item: TranscriptItem): boolean {
  return isTurnResult(item) || isInterrupted(item);
}

export function isCodingSessionTranscriptError(item: TranscriptItem): boolean {
  return isErrorItem(item);
}

function isErrorItem(item: TranscriptItem): boolean {
  if (isSystemInitMetadata(item)) return false;
  if (item.type === "tool") {
    return (
      item.isError || item.status === "failed" || item.renderClass === "error"
    );
  }
  if (item.type !== "lifecycle") return false;
  const searchable = `${item.title} ${item.text}`.toLowerCase();
  return (
    item.renderClass === "error" ||
    /\b(error|failed|failure)\b/.test(searchable)
  );
}

function isSystemInitMetadata(item: TranscriptItem): boolean {
  return (
    item.type === "lifecycle" &&
    item.title === "System Init" &&
    item.renderClass === "status"
  );
}

function isCeremonialDiagnostic(item: TranscriptItem): boolean {
  return item.type === "lifecycle" && item.title === "Context Window Updated";
}

function isDiagnosticItem(item: TranscriptItem): boolean {
  if (item.renderClass === "raw-rail" || item.renderClass === "suppressed") {
    return true;
  }
  if (item.type !== "lifecycle") return false;
  if (item.title.startsWith("Unrecognized item kind:")) return true;
  return DIAGNOSTIC_LIFECYCLE_TITLES.has(item.title);
}

type ParsedTurnResult = {
  body: string;
  durationMs: number | null;
  costUsd: number | null;
};

export function parseCodingSessionTurnResult(text: string): ParsedTurnResult {
  return parseTurnResult(text);
}

function parseTurnResult(text: string): ParsedTurnResult {
  let body = text.trim();
  let costUsd: number | null = null;
  let durationMs: number | null = null;

  const costMatch = body.match(/\s*\(\$([0-9]+(?:\.[0-9]+)?)\)\s*$/);
  if (costMatch) {
    costUsd = Number(costMatch[1]);
    body = body.slice(0, costMatch.index).trimEnd();
  }

  const durationMatch = body.match(/\s*\(([0-9]+(?:\.[0-9]+)?)ms\)\s*$/);
  if (durationMatch) {
    durationMs = Number(durationMatch[1]);
    body = body.slice(0, durationMatch.index).trimEnd();
  }

  return {
    body,
    durationMs: Number.isFinite(durationMs) ? durationMs : null,
    costUsd: Number.isFinite(costUsd) ? costUsd : null,
  };
}

export function formatCodingSessionDuration(durationMs: number): string {
  const seconds = Math.max(0, durationMs) / 1_000;
  if (seconds < 1) return `${Math.round(durationMs)}ms`;
  if (seconds < 10) return `${seconds.toFixed(1)}s`;
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = Math.round(seconds % 60);
  return remainder > 0 ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

export function formatCodingSessionCompletionOutcome(
  completion: CodingSessionTurnCompletion,
): string | null {
  const outcome = completion.outcome?.trim();
  if (!outcome) return null;
  const normalized = outcome.toLowerCase();
  if (isCeremonialCompletionValue(normalized)) return null;
  return normalized.replace(/[_-]+/g, " ");
}

function isCeremonialCompletionValue(value: string): boolean {
  return COMPLETION_CEREMONY_VALUES.has(value.trim().toLowerCase());
}

const COMPLETION_CEREMONY_VALUES = new Set([
  "completed",
  "error",
  "failed",
  "interrupted",
  "result",
  "success",
  "unknown",
]);

export function formatCodingSessionCost(costUsd: number): string {
  if (costUsd < 0.01) return `$${costUsd.toFixed(4)}`;
  return `$${costUsd.toFixed(2)}`;
}

/**
 * Reuses unchanged block objects across append-only transcript updates.
 *
 * The signed projection store emits a new transcript array for every frame.
 * Re-derivation remains linear and deterministic, but stable block identities
 * let memoized turn rows skip all prior DOM/Markdown/tool work when only the
 * live tail changed. Full list virtualization remains intentionally deferred:
 * variable-height Markdown, nested disclosures, and anchored-scroll restoration
 * need one shared virtualizer contract before rows can be safely unmounted.
 */
export function stabilizeCodingSessionTranscriptModel(
  previous: CodingSessionTranscriptModel | null,
  next: CodingSessionTranscriptModel,
): CodingSessionTranscriptModel {
  if (!previous) return next;

  const blocks = next.blocks.map((block, index) => {
    const prior = previous.blocks[index];
    return prior && transcriptBlocksEqual(prior, block) ? prior : block;
  });
  const diagnostics = arraysReferenceEqual(
    previous.diagnostics,
    next.diagnostics,
  )
    ? previous.diagnostics
    : next.diagnostics;

  if (
    arraysReferenceEqual(previous.blocks, blocks) &&
    diagnostics === previous.diagnostics
  ) {
    return previous;
  }
  return { blocks, diagnostics };
}

function transcriptBlocksEqual(
  left: CodingSessionTranscriptBlock,
  right: CodingSessionTranscriptBlock,
): boolean {
  if (left.kind !== right.kind || left.id !== right.id) return false;
  if (left.kind === "standalone" && right.kind === "standalone") {
    return transcriptEntriesEqual(left.entry, right.entry);
  }
  if (left.kind !== "turn" || right.kind !== "turn") return false;
  return (
    left.isWorking === right.isWorking &&
    left.startedAt === right.startedAt &&
    completionsEqual(left.completion, right.completion) &&
    changedFilesEqual(left.changedFiles, right.changedFiles) &&
    arraysReferenceEqual(left.diagnostics, right.diagnostics) &&
    left.entries.length === right.entries.length &&
    left.entries.every((entry, index) =>
      transcriptEntriesEqual(entry, right.entries[index]),
    )
  );
}

function changedFilesEqual(
  left: CodingSessionChangedFile[],
  right: CodingSessionChangedFile[],
): boolean {
  return (
    left.length === right.length &&
    left.every((file, index) => {
      const other = right[index];
      return (
        other !== undefined &&
        file.path === other.path &&
        file.filename === other.filename &&
        file.additions === other.additions &&
        file.deletions === other.deletions &&
        file.editCount === other.editCount &&
        file.diffs.length === other.diffs.length &&
        file.diffs.every((diff, diffIndex) => {
          const otherDiff = other.diffs[diffIndex];
          return (
            otherDiff !== undefined &&
            diff.id === otherDiff.id &&
            diff.path === otherDiff.path &&
            diff.filename === otherDiff.filename &&
            diff.additions === otherDiff.additions &&
            diff.deletions === otherDiff.deletions &&
            diff.lines.length === otherDiff.lines.length &&
            diff.lines.every((line, lineIndex) => {
              const otherLine = otherDiff.lines[lineIndex];
              return (
                otherLine !== undefined &&
                line.kind === otherLine.kind &&
                line.text === otherLine.text
              );
            })
          );
        })
      );
    })
  );
}

function transcriptEntriesEqual(
  left: CodingSessionTranscriptEntry,
  right: CodingSessionTranscriptEntry | undefined,
): boolean {
  if (!right || left.kind !== right.kind) return false;
  if (left.kind === "item" && right.kind === "item") {
    return left.item === right.item;
  }
  if (left.kind !== "tool-group" || right.kind !== "tool-group") return false;
  return (
    left.id === right.id &&
    left.label === right.label &&
    arraysReferenceEqual(left.items, right.items)
  );
}

function completionsEqual(
  left: CodingSessionTurnCompletion | null,
  right: CodingSessionTurnCompletion | null,
): boolean {
  if (left === right) return true;
  if (!left || !right) return false;
  return (
    left.durationMs === right.durationMs &&
    left.costUsd === right.costUsd &&
    left.outcome === right.outcome &&
    left.timestamp === right.timestamp &&
    left.state === right.state
  );
}

function arraysReferenceEqual<T>(
  left: readonly T[],
  right: readonly T[],
): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function normalizeContent(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}
