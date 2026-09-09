/**
 * The strict decoder for kind-44247 handover records (NIP-CSH, `csh1`).
 *
 * Two record types travel on this kind and they answer different questions:
 * a `checkpoint` is one participant's durable statement of *what the work is*
 * (`docs/HANDOVER_IMPL.md` §2.1), and a `continuation` is the claimant's
 * statement of *what they did about it* (§2.2). Neither carries authority —
 * standing is decided by a reader against the 44228 chain in
 * {@link file://./codingSessionHandoverFold.ts} — so this file's whole job is
 * to say whether the bytes are exactly the declared shape.
 *
 * Exact-key in both directions, like `codingSessionObservationWire.ts`: one
 * extra key, one missing key, or a `null` where a value is required is a
 * refusal that **names the key**, never a best guess. A checkpoint whose
 * `revision.preserved` this build cannot read must not render as "everything
 * was preserved"; that is the class of lie this decoder exists to prevent.
 *
 * The decode entry point does not throw: the fold reports every refused event
 * under `excluded` with the reason, and an exception there would lose the
 * other records in the same read.
 */
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_HANDOVER } from "@/shared/constants/kinds";
import { hasDuplicateJsonKeys } from "@/shared/coordination/sessionCoordinationStrictJson";
import { hasExactKeys, isPlainRecord } from "./codingSessionWireDecode";

/**
 * Kind 44247 — re-exported from the shared registry, never re-declared.
 *
 * One number, in one place: this module is where every handover decoder reads
 * it, and the registry is where the app's kinds live.
 */
export { KIND_CODING_SESSION_HANDOVER } from "@/shared/constants/kinds";

/** `csh-v` — the one envelope version this build reads. */
export const CODING_SESSION_HANDOVER_TAG_VERSION = "csh1";
/** The payload schema string every 44247 content carries. */
export const CODING_SESSION_HANDOVER_SCHEMA = "buzz-coding-session-handover/v1";

/** The record types §2 declares. Closed: an unknown type is refused. */
export type CodingSessionHandoverType = "checkpoint" | "continuation";
/** What a continuation did — the two labelled outcomes, never conflated. */
export type CodingSessionContinuationMode = "native-resume" | "reconstructed";
/** How much of the dirty tree the checkpoint's artifacts actually hold. */
export type CodingSessionCheckpointPreserved = "all" | "partial" | "none";
/** The three words a 44244 `report.tests[].outcome` also uses. */
export type CodingSessionHandoverTestOutcome = "passed" | "failed" | "not-run";

const HEX64 = /^[0-9a-f]{64}$/;
const HEX_SHA = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const SHA256 = /^[0-9a-f]{64}$/;
const UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

const MAX_CONTENT_BYTES = 32 * 1024;
const MAX_TASK_BYTES = 4 * 1024;
const MAX_NEXT_ACTION_BYTES = 2 * 1024;
const MAX_NOTE_BYTES = 2 * 1024;
const MAX_LINE_BYTES = 512;
const MAX_REFERENCE_BYTES = 512;
const MAX_ASSIGNMENT_REFS = 16;
const MAX_DECISIONS = 32;
const MAX_ARTIFACTS = 16;
const MAX_TESTS = 32;
const MAX_UNRESOLVED = 32;
const MAX_MISSING = 16;
const MAX_RECOVERED = 32;

const encoder = new TextEncoder();

/** One decoded 44247 record. `body` is the type's own payload. */
export type CodingSessionHandoverRecord = {
  readonly eventId: string;
  readonly author: string;
  readonly createdAt: number;
  readonly sessionRef: string;
  readonly genesisRef: string;
} & (
  | { readonly type: "checkpoint"; readonly body: CodingSessionCheckpointBody }
  | {
      readonly type: "continuation";
      readonly body: CodingSessionContinuationBody;
    }
);

/** The revision a checkpoint was written against. */
export type CodingSessionCheckpointRevision = {
  readonly repoRef: string | null;
  readonly baseSha: string | null;
  readonly headSha: string | null;
  readonly branch: string | null;
  readonly dirty: boolean;
  /**
   * Of the uncommitted bytes, how much the artifacts actually hold.
   *
   * Required, and the only honest answer to "was my work preserved". A
   * `patch` artifact beside `partial` still means work was left behind — a
   * reader that inferred preservation from the artifact list would say the
   * opposite of what the author measured.
   */
  readonly preserved: CodingSessionCheckpointPreserved;
};

/** A committed branch tip the author pushed for the next participant. */
export type CodingSessionWipRefArtifact = {
  readonly kind: "wip-ref";
  readonly repoRef: string;
  readonly ref: string;
  readonly sha: string;
};

/** Uncommitted bytes carried as a NIP-34 patch event. */
export type CodingSessionPatchArtifact = {
  readonly kind: "patch";
  readonly repoRef: string;
  readonly eventId: string;
  readonly baseSha: string;
  readonly bytes: number;
};

/** Uncommitted bytes too large for an event, carried as a Blossom blob. */
export type CodingSessionBlobArtifact = {
  readonly kind: "blob";
  readonly repoRef: string;
  readonly hash: string;
  readonly baseSha: string;
  readonly bytes: number;
};

export type CodingSessionHandoverArtifact =
  | CodingSessionWipRefArtifact
  | CodingSessionPatchArtifact
  | CodingSessionBlobArtifact;

/** One decision the author wants the next participant to keep. */
export type CodingSessionHandoverDecision = {
  readonly eventId: string;
  readonly summary: string;
};

/** One gate the author ran, and what it said. */
export type CodingSessionHandoverTest = {
  readonly name: string;
  readonly command: string;
  readonly outcome: CodingSessionHandoverTestOutcome;
};

/** §2.1 — the author's durable statement of the work. */
export type CodingSessionCheckpointBody = {
  /**
   * The checkpoint of this author's that this one replaces, or `null`.
   *
   * Always present, like `prevAccepted` on a chain link: an absent key is
   * refused by name rather than read as "replaces nothing". The field exists
   * because a clock cannot order two checkpoints written in the same second —
   * the id, a hash, decided which was "latest", and a reconstruction started
   * from the older statement. This is the author's own statement of order, and
   * within one author it is the only ordering a reader trusts.
   */
  readonly prevCheckpointRef: string | null;
  readonly task: string;
  readonly assignmentRefs: readonly string[];
  readonly decisions: readonly CodingSessionHandoverDecision[];
  readonly revision: CodingSessionCheckpointRevision;
  readonly artifacts: readonly CodingSessionHandoverArtifact[];
  readonly tests: readonly CodingSessionHandoverTest[];
  readonly unresolved: readonly string[];
  readonly nextAction: string;
  readonly missing: readonly string[];
};

/** The execution now carrying the work. */
export type CodingSessionHandoverTarget = {
  readonly driver: string;
  readonly instanceId: string;
  readonly sessionId: string;
  readonly generation: number;
};

/** §2.2 — what the claimant did with the claim. */
export type CodingSessionContinuationBody = {
  readonly claimRef: string;
  readonly mode: CodingSessionContinuationMode;
  readonly checkpointRef: string | null;
  readonly target: CodingSessionHandoverTarget;
  readonly recovered: readonly string[];
  readonly missing: readonly string[];
  readonly note: string | null;
};

/** A refusal that says which key was wrong, so a reader can act on it. */
export class CodingSessionHandoverWireError extends Error {
  constructor(message: string) {
    super(`coding-session handover record: ${message}`);
    this.name = "CodingSessionHandoverWireError";
  }
}

/** Either one decoded record, or the exact reason it was refused. */
export type CodingSessionHandoverDecode =
  | { readonly ok: true; readonly value: CodingSessionHandoverRecord }
  | { readonly ok: false; readonly reason: string };

function refuse(message: string): never {
  throw new CodingSessionHandoverWireError(message);
}

function quote(key: string): string {
  return `"${key}"`;
}

function record(value: unknown, at: string): Record<string, unknown> {
  if (!isPlainRecord(value)) refuse(`${at} must be an object`);
  return value;
}

function exact(
  value: unknown,
  keys: readonly string[],
  at: string,
): Record<string, unknown> {
  const object = record(value, at);
  if (hasExactKeys(object, keys)) return object;
  const missing = keys.filter((key) => !Object.hasOwn(object, key));
  if (missing.length > 0) {
    refuse(`${at} is missing ${missing.map(quote).join(", ")}`);
  }
  const extra = Object.keys(object).filter((key) => !keys.includes(key));
  refuse(`${at} carries unsupported ${extra.map(quote).join(", ")}`);
}

function bytes(value: string): number {
  return encoder.encode(value).byteLength;
}

function boundedText(
  object: Record<string, unknown>,
  key: string,
  maxBytes: number,
  at: string,
): string {
  const value = object[key];
  if (
    typeof value !== "string" ||
    value.trim().length === 0 ||
    bytes(value) > maxBytes
  ) {
    refuse(
      `${at} field ${quote(key)} must be a non-empty string of at most ${maxBytes} bytes`,
    );
  }
  return value;
}

function boundedNullableText(
  object: Record<string, unknown>,
  key: string,
  maxBytes: number,
  at: string,
): string | null {
  if (object[key] === null) return null;
  return boundedText(object, key, maxBytes, at);
}

function hex64(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string {
  const value = object[key];
  if (typeof value !== "string" || !HEX64.test(value)) {
    refuse(`${at} field ${quote(key)} must be a lowercase 64-hex id`);
  }
  return value;
}

function nullableHex64(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string | null {
  if (object[key] === null) return null;
  return hex64(object, key, at);
}

function sha(object: Record<string, unknown>, key: string, at: string): string {
  const value = object[key];
  if (typeof value !== "string" || !HEX_SHA.test(value)) {
    refuse(`${at} field ${quote(key)} must be a 40- or 64-hex commit sha`);
  }
  return value;
}

function nullableSha(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string | null {
  if (object[key] === null) return null;
  return sha(object, key, at);
}

function flag(
  object: Record<string, unknown>,
  key: string,
  at: string,
): boolean {
  const value = object[key];
  if (typeof value !== "boolean") {
    refuse(`${at} field ${quote(key)} must be a boolean`);
  }
  return value;
}

function positiveCount(
  object: Record<string, unknown>,
  key: string,
  at: string,
): number {
  const value = object[key];
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    refuse(`${at} field ${quote(key)} must be a non-negative integer`);
  }
  return value as number;
}

function word<T extends string>(
  object: Record<string, unknown>,
  key: string,
  allowed: readonly string[],
  at: string,
): T {
  const value = object[key];
  if (typeof value !== "string" || !allowed.includes(value)) {
    refuse(
      `${at} field ${quote(key)} must be one of ${allowed.map(quote).join(", ")}`,
    );
  }
  return value as T;
}

function boundedList(
  object: Record<string, unknown>,
  key: string,
  maxEntries: number,
  maxBytes: number,
  at: string,
): readonly string[] {
  const value = object[key];
  if (!Array.isArray(value) || value.length > maxEntries) {
    refuse(
      `${at} field ${quote(key)} must be an array of at most ${maxEntries} strings`,
    );
  }
  for (const entry of value) {
    if (
      typeof entry !== "string" ||
      entry.trim().length === 0 ||
      bytes(entry) > maxBytes
    ) {
      refuse(
        `${at} field ${quote(key)} entries must be non-empty strings of at most ${maxBytes} bytes`,
      );
    }
  }
  return Object.freeze([...(value as string[])]);
}

function rows<T>(
  object: Record<string, unknown>,
  key: string,
  maxEntries: number,
  at: string,
  decodeRow: (value: unknown, rowAt: string) => T,
): readonly T[] {
  const value = object[key];
  if (!Array.isArray(value) || value.length > maxEntries) {
    refuse(
      `${at} field ${quote(key)} must be an array of at most ${maxEntries} entries`,
    );
  }
  return Object.freeze(
    value.map((entry, index) => decodeRow(entry, `${at}.${key}[${index}]`)),
  );
}

const REVISION_KEYS = [
  "repoRef",
  "baseSha",
  "headSha",
  "branch",
  "dirty",
  "preserved",
] as const;

function decodeRevision(value: unknown): CodingSessionCheckpointRevision {
  const at = "checkpoint.revision";
  const object = exact(value, REVISION_KEYS, at);
  return Object.freeze({
    repoRef: boundedNullableText(object, "repoRef", MAX_REFERENCE_BYTES, at),
    baseSha: nullableSha(object, "baseSha", at),
    headSha: nullableSha(object, "headSha", at),
    branch: boundedNullableText(object, "branch", MAX_REFERENCE_BYTES, at),
    dirty: flag(object, "dirty", at),
    preserved: word<CodingSessionCheckpointPreserved>(
      object,
      "preserved",
      ["all", "partial", "none"],
      at,
    ),
  });
}

const WIP_REF_KEYS = ["kind", "repoRef", "ref", "sha"] as const;
const PATCH_KEYS = ["kind", "repoRef", "eventId", "baseSha", "bytes"] as const;
const BLOB_KEYS = ["kind", "repoRef", "hash", "baseSha", "bytes"] as const;

function decodeArtifact(
  value: unknown,
  at: string,
): CodingSessionHandoverArtifact {
  const kind = record(value, at).kind;
  if (kind === "wip-ref") {
    const object = exact(value, WIP_REF_KEYS, at);
    return Object.freeze({
      kind: "wip-ref" as const,
      repoRef: boundedText(object, "repoRef", MAX_REFERENCE_BYTES, at),
      ref: boundedText(object, "ref", MAX_REFERENCE_BYTES, at),
      sha: sha(object, "sha", at),
    });
  }
  if (kind === "patch") {
    const object = exact(value, PATCH_KEYS, at);
    return Object.freeze({
      kind: "patch" as const,
      repoRef: boundedText(object, "repoRef", MAX_REFERENCE_BYTES, at),
      eventId: hex64(object, "eventId", at),
      baseSha: sha(object, "baseSha", at),
      bytes: positiveCount(object, "bytes", at),
    });
  }
  if (kind === "blob") {
    const object = exact(value, BLOB_KEYS, at);
    const hash = object.hash;
    if (typeof hash !== "string" || !SHA256.test(hash)) {
      refuse(`${at} field "hash" must be a lowercase sha256 hex digest`);
    }
    return Object.freeze({
      kind: "blob" as const,
      repoRef: boundedText(object, "repoRef", MAX_REFERENCE_BYTES, at),
      hash,
      baseSha: sha(object, "baseSha", at),
      bytes: positiveCount(object, "bytes", at),
    });
  }
  return refuse(`${at} field "kind" must be "wip-ref", "patch" or "blob"`);
}

function decodeDecision(
  value: unknown,
  at: string,
): CodingSessionHandoverDecision {
  const object = exact(value, ["eventId", "summary"], at);
  return Object.freeze({
    eventId: hex64(object, "eventId", at),
    summary: boundedText(object, "summary", MAX_LINE_BYTES, at),
  });
}

function decodeTest(value: unknown, at: string): CodingSessionHandoverTest {
  const object = exact(value, ["name", "command", "outcome"], at);
  return Object.freeze({
    name: boundedText(object, "name", MAX_REFERENCE_BYTES, at),
    command: boundedText(object, "command", MAX_REFERENCE_BYTES, at),
    outcome: word<CodingSessionHandoverTestOutcome>(
      object,
      "outcome",
      ["passed", "failed", "not-run"],
      at,
    ),
  });
}

const CHECKPOINT_KEYS = [
  "prevCheckpointRef",
  "task",
  "assignmentRefs",
  "decisions",
  "revision",
  "artifacts",
  "tests",
  "unresolved",
  "nextAction",
  "missing",
] as const;

function decodeCheckpointBody(value: unknown): CodingSessionCheckpointBody {
  const at = "checkpoint";
  const object = exact(value, CHECKPOINT_KEYS, at);
  const assignmentRefs = object.assignmentRefs;
  if (
    !Array.isArray(assignmentRefs) ||
    assignmentRefs.length > MAX_ASSIGNMENT_REFS ||
    !assignmentRefs.every(
      (entry) => typeof entry === "string" && HEX64.test(entry),
    )
  ) {
    refuse(
      `${at} field "assignmentRefs" must be at most ${MAX_ASSIGNMENT_REFS} lowercase 64-hex ids`,
    );
  }
  return Object.freeze({
    prevCheckpointRef: nullableHex64(object, "prevCheckpointRef", at),
    task: boundedText(object, "task", MAX_TASK_BYTES, at),
    assignmentRefs: Object.freeze([...(assignmentRefs as string[])]),
    decisions: rows(object, "decisions", MAX_DECISIONS, at, decodeDecision),
    revision: decodeRevision(object.revision),
    artifacts: rows(object, "artifacts", MAX_ARTIFACTS, at, decodeArtifact),
    tests: rows(object, "tests", MAX_TESTS, at, decodeTest),
    unresolved: boundedList(
      object,
      "unresolved",
      MAX_UNRESOLVED,
      MAX_LINE_BYTES,
      at,
    ),
    nextAction: boundedText(object, "nextAction", MAX_NEXT_ACTION_BYTES, at),
    missing: boundedList(object, "missing", MAX_MISSING, MAX_LINE_BYTES, at),
  });
}

const TARGET_KEYS = [
  "driver",
  "instanceId",
  "sessionId",
  "generation",
] as const;

function decodeTarget(value: unknown): CodingSessionHandoverTarget {
  const at = "continuation.target";
  const object = exact(value, TARGET_KEYS, at);
  const generation = object.generation;
  if (!Number.isSafeInteger(generation) || (generation as number) <= 0) {
    refuse(`${at} field "generation" must be a positive integer`);
  }
  return Object.freeze({
    driver: boundedText(object, "driver", MAX_REFERENCE_BYTES, at),
    instanceId: boundedText(object, "instanceId", MAX_REFERENCE_BYTES, at),
    sessionId: boundedText(object, "sessionId", MAX_REFERENCE_BYTES, at),
    generation: generation as number,
  });
}

const CONTINUATION_KEYS = [
  "claimRef",
  "mode",
  "checkpointRef",
  "target",
  "recovered",
  "missing",
  "note",
] as const;

function decodeContinuationBody(value: unknown): CodingSessionContinuationBody {
  const at = "continuation";
  const object = exact(value, CONTINUATION_KEYS, at);
  return Object.freeze({
    claimRef: hex64(object, "claimRef", at),
    mode: word<CodingSessionContinuationMode>(
      object,
      "mode",
      ["native-resume", "reconstructed"],
      at,
    ),
    checkpointRef: nullableHex64(object, "checkpointRef", at),
    target: decodeTarget(object.target),
    recovered: boundedList(
      object,
      "recovered",
      MAX_RECOVERED,
      MAX_LINE_BYTES,
      at,
    ),
    missing: boundedList(object, "missing", MAX_MISSING, MAX_LINE_BYTES, at),
    note: boundedNullableText(object, "note", MAX_NOTE_BYTES, at),
  });
}

const PAYLOAD_KEYS = [
  "schema",
  "sessionRef",
  "genesisRef",
  "type",
  "body",
] as const;

/** The five tags a 44247 envelope carries, in this exact order. */
const TAG_NAMES = ["h", "d", "csh-v", "csh-genesis", "csh-type"] as const;

function readExactTags(tags: unknown): readonly string[] {
  if (!Array.isArray(tags) || tags.length !== TAG_NAMES.length) {
    refuse(
      `envelope must carry exactly the tags ${TAG_NAMES.map(quote).join(", ")}`,
    );
  }
  return TAG_NAMES.map((name, index) => {
    const tag: unknown = tags[index];
    if (
      !Array.isArray(tag) ||
      tag.length !== 2 ||
      tag[0] !== name ||
      typeof tag[1] !== "string" ||
      tag[1].length === 0
    ) {
      refuse(`envelope tag ${index} must be a two-value ${quote(name)} tag`);
    }
    return tag[1] as string;
  });
}

/**
 * Decode one 44247 event against the scope a caller already trusts.
 *
 * The caller supplies the channel, umbrella and genesis it asked the relay
 * for; an event that disagrees with any of them is refused rather than
 * re-scoped, because a record that names a different session is not this
 * session's evidence no matter how well formed it is.
 *
 * This never throws: the fold lists refusals beside the records it accepted.
 */
export function decodeCodingSessionHandoverEvent(input: {
  event: RelayEvent;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
}): CodingSessionHandoverDecode {
  try {
    return { ok: true, value: decodeOrThrow(input) };
  } catch (error) {
    if (error instanceof CodingSessionHandoverWireError) {
      return { ok: false, reason: error.message };
    }
    throw error;
  }
}

function decodeOrThrow(input: {
  event: RelayEvent;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
}): CodingSessionHandoverRecord {
  const { event } = input;
  if (event.kind !== KIND_CODING_SESSION_HANDOVER) {
    refuse(`kind ${event.kind} is not ${KIND_CODING_SESSION_HANDOVER}`);
  }
  if (!HEX64.test(event.id) || !HEX64.test(event.pubkey)) {
    refuse("event id and author must be lowercase 64-hex");
  }
  if (!Number.isSafeInteger(event.created_at) || event.created_at < 0) {
    refuse('envelope field "created_at" must be a non-negative integer');
  }
  if (!UUID.test(input.sessionRef) || !HEX64.test(input.genesisRef)) {
    refuse("the supplied scope is not canonical");
  }
  const [channelRef, dTag, version, genesisTag, typeTag] = readExactTags(
    event.tags,
  );
  if (
    channelRef !== input.channelRef ||
    dTag !== input.sessionRef ||
    genesisTag !== input.genesisRef
  ) {
    refuse("envelope crosses or disagrees with its supplied scope");
  }
  if (version !== CODING_SESSION_HANDOVER_TAG_VERSION) {
    refuse(
      `envelope tag "csh-v" must be ${quote(CODING_SESSION_HANDOVER_TAG_VERSION)}`,
    );
  }
  if (typeof event.content !== "string") refuse("content must be a string");
  if (bytes(event.content) > MAX_CONTENT_BYTES) {
    refuse(`content exceeds ${MAX_CONTENT_BYTES} bytes`);
  }
  if (hasDuplicateJsonKeys(event.content)) {
    refuse("content repeats a JSON key");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(event.content);
  } catch {
    return refuse("content is not JSON");
  }
  const payload = exact(parsed, PAYLOAD_KEYS, "payload");
  if (payload.schema !== CODING_SESSION_HANDOVER_SCHEMA) {
    refuse(
      `payload field "schema" must be ${quote(CODING_SESSION_HANDOVER_SCHEMA)}`,
    );
  }
  if (payload.sessionRef !== input.sessionRef) {
    refuse('payload field "sessionRef" disagrees with the envelope');
  }
  if (payload.genesisRef !== input.genesisRef) {
    refuse('payload field "genesisRef" disagrees with the envelope');
  }
  const type = word<CodingSessionHandoverType>(
    payload,
    "type",
    ["checkpoint", "continuation"],
    "payload",
  );
  if (type !== typeTag) {
    refuse('envelope tag "csh-type" disagrees with the payload type');
  }
  const shared = {
    eventId: event.id,
    author: event.pubkey,
    createdAt: event.created_at,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
  };
  if (type === "checkpoint") {
    return Object.freeze({
      ...shared,
      type: "checkpoint" as const,
      body: decodeCheckpointBody(payload.body),
    });
  }
  return Object.freeze({
    ...shared,
    type: "continuation" as const,
    body: decodeContinuationBody(payload.body),
  });
}

/**
 * Build the exact content string a 44247 record signs.
 *
 * One builder, used by the desktop's own publishes and by the tests, so an
 * envelope this app writes is one this app's decoder accepts — the round trip
 * is the only proof that the two agree.
 */
export function buildCodingSessionHandoverContent(input: {
  sessionRef: string;
  genesisRef: string;
  type: CodingSessionHandoverType;
  body: CodingSessionCheckpointBody | CodingSessionContinuationBody;
}): string {
  return JSON.stringify({
    schema: CODING_SESSION_HANDOVER_SCHEMA,
    sessionRef: input.sessionRef,
    genesisRef: input.genesisRef,
    type: input.type,
    body: input.body,
  });
}

/** The five envelope tags, in the order the decoder requires. */
export function buildCodingSessionHandoverTags(input: {
  channelId: string;
  sessionRef: string;
  genesisRef: string;
  type: CodingSessionHandoverType;
}): string[][] {
  return [
    ["h", input.channelId],
    ["d", input.sessionRef],
    ["csh-v", CODING_SESSION_HANDOVER_TAG_VERSION],
    ["csh-genesis", input.genesisRef],
    ["csh-type", input.type],
  ];
}
