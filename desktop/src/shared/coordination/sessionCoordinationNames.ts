/**
 * One umbrella session's display name: a person's 44229 or a provider's
 * generated 44252 title, ranked by one rule every reader shares.
 *
 * This is the TypeScript mirror of `crates/beekeeper-core/src/coding_session_title.rs`
 * (`validate_coding_session_title_parts`, `resolve_session_display_name`),
 * pinned by `conformance/session-display-name/` — the vectors are loaded
 * byte-for-byte by `features/coding-sessions/lib/codingSessionTitle.test.mjs`.
 * The rule, from that contract:
 *
 * 1. **person** — the latest valid 44229 signed by the founder;
 * 2. **generated** — only when tier 1 is empty, the **earliest** valid 44252
 *    whose signer is the `providerAuthorityPubkey` of the execution its
 *    `cs-target` names, inside this umbrella. Earliest, so a shown title never
 *    flips;
 * 3. **fallback** — the founding execution's title, then "Untitled session".
 *
 * Tiers are ranked, never timed: a person's name beats any generated title,
 * however old the name. A title from a signer without standing is ignored and
 * counted, never shown — an isolation rule, not a security one.
 *
 * It lives here rather than beside the desktop hook because the session-
 * coordination fold (Pulse, Agent Progress) routes its names through it, and
 * this directory must stay free of runtime imports from outside itself
 * (`conformance/project-pulse-fold` loads the fold under plain `node --test`).
 * `features/coding-sessions/lib/codingSessionTitle.ts` re-exports it for the
 * desktop readers. Erasable TypeScript only.
 */

import { hasDuplicateJsonKeys } from "./sessionCoordinationStrictJson.ts";

/** NIP-CSN: a person's session name. */
export const SESSION_NAME_KIND = 44229;
/** NIP-CSG § Generated title: a provider's generated session title. */
export const SESSION_GENERATED_TITLE_KIND = 44252;
/** Exact version carried by a 44229's `csnm-v` tag. */
export const CODING_SESSION_NAME_TAG_VERSION = "csnm1-1";
/** Exact version carried by a 44252's `cstl-v` tag. */
export const CODING_SESSION_TITLE_TAG_VERSION = "cstl1-1";
/** Exact `schema` of a generated-title payload. */
export const CODING_SESSION_TITLE_SCHEMA = "buzz-coding-session-title/v1";
/** A name or title is one line of at most this many UTF-8 bytes. */
export const MAX_SESSION_NAME_BYTES = 256;
/** Maximum UTF-8 byte length of a 44252's content. */
export const MAX_CODING_SESSION_TITLE_CONTENT_BYTES = 2048;
/** Maximum UTF-8 byte length of a 44252's `model`. */
export const MAX_CODING_SESSION_TITLE_MODEL_BYTES = 128;
/** The last fallback when nothing names the session. */
export const UNTITLED_SESSION_NAME = "Untitled session";

/** Mirrors `MAX_IDENTIFIER_BYTES` in `coding_session_command.rs`. */
const MAX_TARGET_IDENTIFIER_BYTES = 256;
const TARGET_KEY_PREFIX = "coding-session/v1|";
const TITLE_PAYLOAD_KEYS = [
  "schema",
  "title",
  "model",
  "basis",
  "sourceCommand",
  "createEventId",
];

/** A Nostr event without its signature, as the resolver reads it. */
export type SessionNameRecord = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
};

/** One execution of the umbrella, as the reader knows it from 44223 metadata. */
export type SessionExecutionAuthority = {
  /** The exact `cs-target` key, generation included. */
  targetKey: string;
  /** The provider pubkey that signs that generation's facts. */
  providerAuthorityPubkey: string;
};

/** What the resolver knows about the umbrella besides its events. */
export type SessionDisplayNameScope = {
  channelId: string;
  sessionRef: string;
  /** Without a founder no 44229 is a person's name. */
  founderPubkey: string | null;
  foundingExecutionTitle: string | null;
  executions: readonly SessionExecutionAuthority[];
};

export type SessionDisplayNameOrigin = "person" | "generated" | "fallback";

/** What the resolver set aside, so a reader can say so instead of hiding it. */
export type SessionDisplayNameDiagnostics = {
  /** Valid 44229s for this session not signed by the founder. */
  foreignNames: number;
  /** Valid 44252s for this session from a signer without standing. */
  foreignTitles: number;
  /** In-scope 44229/44252s that fail their envelope. */
  malformed: number;
};

/** The resolved display name of one umbrella, exactly the Rust shape. */
export type SessionDisplayName = {
  name: string;
  origin: SessionDisplayNameOrigin;
  /** Set for `generated` only. */
  model: string | null;
  /** Lowercase hex; set for `generated` only. */
  signerPubkey: string | null;
  diagnostics: SessionDisplayNameDiagnostics;
};

/** {@link SessionDisplayName} plus the winning record, for readers that cite it. */
export type SessionDisplayNameWithSource = SessionDisplayName & {
  /** The winning 44229 or 44252, or null for `fallback`. */
  source: SessionNameRecord | null;
};

/** The strict JSON content of a 44252. */
export type CodingSessionTitlePayload = {
  schema: string;
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

/** A decoded `coding-session/v1` target key. */
export type SessionTitleTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

const encoder = new TextEncoder();

function utf8Bytes(value: string): number {
  return encoder.encode(value).byteLength;
}

/** `char::is_control` — the Unicode `Cc` category. */
function hasControlCharacter(value: string): boolean {
  return /\p{Cc}/u.test(value);
}

/** What `uuid::Uuid::parse_str` accepts: simple, hyphenated, braced, urn. */
function isParseableUuid(value: string): boolean {
  const hyphenated =
    /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/;
  if (/^[0-9a-fA-F]{32}$/.test(value) || hyphenated.test(value)) return true;
  if (value.startsWith("{") && value.endsWith("}")) {
    return hyphenated.test(value.slice(1, -1));
  }
  return (
    value.startsWith("urn:uuid:") &&
    hyphenated.test(value.slice("urn:uuid:".length))
  );
}

/** A `d` sessionRef: the lowercase canonical hyphenated UUID, exactly. */
function isCanonicalSessionRef(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
    value,
  );
}

/** `validate_coding_session_name_content`: one line, text, at most 256 bytes. */
export function isValidSessionNameContent(content: string): boolean {
  return (
    utf8Bytes(content) <= MAX_SESSION_NAME_BYTES &&
    content.trim().length > 0 &&
    !content.includes("\r") &&
    !content.includes("\n")
  );
}

function hasTwoFieldTags(tags: readonly unknown[], count: number): boolean {
  return (
    tags.length === count &&
    tags.every(
      (tag) =>
        Array.isArray(tag) &&
        tag.length === 2 &&
        typeof tag[0] === "string" &&
        typeof tag[1] === "string",
    )
  );
}

/** `validate_coding_session_name_parts`: the exact 44229 envelope. */
export function isValidSessionNameParts(
  tags: readonly string[][],
  content: string,
): boolean {
  return (
    isValidSessionNameContent(content) &&
    hasTwoFieldTags(tags, 3) &&
    tags[0][0] === "h" &&
    isParseableUuid(tags[0][1]) &&
    tags[1][0] === "d" &&
    isCanonicalSessionRef(tags[1][1]) &&
    tags[2][0] === "csnm-v" &&
    tags[2][1] === CODING_SESSION_NAME_TAG_VERSION
  );
}

function isLowerHexEventId(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

/** `parse_coding_session_title_content`: strict v1 JSON, or null. */
export function parseCodingSessionTitleContent(
  content: string,
): CodingSessionTitlePayload | null {
  if (utf8Bytes(content) > MAX_CODING_SESSION_TITLE_CONTENT_BYTES) return null;
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch {
    return null;
  }
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    hasDuplicateJsonKeys(content)
  ) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record);
  if (
    keys.length !== TITLE_PAYLOAD_KEYS.length ||
    !TITLE_PAYLOAD_KEYS.every((key) => Object.hasOwn(record, key))
  ) {
    return null;
  }
  const { schema, title, model, basis, sourceCommand, createEventId } = record;
  if (
    schema !== CODING_SESSION_TITLE_SCHEMA ||
    typeof title !== "string" ||
    !isValidSessionNameContent(title) ||
    typeof model !== "string" ||
    model.trim().length === 0 ||
    utf8Bytes(model) > MAX_CODING_SESSION_TITLE_MODEL_BYTES ||
    hasControlCharacter(model) ||
    basis !== "first-message" ||
    (sourceCommand !== null && !isLowerHexEventId(sourceCommand)) ||
    !isLowerHexEventId(createEventId)
  ) {
    return null;
  }
  return { schema, title, model, basis, sourceCommand, createEventId };
}

/**
 * `parse_coding_session_target_key`: decode a `cs-target` key strictly — it
 * must re-encode to exactly the input — or null.
 */
export function parseSessionTitleTargetKey(
  key: string,
): SessionTitleTarget | null {
  if (!key.startsWith(TARGET_KEY_PREFIX)) return null;
  const bytes = encoder.encode(key.slice(TARGET_KEY_PREFIX.length));
  const decoder = new TextDecoder("utf-8", { fatal: true });
  const fields: string[] = [];
  let offset = 0;
  while (offset < bytes.length) {
    let colon = offset;
    while (colon < bytes.length && bytes[colon] !== 0x3a) colon += 1;
    if (colon === offset || colon === bytes.length) return null;
    const digits = decoder.decode(bytes.subarray(offset, colon));
    if (!/^[0-9]+$/.test(digits)) return null;
    const length = Number(digits);
    const start = colon + 1;
    if (!Number.isSafeInteger(length) || start + length > bytes.length) {
      return null;
    }
    try {
      fields.push(decoder.decode(bytes.subarray(start, start + length)));
    } catch {
      return null;
    }
    offset = start + length;
  }
  if (fields.length !== 4) return null;
  const [driver, instanceId, sessionId, generationText] = fields;
  for (const value of [driver, instanceId, sessionId]) {
    if (
      value.trim().length === 0 ||
      utf8Bytes(value) > MAX_TARGET_IDENTIFIER_BYTES ||
      hasControlCharacter(value)
    ) {
      return null;
    }
  }
  if (!/^[0-9]+$/.test(generationText)) return null;
  const generation = Number(generationText);
  if (!Number.isSafeInteger(generation) || generation <= 0) return null;
  const reencoded = `${TARGET_KEY_PREFIX}${[
    driver,
    instanceId,
    sessionId,
    String(generation),
  ]
    .map((field) => `${utf8Bytes(field)}:${field}`)
    .join("")}`;
  if (reencoded !== key) return null;
  return { driver, instanceId, sessionId, generation };
}

/**
 * `validate_coding_session_title_parts`: the exact ordered 44252 envelope —
 * `h`, `d`, `cstl-v`, `cs-target`, two fields each — and strict v1 content.
 */
export function parseCodingSessionTitleParts(
  tags: readonly string[][],
  content: string,
): CodingSessionTitleEnvelope | null {
  if (
    !hasTwoFieldTags(tags, 4) ||
    tags[0][0] !== "h" ||
    !isParseableUuid(tags[0][1]) ||
    tags[1][0] !== "d" ||
    !isCanonicalSessionRef(tags[1][1]) ||
    tags[2][0] !== "cstl-v" ||
    tags[2][1] !== CODING_SESSION_TITLE_TAG_VERSION ||
    tags[3][0] !== "cs-target" ||
    parseSessionTitleTargetKey(tags[3][1]) === null
  ) {
    return null;
  }
  const payload = parseCodingSessionTitleContent(content);
  if (!payload) return null;
  return {
    channelId: tags[0][1],
    sessionRef: tags[1][1],
    targetKey: tags[3][1],
    payload,
  };
}

function carriesTag(
  record: SessionNameRecord,
  name: string,
  value: string,
): boolean {
  return record.tags.some(
    (tag) =>
      Array.isArray(tag) &&
      tag.length >= 2 &&
      tag[0] === name &&
      tag[1] === value,
  );
}

/** `(created_at, id)`, ids compared as lowercase strings. */
function compareOrder(
  left: SessionNameRecord,
  right: SessionNameRecord,
): number {
  if (left.created_at !== right.created_at) {
    return left.created_at < right.created_at ? -1 : 1;
  }
  const leftId = left.id.toLowerCase();
  const rightId = right.id.toLowerCase();
  return leftId < rightId ? -1 : leftId > rightId ? 1 : 0;
}

/**
 * {@link resolveSessionDisplayName}, also returning the winning record so a
 * reader can cite it (Pulse lists it in `sourceEventIds`).
 */
export function resolveSessionDisplayNameWithSource(
  scope: SessionDisplayNameScope,
  records: Iterable<SessionNameRecord>,
): SessionDisplayNameWithSource {
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
      record.kind !== SESSION_NAME_KIND &&
      record.kind !== SESSION_GENERATED_TITLE_KIND
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
    if (record.kind === SESSION_NAME_KIND) {
      if (!isValidSessionNameParts(record.tags, record.content)) {
        diagnostics.malformed += 1;
        continue;
      }
      if (founder !== signer) {
        diagnostics.foreignNames += 1;
        continue;
      }
      if (!person || compareOrder(record, person) > 0) person = record;
      continue;
    }
    const envelope = parseCodingSessionTitleParts(record.tags, record.content);
    if (!envelope) {
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
    if (!generated || compareOrder(record, generated.record) < 0) {
      generated = { record, payload: envelope.payload };
    }
  }

  if (person) {
    return {
      name: person.content,
      origin: "person",
      model: null,
      signerPubkey: null,
      diagnostics,
      source: person,
    };
  }
  if (generated) {
    return {
      name: generated.payload.title,
      origin: "generated",
      model: generated.payload.model,
      signerPubkey: generated.record.pubkey.toLowerCase(),
      diagnostics,
      source: generated.record,
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
    source: null,
  };
}

/**
 * `resolve_session_display_name`: one umbrella's display name from its 44229
 * and 44252 events, in any order. Signatures are the caller's job.
 */
export function resolveSessionDisplayName(
  scope: SessionDisplayNameScope,
  records: Iterable<SessionNameRecord>,
): SessionDisplayName {
  const { source: _source, ...resolved } = resolveSessionDisplayNameWithSource(
    scope,
    records,
  );
  return resolved;
}

/** 44229/44252 records grouped by their first `d` value, for repeated lookups. */
export type SessionNameRecordIndex = ReadonlyMap<string, SessionNameRecord[]>;

/** Group every 44229/44252 by `d`, so each session resolves over its own. */
export function indexSessionNameRecords(
  records: Iterable<SessionNameRecord>,
): SessionNameRecordIndex {
  const index = new Map<string, SessionNameRecord[]>();
  for (const record of records) {
    if (
      record.kind !== SESSION_NAME_KIND &&
      record.kind !== SESSION_GENERATED_TITLE_KIND
    ) {
      continue;
    }
    const sessionRef = record.tags.find(
      (tag) =>
        Array.isArray(tag) && tag[0] === "d" && typeof tag[1] === "string",
    )?.[1];
    if (!sessionRef) continue;
    const bucket = index.get(sessionRef);
    if (bucket) bucket.push(record);
    else index.set(sessionRef, [record]);
  }
  return index;
}

/** NIP-CSG: the genesis that founds an umbrella; its signer is the founder. */
export const SESSION_GENESIS_KIND = 44226;
/** `MAX_GENESIS_CONTENT_BYTES` in `coding_session_genesis.rs`. */
const MAX_GENESIS_CONTENT_BYTES = 1024;
const HEX64 = /^[0-9a-f]{64}$/;

/**
 * `decode_coding_session_genesis`: the founded `sessionRef` of a 44226's
 * content, or null. Exactly `{sessionRef, v}` or `{sessionRef, v, adopts:
 * {createEventId, receiptEventId}}`, no duplicate keys, `v` the integer 1.
 */
export function decodeSessionGenesisSessionRef(content: string): string | null {
  if (utf8Bytes(content) > MAX_GENESIS_CONTENT_BYTES) return null;
  let value: unknown;
  try {
    value = JSON.parse(content);
  } catch {
    return null;
  }
  if (
    typeof value !== "object" ||
    value === null ||
    Array.isArray(value) ||
    hasDuplicateJsonKeys(content)
  ) {
    return null;
  }
  const record = value as Record<string, unknown>;
  const hasAdopts = Object.hasOwn(record, "adopts");
  const expected = hasAdopts
    ? ["sessionRef", "v", "adopts"]
    : ["sessionRef", "v"];
  const keys = Object.keys(record);
  if (
    keys.length !== expected.length ||
    !expected.every((key) => Object.hasOwn(record, key))
  ) {
    return null;
  }
  // serde reads `v` as a u64: `1.0` or `1e0` is not one, though JSON.parse
  // cannot tell them from `1`.
  if (record.v !== 1 || /"v"\s*:\s*1[.eE]/.test(content)) return null;
  if (typeof record.sessionRef !== "string") return null;
  if (!isCanonicalSessionRef(record.sessionRef)) return null;
  if (hasAdopts) {
    const adopts = record.adopts;
    if (
      typeof adopts !== "object" ||
      adopts === null ||
      Array.isArray(adopts)
    ) {
      return null;
    }
    const { createEventId, receiptEventId, ...rest } = adopts as Record<
      string,
      unknown
    >;
    if (
      Object.keys(rest).length > 0 ||
      typeof createEventId !== "string" ||
      typeof receiptEventId !== "string" ||
      !HEX64.test(createEventId) ||
      !HEX64.test(receiptEventId)
    ) {
      return null;
    }
  }
  return record.sessionRef;
}

/** One decodable 44226, as the founder lookup needs it. */
export type SessionGenesisWitness = {
  channelId: string;
  /** Lowercase hex: the founder. */
  founderPubkey: string;
  sessionRef: string;
};

/** 44226 genesis by event id — looked up by id only, never by label. */
export type SessionGenesisIndex = ReadonlyMap<string, SessionGenesisWitness>;

/**
 * Index every decodable 44226 by its event id, with its channel (first
 * non-empty `h`), signer and founded `sessionRef` — the Rust Pulse fold's
 * `NameInputs::observe` for genesis. A genesis is reached only through the
 * `genesisRef` an accepted create names, never by its `csg-session` label
 * (`coding_session_genesis.rs`, "what the tag does not mean").
 */
export function indexSessionGeneses(
  events: Iterable<SessionNameRecord>,
): SessionGenesisIndex {
  const index = new Map<string, SessionGenesisWitness>();
  for (const event of events) {
    if (event.kind !== SESSION_GENESIS_KIND) continue;
    const channelId = event.tags.find(
      (tag) => Array.isArray(tag) && tag[0] === "h",
    )?.[1];
    if (typeof channelId !== "string" || channelId.length === 0) continue;
    const sessionRef = decodeSessionGenesisSessionRef(event.content);
    if (!sessionRef) continue;
    index.set(event.id, {
      channelId,
      founderPubkey: event.pubkey.toLowerCase(),
      sessionRef,
    });
  }
  return index;
}

/**
 * One accepted generation of a coordinated session, as the name resolver
 * needs it: its exact target and provider, and — for a `session.create` — the
 * genesis that create names.
 */
export type CoordinatedNameWitness = {
  targetKey: string;
  providerAuthorityPubkey: string;
  /** The accepted `session.create`'s `action.genesisRef`, else null. */
  genesisRef: string | null;
};

/** `action.genesisRef` of an accepted (already strictly validated) create. */
function createGenesisRef(command: SessionNameRecord): string | null {
  try {
    const content = JSON.parse(command.content) as {
      action?: { type?: unknown; genesisRef?: unknown };
    };
    const genesisRef = content.action?.genesisRef;
    return content.action?.type === "session.create" &&
      typeof genesisRef === "string" &&
      HEX64.test(genesisRef)
      ? genesisRef
      : null;
  } catch {
    return null;
  }
}

/**
 * Record one accepted generation as a name witness for its session: its exact
 * target, its provider, and — for a `session.create` — the genesis it names,
 * whose signer is the founder.
 */
export function witnessSessionName(
  witnesses: Map<string, CoordinatedNameWitness[]>,
  sessionKey: string,
  generation: { targetKey: string; providerAuthorityPubkey: string },
  action: string,
  command: SessionNameRecord,
): void {
  const list = witnesses.get(sessionKey) ?? [];
  list.push({
    targetKey: generation.targetKey,
    providerAuthorityPubkey: generation.providerAuthorityPubkey,
    genesisRef: action === "create" ? createGenesisRef(command) : null,
  });
  witnesses.set(sessionKey, list);
}

/** Where a coordinated session's name came from; never `fallback`. */
export type CoordinatedSessionNameOrigin = {
  origin: "person" | "generated";
  model: string | null;
  signerPubkey: string | null;
};

/**
 * The founder the session's accepted creates prove through the geneses they
 * name: same channel, same `sessionRef`. None, or two different founders (a
 * dispute), is null — and a null founder makes no 44229 a person's name.
 */
export function provenSessionFounder(input: {
  channelId: string;
  sessionRef: string;
  witnesses: readonly CoordinatedNameWitness[];
  geneses: SessionGenesisIndex;
}): string | null {
  const founders = new Set<string>();
  for (const witness of input.witnesses) {
    if (!witness.genesisRef) continue;
    const genesis = input.geneses.get(witness.genesisRef);
    if (
      genesis &&
      genesis.channelId === input.channelId &&
      genesis.sessionRef === input.sessionRef
    ) {
      founders.add(genesis.founderPubkey);
    }
  }
  return founders.size === 1 ? [...founders][0] : null;
}

/**
 * Resolve a coordinated session's name for the shared fold (Pulse, Agent
 * Progress), exactly as the Rust Pulse fold does (`pulse_fold_names.rs`).
 *
 * The founder is the signer of the 44226 genesis the session's accepted
 * `session.create` names by `genesisRef` ({@link provenSessionFounder}) —
 * never a create's own signer, which may be a delegate. Its executions are its
 * accepted generations, each with the provider authority its own receipt
 * proved. A `fallback` answer is reported as no name at all (`name: null`):
 * the fold has always said "unnamed" and left the label to its adapter.
 */
export function resolveCoordinatedSessionName(input: {
  channelId: string;
  sessionRef: string;
  witnesses: readonly CoordinatedNameWitness[];
  geneses: SessionGenesisIndex;
  index: SessionNameRecordIndex;
}):
  | (CoordinatedSessionNameOrigin & { name: string; sourceEventId: string })
  | null {
  const resolved = resolveSessionDisplayNameWithSource(
    {
      channelId: input.channelId,
      sessionRef: input.sessionRef,
      founderPubkey: provenSessionFounder(input),
      foundingExecutionTitle: null,
      executions: input.witnesses.map((witness) => ({
        targetKey: witness.targetKey,
        providerAuthorityPubkey: witness.providerAuthorityPubkey,
      })),
    },
    input.index.get(input.sessionRef) ?? [],
  );
  if (resolved.origin === "fallback" || !resolved.source) return null;
  return {
    name: resolved.name,
    origin: resolved.origin,
    model: resolved.model,
    signerPubkey: resolved.signerPubkey,
    sourceEventId: resolved.source.id,
  };
}
