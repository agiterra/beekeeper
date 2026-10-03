/**
 * Per-kind mapping: one decoded transcript item -> one `TranscriptItem`.
 *
 * Split out of `codingSessionTranscriptProjection.ts`, which owns the envelope
 * layer (identity, turn presentation, tool pairing) and calls into these.
 * Every builder is TOTAL and bounded: it reads defensively, never assumes a
 * field is present or well-typed, and never throws.
 */

import { normalizeToolNameText } from "@/features/agents/ui/agentSessionToolCatalog";
import { classifyTool } from "@/features/agents/ui/agentSessionToolClassifier";
import type {
  ToolStatus,
  TranscriptItem,
} from "@/features/agents/ui/agentSessionTypes";
import { containsAttachmentReference } from "./codingSessionAttachmentReference";
import { readCostBasis } from "./codingSessionCostBasis";
import {
  boundEntries,
  capArray,
  isPrimitive,
  isQuarantineItem,
  isRecord,
  MAX_METADATA_ARRAY_ITEM_LENGTH,
  MAX_METADATA_ARRAY_ITEMS,
  MAX_METADATA_FIELD_LENGTH,
  normalizeToolResultContent,
  safeString,
  safeStringArray,
  stringifyToolResultContent,
} from "./codingSessionDefensive";
import { formatRedactedBytes } from "@/shared/lib/redactionMarker";

import { codingSessionBoundaryRow } from "./codingSessionBoundaryStatus";
import { normalizeOperatorPubkey } from "./codingSessionPromptAttribution";
import { codingSessionToolOutputGapField } from "./codingSessionToolOutputCompleteness";
import type { CodingSessionQuarantineItemV1 } from "./codingSessionTranscriptItemContract";

/** Display identity for the signer whose events these are. */
export type CodingSessionBridgeSource = { pubkey: string; label: string };

/** The largest a stamped `commandId` may be, matching the 44220 bound. */
const MAX_PROMPT_COMMAND_ID_BYTES = 256;
/**
 * Control characters are rejected outright rather than stripped: a command id
 * is an opaque join key, so a mangled one must not silently join anything.
 * Written as a scan rather than a regex because a control-character class in a
 * pattern is itself a lint error here.
 */
function hasControlCharacters(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/**
 * Read the 44220 command id a provider stamped on a `user_prompt`, or
 * `undefined`.
 *
 * Same bounds the command itself is held to (non-blank, at most 256 bytes, no
 * control characters) and no normalisation beyond that: the value has to
 * compare byte-for-byte with the id this client minted, so trimming or
 * case-folding it would invent matches.
 */
export function readCodingSessionPromptCommandId(
  value: unknown,
): string | undefined {
  if (typeof value !== "string") return undefined;
  if (value.trim().length === 0) return undefined;
  if (hasControlCharacters(value)) return undefined;
  return new TextEncoder().encode(value).byteLength <=
    MAX_PROMPT_COMMAND_ID_BYTES
    ? value
    : undefined;
}

/**
 * A projected transcript item, plus the coding-session-only join key.
 *
 * The renderer's `TranscriptItem` is shared with the local ACP surface, which
 * has no notion of a signed 44220; rather than widening that type for one
 * feature, coding sessions carry `commandId` in their own alias. Every
 * consumer that only wants a `TranscriptItem` still gets one.
 */
export type CodingSessionProjectedTranscriptItem = TranscriptItem & {
  /**
   * The command id of the turn this item belongs to, when the provider
   * stamped one. Only `user_prompt` echoes carry it today.
   */
  commandId?: string;
  /** Exact signed kind-44225 event this item was projected from. */
  sourceEventId?: string;
  /** Provider sequence and exact target used for durable coordination cursors. */
  sourceEventSeq?: number;
  sourceTargetKey?: string;
};

/** Everything a builder needs that does not come from the item itself. */
export type CodingSessionItemIdentity = {
  id: string;
  sessionId: string;
  targetKey: string;
  channelId: string | null;
  timestamp: string;
  turnId?: string;
  hasDeclaredTurn?: boolean;
  declaredTurnId?: string;
  acpSource?: string;
  bridgeSource?: CodingSessionBridgeSource | null;
  sourceEventId?: string;
  sourceEventSeq?: number;
  /** The provider's own session UUID; see `TranscriptItemIdentity`. */
  providerSessionId?: string | null;
};

/** Stamp the signer's display identity onto a finished item. */
export function finalizeCodingSessionItem(
  item: CodingSessionProjectedTranscriptItem,
  ctx: Identity,
): CodingSessionProjectedTranscriptItem {
  const stamped =
    ctx.providerSessionId != null
      ? { ...item, providerSessionId: ctx.providerSessionId }
      : item;
  const sourced = ctx.sourceEventId
    ? {
        ...stamped,
        sourceEventId: ctx.sourceEventId,
        sourceEventSeq: ctx.sourceEventSeq,
        sourceTargetKey: ctx.targetKey,
      }
    : stamped;
  return ctx.bridgeSource
    ? { ...sourced, bridgeSource: ctx.bridgeSource }
    : sourced;
}

type Identity = CodingSessionItemIdentity;

export function buildBaseTranscriptItem(
  item: unknown,
  ctx: Identity,
): CodingSessionProjectedTranscriptItem {
  if (isQuarantineItem(item)) {
    return buildQuarantineStatusItem(item, ctx);
  }

  if (!isRecord(item) || typeof item.kind !== "string") {
    return buildStatusItem(ctx, "Unrecognized transcript item", [
      "Received a transcript item with no recognized `kind` field. No content is surfaced.",
    ]);
  }

  switch (item.kind) {
    case "user_prompt":
      return buildUserPromptMessage(item, ctx);
    case "assistant_text":
      return buildAssistantTextMessage(item, ctx);
    case "tool_call":
      return (
        buildPlanFromExitPlanModeToolCall(item, ctx) ??
        buildToolCallItem(item, ctx)
      );
    case "tool_result":
      return buildToolResultItem(item, ctx);
    case "result":
      return buildResultLifecycleItem(item, ctx);
    case "status":
      return buildStatusLifecycleItem(item, ctx);
    case "system_init":
      return buildSystemInitStatusItem(item, ctx);
    case "account_info":
      return buildAccountInfoStatusItem(item, ctx);
    case "context_window_updated":
      return buildContextWindowStatusItem(item, ctx);
    case "compact_boundary":
      return buildSimpleLifecycleItem(ctx, "Context compact boundary", "");
    case "compact_summary":
      return buildSimpleLifecycleItem(
        ctx,
        "Context compacted",
        typeof item.summary === "string" ? item.summary : "",
      );
    case "context_cleared":
      return buildSimpleLifecycleItem(ctx, "Context cleared", "");
    case "interrupted":
      return buildSimpleLifecycleItem(ctx, "Interrupted", "");
    case "plan":
      return buildPlanItem(item, ctx);
    case "elided":
      return buildElidedStatusItem(item, ctx);
    case "reasoning":
      return buildReasoningItem(item, ctx);
    default:
      return buildStatusItem(
        ctx,
        `Unrecognized item kind: ${safeString(item.kind, 80)}`,
        [
          `kind="${safeString(item.kind, 80)}" is not a recognized coding-session item kind. No payload content is surfaced.`,
        ],
      );
  }
}

function buildUserPromptMessage(
  item: Record<string, unknown>,
  ctx: Identity,
): CodingSessionProjectedTranscriptItem {
  const content = typeof item.content === "string" ? item.content : "";
  const suffixes: string[] = [];
  // The count is a fallback, not the display. A turn that references its
  // attachments inline renders them where they were written — the picture
  // itself, or a link to the pasted file — so restating "1 attachment"
  // underneath would describe something the reader can already see. The chip
  // survives only for a prompt whose attachments have no reference in the prose
  // — an older client, or one whose upload never landed — where it is the only
  // evidence anything was ever part of the turn.
  if (
    typeof item.attachmentCount === "number" &&
    item.attachmentCount > 0 &&
    !containsAttachmentReference(content)
  ) {
    suffixes.push(
      `${item.attachmentCount} attachment${item.attachmentCount === 1 ? "" : "s"}`,
    );
  }
  const text =
    suffixes.length > 0 ? `${content}\n\n(${suffixes.join(", ")})` : content;

  return {
    id: ctx.id,
    type: "message",
    renderClass: "message",
    role: "user",
    title: item.steered === true ? "Steered prompt" : "Prompt",
    text,
    timestamp: ctx.timestamp,
    messageId: typeof item.messageId === "string" ? item.messageId : undefined,
    turnId: ctx.turnId,
    acpSource: ctx.acpSource,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
    // The provider's verified commanding signer. Kept as `undefined` (not
    // `null`) when absent or malformed so the renderer's "no attribution"
    // branch covers old items and junk alike.
    operatorPubkey: normalizeOperatorPubkey(item.operatorPubkey) ?? undefined,
    // The turn command this echo answers, when the provider named it. This is
    // what lets an optimistic row retire against the right echo instead of
    // guessing from the words.
    commandId: readCodingSessionPromptCommandId(item.commandId),
    // Only ever set, never `false`: a prompt that opened its turn carries no
    // key, so older projections stay byte-identical.
    ...(item.steered === true ? { steered: true } : {}),
  };
}

function buildAssistantTextMessage(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  return {
    id: ctx.id,
    type: "message",
    renderClass: "message",
    role: "assistant",
    title: "Response",
    text: typeof item.text === "string" ? item.text : "",
    timestamp: ctx.timestamp,
    messageId: typeof item.messageId === "string" ? item.messageId : undefined,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

/**
 * ACP's tool discriminant as published, or `null` when none was sent.
 *
 * Never invented: the producer omits it when the adapter sent none, and a
 * guessed discriminant is worse than an absent one.
 */
function readToolKind(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0
    ? safeString(value, MAX_METADATA_FIELD_LENGTH)
    : null;
}

/**
 * The file paths the producer published for an edit-kind call, from the `edit`
 * payload's `paths` (ACP `locations` plus its diff blocks).
 *
 * Defensive by contract: the union is permissive, so a malformed payload
 * degrades to no paths rather than throwing.
 */
function readEditPaths(value: unknown): string[] {
  if (!isRecord(value) || !Array.isArray(value.paths)) return [];
  const paths: string[] = [];
  for (const entry of value.paths) {
    if (typeof entry !== "string") continue;
    const path = safeString(entry, MAX_METADATA_FIELD_LENGTH).trim();
    if (path.length > 0 && !paths.includes(path)) paths.push(path);
  }
  return paths;
}

export function buildToolCallItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const tool = isRecord(item.tool) ? item.tool : {};
  const toolName =
    typeof tool.toolName === "string" && tool.toolName.length > 0
      ? safeString(tool.toolName, MAX_METADATA_FIELD_LENGTH)
      : "unknown_tool";
  const args = isRecord(tool.input) ? tool.input : {};
  const toolKind = readToolKind(tool.toolKind);
  const editPaths = readEditPaths(tool.edit);
  const descriptor = classifyTool({
    title: toolName,
    toolName,
    buzzToolName: null,
    args,
    result: "",
    isError: false,
  });

  return {
    id: ctx.id,
    type: "tool",
    renderClass: descriptor.renderClass,
    descriptor,
    title: toolName,
    toolName,
    buzzToolName: null,
    status: "executing" satisfies ToolStatus,
    args,
    result: "",
    isError: false,
    toolKind,
    editPaths,
    timestamp: ctx.timestamp,
    startedAt: ctx.timestamp,
    completedAt: null,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

export function buildToolResultItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const toolId =
    typeof item.toolId === "string" && item.toolId.length > 0
      ? safeString(item.toolId, MAX_METADATA_FIELD_LENGTH)
      : "unknown-tool";
  const toolName =
    typeof item.toolName === "string" && item.toolName.length > 0
      ? safeString(item.toolName, MAX_METADATA_FIELD_LENGTH)
      : toolId;
  const args = isRecord(item.input) ? item.input : {};
  const toolKind = readToolKind(item.toolKind);
  const editPaths = readEditPaths(item.edit);
  const isError = item.isError === true;
  const result = stringifyToolResultContent(
    normalizeToolResultContent(item.content, args),
  );
  const descriptor = classifyTool({
    title: toolName,
    toolName,
    buzzToolName: null,
    args,
    result,
    isError,
  });

  return {
    id: ctx.id,
    type: "tool",
    renderClass: descriptor.renderClass,
    descriptor,
    title: toolName,
    toolName,
    buzzToolName: null,
    status: (isError ? "failed" : "completed") satisfies ToolStatus,
    args,
    result,
    isError,
    toolKind,
    editPaths,
    ...codingSessionToolOutputGapField(item),
    timestamp: ctx.timestamp,
    startedAt: ctx.timestamp,
    completedAt: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

export function buildPairedToolResultItem(
  callItem: Record<string, unknown>,
  callCtx: Identity,
  resultItem: Record<string, unknown>,
  resultCtx: Identity,
): TranscriptItem {
  const callTool = isRecord(callItem.tool) ? callItem.tool : {};
  const toolName =
    typeof callTool.toolName === "string" && callTool.toolName.length > 0
      ? safeString(callTool.toolName, MAX_METADATA_FIELD_LENGTH)
      : "unknown_tool";
  // The result is the provider's final view after streamed tool arguments and
  // edit locations have settled. Prefer it over the opening call, while still
  // accepting older providers that only stamped the call.
  const args = isRecord(resultItem.input)
    ? resultItem.input
    : isRecord(callTool.input)
      ? callTool.input
      : {};
  const toolKind =
    readToolKind(resultItem.toolKind) ?? readToolKind(callTool.toolKind);
  const resultEditPaths = readEditPaths(resultItem.edit);
  const editPaths =
    resultEditPaths.length > 0 ? resultEditPaths : readEditPaths(callTool.edit);
  const isError = resultItem.isError === true;
  const result = stringifyToolResultContent(
    normalizeToolResultContent(resultItem.content, args),
  );
  const descriptor = classifyTool({
    title: toolName,
    toolName,
    buzzToolName: null,
    args,
    result,
    isError,
  });

  return {
    id: callCtx.id,
    type: "tool",
    renderClass: descriptor.renderClass,
    descriptor,
    title: toolName,
    toolName,
    buzzToolName: null,
    status: (isError ? "failed" : "completed") satisfies ToolStatus,
    args,
    result,
    isError,
    toolKind,
    editPaths,
    ...codingSessionToolOutputGapField(resultItem),
    timestamp: callCtx.timestamp,
    startedAt: callCtx.timestamp,
    completedAt: resultCtx.timestamp,
    turnId: callCtx.turnId,
    sessionId: callCtx.sessionId,
    channelId: callCtx.channelId,
  };
}

export function buildPlanFromExitPlanModeToolCall(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem | null {
  const tool = isRecord(item.tool) ? item.tool : {};
  const toolName = typeof tool.toolName === "string" ? tool.toolName : "";
  if (normalizeToolNameText(toolName) !== "exit_plan_mode") {
    return null;
  }
  const input = isRecord(tool.input) ? tool.input : {};
  const text =
    typeof input.plan === "string"
      ? input.plan
      : typeof input.text === "string"
        ? input.text
        : null;
  if (text === null) {
    return null;
  }
  return {
    id: ctx.id,
    type: "plan",
    renderClass: "plan",
    title: "Plan proposal",
    text,
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

/**
 * A first-class `plan` item: a replacement snapshot of the agent's plan.
 *
 * `text` is the producer's rendered markdown checklist and is what the task
 * rail parses, so it is preferred verbatim. When it is missing the checklist
 * is re-rendered from `entries` in the same format rather than leaving an
 * empty plan card behind.
 */
function buildPlanItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const declared = typeof item.text === "string" ? item.text : "";
  const text =
    declared.trim().length > 0 ? declared : renderPlanChecklist(item.entries);
  return {
    id: ctx.id,
    type: "plan",
    renderClass: "plan",
    title: "Plan",
    text,
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

function renderPlanChecklist(entries: unknown): string {
  if (!Array.isArray(entries)) {
    return "";
  }
  return capArray(
    entries.filter(isRecord).flatMap((entry) => {
      const content =
        typeof entry.content === "string"
          ? safeString(entry.content, MAX_METADATA_FIELD_LENGTH)
          : "";
      if (content.length === 0) {
        return [];
      }
      const status = typeof entry.status === "string" ? entry.status : "";
      const checkbox = status === "completed" ? "[x]" : "[ ]";
      const suffix = status === "in_progress" ? " (in progress)" : "";
      return [`- ${checkbox} ${content}${suffix}`];
    }),
    MAX_METADATA_ARRAY_ITEMS,
    (overflowCount) => `- [ ] … (+${overflowCount} more)`,
  ).join("\n");
}

/**
 * An item the producer had to drop whole because it would not fit the
 * envelope cap. Surfaced as a visible placeholder — the reader must be able
 * to see that something existed here, and how much of it.
 *
 * The size and digest travel structurally, not formatted into `text`, so the
 * renderer can show them in the same pill vocabulary as a privacy redaction
 * while still saying which of the two happened. `text` keeps a readable
 * one-line fallback for every consumer that has not learned the field —
 * exports, the diagnostics rail, anything reading the item as prose.
 */
function buildElidedStatusItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const bytes = typeof item.byteCount === "number" ? item.byteCount : null;
  // The producer writes bare hex (`transcript.rs` `digest()`), but this is a
  // relay-supplied field and the renderer adds its own `sha256:` label, so a
  // producer that ever prefixes it must not read `sha256:sha256:…`.
  const digest =
    typeof item.contentDigest === "string" && item.contentDigest.length > 0
      ? safeString(item.contentDigest.replace(/^sha256:/i, ""), 120)
      : null;
  const reason = safeString(item.reason ?? "unknown", 80);
  const size =
    bytes === null ? "an unknown amount" : formatRedactedBytes(bytes);
  return {
    id: ctx.id,
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_DROPPED_TITLE,
    text: `${size} dropped (${reason})${digest ? ` · sha256:${digest}` : ""}`,
    elision: { bytes, digest, reason },
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

/**
 * The agent's chain of reasoning.
 *
 * The donor contract forbade this outright; this fork admits it deliberately
 * (the producer emits it by default) because reasoning is one of the things
 * sessions are stored to analyse. It lands in the transcript's `thought` lane
 * rather than the message lane, so the renderer decides how prominent it is
 * without the adapter having to drop the content.
 */
function buildReasoningItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  return {
    id: ctx.id,
    type: "thought",
    renderClass: "thought",
    title: "Reasoning",
    text: typeof item.text === "string" ? item.text : "",
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

export function toolIdFromToolCall(
  item: Record<string, unknown>,
): string | null {
  const tool = isRecord(item.tool) ? item.tool : {};
  return typeof tool.toolId === "string" && tool.toolId.length > 0
    ? tool.toolId
    : null;
}

export function toolIdFromToolResult(
  item: Record<string, unknown>,
): string | null {
  return typeof item.toolId === "string" && item.toolId.length > 0
    ? item.toolId
    : null;
}

/** The six numbers `TurnUsageReport` may carry, in the order the wire lists them. */
const RESULT_USAGE_FIELDS = [
  "inputTokens",
  "outputTokens",
  "cacheReadTokens",
  "cacheWriteTokens",
  "toolCalls",
  "contextWindow",
] as const;

/**
 * The `result` item's per-turn `usage` block, read defensively.
 *
 * Only the six fields the wire's `TurnUsageReport` defines survive, and only
 * when they are finite numbers — the block is additive and every field is
 * independently optional, so an unreadable or absent field is dropped rather
 * than reported as `0`. A block with nothing readable in it becomes `null`,
 * which is what "the driver reported no usage" means to every consumer.
 */
function buildResultUsage(
  raw: unknown,
): NonNullable<Extract<TranscriptItem, { type: "lifecycle" }>["usage"]> | null {
  if (!isRecord(raw)) return null;
  const usage: Record<string, number> = {};
  for (const field of RESULT_USAGE_FIELDS) {
    const value = raw[field];
    if (typeof value === "number" && Number.isFinite(value)) {
      usage[field] = value;
    }
  }
  return Object.keys(usage).length > 0 ? usage : null;
}

function buildResultLifecycleItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const subtype = safeString(
    typeof item.subtype === "string" ? item.subtype : "unknown",
    80,
  );
  const isError = item.isError === true;
  const durationMs =
    typeof item.durationMs === "number" ? item.durationMs : null;
  const costUsd = typeof item.costUsd === "number" ? item.costUsd : null;
  const usage = buildResultUsage(item.usage);
  const resultText = typeof item.result === "string" ? item.result : "";
  // A provider that rewrote the raw error into an operator sentence keeps the
  // raw text under `detail`; it reads as a second paragraph under the
  // sentence so the remedy comes first and the evidence is still on the row.
  const detail =
    typeof item.detail === "string" && item.detail.trim().length > 0
      ? item.detail.trim()
      : "";
  const text =
    detail && detail !== resultText
      ? `${resultText}\n\n${detail}`.trim()
      : resultText;

  // Duration, cost and usage travel as structured fields, never baked into
  // `text` — the model reads them directly and the result prose stays clean.
  return {
    id: ctx.id,
    type: "lifecycle",
    renderClass: isError ? "error" : "status",
    title: "Turn result",
    text,
    outcome: subtype,
    durationMs,
    costUsd,
    costBasis: costUsd === null ? null : readCostBasis(item.costBasis),
    usage,
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

/**
 * The title continuity rows carry, distinct from the generic "Status" title
 * precisely so the transcript model's diagnostics gate
 * (`DIAGNOSTIC_LIFECYCLE_TITLES`) does not sweep them into the collapsed
 * "Session details" disclosure. Whether an agent kept, replayed, or lost its
 * prior context is the first thing a reader needs to know about a transcript —
 * it belongs in the reading order, not behind a click.
 */
export const CODING_SESSION_CONTINUITY_TITLE = "Session continuity";

/**
 * Title for a row standing in for an item the producer dropped whole.
 *
 * "Dropped", not "elided": the wire word is precise about the mechanism and
 * opaque about the meaning, and the reader needs the meaning. Deliberately
 * absent from `DIAGNOSTIC_LIFECYCLE_TITLES` — a dropped item belongs in the
 * reading order, where it was dropped.
 */
export const CODING_SESSION_DROPPED_TITLE = "Content dropped";

/**
 * Provider continuity slugs, in the reader's terms.
 *
 * Additive by design: the provider may publish slugs this build has never
 * seen, and an unknown one keeps the generic "Status" title and the
 * diagnostics routing that goes with it. Only a slug whose meaning is known
 * here earns a first-class row — guessing prose for an unknown slug would be
 * inventing a fact about the session's history.
 */
export const CODING_SESSION_CONTINUITY_STATUSES: ReadonlyMap<string, string> =
  new Map([
    ["session_fresh", "Started fresh — no prior session context"],
    [
      "session_rehydrated",
      "Rehydrated — verified session history is available to this agent",
    ],
    [
      "session_resumed",
      "Resumed — reconnected to the provider's native session",
    ],
    [
      "session_loaded",
      "Loaded — the provider replayed its native session history",
    ],
    ["session_restarted_without_context", "Restarted without prior context"],
  ]);

/**
 * Reader-facing clauses for the closed set of slugs a create or resume may
 * publish when continuity was lost (`CONTEXT_UNAVAILABLE_REASONS` in
 * `crates/buzz-core/src/coding_session_payload.rs`).
 *
 * Only `session_fresh` and `session_restarted_without_context` ever carry a
 * `reason` — see `REASON_CARRYING_CONTINUITY_STATUSES`. A clause is appended
 * to the base continuity prose as ` — ${clause}`.
 */
export const CODING_SESSION_CONTINUITY_REASONS: ReadonlyMap<string, string> =
  new Map([
    [
      "no_prior_execution",
      "this is the session's first execution, so there was no prior work to carry",
    ],
    [
      // Only a resume publishes this one. A resumed execution has prior work by
      // definition, so it can never claim a first execution — what it lacks is
      // the umbrella the projector is keyed on.
      "no_umbrella_context",
      "this execution was not created under an umbrella session, so there was no verified history to rebuild",
    ],
    [
      "context_fact_conflict",
      "conflicting signed facts were found for this session — for example two executions under one create — so verified history was withheld rather than guessed",
    ],
    [
      "relay_unavailable",
      "the relay could not be reached to rebuild verified history",
    ],
    ["relay_query_failed", "the relay query for verified history failed"],
    [
      "unverifiable_source_fact",
      "a source fact failed verification, so verified history was withheld",
    ],
    [
      "source_exceeds_projection_bound",
      "this session's history is larger than the projector's bounds",
    ],
    [
      "context_sidecar_unavailable",
      "no context sidecar is installed on this computer",
    ],
    [
      "context_sidecar_path_invalid",
      "the configured context sidecar path is not absolute",
    ],
    ["brief_encode_failed", "the verified brief could not be encoded"],
    [
      "package_write_failed",
      "the verified package could not be written to local storage",
    ],
  ]);

/**
 * The only two continuity statuses a lost verified package can attach a
 * `reason` to (`crates/buzz-session-provider/src/lib.rs`'s create and resume
 * disclosure sites). A `reason` on any other status is ignored rather than
 * rendered, since none of those statuses represent a continuity loss.
 */
const REASON_CARRYING_CONTINUITY_STATUSES: ReadonlySet<string> = new Set([
  "session_fresh",
  "session_restarted_without_context",
]);

/**
 * Append a reason clause to base continuity prose, when one is present.
 *
 * A recognized reason renders its reader-facing clause
 * (`CODING_SESSION_CONTINUITY_REASONS`). An unrecognized reason still
 * renders — as its raw slug in parentheses — because guessing prose for it
 * would be inventing a fact, but dropping it silently would hide a real
 * disclosure (H6). An absent or non-string reason renders the base prose
 * unchanged.
 */
function withReasonClause(continuity: string, reason: unknown): string {
  if (typeof reason !== "string" || reason.length === 0) {
    return continuity;
  }
  const bounded = safeString(reason, 200);
  const clause = CODING_SESSION_CONTINUITY_REASONS.get(bounded);
  return clause ? `${continuity} — ${clause}` : `${continuity} (${bounded})`;
}

function buildStatusLifecycleItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const status = safeString(
    typeof item.status === "string" ? item.status : "",
    200,
  );
  const boundary = codingSessionBoundaryRow(status, item.reason);
  if (boundary !== undefined) {
    return buildSimpleLifecycleItem(ctx, boundary.title, boundary.text);
  }
  const continuity = CODING_SESSION_CONTINUITY_STATUSES.get(status);
  if (!continuity) {
    return buildSimpleLifecycleItem(ctx, "Status", status);
  }
  const text = REASON_CARRYING_CONTINUITY_STATUSES.has(status)
    ? withReasonClause(continuity, item.reason)
    : continuity;
  return buildSimpleLifecycleItem(ctx, CODING_SESSION_CONTINUITY_TITLE, text);
}

function buildSimpleLifecycleItem(
  ctx: Identity,
  title: string,
  text: string,
): TranscriptItem {
  return {
    id: ctx.id,
    type: "lifecycle",
    renderClass: "status",
    title,
    text,
    timestamp: ctx.timestamp,
    turnId: ctx.turnId,
    sessionId: ctx.sessionId,
    channelId: ctx.channelId,
  };
}

/**
 * Bounded informational items — system/account/context/quarantine markers
 * and the unrecognized-shape fallbacks. Deliberately a `lifecycle`
 * (renderClass `"status"`) item, NOT `metadata`/`"raw-rail"`: compact
 * preview treats `raw-rail` as non-renderable
 * (`AgentSessionTranscriptList.tsx`'s private `isRenderableCompactItem`),
 * which would silently hide these rows in that view — exactly the "never
 * dropped" guarantee this adapter must hold. `lifecycle`/`status` is
 * compact-renderable and still fully bounded/inert.
 */
function buildStatusItem(
  ctx: Identity,
  title: string,
  bodyLines: string[],
): TranscriptItem {
  return buildSimpleLifecycleItem(
    ctx,
    safeString(title, 160),
    bodyLines.join("\n"),
  );
}

function buildSystemInitStatusItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const provider = safeString(
    typeof item.provider === "string" ? item.provider : "",
  );
  const model = safeString(typeof item.model === "string" ? item.model : "");
  const tools = safeStringArray(item.tools);
  const agents = safeStringArray(item.agents);
  const slashCommands = safeStringArray(item.slashCommands);
  const mcpServerLabels = (
    Array.isArray(item.mcpServers) ? item.mcpServers : []
  )
    .filter(isRecord)
    .map((server) => {
      const name = safeString(
        typeof server.name === "string" ? server.name : "unknown",
        MAX_METADATA_ARRAY_ITEM_LENGTH,
      );
      const status =
        typeof server.status === "string"
          ? ` (${safeString(server.status, MAX_METADATA_ARRAY_ITEM_LENGTH)})`
          : "";
      return `${name}${status}`;
    });
  // capArray does the slicing itself — it must run on the FULL label list
  // (not a pre-sliced one) so the overflow marker is derived correctly.
  const mcpServers = capArray(
    mcpServerLabels,
    MAX_METADATA_ARRAY_ITEMS,
    (overflowCount) => `… (+${overflowCount} more)`,
  );

  return buildStatusItem(ctx, "System Init", [
    `provider: ${provider}`,
    `model: ${model}`,
    `tools: ${tools.join(", ")}`,
    `agents: ${agents.join(", ")}`,
    `slashCommands: ${slashCommands.join(", ")}`,
    `mcpServers: ${mcpServers.join(", ")}`,
  ]);
}

function buildAccountInfoStatusItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const record = isRecord(item.accountInfo) ? item.accountInfo : {};
  const body = boundEntries(record)
    .filter(([, value]) => isPrimitive(value))
    .map(([key, value]) => `${safeString(key, 80)}: ${safeString(value, 120)}`);

  return buildStatusItem(ctx, "Account Info", body);
}

function buildContextWindowStatusItem(
  item: Record<string, unknown>,
  ctx: Identity,
): TranscriptItem {
  const usage = isRecord(item.usage) ? item.usage : {};
  const body = boundEntries(usage)
    .filter(
      ([, value]) => typeof value === "number" || typeof value === "boolean",
    )
    .map(([key, value]) => `${safeString(key, 80)}: ${safeString(value, 120)}`);

  return buildStatusItem(ctx, "Context Window Updated", body);
}

function buildQuarantineStatusItem(
  quarantine: CodingSessionQuarantineItemV1,
  ctx: Identity,
): TranscriptItem {
  return buildStatusItem(ctx, "Quarantined transcript event", [
    `quarantineClass: ${safeString(quarantine.quarantineClass ?? "unknown")}`,
    `decodeError: ${safeString(quarantine.decodeError ?? "unknown")}`,
    `sourceKey: ${safeString(quarantine.sourceKey ?? "unknown")}`,
    `byteCount: ${
      typeof quarantine.byteCount === "number"
        ? quarantine.byteCount
        : "unknown"
    }`,
    `contentDigest: ${safeString(quarantine.contentDigest ?? "unknown")}`,
    `claimedKind: ${safeString(quarantine.claimedKind ?? "null")}`,
    `claimedEntrySchema: ${safeString(
      quarantine.claimedEntrySchema ?? "null",
    )}`,
  ]);
}
