/**
 * NIP-CSCK turn checkpoints (kind 44231): the strict reader and its fold.
 *
 * A mirror of `crates/beekeeper-core/src/coding_session_checkpoint.rs`, which
 * is the normative decoder (with `docs/nips/NIP-CSCK.md`). Every rule there is
 * a rule here, so the desktop and the CLI accept and refuse the same signed
 * bytes:
 *
 * - unknown keys are rejected at every level, and absent is not null;
 * - exactly one of `git` and `unavailable` is non-null;
 * - paths are repo-relative, canonical, and carry no redaction marker;
 * - five ordered two-field tags, every value re-derived from the content.
 *
 * What the relay cannot check, this does: the signer must be the key that
 * signs the same generation's 44225 transcript items. A checkpoint signed by
 * any other key — or naming a target this view holds no transcript for — is
 * refused and counted, never shown.
 *
 * The fold keeps one `turn` checkpoint per `(signer, target, turnId)`, the
 * highest `throughSeq` winning, and keeps `pre_rewind` captures apart. A
 * second event with the same `csck-key` is a duplicate, not a revision: the
 * first one seen (oldest `created_at`, then id) stays.
 */
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_CHECKPOINT } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import { encodeStructuredKey } from "./codingSessionKeys";
import {
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./codingSessionWireDecode";

export const CODING_SESSION_CHECKPOINT_SCHEMA =
  "buzz-coding-session-checkpoint/v1";
export const CODING_SESSION_CHECKPOINT_TAG_VERSION = "csck1-1";
export const CODING_SESSION_CHECKPOINT_KEY_DOMAIN =
  "coding-session-checkpoint/v1";

export const MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES = 32 * 1024;
export const MAX_CHECKPOINT_IDENTIFIER_BYTES = 512;
export const MAX_CHECKPOINT_FILES = 256;
export const MAX_CHECKPOINT_OMITTED = 32;
export const MAX_CHECKPOINT_UNAVAILABLE_SENTENCE_BYTES = 512;
export const MAX_CHECKPOINT_PATH_BYTES = 1024;
export const MAX_CHECKPOINT_BRANCH_BYTES = 512;

const REDACTION_MARKERS = ["[elided private context:", "••••••••", "[redacted"];
/** `char::is_control`: the Unicode `Cc` category. */
// biome-ignore lint/suspicious/noControlCharactersInRegex: the point of it.
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/u;
const OID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const CANONICAL_UUID =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export type CodingSessionCheckpointReason = "turn" | "pre_rewind";
export type CodingSessionCheckpointFileStatus =
  | "added"
  | "modified"
  | "deleted"
  | "renamed";
export type CodingSessionCheckpointOmissionReason = "too_large" | "unreadable";
export type CodingSessionCheckpointUnavailableCode =
  | "NOT_A_REPOSITORY"
  | "BOUNDARY_UNPREPARED"
  | "TIMED_OUT"
  | "GIT_FAILED";

export type CodingSessionCheckpointFile = {
  path: string;
  status: CodingSessionCheckpointFileStatus;
  from: string | null;
  additions: number | null;
  deletions: number | null;
};

export type CodingSessionCheckpointGit = {
  head: string | null;
  branch: string | null;
  baseTree: string | null;
  tree: string;
  commit: string;
  outsideTurn: boolean | null;
  complete: boolean;
  omitted: readonly {
    path: string;
    reason: CodingSessionCheckpointOmissionReason;
  }[];
  omittedNotListed: number;
};

export type CodingSessionCheckpointPayload = {
  schema: typeof CODING_SESSION_CHECKPOINT_SCHEMA;
  session: CodingSessionCommandTarget;
  turnId: string | null;
  reason: CodingSessionCheckpointReason;
  coverage: { fromSeq: number; throughSeq: number };
  git: CodingSessionCheckpointGit | null;
  files: readonly CodingSessionCheckpointFile[];
  filesNotListed: number;
  restorable: boolean;
  unavailable: {
    code: CodingSessionCheckpointUnavailableCode;
    sentence: string;
  } | null;
  summary: null;
};

export type CodingSessionCheckpointDecode =
  | { ok: true; value: Readonly<CodingSessionCheckpointPayload> }
  | { ok: false; error: string };

const PAYLOAD_KEYS = [
  "schema",
  "session",
  "turnId",
  "reason",
  "coverage",
  "git",
  "files",
  "filesNotListed",
  "restorable",
  "unavailable",
  "summary",
] as const;
const PAYLOAD_NULLABLE = ["turnId", "git", "unavailable", "summary"];
const SESSION_KEYS = ["driver", "instanceId", "sessionId", "generation"];
const COVERAGE_KEYS = ["fromSeq", "throughSeq"];
const GIT_KEYS = [
  "head",
  "branch",
  "baseTree",
  "tree",
  "commit",
  "outsideTurn",
  "complete",
  "omitted",
  "omittedNotListed",
];
const GIT_NULLABLE = ["head", "branch", "baseTree", "outsideTurn"];
const OMISSION_KEYS = ["path", "reason"];
const FILE_KEYS = ["path", "status", "from", "additions", "deletions"];
const FILE_NULLABLE = ["from", "additions", "deletions"];
const UNAVAILABLE_KEYS = ["code", "sentence"];

const REASONS = ["turn", "pre_rewind"];
const OMISSION_REASONS = ["too_large", "unreadable"];
const FILE_STATUSES = ["added", "modified", "deleted", "renamed"];
const UNAVAILABLE_CODES = [
  "NOT_A_REPOSITORY",
  "BOUNDARY_UNPREPARED",
  "TIMED_OUT",
  "GIT_FAILED",
];

class Refusal extends Error {}

function refuse(message: string): never {
  throw new Refusal(`coding-session checkpoint ${message}`);
}

function byteLength(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/** Every key present, no unknown key, and `null` only where allowed. */
function exact(
  value: unknown,
  keys: readonly string[],
  nullable: readonly string[],
  what: string,
): Record<string, unknown> {
  if (!isPlainRecord(value)) refuse(`${what} must be an object`);
  for (const key of keys) {
    if (!Object.hasOwn(value, key)) {
      refuse(`${what} is missing "${key}": absent is not null`);
    }
    if (value[key] === null && !nullable.includes(key)) {
      refuse(`${what} field "${key}" requires a value`);
    }
  }
  for (const key of Object.keys(value)) {
    if (!keys.includes(key)) {
      refuse(`${what} carries unsupported field "${key}"`);
    }
  }
  return value;
}

function token<T extends string>(
  value: unknown,
  allowed: readonly string[],
  what: string,
): T {
  if (typeof value !== "string") refuse(`${what} must be a string`);
  if (!allowed.includes(value)) refuse(`${what} carries unsupported token`);
  return value as T;
}

function array(value: unknown, max: number, what: string): unknown[] {
  if (!Array.isArray(value)) refuse(`${what} must be an array`);
  if (value.length > max) refuse(`${what} exceeds ${max} entries`);
  return value;
}

/** A JSON integer that serde reads as `u64` and v1 bounds to 2^53 − 1. */
function count(value: unknown, what: string, positive = false): number {
  if (
    typeof value !== "number" ||
    !Number.isSafeInteger(value) ||
    value < (positive ? 1 : 0)
  ) {
    refuse(
      `${what} must be a ${positive ? "positive" : "non-negative"} safe integer`,
    );
  }
  return value;
}

function bool(value: unknown, what: string): boolean {
  if (typeof value !== "boolean") refuse(`${what} must be a boolean`);
  return value;
}

/**
 * Bounded, non-blank, single-line text. Blank as Rust's `str::trim` sees it:
 * JavaScript's `trim` also strips U+FEFF, which Rust does not count as
 * whitespace, so it is shielded first.
 */
function singleLine(value: unknown, max: number, what: string): string {
  if (typeof value !== "string") refuse(`${what} must be a string`);
  if (value.replaceAll("﻿", "x").trim().length === 0) {
    refuse(`${what} must not be blank`);
  }
  if (byteLength(value) > max) refuse(`${what} exceeds ${max} bytes`);
  if (CONTROL.test(value))
    refuse(`${what} must not contain control characters`);
  return value;
}

function oid(value: unknown, what: string): string {
  if (typeof value !== "string" || !OID.test(value)) {
    refuse(`${what} must be a lowercase 40- or 64-hex git object id`);
  }
  return value;
}

/** Whether `path` is a repo-relative path a checkpoint may carry. */
export function isPublishableCheckpointPath(path: unknown): path is string {
  if (typeof path !== "string" || path.length === 0) return false;
  if (byteLength(path) > MAX_CHECKPOINT_PATH_BYTES) return false;
  if (CONTROL.test(path)) return false;
  const drive = /^[A-Za-z]:/.test(path);
  if (path.startsWith("/") || path.startsWith("\\") || drive) return false;
  if (path.startsWith("refs/")) return false;
  if (REDACTION_MARKERS.some((marker) => path.includes(marker))) return false;
  return path
    .split(/[/\\]/)
    .every((segment) => segment !== "" && segment !== "." && segment !== "..");
}

function path(value: unknown, what: string): string {
  if (!isPublishableCheckpointPath(value)) {
    refuse(`${what} must be a canonical repo-relative path`);
  }
  return value;
}

function decodeSession(value: unknown): CodingSessionCommandTarget {
  const session = exact(value, SESSION_KEYS, [], "session");
  const max = MAX_CHECKPOINT_IDENTIFIER_BYTES;
  return Object.freeze({
    driver: singleLine(session.driver, max, "session.driver"),
    instanceId: singleLine(session.instanceId, max, "session.instanceId"),
    sessionId: singleLine(session.sessionId, max, "session.sessionId"),
    generation: count(session.generation, "session.generation", true),
  });
}

function decodeGit(value: unknown): CodingSessionCheckpointGit {
  const git = exact(value, GIT_KEYS, GIT_NULLABLE, "git");
  const tree = oid(git.tree, "git.tree");
  const commit = oid(git.commit, "git.commit");
  const head = git.head === null ? null : oid(git.head, "git.head");
  const baseTree =
    git.baseTree === null ? null : oid(git.baseTree, "git.baseTree");
  // One repository has one hash algorithm.
  if (
    [commit, head, baseTree].some(
      (id) => id !== null && id.length !== tree.length,
    )
  ) {
    refuse("git object ids must all be SHA-1 or all SHA-256");
  }
  let branch: string | null = null;
  if (git.branch !== null) {
    branch = singleLine(git.branch, MAX_CHECKPOINT_BRANCH_BYTES, "git.branch");
    if (branch.startsWith("refs/")) refuse("git.branch must not be a ref name");
  }
  const outsideTurn =
    git.outsideTurn === null ? null : bool(git.outsideTurn, "git.outsideTurn");
  const complete = bool(git.complete, "git.complete");
  const seen = new Set<string>();
  const omitted = array(git.omitted, MAX_CHECKPOINT_OMITTED, "git.omitted").map(
    (raw) => {
      const entry = exact(raw, OMISSION_KEYS, [], "git.omitted entry");
      const reason = token<CodingSessionCheckpointOmissionReason>(
        entry.reason,
        OMISSION_REASONS,
        "git.omitted reason",
      );
      const omittedPath = path(entry.path, "git.omitted path");
      if (seen.has(omittedPath)) refuse("git.omitted names one path twice");
      seen.add(omittedPath);
      return Object.freeze({ path: omittedPath, reason });
    },
  );
  const omittedNotListed = count(git.omittedNotListed, "git.omittedNotListed");
  if (complete && (omitted.length > 0 || omittedNotListed > 0)) {
    refuse("git.complete must be false when a path was omitted");
  }
  return Object.freeze({
    head,
    branch,
    baseTree,
    tree,
    commit,
    outsideTurn,
    complete,
    omitted: Object.freeze(omitted),
    omittedNotListed,
  });
}

function decodeFiles(value: unknown): CodingSessionCheckpointFile[] {
  const seen = new Set<string>();
  return array(value, MAX_CHECKPOINT_FILES, "files").map((raw) => {
    const entry = exact(raw, FILE_KEYS, FILE_NULLABLE, "files entry");
    const status = token<CodingSessionCheckpointFileStatus>(
      entry.status,
      FILE_STATUSES,
      "files status",
    );
    const filePath = path(entry.path, "files path");
    if (seen.has(filePath)) refuse("files names one path twice");
    seen.add(filePath);
    let from: string | null = null;
    if (status === "renamed") {
      if (entry.from === null)
        refuse("files from must be present when renamed");
      from = path(entry.from, "files from");
      if (from === filePath) refuse("files from must differ from path");
    } else if (entry.from !== null) {
      refuse("files from must be null unless status is renamed");
    }
    const additions =
      entry.additions === null
        ? null
        : count(entry.additions, "files additions");
    const deletions =
      entry.deletions === null
        ? null
        : count(entry.deletions, "files deletions");
    return Object.freeze({
      path: filePath,
      status,
      from,
      additions,
      deletions,
    });
  });
}

/** Strictly decode kind 44231 content. */
export function decodeCodingSessionCheckpoint(
  content: unknown,
): CodingSessionCheckpointDecode {
  // Size, then duplicate keys, then JSON — `parseBoundedJson` refuses all
  // three, as serde does when `buzz-core` decodes the same bytes.
  const parsed = parseBoundedJson(
    content,
    MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES,
  );
  if (parsed === null) {
    return {
      ok: false,
      error: "coding-session checkpoint content is malformed",
    };
  }
  try {
    const payload = exact(parsed, PAYLOAD_KEYS, PAYLOAD_NULLABLE, "payload");
    if (payload.schema !== CODING_SESSION_CHECKPOINT_SCHEMA) {
      refuse("schema is unsupported");
    }
    if (payload.summary !== null) refuse("summary must be null in v1");
    const reason = token<CodingSessionCheckpointReason>(
      payload.reason,
      REASONS,
      "reason",
    );
    const session = decodeSession(payload.session);
    let turnId: string | null = null;
    if (payload.turnId !== null) {
      turnId = singleLine(
        payload.turnId,
        MAX_CHECKPOINT_IDENTIFIER_BYTES,
        "turnId",
      );
    } else if (reason === "turn") {
      refuse("turnId must be present when reason is turn");
    }
    const coverageRaw = exact(payload.coverage, COVERAGE_KEYS, [], "coverage");
    const coverage = Object.freeze({
      fromSeq: count(coverageRaw.fromSeq, "coverage.fromSeq", true),
      throughSeq: count(coverageRaw.throughSeq, "coverage.throughSeq", true),
    });
    if (coverage.fromSeq > coverage.throughSeq) {
      refuse("coverage.fromSeq must not exceed throughSeq");
    }
    const files = decodeFiles(payload.files);
    const filesNotListed = count(payload.filesNotListed, "filesNotListed");
    const restorable = bool(payload.restorable, "restorable");
    let unavailable: CodingSessionCheckpointPayload["unavailable"] = null;
    if (payload.unavailable !== null) {
      const raw = exact(
        payload.unavailable,
        UNAVAILABLE_KEYS,
        [],
        "unavailable",
      );
      unavailable = Object.freeze({
        code: token<CodingSessionCheckpointUnavailableCode>(
          raw.code,
          UNAVAILABLE_CODES,
          "unavailable.code",
        ),
        sentence: singleLine(
          raw.sentence,
          MAX_CHECKPOINT_UNAVAILABLE_SENTENCE_BYTES,
          "unavailable.sentence",
        ),
      });
    }
    const git = payload.git === null ? null : decodeGit(payload.git);
    if ((git === null) === (unavailable === null)) {
      refuse("git and unavailable: exactly one is non-null");
    }
    if (unavailable !== null && (files.length > 0 || filesNotListed !== 0)) {
      refuse("files must be empty when git is unavailable");
    }
    return {
      ok: true,
      value: Object.freeze({
        schema: CODING_SESSION_CHECKPOINT_SCHEMA,
        session,
        turnId,
        reason,
        coverage,
        git,
        files: Object.freeze(files),
        filesNotListed,
        restorable,
        unavailable,
        summary: null,
      }),
    };
  } catch (error) {
    if (error instanceof Refusal) return { ok: false, error: error.message };
    throw error;
  }
}

/** The `csck-key` tag value, length-prefixed exactly as `cst-key` is. */
export function codingSessionCheckpointSemanticKey(
  target: CodingSessionCommandTarget,
  reason: CodingSessionCheckpointReason,
  throughSeq: number,
): string {
  return encodeStructuredKey(
    CODING_SESSION_CHECKPOINT_KEY_DOMAIN,
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
    String(throughSeq),
    reason,
  );
}

/** One verified checkpoint. Immutable: the fold hands out these objects. */
export type CodingSessionCheckpointEntry = Readonly<{
  eventId: string;
  createdAt: number;
  channelId: string;
  targetKey: string;
  signerPubkey: string;
  semanticKey: string;
  payload: Readonly<CodingSessionCheckpointPayload>;
}>;

/**
 * Who may sign a target's checkpoints: the keys that sign its 44225 items in
 * this view, by `cs-target` key.
 */
export type CodingSessionCheckpointScope = {
  channelId: string;
  signersByTargetKey: ReadonlyMap<string, ReadonlySet<string>>;
};

export type CodingSessionCheckpointClassification =
  | { kind: "checkpoint"; entry: CodingSessionCheckpointEntry }
  | { kind: "irrelevant" }
  | { kind: "malformed"; error: string }
  | { kind: "unknown-target" }
  | { kind: "foreign-signer" }
  | { kind: "invalid-signature" };

/** Verify one candidate against the exact envelope and its signer. */
export function classifyCodingSessionCheckpointEvent(
  event: RelayEvent,
  scope: CodingSessionCheckpointScope,
): CodingSessionCheckpointClassification {
  if (event.kind !== KIND_CODING_SESSION_CHECKPOINT)
    return { kind: "irrelevant" };
  const tags = Array.isArray(event.tags)
    ? parseExactTags(event.tags, [
        "h",
        "csck-v",
        "cs-target",
        "csck-seq",
        "csck-key",
      ])
    : null;
  if (!tags) {
    return {
      kind: "malformed",
      error: "requires exactly five ordered two-field tags",
    };
  }
  if (!CANONICAL_UUID.test(tags[0])) {
    return {
      kind: "malformed",
      error: "h tag must be a lowercase canonical UUID",
    };
  }
  if (tags[0] !== scope.channelId) return { kind: "irrelevant" };
  if (tags[1] !== CODING_SESSION_CHECKPOINT_TAG_VERSION) {
    return { kind: "malformed", error: "csck-v tag is unsupported" };
  }
  const decoded = decodeCodingSessionCheckpoint(event.content);
  if (!decoded.ok) return { kind: "malformed", error: decoded.error };
  const payload = decoded.value;
  const targetKey = buildCodingSessionTargetKey(payload.session);
  const semanticKey = codingSessionCheckpointSemanticKey(
    payload.session,
    payload.reason,
    payload.coverage.throughSeq,
  );
  if (
    tags[2] !== targetKey ||
    tags[3] !== String(payload.coverage.throughSeq) ||
    tags[4] !== semanticKey
  ) {
    return { kind: "malformed", error: "a tag does not match the payload" };
  }
  const signers = scope.signersByTargetKey.get(targetKey);
  if (!signers || signers.size === 0) return { kind: "unknown-target" };
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!signers.has(signerPubkey)) return { kind: "foreign-signer" };
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };
  return {
    kind: "checkpoint",
    entry: Object.freeze({
      eventId: event.id,
      createdAt: event.created_at,
      channelId: tags[0],
      targetKey,
      signerPubkey,
      semanticKey,
      payload,
    }),
  };
}

/** Where one generation's checkpoints are kept: its signer and target. */
export function codingSessionCheckpointScopeKey(
  signerPubkey: string,
  targetKey: string,
): string {
  return `${signerPubkey}\u0000${targetKey}`;
}

/** One generation's checkpoints. */
export type CodingSessionGenerationCheckpoints = Readonly<{
  scopeKey: string;
  targetKey: string;
  signerPubkey: string;
  /** One per turn, ascending by `throughSeq`. */
  turns: readonly CodingSessionCheckpointEntry[];
  byTurnId: ReadonlyMap<string, CodingSessionCheckpointEntry>;
  /** `pre_rewind` captures, ascending by `throughSeq`; never in `turns`. */
  preRewind: readonly CodingSessionCheckpointEntry[];
}>;

export type CodingSessionCheckpointRejections = Readonly<{
  malformed: number;
  unknownTarget: number;
  foreignSigner: number;
  invalidSignature: number;
  duplicate: number;
}>;

export type CodingSessionCheckpointFold = Readonly<{
  byScope: ReadonlyMap<string, CodingSessionGenerationCheckpoints>;
  rejected: CodingSessionCheckpointRejections;
}>;

export const EMPTY_CODING_SESSION_CHECKPOINT_FOLD: CodingSessionCheckpointFold =
  Object.freeze({
    byScope: new Map(),
    rejected: Object.freeze({
      malformed: 0,
      unknownTarget: 0,
      foreignSigner: 0,
      invalidSignature: 0,
      duplicate: 0,
    }),
  });

const oldestFirst = (
  left: CodingSessionCheckpointEntry,
  right: CodingSessionCheckpointEntry,
): number =>
  left.createdAt - right.createdAt || left.eventId.localeCompare(right.eventId);

const bySeq = (
  left: CodingSessionCheckpointEntry,
  right: CodingSessionCheckpointEntry,
): number =>
  left.payload.coverage.throughSeq - right.payload.coverage.throughSeq;

/**
 * Fold classified checkpoints. `classify` lets a caller cache classifications
 * by event id, so an unchanged event folds to the same entry object and the
 * turns that read it keep their identity.
 */
export function foldCodingSessionCheckpoints(
  events: readonly RelayEvent[],
  scope: CodingSessionCheckpointScope,
  classify: (
    event: RelayEvent,
    scope: CodingSessionCheckpointScope,
  ) => CodingSessionCheckpointClassification = classifyCodingSessionCheckpointEvent,
): CodingSessionCheckpointFold {
  const rejected = {
    malformed: 0,
    unknownTarget: 0,
    foreignSigner: 0,
    invalidSignature: 0,
    duplicate: 0,
  };
  const accepted: CodingSessionCheckpointEntry[] = [];
  const seenIds = new Set<string>();
  for (const event of events) {
    if (seenIds.has(event.id)) continue;
    seenIds.add(event.id);
    const result = classify(event, scope);
    switch (result.kind) {
      case "checkpoint":
        accepted.push(result.entry);
        break;
      case "malformed":
        rejected.malformed += 1;
        break;
      case "unknown-target":
        rejected.unknownTarget += 1;
        break;
      case "foreign-signer":
        rejected.foreignSigner += 1;
        break;
      case "invalid-signature":
        rejected.invalidSignature += 1;
        break;
      default:
        break;
    }
  }
  accepted.sort(oldestFirst);
  const kept = new Map<string, CodingSessionCheckpointEntry>();
  for (const entry of accepted) {
    const identity = codingSessionCheckpointScopeKey(
      entry.signerPubkey,
      entry.semanticKey,
    );
    if (kept.has(identity)) {
      rejected.duplicate += 1;
      continue;
    }
    kept.set(identity, entry);
  }
  const groups = new Map<
    string,
    {
      targetKey: string;
      signerPubkey: string;
      turns: Map<string, CodingSessionCheckpointEntry>;
      preRewind: CodingSessionCheckpointEntry[];
    }
  >();
  for (const entry of kept.values()) {
    const scopeKey = codingSessionCheckpointScopeKey(
      entry.signerPubkey,
      entry.targetKey,
    );
    let group = groups.get(scopeKey);
    if (!group) {
      group = {
        targetKey: entry.targetKey,
        signerPubkey: entry.signerPubkey,
        turns: new Map(),
        preRewind: [],
      };
      groups.set(scopeKey, group);
    }
    if (
      entry.payload.reason === "pre_rewind" ||
      entry.payload.turnId === null
    ) {
      group.preRewind.push(entry);
      continue;
    }
    const held = group.turns.get(entry.payload.turnId);
    if (
      !held ||
      entry.payload.coverage.throughSeq > held.payload.coverage.throughSeq
    ) {
      group.turns.set(entry.payload.turnId, entry);
    }
  }
  const byScope = new Map<string, CodingSessionGenerationCheckpoints>();
  for (const [scopeKey, group] of groups) {
    const turns = [...group.turns.values()].sort(bySeq);
    byScope.set(
      scopeKey,
      Object.freeze({
        scopeKey,
        targetKey: group.targetKey,
        signerPubkey: group.signerPubkey,
        turns: Object.freeze(turns),
        byTurnId: new Map(
          turns.map((entry) => [entry.payload.turnId as string, entry]),
        ),
        preRewind: Object.freeze(group.preRewind.sort(bySeq)),
      }),
    );
  }
  return Object.freeze({ byScope, rejected: Object.freeze(rejected) });
}

/** The turn checkpoint before `entry` in its generation, or null. */
export function previousCodingSessionCheckpoint(
  generation: CodingSessionGenerationCheckpoints,
  entry: CodingSessionCheckpointEntry,
): CodingSessionCheckpointEntry | null {
  const index = generation.turns.indexOf(entry);
  return index > 0 ? (generation.turns[index - 1] ?? null) : null;
}
