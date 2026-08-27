import type { CodingSessionQuarantineItemV1 } from "./transcriptItemContract.ts";

/**
 * Copied from `desktop/src/features/coding-sessions/lib/codingSessionDefensive.ts`.
 *
 * Bounds for BOUNDED METADATA fields only (quarantine records, system/
 * account/context-window markers, echoed unknown `kind` strings, tool
 * name/id identifiers). These are supposed to be short labels/identifiers by
 * construction; nothing upstream guarantees that, so this adapter bounds them
 * itself rather than trusting it.
 *
 * Primary chat/tool CONTENT (`user_prompt.content`, `assistant_text.text`,
 * `tool_result.content`, `result.result`) is deliberately NOT bounded here:
 * the producer already enforces the 32 KiB envelope cap and replaces anything
 * that will not fit with an explicit `elided` marker, so re-truncating content
 * at this layer would only hide a bound that has already been declared.
 */
export const MAX_METADATA_FIELD_LENGTH = 300;
export const MAX_METADATA_ARRAY_ITEMS = 20;
export const MAX_METADATA_ARRAY_ITEM_LENGTH = 80;

export function safeString(
  value: unknown,
  maxLen: number = MAX_METADATA_FIELD_LENGTH,
): string {
  const raw =
    typeof value === "string"
      ? value
      : value === null || value === undefined
        ? ""
        : safeCoerceToString(value);
  // Strip control characters (including newlines) so a single bounded
  // metadata field can't smuggle multi-line structure or terminal escapes.
  // Written as a charCode scan (not a regex control-char class) so the
  // source never embeds raw control bytes.
  const flattened = stripControlCharacters(raw);
  if (flattened.length <= maxLen) {
    return flattened;
  }
  return `${flattened.slice(0, maxLen)}… (+${flattened.length - maxLen} chars truncated)`;
}

export function safeCoerceToString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return "[unstringifiable]";
  }
}

/**
 * Replaces C0 control characters (0x00-0x1f) and DEL (0x7f) with a space,
 * then collapses runs of spaces. Written as a charCode scan rather than a
 * regex control-character class so the source file never has to embed raw
 * control bytes (or escape sequences that could be mis-decoded) directly in
 * a regex literal.
 */
function stripControlCharacters(value: string): string {
  let result = "";
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    result += code <= 0x1f || code === 0x7f ? " " : value[i];
  }
  return result.replace(/ +/g, " ");
}

export function safeStringArray(
  value: unknown,
  maxItems: number = MAX_METADATA_ARRAY_ITEMS,
): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  const strings = value
    .filter((entry): entry is string => typeof entry === "string")
    .map((entry) => safeString(entry, MAX_METADATA_ARRAY_ITEM_LENGTH));
  return capArray(
    strings,
    maxItems,
    (overflowCount) => `… (+${overflowCount} more)`,
  );
}

/** Caps an already-typed array at `maxItems`, appending a bounded overflow marker. */
export function capArray<T>(
  values: T[],
  maxItems: number,
  overflowLabel: (overflowCount: number) => T,
): T[] {
  if (values.length <= maxItems) {
    return values;
  }
  return [
    ...values.slice(0, maxItems),
    overflowLabel(values.length - maxItems),
  ];
}

export function boundEntries(
  record: Record<string, unknown>,
  maxItems: number = MAX_METADATA_ARRAY_ITEMS,
): [string, unknown][] {
  const entries = Object.entries(record);
  if (entries.length <= maxItems) {
    return entries;
  }
  return [
    ...entries.slice(0, maxItems),
    [`… (+${entries.length - maxItems} more keys)`, ""],
  ];
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isPrimitive(
  value: unknown,
): value is string | number | boolean {
  return (
    typeof value === "string" ||
    typeof value === "number" ||
    typeof value === "boolean"
  );
}

export function isQuarantineItem(
  item: unknown,
): item is CodingSessionQuarantineItemV1 {
  return isRecord(item) && item.schema === "seat-transcript-quarantine/v1";
}

export function stringifyToolResultContent(content: unknown): string {
  if (typeof content === "string") {
    return content;
  }
  try {
    return JSON.stringify(content) ?? safeCoerceToString(content);
  } catch {
    return safeCoerceToString(content);
  }
}

export function normalizeToolResultContent(
  content: unknown,
  args: Record<string, unknown>,
): unknown {
  const oldString = getStringFromRecordAllowingEmpty(args, [
    "oldString",
    "old_string",
  ]);
  const newString = getStringFromRecordAllowingEmpty(args, [
    "newString",
    "new_string",
  ]);
  if (oldString === null || newString === null) {
    return content;
  }
  const path =
    getStringFromRecord(args, [
      "path",
      "file",
      "file_path",
      "target_file",
      "filePath",
    ]) ?? "file";
  const safePath = safeString(path);
  return [
    `--- a/${safePath}`,
    `+++ b/${safePath}`,
    "@@",
    ...prefixDiffLines(oldString, "-"),
    ...prefixDiffLines(newString, "+"),
  ].join("\n");
}

function prefixDiffLines(value: string, prefix: "-" | "+"): string[] {
  return splitLogicalLines(value).map((line) => `${prefix}${line}`);
}

function splitLogicalLines(value: string): string[] {
  if (value.length === 0) {
    return [];
  }
  const lines = value.replace(/\r\n|\r/g, "\n").split("\n");
  if (lines[lines.length - 1] === "") {
    lines.pop();
  }
  return lines;
}

export function getStringFromRecord(
  record: Record<string, unknown>,
  keys: string[],
): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return null;
}

export function getStringFromRecordAllowingEmpty(
  record: Record<string, unknown>,
  keys: string[],
): string | null {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string") {
      return value;
    }
  }
  return null;
}

export function toIsoTimestamp(epochMs: number): string {
  const date = new Date(epochMs);
  return Number.isFinite(date.getTime())
    ? date.toISOString()
    : new Date(0).toISOString();
}
