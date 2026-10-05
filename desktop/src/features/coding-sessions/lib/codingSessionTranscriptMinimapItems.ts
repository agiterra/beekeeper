/**
 * The transcript minimap's items and geometry (SV-26), ported from T3 Code's
 * `timelineMinimapItems.ts` and `MessagesTimeline.logic.ts:243-246, 313-439`.
 *
 * One item per turn that opens with a prompt. A turn the producer started
 * with no prompt (an autonomous continuation) gets no dash, exactly as T3
 * draws a dash only for a user message. Beyond T3, an item also carries what
 * the hover card says about the turn: who prompted it (DB12 colours it), its
 * duration, its changed files and whether it failed.
 *
 * Pure functions; the component owns the DOM.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  isCodingSessionTeamWakeCommandId,
  normalizeOperatorPubkey,
} from "./codingSessionPromptAttribution";
import type {
  CodingSessionTranscriptEntry,
  CodingSessionTranscriptModel,
  CodingSessionTranscriptTurn,
} from "./codingSessionTranscriptModelTypes";

/**
 * Whose prompt opened the turn, as far as the item vouches.
 *
 * - `you`: stamped with the viewer's own key and no automatic prefix.
 * - `other`: stamped with somebody else's key (a person or a seat).
 * - `automatic`: a team wake the app published; nobody typed it.
 * - `unrecorded`: no stamp at all — unknown, never assumed to be the viewer.
 */
export type CodingSessionMinimapAuthorKind =
  | "you"
  | "other"
  | "automatic"
  | "unrecorded";

export type CodingSessionMinimapItem = {
  /** The prompt item's id: stable across layouts and remounts. */
  readonly id: string;
  /** The row or block key the transcript renders the turn under. */
  readonly key: string;
  /** The turn's row index in its list (the virtualizer's index). */
  readonly rowIndex: number;
  readonly userText: string | null;
  readonly assistantText: string | null;
  readonly authorKind: CodingSessionMinimapAuthorKind;
  /** The stamped operator, when there is one. */
  readonly authorPubkey: string | null;
  /** When the turn began, in ms; null when nothing dates it. */
  readonly startedAtMs: number | null;
  /** The turn's own measured duration; null when unreported. */
  readonly durationMs: number | null;
  readonly changedFileCount: number;
  /** The first three changed files' names, in the turn's order. */
  readonly changedFileNames: readonly string[];
  /** The turn reported a failed completion. */
  readonly failed: boolean;
  /** The turn is the one being worked on now. */
  readonly working: boolean;
};

/** One turn and where the transcript renders it. */
export type CodingSessionMinimapTurnSource = {
  key: string;
  rowIndex: number;
  turn: CodingSessionTranscriptTurn;
  /** The turn's start when the turn itself carries none (umbrella blocks). */
  fallbackStartedAtMs?: number | null;
};

/** How many changed file names the card lists before "and N more". */
export const CODING_SESSION_MINIMAP_CARD_FILE_NAMES = 3;

function entryItems(entry: CodingSessionTranscriptEntry): TranscriptItem[] {
  if (entry.kind === "item") return [entry.item];
  if (entry.kind === "tool-group") return entry.items;
  return [];
}

function parseTime(value: string | null | undefined): number | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
}

/** Classify a prompt's author for DB12's colours. */
export function resolveCodingSessionMinimapAuthor(
  prompt: Pick<
    Extract<TranscriptItem, { type: "message" }>,
    "operatorPubkey" | "commandId"
  >,
  currentUserPubkey: string | null,
): { kind: CodingSessionMinimapAuthorKind; pubkey: string | null } {
  if (isCodingSessionTeamWakeCommandId(prompt.commandId)) {
    return { kind: "automatic", pubkey: null };
  }
  const operator = normalizeOperatorPubkey(prompt.operatorPubkey);
  if (operator === null) return { kind: "unrecorded", pubkey: null };
  const viewer = normalizeOperatorPubkey(currentUserPubkey);
  return {
    kind: viewer !== null && viewer === operator ? "you" : "other",
    pubkey: operator,
  };
}

/**
 * The minimap item for one turn, or null when the turn has no prompt.
 *
 * The reply is the turn's last assistant message, as T3 previews the final
 * assistant text before the next prompt. A synthesized Turn result body
 * (`:assistant-result`) is the provider's closing word, not the agent's
 * answer: it is the reply only when the agent wrote no prose of its own, the
 * same rule the transcript's answer block follows
 * (`findCodingSessionAnswerIndex`).
 */
export function deriveCodingSessionMinimapItem(
  source: CodingSessionMinimapTurnSource,
  currentUserPubkey: string | null,
): CodingSessionMinimapItem | null {
  const { turn } = source;
  let prompt: Extract<TranscriptItem, { type: "message" }> | null = null;
  let reply: string | null = null;
  let resultBody: string | null = null;
  for (const entry of turn.entries) {
    for (const item of entryItems(entry)) {
      if (item.type !== "message") continue;
      if (item.role === "user") {
        prompt ??= item;
      } else if (prompt !== null && item.text?.trim()) {
        if (item.id.endsWith(":assistant-result")) resultBody = item.text;
        else reply = item.text;
      }
    }
  }
  reply ??= resultBody;
  if (prompt === null) return null;
  const author = resolveCodingSessionMinimapAuthor(prompt, currentUserPubkey);
  const durationMs =
    turn.completion?.durationMs ?? turn.fold?.durationMs ?? null;
  return {
    id: prompt.id,
    key: source.key,
    rowIndex: source.rowIndex,
    userText: prompt.text,
    assistantText: reply,
    authorKind: author.kind,
    authorPubkey: author.pubkey,
    startedAtMs:
      parseTime(turn.startedAt) ??
      parseTime(prompt.timestamp) ??
      source.fallbackStartedAtMs ??
      null,
    durationMs,
    changedFileCount: turn.changedFiles.length,
    changedFileNames: turn.changedFiles
      .slice(0, CODING_SESSION_MINIMAP_CARD_FILE_NAMES)
      .map((file) => file.filename || file.path),
    failed: turn.completion?.state === "failed",
    working: turn.isWorking,
  };
}

/**
 * The single-session layout's items: one per turn row with a prompt, keyed
 * by the row key `buildCodingSessionTranscriptRows` gives it (`turn:<id>`),
 * with the row's index for the virtualizer.
 */
export function deriveCodingSessionMinimapItemsFromModel(
  model: CodingSessionTranscriptModel,
  currentUserPubkey: string | null,
): CodingSessionMinimapItem[] {
  const items: CodingSessionMinimapItem[] = [];
  model.blocks.forEach((block, rowIndex) => {
    if (block.kind !== "turn") return;
    const item = deriveCodingSessionMinimapItem(
      { key: `turn:${block.id}`, rowIndex, turn: block },
      currentUserPubkey,
    );
    if (item) items.push(item);
  });
  return items;
}

/** Many turns, already located, in reading order. */
export function deriveCodingSessionMinimapItems(
  sources: readonly CodingSessionMinimapTurnSource[],
  currentUserPubkey: string | null,
): CodingSessionMinimapItem[] {
  const items: CodingSessionMinimapItem[] = [];
  for (const source of sources) {
    const item = deriveCodingSessionMinimapItem(source, currentUserPubkey);
    if (item) items.push(item);
  }
  return items;
}

function compactPreview(text: string | null | undefined): string | null {
  const compact = text?.replace(/\s+/g, " ").trim() ?? "";
  return compact.length > 0 ? compact : null;
}

/** The card's prompt line: the prompt's first non-empty line, compacted. */
export function codingSessionMinimapPromptLine(
  text: string | null | undefined,
): string | null {
  const first = (text ?? "").split("\n").find((line) => line.trim() !== "");
  return compactPreview(first);
}

/** The card's reply start: whitespace collapsed, as T3 previews it. */
export function codingSessionMinimapReplyPreview(
  text: string | null | undefined,
): string | null {
  return compactPreview(text);
}

// --- Geometry, ported from T3 (`MessagesTimeline.logic.ts`). -------------

const MINIMAP_ITEM_SPACING = 8;
/** T3 shows the minimap from two items. */
export const CODING_SESSION_MINIMAP_MIN_ITEMS = 2;
const MINIMAP_MAX_HEIGHT_CSS = "calc(100vh - 18rem)";
const MINIMAP_PERSISTENT_GUTTER = 48;
const MINIMAP_HIT_STRIP_LEFT = 12;
const MINIMAP_HIT_STRIP_MAX_WIDTH = 40;
const MINIMAP_EXPANDED_HIT_STRIP_WIDTH = "22rem";
const MINIMAP_NAVIGATION_REACH = 14;

export function codingSessionMinimapHeightStyle(itemCount: number): string {
  const natural = Math.max(1, (itemCount - 1) * MINIMAP_ITEM_SPACING);
  return `min(${natural}px, ${MINIMAP_MAX_HEIGHT_CSS})`;
}

export function codingSessionMinimapTopPercent(
  index: number,
  itemCount: number,
): number {
  if (itemCount <= 1) return 0;
  return (Math.max(0, Math.min(index, itemCount - 1)) / (itemCount - 1)) * 100;
}

export function codingSessionMinimapIndexFromPointer(input: {
  itemCount: number;
  railTop: number;
  railHeight: number;
  pointerY: number;
}): number | null {
  if (input.itemCount <= 0 || input.railHeight <= 0) return null;
  if (input.itemCount === 1) return 0;
  const progress = Math.max(
    0,
    Math.min(1, (input.pointerY - input.railTop) / input.railHeight),
  );
  return Math.max(
    0,
    Math.min(input.itemCount - 1, Math.round(progress * (input.itemCount - 1))),
  );
}

/** The first item in view, else the last one above the view. */
export function codingSessionMinimapCurrentIndex(input: {
  scrollTop: number;
  scrollBottom: number;
  itemBounds: ReadonlyArray<{ top: number | null; height: number | null }>;
}): number | null {
  let preceding: number | null = null;
  for (const [index, item] of input.itemBounds.entries()) {
    if (item.top === null) continue;
    if (codingSessionMinimapBoundsInView(item, input)) return index;
    if (item.top <= input.scrollTop) preceding = index;
  }
  return preceding;
}

export function codingSessionMinimapBoundsInView(
  item: { top: number | null; height: number | null },
  view: { scrollTop: number; scrollBottom: number },
): boolean {
  return (
    item.top !== null &&
    item.top < view.scrollBottom &&
    item.top + Math.max(1, item.height ?? 1) > view.scrollTop
  );
}

function sideGutter(viewportWidth: number, contentWidth: number): number {
  if (
    !Number.isFinite(viewportWidth) ||
    viewportWidth <= 0 ||
    !Number.isFinite(contentWidth)
  ) {
    return 0;
  }
  return Math.max(
    0,
    (viewportWidth - Math.min(viewportWidth, contentWidth)) / 2,
  );
}

/** T3's 48 px rule: a gutter that wide keeps the minimap always visible. */
export function codingSessionMinimapHasPersistentGutter(
  viewportWidth: number,
  contentWidth: number,
): boolean {
  return sideGutter(viewportWidth, contentWidth) >= MINIMAP_PERSISTENT_GUTTER;
}

/**
 * The hover strip's width, capped at 40 px and at the gutter, so it never
 * covers the content column's text; 0 disables it.
 */
export function codingSessionMinimapHitStripWidth(
  viewportWidth: number,
  contentWidth: number,
): number {
  return Math.max(
    0,
    Math.min(
      MINIMAP_HIT_STRIP_MAX_WIDTH,
      Math.floor(sideGutter(viewportWidth, contentWidth)) -
        MINIMAP_HIT_STRIP_LEFT,
    ),
  );
}

export function codingSessionMinimapNavigationInteractive(
  collapsedWidth: number,
): boolean {
  return collapsedWidth >= MINIMAP_NAVIGATION_REACH;
}

export function codingSessionMinimapInteractiveWidth(
  collapsedWidth: number,
  expanded: boolean,
): number | string {
  return expanded ? MINIMAP_EXPANDED_HIT_STRIP_WIDTH : collapsedWidth;
}
