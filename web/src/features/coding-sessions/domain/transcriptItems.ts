/**
 * Per-kind mapping: one decoded 44225 item -> one {@link ProjectedTranscriptItem}.
 *
 * Rewritten from
 * `desktop/src/features/coding-sessions/lib/codingSessionTranscriptItems.ts`
 * against a web-local row type: the desktop builders target the shared ACP
 * `TranscriptItem` renderer, which lives in a package this one may not import.
 * The per-kind semantics of D9 are the same; only the output shape differs.
 *
 * Every builder is TOTAL and bounded: it reads defensively, never assumes a
 * field is present or well-typed, and never throws.
 */
import { truncatePubkey } from "../../../shared/lib/pubkey.ts";
import {
  boundEntries,
  isPrimitive,
  isRecord,
  MAX_METADATA_ARRAY_ITEM_LENGTH,
  normalizeToolResultContent,
  safeString,
  safeStringArray,
  stringifyToolResultContent,
} from "./defensive.ts";
import type { ProjectedTranscriptItem } from "./types.ts";

/** Identity and scope for one projected row. */
export type TranscriptItemIdentity = {
  id: string;
  blockKey: string;
  targetKey: string;
  turnId: string | null;
  timestamp: number;
  eventSeq: number;
};

const MAX_PROMPT_COMMAND_ID_BYTES = 256;

/**
 * Continuity slugs a `status` item may carry. Anything else renders as a
 * generic `Status` row rather than being echoed as a title.
 */
const KNOWN_STATUS_SLUGS = new Map<string, string>([
  ["connected", "Provider connected"],
  ["disconnected", "Provider disconnected"],
  ["reconnected", "Provider reconnected"],
  ["resumed", "Session resumed"],
  ["resumed_without_context", "Session resumed without context"],
  ["interrupted", "Turn interrupted"],
  ["stopped", "Session stopped"],
  ["idle", "Session idle"],
  ["running", "Session running"],
  ["waiting_for_input", "Waiting for input"],
]);

function blankItem(identity: TranscriptItemIdentity): ProjectedTranscriptItem {
  return {
    id: identity.id,
    blockKey: identity.blockKey,
    turnId: identity.turnId,
    role: "lifecycle",
    title: "Status",
    text: "",
    folded: false,
    timestamp: identity.timestamp,
    eventSeq: identity.eventSeq,
    tool: null,
    lifecycle: null,
    meta: [],
    unknownKind: null,
  };
}

function hasControlCharacters(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code < 0x20 || code === 0x7f) return true;
  }
  return false;
}

/**
 * Read the 44220 command id a provider stamped on a `user_prompt`.
 *
 * Same bounds the command itself is held to (non-blank, at most 256 bytes, no
 * control characters) and no normalisation beyond that: the value has to
 * compare byte-for-byte with a minted id, so trimming would invent matches.
 */
export function readPromptCommandId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  if (value.trim().length === 0) return null;
  if (hasControlCharacters(value)) return null;
  return new TextEncoder().encode(value).byteLength <=
    MAX_PROMPT_COMMAND_ID_BYTES
    ? value
    : null;
}

/** Exactly 64 lowercase hex, or null. Never a best guess at an operator. */
export function normalizeOperatorPubkey(value: unknown): string | null {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value)
    ? value
    : null;
}

/** The `toolId` a `tool_call` declares, or null. */
export function toolIdFromToolCall(
  item: Record<string, unknown>,
): string | null {
  const tool = isRecord(item.tool) ? item.tool : null;
  const toolId = tool?.toolId;
  return typeof toolId === "string" && toolId.length > 0 ? toolId : null;
}

/** The `toolId` a `tool_result` claims to answer, or null. */
export function toolIdFromToolResult(
  item: Record<string, unknown>,
): string | null {
  return typeof item.toolId === "string" && item.toolId.length > 0
    ? item.toolId
    : null;
}

function toolArgs(item: Record<string, unknown>): Record<string, unknown> {
  const tool = isRecord(item.tool) ? item.tool : null;
  return isRecord(tool?.input) ? (tool.input as Record<string, unknown>) : {};
}

function toolName(item: Record<string, unknown>): string {
  const tool = isRecord(item.tool) ? item.tool : null;
  const name = tool?.toolName;
  return typeof name === "string" && name.trim().length > 0
    ? safeString(name, MAX_METADATA_ARRAY_ITEM_LENGTH)
    : "tool";
}

/** One-line fold of the tool arguments, for the collapsed row. */
export function summarizeToolArgs(args: Record<string, unknown>): string {
  return boundEntries(args)
    .map(([key, value]) =>
      isPrimitive(value)
        ? `${key}=${safeString(String(value), 80)}`
        : `${key}=${safeString(stringifyToolResultContent(value), 80)}`,
    )
    .join(" ");
}

/** `tool_call` -> a folded, expandable tool row awaiting its result. */
export function buildToolCallItem(
  item: Record<string, unknown>,
  identity: TranscriptItemIdentity,
): ProjectedTranscriptItem {
  const args = toolArgs(item);
  const name = toolName(item);
  return {
    ...blankItem(identity),
    role: "tool",
    title: name,
    text: summarizeToolArgs(args),
    folded: true,
    tool: {
      toolName: name,
      toolId: toolIdFromToolCall(item),
      args,
      status: "pending",
      result: "",
    },
  };
}

/** A `tool_result` with no pending call in the same target stream. */
export function buildOrphanToolResultItem(
  item: Record<string, unknown>,
  identity: TranscriptItemIdentity,
): ProjectedTranscriptItem {
  const isError = item.isError === true;
  const name =
    typeof item.toolName === "string" && item.toolName.trim().length > 0
      ? safeString(item.toolName, MAX_METADATA_ARRAY_ITEM_LENGTH)
      : "tool";
  return {
    ...blankItem(identity),
    role: "tool",
    title: name,
    text: "",
    folded: true,
    tool: {
      toolName: name,
      toolId: toolIdFromToolResult(item),
      args: {},
      status: isError ? "error" : "completed",
      result: stringifyToolResultContent(item.content),
    },
    meta: ["result without a matching call"],
  };
}

/** Collapse a pending `tool_call` and its `tool_result` into one row. */
export function buildPairedToolItem(
  call: ProjectedTranscriptItem,
  result: Record<string, unknown>,
): ProjectedTranscriptItem {
  const args = call.tool?.args ?? {};
  const isError = result.isError === true;
  return {
    ...call,
    tool: {
      toolName: call.tool?.toolName ?? "tool",
      toolId: call.tool?.toolId ?? null,
      args,
      status: isError ? "error" : "completed",
      result: stringifyToolResultContent(
        normalizeToolResultContent(result.content, args),
      ),
    },
  };
}

/** Every non-tool kind, plus the unknown-kind fallback. */
export function buildNonToolItem(
  item: unknown,
  identity: TranscriptItemIdentity,
): ProjectedTranscriptItem {
  const base = blankItem(identity);
  if (!isRecord(item) || typeof item.kind !== "string") {
    return {
      ...base,
      title: "Unrecognized transcript item",
      unknownKind: null,
    };
  }
  const kind = item.kind;
  if (kind === "user_prompt") {
    const steered = item.steered === true;
    const operatorPubkey = normalizeOperatorPubkey(item.operatorPubkey);
    const commandId = readPromptCommandId(item.commandId);
    const meta: string[] = [];
    if (operatorPubkey) meta.push(`by ${truncatePubkey(operatorPubkey)}`);
    if (commandId) meta.push(`command ${safeString(commandId, 12)}`);
    return {
      ...base,
      role: "user",
      title: steered ? "Steered prompt" : "Prompt",
      text: typeof item.content === "string" ? item.content : "",
      meta,
    };
  }
  if (kind === "assistant_text") {
    return {
      ...base,
      role: "assistant",
      title: "Assistant",
      text: typeof item.text === "string" ? item.text : "",
    };
  }
  if (kind === "reasoning") {
    return {
      ...base,
      role: "assistant",
      title: "Reasoning",
      text: typeof item.text === "string" ? item.text : "",
      folded: true,
    };
  }
  if (kind === "plan") {
    return {
      ...base,
      title: "Plan",
      text: typeof item.text === "string" ? item.text : planText(item.entries),
    };
  }
  if (kind === "result") {
    return {
      ...base,
      title: "Turn result",
      text: typeof item.result === "string" ? item.result : "",
      lifecycle: {
        durationMs:
          typeof item.durationMs === "number" &&
          Number.isFinite(item.durationMs)
            ? item.durationMs
            : null,
        costUsd:
          typeof item.costUsd === "number" && Number.isFinite(item.costUsd)
            ? item.costUsd
            : null,
        isError: item.isError === true || item.subtype === "error",
      },
    };
  }
  if (kind === "elided") {
    // Content is gone by construction. Only its size and reason survive, and
    // nothing here may imply the payload is recoverable.
    const meta: string[] = [];
    if (typeof item.reason === "string") {
      meta.push(safeString(item.reason));
    }
    if (typeof item.byteCount === "number" && Number.isFinite(item.byteCount)) {
      meta.push(`${item.byteCount} bytes`);
    }
    return { ...base, title: "Content elided", meta };
  }
  if (kind === "status") {
    const slug = typeof item.status === "string" ? item.status : "";
    const known = KNOWN_STATUS_SLUGS.get(slug);
    return {
      ...base,
      title: known ?? "Status",
      meta: known ? [] : slug ? [safeString(slug)] : [],
    };
  }
  if (kind === "interrupted") {
    return { ...base, title: "Turn interrupted" };
  }
  if (kind === "compact_boundary") {
    return { ...base, title: "Context compacted" };
  }
  if (kind === "compact_summary") {
    return {
      ...base,
      title: "Compaction summary",
      text: typeof item.summary === "string" ? item.summary : "",
      folded: true,
    };
  }
  if (kind === "context_cleared") {
    return { ...base, title: "Context cleared" };
  }
  if (kind === "system_init") {
    const meta: string[] = [];
    if (typeof item.provider === "string") meta.push(safeString(item.provider));
    if (typeof item.model === "string") meta.push(safeString(item.model));
    meta.push(...safeStringArray(item.tools));
    return { ...base, title: "Session started", meta, folded: true };
  }
  if (kind === "account_info") {
    return {
      ...base,
      title: "Account",
      meta: isRecord(item.accountInfo)
        ? boundEntries(item.accountInfo).map(
            ([key, value]) => `${key}=${safeString(String(value))}`,
          )
        : [],
      folded: true,
    };
  }
  if (kind === "context_window_updated") {
    return {
      ...base,
      title: "Context window",
      meta: isRecord(item.usage)
        ? boundEntries(item.usage).map(
            ([key, value]) => `${key}=${safeString(String(value))}`,
          )
        : [],
      folded: true,
    };
  }
  // Unknown kind: name it and surface NO payload. Echoing an unrecognized
  // producer's fields is exactly the guess this reader must not make.
  return {
    ...base,
    title: `Unsupported item (${safeString(kind, 64)})`,
    text: "",
    unknownKind: safeString(kind, 64),
  };
}

function planText(entries: unknown): string {
  if (!Array.isArray(entries)) return "";
  return entries
    .filter(isRecord)
    .map((entry) => {
      const content =
        typeof entry.content === "string" ? safeString(entry.content, 300) : "";
      const status =
        typeof entry.status === "string" ? safeString(entry.status, 40) : "";
      return status ? `- [${status}] ${content}` : `- ${content}`;
    })
    .join("\n");
}
