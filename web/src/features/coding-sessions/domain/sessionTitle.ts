/**
 * NIP-CSG § Generated title: the one display-name resolver, on the web.
 *
 * A session's name comes from two records by two kinds of author. A person's
 * name is a founder-signed 44229; a generated title is a 44252 signed by the
 * provider that ran the founder's first turn — a model's words, so it is never
 * published or shown as a person's name. This file ranks them exactly as
 * `resolve_session_display_name` in `crates/beekeeper-core/src/coding_session_title.rs`
 * does, and `sessionTitle.test.mjs` binds it to the shared vectors in
 * `conformance/session-display-name/` (CONTRACT.md is the rule in prose):
 *
 * 1. **person** — the latest valid founder-signed 44229;
 * 2. **generated** — only when tier 1 is empty: the EARLIEST valid 44252
 *    whose signer is the provider authority of the umbrella execution its
 *    `cs-target` names, so a title never flips once shown;
 * 3. **fallback** — the founding execution's title, then "Untitled session".
 *
 * Tiers are ranked, never timed. Nothing here reads a clock or verifies a
 * signature: the caller verifies before folding, and the vectors' ids and
 * signers are synthetic labels.
 */
import {
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_NAME,
} from "../../../shared/lib/kinds.ts";
import {
  hasDuplicateJsonKeys,
  hasExactKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
} from "./wireDecode.ts";

/** Exact version carried by the `cstl-v` tag. */
export const CODING_SESSION_TITLE_TAG_VERSION = "cstl1-1" as const;
/** Exact `schema` of a generated-title payload. */
export const CODING_SESSION_TITLE_SCHEMA =
  "buzz-coding-session-title/v1" as const;
/** Maximum UTF-8 bytes of a 44252's content. */
export const MAX_CODING_SESSION_TITLE_CONTENT_BYTES = 2048;
/** Maximum UTF-8 bytes of the payload's `model`. */
export const MAX_CODING_SESSION_TITLE_MODEL_BYTES = 128;
/** Maximum UTF-8 bytes of a name or title: the NIP-CSN content rule. */
export const MAX_SESSION_DISPLAY_NAME_BYTES = 256;
/** The last fallback when nothing names the session. */
export const UNTITLED_SESSION_NAME = "Untitled session";

const NAME_TAG_VERSION = "csnm1-1";
const TARGET_KEY_PREFIX = "coding-session/v1|";
const MAX_TARGET_IDENTIFIER_BYTES = 256;
const MAX_SAFE_GENERATION = BigInt(Number.MAX_SAFE_INTEGER);
const HEX_EVENT_ID = /^[0-9a-f]{64}$/;
const CONTROL_CHARACTER = /\p{Cc}/u;
const UUID_FORMS = [
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
  /^[0-9a-f]{32}$/i,
  /^\{[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\}$/i,
  /^urn:uuid:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
];

/** One event as the resolver reads it: a Nostr event, signature optional. */
export type SessionNameRecord = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** One execution of the umbrella, from its provider-signed facts. */
export type SessionExecutionAuthority = {
  /** The exact `cs-target` key, generation included. */
  targetKey: string;
  /** The provider pubkey that signs that generation's facts. */
  providerAuthorityPubkey: string;
};

/** Everything besides the events that the resolver needs to know. */
export type SessionDisplayNameScope = {
  channelId: string;
  sessionRef: string;
  /** The genesis founder; without one no 44229 is a person's name. */
  founderPubkey: string | null;
  /** The founding execution's 44223 title, when it has one. */
  foundingExecutionTitle: string | null;
  /** Every generation of every execution the reader holds. */
  executions: readonly SessionExecutionAuthority[];
};

export type SessionDisplayNameOrigin = "person" | "generated" | "fallback";

/** What the resolver set aside, so a surface can say so instead of hiding it. */
export type SessionDisplayNameDiagnostics = {
  /** Valid 44229s for this session not signed by the founder. */
  foreignNames: number;
  /** Valid 44252s whose signer has no standing for the target they name. */
  foreignTitles: number;
  /** In-scope 44229/44252 events that fail their envelope. */
  malformed: number;
};

export type SessionDisplayName = {
  name: string;
  origin: SessionDisplayNameOrigin;
  /** The model that generated it; `generated` only. */
  model: string | null;
  /** Lowercase hex provider that signed it; `generated` only. */
  signerPubkey: string | null;
  diagnostics: SessionDisplayNameDiagnostics;
};

/** The strict v1 content of a 44252. */
export type CodingSessionTitlePayload = {
  schema: typeof CODING_SESSION_TITLE_SCHEMA;
  title: string;
  model: string;
  basis: "first-message";
  sourceCommand: string | null;
  createEventId: string;
};

/** A structurally valid 44252, decoded. */
export type CodingSessionTitleEnvelope = {
  channelId: string;
  sessionRef: string;
  targetKey: string;
  payload: CodingSessionTitlePayload;
};

const PAYLOAD_KEYS = [
  "basis",
  "createEventId",
  "model",
  "schema",
  "sourceCommand",
  "title",
] as const;

function utf8Bytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/** The NIP-CSN content rule, shared by 44229 content and a 44252 `title`. */
export function isValidSessionDisplayNameText(value: string): boolean {
  return (
    utf8Bytes(value) <= MAX_SESSION_DISPLAY_NAME_BYTES &&
    value.trim().length > 0 &&
    !value.includes("\r") &&
    !value.includes("\n")
  );
}

function isUuidText(value: string): boolean {
  return UUID_FORMS.some((form) => form.test(value));
}

/** Two-field tags with exactly these names, in this order. */
function exactTagValues(
  tags: unknown,
  names: readonly string[],
): string[] | null {
  if (!Array.isArray(tags) || tags.length !== names.length) return null;
  const values: string[] = [];
  for (let index = 0; index < names.length; index += 1) {
    const tag: unknown = tags[index];
    if (
      !Array.isArray(tag) ||
      tag.length !== 2 ||
      tag[0] !== names[index] ||
      typeof tag[1] !== "string"
    ) {
      return null;
    }
    values.push(tag[1]);
  }
  return values;
}

/** Whether a 44229's tags and content pass the NIP-CSN envelope. */
export function isValidCodingSessionNameParts(
  tags: unknown,
  content: string,
): boolean {
  if (!isValidSessionDisplayNameText(content)) return false;
  const values = exactTagValues(tags, ["h", "d", "csnm-v"]);
  return (
    values !== null &&
    isUuidText(values[0]) &&
    isCodingSessionSessionRef(values[1]) &&
    values[2] === NAME_TAG_VERSION
  );
}

/**
 * Decode a `cs-target` key as `coding_session_target_key` writes it, or null.
 *
 * Lengths are UTF-8 byte counts, so the walk is over bytes. Strict: the key
 * must re-encode to exactly itself (no leading zeros, no `+`), and every field
 * obeys a 44220 command target's bounds.
 */
export function isValidCodingSessionTargetKey(key: string): boolean {
  if (!key.startsWith(TARGET_KEY_PREFIX)) return false;
  const bytes = new TextEncoder().encode(key.slice(TARGET_KEY_PREFIX.length));
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const fields: string[] = [];
  let cursor = 0;
  while (cursor < bytes.length) {
    const colon = bytes.indexOf(0x3a, cursor);
    if (colon <= cursor) return false;
    let length = 0;
    for (let index = cursor; index < colon; index += 1) {
      const byte = bytes[index];
      if (byte < 0x30 || byte > 0x39) return false;
      length = length * 10 + (byte - 0x30);
      if (length > bytes.length) return false;
    }
    const end = colon + 1 + length;
    if (end > bytes.length) return false;
    try {
      fields.push(decoder.decode(bytes.subarray(colon + 1, end)));
    } catch {
      return false;
    }
    cursor = end;
  }
  if (fields.length !== 4) return false;
  for (const field of fields.slice(0, 3)) {
    if (
      field.trim().length === 0 ||
      utf8Bytes(field) > MAX_TARGET_IDENTIFIER_BYTES ||
      CONTROL_CHARACTER.test(field)
    ) {
      return false;
    }
  }
  if (!/^[0-9]+$/.test(fields[3])) return false;
  const generation = BigInt(fields[3]);
  if (generation === 0n || generation > MAX_SAFE_GENERATION) return false;
  const reencoded =
    TARGET_KEY_PREFIX +
    [fields[0], fields[1], fields[2], generation.toString()]
      .map((field) => `${utf8Bytes(field)}:${field}`)
      .join("");
  return reencoded === key;
}

/** Decode and validate a 44252's content, or null. */
export function parseCodingSessionTitleContent(
  content: string,
): CodingSessionTitlePayload | null {
  if (
    utf8Bytes(content) > MAX_CODING_SESSION_TITLE_CONTENT_BYTES ||
    hasDuplicateJsonKeys(content)
  ) {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (!isPlainRecord(parsed) || !hasExactKeys(parsed, [...PAYLOAD_KEYS])) {
    return null;
  }
  const { schema, title, model, basis, sourceCommand, createEventId } = parsed;
  if (
    schema !== CODING_SESSION_TITLE_SCHEMA ||
    typeof title !== "string" ||
    !isValidSessionDisplayNameText(title) ||
    typeof model !== "string" ||
    model.trim().length === 0 ||
    utf8Bytes(model) > MAX_CODING_SESSION_TITLE_MODEL_BYTES ||
    CONTROL_CHARACTER.test(model) ||
    basis !== "first-message" ||
    !(
      sourceCommand === null ||
      (typeof sourceCommand === "string" && HEX_EVENT_ID.test(sourceCommand))
    ) ||
    typeof createEventId !== "string" ||
    !HEX_EVENT_ID.test(createEventId)
  ) {
    return null;
  }
  return { schema, title, model, basis, sourceCommand, createEventId };
}

/**
 * Validate a 44252's exact ordered envelope — `h`, `d`, `cstl-v`,
 * `cs-target`, each two fields — and strict v1 content. Checks no standing.
 */
export function parseCodingSessionTitleParts(
  tags: unknown,
  content: string,
): CodingSessionTitleEnvelope | null {
  const values = exactTagValues(tags, ["h", "d", "cstl-v", "cs-target"]);
  if (
    values === null ||
    !isUuidText(values[0]) ||
    !isCodingSessionSessionRef(values[1]) ||
    values[2] !== CODING_SESSION_TITLE_TAG_VERSION ||
    !isValidCodingSessionTargetKey(values[3])
  ) {
    return null;
  }
  const payload = parseCodingSessionTitleContent(content);
  if (payload === null) return null;
  return {
    channelId: values[0],
    sessionRef: values[1],
    targetKey: values[3],
    payload,
  };
}

function carriesTag(
  record: SessionNameRecord,
  name: string,
  value: string,
): boolean {
  return (
    Array.isArray(record.tags) &&
    record.tags.some(
      (tag) =>
        Array.isArray(tag) &&
        tag.length >= 2 &&
        tag[0] === name &&
        tag[1] === value,
    )
  );
}

/** `(created_at, id)`: negative when `left` sorts first. */
function compareOrder(
  left: SessionNameRecord,
  right: SessionNameRecord,
): number {
  if (left.created_at !== right.created_at) {
    return left.created_at < right.created_at ? -1 : 1;
  }
  if (left.id === right.id) return 0;
  return left.id < right.id ? -1 : 1;
}

/**
 * Resolve one umbrella's display name from its 44229 and 44252 events.
 *
 * Order-independent: events of other kinds, or without this session's exact
 * `h` and `d`, are neither used nor counted.
 */
export function resolveSessionDisplayName(
  scope: SessionDisplayNameScope,
  records: Iterable<SessionNameRecord>,
): SessionDisplayName {
  const founder = scope.founderPubkey?.toLowerCase() ?? null;
  const diagnostics: SessionDisplayNameDiagnostics = {
    foreignNames: 0,
    foreignTitles: 0,
    malformed: 0,
  };
  let person: SessionNameRecord | null = null;
  let generated: {
    record: SessionNameRecord;
    payload: CodingSessionTitlePayload;
  } | null = null;

  for (const record of records) {
    if (
      record.kind !== KIND_CODING_SESSION_NAME &&
      record.kind !== KIND_CODING_SESSION_GENERATED_TITLE
    ) {
      continue;
    }
    if (
      !carriesTag(record, "h", scope.channelId) ||
      !carriesTag(record, "d", scope.sessionRef)
    ) {
      continue;
    }
    const signer = record.pubkey.toLowerCase();
    if (record.kind === KIND_CODING_SESSION_NAME) {
      if (!isValidCodingSessionNameParts(record.tags, record.content)) {
        diagnostics.malformed += 1;
        continue;
      }
      // The person tier is the founder's alone. A 44229 from anyone else —
      // or any 44229 while the founder is unknown — is nobody's name here.
      if (founder === null || signer !== founder) {
        diagnostics.foreignNames += 1;
        continue;
      }
      if (person === null || compareOrder(record, person) > 0) {
        person = record;
      }
      continue;
    }
    const envelope = parseCodingSessionTitleParts(record.tags, record.content);
    if (envelope === null) {
      diagnostics.malformed += 1;
      continue;
    }
    const standing = scope.executions.some(
      (execution) =>
        execution.targetKey === envelope.targetKey &&
        execution.providerAuthorityPubkey.toLowerCase() === signer,
    );
    if (!standing) {
      diagnostics.foreignTitles += 1;
      continue;
    }
    if (generated === null || compareOrder(record, generated.record) < 0) {
      generated = { record, payload: envelope.payload };
    }
  }

  if (person !== null) {
    return {
      name: person.content,
      origin: "person",
      model: null,
      signerPubkey: null,
      diagnostics,
    };
  }
  if (generated !== null) {
    return {
      name: generated.payload.title,
      origin: "generated",
      model: generated.payload.model,
      signerPubkey: generated.record.pubkey.toLowerCase(),
      diagnostics,
    };
  }
  const founding = scope.foundingExecutionTitle;
  return {
    name:
      founding !== null && founding.trim().length > 0
        ? founding
        : UNTITLED_SESSION_NAME,
    origin: "fallback",
    model: null,
    signerPubkey: null,
    diagnostics,
  };
}
