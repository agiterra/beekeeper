/**
 * Project Pulse entries (kind 44240) — the TypeScript twin of
 * `crates/buzz-core/src/pulse.rs`.
 *
 * A Pulse entry says what its author *intends*: a plan, a milestone, a note, a
 * handoff, or a blocker. It never asserts an observed fact about a worktree —
 * observed facts stay in the coding-session kinds and are joined to entries by
 * the fold (`pulseFold.ts`), never by this module.
 *
 * `supersedes` is validated **syntactically only** here, exactly as buzz-core
 * does: whether a supersession is *honored* is a fold question (same author,
 * `(created_at, event id)` ordering) answered against
 * `conformance/project-pulse-fold/`.
 *
 * LOAD-BEARING CONSTRAINT: this module and `pulseFold.ts` must keep ZERO
 * runtime imports outside this directory and use erasable TypeScript syntax
 * only (no enums, no namespaces, no parameter properties).
 * `conformance/project-pulse-fold/implementation.test.mjs` imports them under
 * plain `node --test` with Node's native type stripping, so an aliased (`@/…`)
 * or non-erasable construct breaks the conformance gate. That is why the
 * coordinate normalizer below is a local mirror of
 * `buzz_core::kind::normalize_project_coordinate` rather than an import of
 * `projectContainerModel.normalizeProjectRef`; `pulseEntry.test.mjs` pins the
 * two against each other.
 */

/** Wire kind of a Pulse entry. Pinned against `KIND_PULSE_ENTRY` in tests. */
export const PULSE_ENTRY_KIND = 44240;

/** Exact `schema` value carried by kind 44240 content. */
export const PULSE_ENTRY_SCHEMA = "buzz-pulse-entry/v1";

/** Exact version carried by the `pu-v` tag. */
export const PULSE_ENTRY_TAG_VERSION = "pu1-1";

/** Maximum UTF-8 byte length of a complete Pulse entry payload. */
export const MAX_PULSE_ENTRY_CONTENT_BYTES = 16 * 1024;

/** Maximum UTF-8 byte length of an entry's prose. */
export const MAX_PULSE_TEXT_BYTES = 4 * 1024;

/** Maximum number of claimed code areas on one entry. */
export const MAX_PULSE_CODE_AREAS = 32;

/** Maximum UTF-8 byte length of one claimed code area. */
export const MAX_PULSE_CODE_AREA_BYTES = 256;

/** Maximum UTF-8 byte length of a branch shortname (tag or content field). */
export const MAX_PULSE_BRANCH_BYTES = 256;

/** Every accepted `pu-type` value, in the wire order buzz-core lists them. */
export const PULSE_ENTRY_TYPES = [
  "plan",
  "milestone",
  "note",
  "handoff",
  "blocker",
] as const;

/** What an entry claims about its author's work. */
export type PulseEntryType = (typeof PULSE_ENTRY_TYPES)[number];

/** Strict public JSON carried by a Pulse entry (kind 44240). */
export type PulseEntry = {
  schema: typeof PULSE_ENTRY_SCHEMA;
  type: PulseEntryType;
  text: string;
  /** Author *claims*, never observed facts; one leading `./` already stripped. */
  codeAreas: string[];
  branch: string | null;
  supersedes: string | null;
};

/** A signature-stripped Nostr event — the shape both `POST /query` and the
 * desktop relay client hand back. */
export type PulseEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** Decode outcome. The error string is the same sentence buzz-core produces
 * where the two can be identical; consumers surface it, never swallow it. */
export type PulseEntryDecodeResult =
  | { ok: true; entry: PulseEntry }
  | { ok: false; error: string };

const PULSE_ENTRY_FIELDS = [
  "schema",
  "type",
  "text",
  "codeAreas",
  "branch",
  "supersedes",
];

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const EVENT_ID_PATTERN = /^[0-9a-f]{64}$/;
const HEX64_PATTERN = /^[0-9a-fA-F]{64}$/;

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function hasControlCharacter(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0;
    // Matches Rust's `char::is_control`: C0 and C1 control ranges.
    if (code <= 0x1f || (code >= 0x7f && code <= 0x9f)) return true;
  }
  return false;
}

/** Strip a single leading `./` — the one normalization a code area gets. */
function stripLeadingDotSlash(path: string): string {
  return path.startsWith("./") ? path.slice(2) : path;
}

/** `true` for a Windows drive prefix such as `C:\src` or `c:/src`. */
function hasDrivePrefix(path: string): boolean {
  return /^[A-Za-z]:/.test(path);
}

/**
 * Canonicalize a project coordinate to `30621:<lowercase-hex>:<dtag>`, or
 * `null` when it is not one. Mirrors `buzz_core::kind::normalize_project_coordinate`
 * byte for byte, including the 64-character dtag ceiling.
 */
export function normalizePulseProjectCoordinate(value: string): string | null {
  const first = value.indexOf(":");
  if (first < 0) return null;
  const second = value.indexOf(":", first + 1);
  if (second < 0) return null;
  const kind = value.slice(0, first);
  const pubkey = value.slice(first + 1, second);
  const dtag = value.slice(second + 1);
  if (kind !== "30621") return null;
  if (!HEX64_PATTERN.test(pubkey)) return null;
  if (dtag.length === 0 || [...dtag].length > 64 || hasControlCharacter(dtag)) {
    return null;
  }
  return `30621:${pubkey.toLowerCase()}:${dtag}`;
}

/**
 * Validate one claimed code area.
 *
 * Code areas are repository-relative paths, never host paths. Rejected: an
 * empty path, a leading `/` or `~`, a Windows drive prefix, a `\` separator,
 * any `//`, any `..` **substring** (the substring rule the git manifest guard
 * uses — a segment-only check is not the same rule), a trailing `/`, any
 * control character, and anything over {@link MAX_PULSE_CODE_AREA_BYTES}.
 *
 * Returns `null` when the path is valid, else the rejection reason.
 */
export function validatePulseCodeArea(rawPath: string): string | null {
  const path = stripLeadingDotSlash(rawPath);
  if (path.length === 0) return "pulse code area must not be empty";
  if (utf8Bytes(path) > MAX_PULSE_CODE_AREA_BYTES) {
    return `pulse code area exceeds ${MAX_PULSE_CODE_AREA_BYTES} bytes`;
  }
  if (path.startsWith("/") || path.startsWith("~")) {
    return "pulse code area must be repository-relative";
  }
  if (hasDrivePrefix(path))
    return "pulse code area must not carry a drive prefix";
  if (path.includes("\\")) return "pulse code area must use / separators";
  if (path.includes("..")) return "pulse code area must not contain ..";
  if (path.includes("//") || path.endsWith("/")) {
    return "pulse code area must not contain an empty path segment";
  }
  if (hasControlCharacter(path)) {
    return "pulse code area must not contain control characters";
  }
  return null;
}

function validateBranch(branch: string): string | null {
  if (branch.length === 0) return "pulse entry branch must not be empty";
  if (utf8Bytes(branch) > MAX_PULSE_BRANCH_BYTES) {
    return `pulse entry branch exceeds ${MAX_PULSE_BRANCH_BYTES} bytes`;
  }
  if (hasControlCharacter(branch)) {
    return "pulse entry branch must not contain control characters";
  }
  return null;
}

function isPulseEntryType(value: unknown): value is PulseEntryType {
  return (
    typeof value === "string" &&
    (PULSE_ENTRY_TYPES as readonly string[]).includes(value)
  );
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Strictly decode and validate Pulse entry content.
 *
 * The whole-content cap is checked **before** parsing, matching buzz-core's
 * module-local precedent, and unknown keys are reported as such rather than as
 * a generic parse failure. `codeAreas` come back normalized (one leading `./`
 * stripped); duplicates after that strip are a rejection, never a silent merge.
 */
export function decodePulseEntry(content: string): PulseEntryDecodeResult {
  if (utf8Bytes(content) > MAX_PULSE_ENTRY_CONTENT_BYTES) {
    return {
      ok: false,
      error: `pulse entry content exceeds ${MAX_PULSE_ENTRY_CONTENT_BYTES} bytes`,
    };
  }
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch {
    return { ok: false, error: "malformed pulse entry payload" };
  }
  if (!isPlainObject(value)) {
    return { ok: false, error: "pulse entry payload must be an object" };
  }
  const unknown = Object.keys(value).find(
    (key) => !PULSE_ENTRY_FIELDS.includes(key),
  );
  if (unknown !== undefined) {
    return {
      ok: false,
      error: `pulse entry payload has unsupported field "${unknown}"`,
    };
  }
  if (value.schema !== PULSE_ENTRY_SCHEMA) {
    return { ok: false, error: "unsupported pulse entry schema" };
  }
  if (!isPulseEntryType(value.type)) {
    return {
      ok: false,
      error:
        "pulse entry type must be one of plan|milestone|note|handoff|blocker",
    };
  }
  if (typeof value.text !== "string" || value.text.trim().length === 0) {
    return { ok: false, error: "pulse entry text must contain prose" };
  }
  if (utf8Bytes(value.text) > MAX_PULSE_TEXT_BYTES) {
    return {
      ok: false,
      error: `pulse entry text exceeds ${MAX_PULSE_TEXT_BYTES} bytes`,
    };
  }
  const rawAreas = value.codeAreas ?? [];
  if (
    !Array.isArray(rawAreas) ||
    rawAreas.some((area) => typeof area !== "string")
  ) {
    return { ok: false, error: "pulse entry codeAreas must be strings" };
  }
  if (rawAreas.length > MAX_PULSE_CODE_AREAS) {
    return {
      ok: false,
      error: `pulse entry claims more than ${MAX_PULSE_CODE_AREAS} code areas`,
    };
  }
  const codeAreas: string[] = [];
  for (const area of rawAreas as string[]) {
    const rejection = validatePulseCodeArea(area);
    if (rejection) return { ok: false, error: rejection };
    codeAreas.push(stripLeadingDotSlash(area));
  }
  const seen = new Set<string>();
  for (const area of codeAreas) {
    if (seen.has(area)) {
      return { ok: false, error: `pulse entry repeats code area "${area}"` };
    }
    seen.add(area);
  }
  const branch = value.branch ?? null;
  if (branch !== null && typeof branch !== "string") {
    return { ok: false, error: "pulse entry branch must be a string or null" };
  }
  if (branch !== null) {
    const rejection = validateBranch(branch);
    if (rejection) return { ok: false, error: rejection };
  }
  const supersedes = value.supersedes ?? null;
  if (supersedes !== null && typeof supersedes !== "string") {
    return {
      ok: false,
      error: "pulse entry supersedes must be a string or null",
    };
  }
  if (supersedes !== null && !EVENT_ID_PATTERN.test(supersedes)) {
    return {
      ok: false,
      error:
        "pulse entry supersedes must be a 64-character lowercase hex event id",
    };
  }
  return {
    ok: true,
    entry: {
      schema: PULSE_ENTRY_SCHEMA,
      type: value.type,
      text: value.text,
      codeAreas,
      branch,
      supersedes,
    },
  };
}

/**
 * Validate a signed Pulse entry end to end: kind, tag grammar, canonical
 * project coordinate, and content envelope. The TypeScript twin of
 * `buzz_core::pulse::validate_pulse_entry_envelope`.
 *
 * **Tag grammar** — position-independent, multiplicity-constrained, closed key
 * set: exactly one `a`, one `pu-v`, and one `pu-type`; at most one each of
 * `h`, `branch`, and `pu-session`; every tag exactly two fields; any other key
 * is a rejection. Nostr imposes no relative tag ordering, so order is never
 * validated.
 */
export function validatePulseEntryEnvelope(
  event: PulseEvent,
): PulseEntryDecodeResult {
  if (event.kind !== PULSE_ENTRY_KIND) {
    return { ok: false, error: "event is not a pulse entry (kind 44240)" };
  }
  const decoded = decodePulseEntry(event.content);
  if (!decoded.ok) return decoded;

  const slots = new Map<string, string>();
  for (const tag of event.tags) {
    if (tag.length !== 2) {
      return {
        ok: false,
        error: "pulse entry tags must have exactly two fields",
      };
    }
    const [key, value] = tag;
    if (
      key !== "a" &&
      key !== "pu-v" &&
      key !== "pu-type" &&
      key !== "h" &&
      key !== "branch" &&
      key !== "pu-session"
    ) {
      return {
        ok: false,
        error: `pulse entry has unsupported tag key "${key}"`,
      };
    }
    if (slots.has(key)) {
      return { ok: false, error: `pulse entry has more than one ${key} tag` };
    }
    slots.set(key, value);
  }

  const coordinate = slots.get("a");
  if (coordinate === undefined) {
    return { ok: false, error: "pulse entry requires one a tag" };
  }
  if (normalizePulseProjectCoordinate(coordinate) !== coordinate) {
    return {
      ok: false,
      error:
        "44240 `a` tag must be a canonical 30621:<lowercase-hex>:<dtag> coordinate",
    };
  }
  if (slots.get("pu-v") !== PULSE_ENTRY_TAG_VERSION) {
    return { ok: false, error: "unsupported pulse entry tag version" };
  }
  const tagType = slots.get("pu-type");
  if (tagType === undefined) {
    return { ok: false, error: "pulse entry requires one pu-type tag" };
  }
  if (!isPulseEntryType(tagType)) {
    return {
      ok: false,
      error:
        "pulse entry type must be one of plan|milestone|note|handoff|blocker",
    };
  }
  if (tagType !== decoded.entry.type) {
    return {
      ok: false,
      error: "pulse entry pu-type tag does not match content type",
    };
  }
  const channel = slots.get("h");
  if (channel !== undefined && !UUID_PATTERN.test(channel)) {
    return {
      ok: false,
      error: "pulse entry h tag must be a lowercase canonical channel UUID",
    };
  }
  const tagBranch = slots.get("branch");
  if (tagBranch !== undefined) {
    const rejection = validateBranch(tagBranch);
    if (rejection) return { ok: false, error: rejection };
    if (decoded.entry.branch !== null && decoded.entry.branch !== tagBranch) {
      return {
        ok: false,
        error: "pulse entry branch tag does not match content branch",
      };
    }
  }
  const sessionRef = slots.get("pu-session");
  if (sessionRef !== undefined && !UUID_PATTERN.test(sessionRef)) {
    return {
      ok: false,
      error: "pulse entry pu-session must be a lowercase canonical UUID",
    };
  }
  if (decoded.entry.supersedes === event.id) {
    return { ok: false, error: "pulse entry must not supersede itself" };
  }
  return decoded;
}

/**
 * The project coordinate a Pulse entry is scoped to, normalized.
 *
 * Parsing is tolerant of hex case so a smuggled case-variant coordinate cannot
 * dodge a per-event check, even though ingest requires the tag to already be
 * canonical. Returns `null` when the event does not carry exactly one
 * well-formed `a` tag — the gate closes, it does not open.
 */
export function pulseEntryProjectCoordinate(event: PulseEvent): string | null {
  const values = event.tags
    .filter((tag) => tag[0] === "a")
    .map((tag) => tag[1] ?? "");
  if (values.length !== 1) return null;
  return normalizePulseProjectCoordinate(values[0]);
}

/** The `pu-session` tag value (the umbrella session UUID), or `null`. */
export function pulseEntrySessionRef(event: PulseEvent): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === "pu-session");
  return tag?.[1] ?? null;
}
