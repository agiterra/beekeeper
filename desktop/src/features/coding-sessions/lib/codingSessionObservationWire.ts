/**
 * The transport decoder for the native kind-44246 observation fold.
 *
 * TypeScript **never folds 44246** (§0.5, I6). `buzz-core` decides
 * newest-wins, the `(author, source, gate)` dedupe key, the bounds and the
 * disclosures; the native adapter flattens that answer; this file checks that
 * what came back has exactly the declared shape and refuses anything else.
 *
 * Exact-key on purpose, in both directions. A response with one extra key, one
 * missing key, or a `null` where a value is required decodes to a **thrown
 * error naming the key** rather than to a best guess — and the test that
 * exercises it reads a fixture the Rust adapter generates, so a field the
 * adapter renames and this file does not is a failing Rust test rather than a
 * silently empty card (REVIEW-B1c B1).
 */
import { hasExactKeys, isPlainRecord } from "./codingSessionWireDecode";

export const CODING_SESSION_OBSERVATION_FOLD_COMMAND =
  "fold_coding_session_observations_command";
export const CODING_SESSION_OBSERVATION_FOLD_REQUEST_SCHEMA =
  "buzz-coding-session-observation-fold-request/v2";
export const CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA =
  "buzz-coding-session-observation-fold-adapter/v2";

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * How a record came to exist: a mechanism watched it, its subject said it, or
 * a bench measured it.
 *
 * `measured` was widened onto the closed enum for the registry bench. It is
 * honestly a second axis in one key — `observed`/`declared` says who saw it,
 * `measured` says why it ran — and that is a named residual, not a design.
 */
export type CodingSessionObservationSource =
  | "observed"
  | "declared"
  | "measured";
/** The three words a 44244 `report.tests[].outcome` also uses. */
export type CodingSessionObservationGateOutcome =
  | "passed"
  | "failed"
  | "not-run";
/** The red-first loop, in the order §1e declares it. */
export type CodingSessionObservationPhaseWord =
  | "planning"
  | "red"
  | "green"
  | "gates"
  | "reporting";
/** What an author did about one finding. */
export type CodingSessionObservationDisposition =
  | "found"
  | "fixed"
  | "cross-lane"
  | "needs-ruling"
  | "wont-fix";

/**
 * The declared phase order, and the only order the Audit tab groups in.
 *
 * Not arrival order, and never author time (§8 I4): a phase word is a closed
 * token and its position is a property of the loop, not of when somebody
 * happened to publish it.
 */
export const CODING_SESSION_OBSERVATION_PHASE_ORDER: readonly CodingSessionObservationPhaseWord[] =
  Object.freeze(["planning", "red", "green", "gates", "reporting"]);

const SOURCES: readonly string[] = ["observed", "declared", "measured"];
const OUTCOMES: readonly string[] = ["passed", "failed", "not-run"];
const DISPOSITIONS: readonly string[] = [
  "found",
  "fixed",
  "cross-lane",
  "needs-ruling",
  "wont-fix",
];

export type CodingSessionObservationCheckpointRow = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly source: CodingSessionObservationSource;
  readonly assignmentRef: string | null;
  readonly phase: CodingSessionObservationPhaseWord;
  readonly testsWritten: number;
  readonly testsRed: number;
  readonly testsGreen: number;
  readonly lastCommand: string | null;
  readonly lastSummary: string | null;
  readonly note: string | null;
};

export type CodingSessionObservationGateRow = {
  readonly authorPubkey: string;
  readonly source: CodingSessionObservationSource;
  readonly eventIds: readonly string[];
  readonly droppedEventIds: number;
  readonly assignmentRef: string | null;
  readonly gate: string;
  readonly outcome: CodingSessionObservationGateOutcome;
  readonly command: string;
  readonly summary: string | null;
  readonly durationMs: number | null;
  /**
   * The commit this gate ran against, lowercase hex, or `null`.
   *
   * `null` is a row that names no commit — one signed before the key existed
   * (2026-09-03), or one whose workdir had no resolvable `HEAD`. It is never
   * read as "the commit currently checked out": absent names nothing.
   */
  readonly headSha: string | null;
  /** Whether the tree was dirty when it ran, or `null`. */
  readonly dirty: boolean | null;
};

export type CodingSessionObservationFindingRow = {
  readonly authorPubkey: string;
  readonly source: CodingSessionObservationSource;
  readonly eventIds: readonly string[];
  readonly droppedEventIds: number;
  readonly assignmentRef: string | null;
  readonly findingId: string;
  readonly title: string;
  readonly disposition: CodingSessionObservationDisposition;
  readonly detail: string | null;
  readonly refs: readonly string[];
  readonly decisionRef: string | null;
};

export type CodingSessionObservationPhaseRow = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly source: CodingSessionObservationSource;
  readonly assignmentRef: string | null;
  readonly phase: string;
  readonly startedAtMs: number;
  readonly endedAtMs: number | null;
  readonly durationMs: number | null;
};

/**
 * One gate start the provider signed (SV-41): an observed `gate:<gate>` phase
 * row saying a recognised gate command is running, paired by `buzz-core` with
 * its own closing row when the page holds one.
 *
 * Not an outcome. The close fields are all `null` while the start is open;
 * whether an open start has gone stale is a question about "now", answered by
 * `selectCodingSessionRunningGates` against `gateStartStaleAfterMs`.
 */
export type CodingSessionObservationGateStartRow = {
  readonly eventId: string;
  /** The provider instance that watched the call — never the seat. */
  readonly authorPubkey: string;
  /** The provider's table name for the gate, e.g. `cargo test`. */
  readonly gate: string;
  /** When the call began, by the provider's clock. */
  readonly startedAtMs: number;
  readonly assignmentRef: string | null;
  readonly closeEventId: string | null;
  /** When the call ended, by the provider's clock; `null` while open. */
  readonly endedAtMs: number | null;
  /** The provider's measured span; `null` while open or when unmeasured. */
  readonly durationMs: number | null;
};

export type CodingSessionObservationUnresolvedRow = {
  readonly eventId: string;
  readonly assignmentRef: string;
};

export type CodingSessionObservationIgnoredRow = {
  readonly eventId: string;
  readonly reason: string;
};

export type CodingSessionObservationTruncation = {
  readonly checkpoints: number;
  readonly gates: number;
  readonly findings: number;
  readonly phases: number;
  readonly unresolved: number;
  readonly ignored: number;
  readonly misclaimedObserved: number;
  readonly entryEventIds: number;
  /** Gate rows a later statement by the same author and provenance replaced. */
  readonly displacedGates: number;
  /** Findings a later disposition by the same author and provenance replaced. */
  readonly displacedFindings: number;
  /** Gate starts not listed (the oldest fall off first). */
  readonly gateStarts: number;
  /** Closing rows whose start the page does not hold. A disclosure. */
  readonly gateStartClosesUnmatched: number;
};

/** One row that claimed `observed` without a provider instance behind it. */
export type CodingSessionObservationMisclaimedRow = {
  readonly eventId: string;
  readonly authorPubkey: string;
};

export type CodingSessionObservationFold = {
  readonly schema: typeof CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA;
  readonly implementation: "buzz-core";
  readonly inputEventIds: readonly string[];
  readonly sessionRef: string;
  readonly genesisRef: string;
  readonly checkpoints: readonly CodingSessionObservationCheckpointRow[];
  readonly gates: readonly CodingSessionObservationGateRow[];
  readonly findings: readonly CodingSessionObservationFindingRow[];
  /** Phase timings. Never a gate start: those arrive in `gateStarts`. */
  readonly phases: readonly CodingSessionObservationPhaseRow[];
  /** Gate starts, open or closed, in the order of their start rows. */
  readonly gateStarts: readonly CodingSessionObservationGateStartRow[];
  /**
   * How long an unclosed start may read as running, from `buzz-core`
   * (`GATE_START_STALE_AFTER_MS`). Never copied into TypeScript.
   */
  readonly gateStartStaleAfterMs: number;
  readonly unresolved: readonly CodingSessionObservationUnresolvedRow[];
  readonly ignored: readonly CodingSessionObservationIgnoredRow[];
  /**
   * Rows that claimed `observed` from a signer no provider instance backs.
   *
   * Each is folded as `declared` (REVIEW-L5 F2). Empty when nothing claimed
   * falsely — or when nothing was checked, which `provenanceChecked`
   * distinguishes.
   */
  readonly misclaimedObserved: readonly CodingSessionObservationMisclaimedRow[];
  /**
   * Whether the caller supplied the provider set at all.
   *
   * `false` means no `observed` claim in this fold has been verified, which is
   * a different fact from every claim checking out (§8 I9).
   */
  readonly provenanceChecked: boolean;
  readonly truncated: CodingSessionObservationTruncation;
  readonly disclosure: string;
};

/** A refusal that says which key was wrong, so a reader can act on it. */
export class CodingSessionObservationWireError extends Error {
  constructor(message: string) {
    super(`native coding-session observation fold: ${message}`);
    this.name = "CodingSessionObservationWireError";
  }
}

function refuse(message: string): never {
  throw new CodingSessionObservationWireError(message);
}

function record(value: unknown, at: string): Record<string, unknown> {
  if (!isPlainRecord(value)) refuse(`${at} must be an object`);
  return value;
}

/**
 * Exactly these keys, no more and no fewer, and say which is wrong.
 *
 * A decoder that answered "malformed" would send a reader to read the whole
 * adapter; naming the key sends them to one line of it.
 */
function exact(
  value: unknown,
  keys: readonly string[],
  at: string,
  readOptional: readonly string[] = [],
): Record<string, unknown> {
  const object = record(value, at);
  if (hasExactKeys(object, keys)) return object;
  const missing = keys.filter(
    (key) => !Object.hasOwn(object, key) && !readOptional.includes(key),
  );
  if (missing.length > 0) {
    refuse(`${at} is missing ${missing.map(quote).join(", ")}`);
  }
  const extra = Object.keys(object).filter((key) => !keys.includes(key));
  if (extra.length > 0) {
    refuse(`${at} carries unsupported ${extra.map(quote).join(", ")}`);
  }
  return object;
}

/**
 * A boolean, or `null`, refused by name when it is neither.
 *
 * A string `"false"` is not a measurement of anything, and reading one as a
 * boolean would make a dirty worktree read as clean.
 */
function nullableFlag(
  object: Record<string, unknown>,
  key: string,
  at: string,
): boolean | null {
  const value = object[key];
  if (value === undefined || value === null) return null;
  if (typeof value !== "boolean") {
    refuse(`${at} field ${quote(key)} must be a boolean or null`);
  }
  return value;
}

function quote(key: string): string {
  return `"${key}"`;
}

function text(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string {
  const value = object[key];
  if (typeof value !== "string" || value.length === 0) {
    refuse(`${at} field ${quote(key)} must be a non-empty string`);
  }
  return value;
}

function nullableText(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string | null {
  const value = object[key];
  // `undefined` is an absent read-optional key; every required key is proved
  // present by `exact` before this runs.
  if (value === undefined || value === null) return null;
  if (typeof value !== "string" || value.length === 0) {
    refuse(`${at} field ${quote(key)} must be a non-empty string or null`);
  }
  return value;
}

function eventId(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string {
  const value = object[key];
  if (typeof value !== "string" || !HEX64.test(value)) {
    refuse(`${at} field ${quote(key)} must be a lowercase 64-hex event id`);
  }
  return value;
}

function nullableEventId(
  object: Record<string, unknown>,
  key: string,
  at: string,
): string | null {
  if (object[key] === null) return null;
  return eventId(object, key, at);
}

function count(
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

function nullableCount(
  object: Record<string, unknown>,
  key: string,
  at: string,
): number | null {
  if (object[key] === null) return null;
  return count(object, key, at);
}

function bool(
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

function eventIds(
  object: Record<string, unknown>,
  key: string,
  at: string,
): readonly string[] {
  const value = object[key];
  if (!Array.isArray(value)) {
    refuse(`${at} field ${quote(key)} must be an array of event ids`);
  }
  for (const entry of value) {
    if (typeof entry !== "string" || !HEX64.test(entry)) {
      refuse(`${at} field ${quote(key)} must be an array of event ids`);
    }
  }
  return Object.freeze([...(value as string[])]);
}

function rows<T>(
  object: Record<string, unknown>,
  key: string,
  decodeRow: (value: unknown, at: string) => T,
): readonly T[] {
  const value = object[key];
  if (!Array.isArray(value)) {
    refuse(`response field ${quote(key)} must be an array`);
  }
  return Object.freeze(
    value.map((entry, index) => decodeRow(entry, `${key}[${index}]`)),
  );
}

const CHECKPOINT_KEYS = [
  "eventId",
  "authorPubkey",
  "source",
  "assignmentRef",
  "phase",
  "testsWritten",
  "testsRed",
  "testsGreen",
  "lastCommand",
  "lastSummary",
  "note",
] as const;

function decodeCheckpoint(
  value: unknown,
  at: string,
): CodingSessionObservationCheckpointRow {
  const object = exact(value, CHECKPOINT_KEYS, at);
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    authorPubkey: eventId(object, "authorPubkey", at),
    source: word<CodingSessionObservationSource>(object, "source", SOURCES, at),
    assignmentRef: nullableEventId(object, "assignmentRef", at),
    phase: word<CodingSessionObservationPhaseWord>(
      object,
      "phase",
      CODING_SESSION_OBSERVATION_PHASE_ORDER,
      at,
    ),
    testsWritten: count(object, "testsWritten", at),
    testsRed: count(object, "testsRed", at),
    testsGreen: count(object, "testsGreen", at),
    lastCommand: nullableText(object, "lastCommand", at),
    lastSummary: nullableText(object, "lastSummary", at),
    note: nullableText(object, "note", at),
  });
}

const GATE_KEYS = [
  "authorPubkey",
  "source",
  "eventIds",
  "droppedEventIds",
  "assignmentRef",
  "gate",
  "outcome",
  "command",
  "summary",
  "durationMs",
  "headSha",
  "dirty",
] as const;

/**
 * Gate-row keys this decoder accepts as **absent**, though the adapter always
 * writes them.
 *
 * The finding-31 rule, applied on this side of the boundary too. The adapter
 * and this decoder ship in one binary and so cannot skew today — but a fold
 * response replayed from a fixture, a cache, or a build older than
 * 2026-09-03 can, and a decoder that refused it outright would lose the
 * history rather than read a row that names no commit. Absent decodes as
 * `null`, which admits nothing anywhere.
 */
const GATE_READ_OPTIONAL_KEYS = ["headSha", "dirty"] as const;

function decodeGate(
  value: unknown,
  at: string,
): CodingSessionObservationGateRow {
  const object = exact(value, GATE_KEYS, at, GATE_READ_OPTIONAL_KEYS);
  return Object.freeze({
    authorPubkey: eventId(object, "authorPubkey", at),
    source: word<CodingSessionObservationSource>(object, "source", SOURCES, at),
    eventIds: eventIds(object, "eventIds", at),
    droppedEventIds: count(object, "droppedEventIds", at),
    assignmentRef: nullableEventId(object, "assignmentRef", at),
    gate: text(object, "gate", at),
    outcome: word<CodingSessionObservationGateOutcome>(
      object,
      "outcome",
      OUTCOMES,
      at,
    ),
    command: text(object, "command", at),
    summary: nullableText(object, "summary", at),
    durationMs: nullableCount(object, "durationMs", at),
    headSha: nullableText(object, "headSha", at),
    dirty: nullableFlag(object, "dirty", at),
  });
}

const FINDING_KEYS = [
  "authorPubkey",
  "source",
  "eventIds",
  "droppedEventIds",
  "assignmentRef",
  "findingId",
  "title",
  "disposition",
  "detail",
  "refs",
  "decisionRef",
] as const;

function decodeFinding(
  value: unknown,
  at: string,
): CodingSessionObservationFindingRow {
  const object = exact(value, FINDING_KEYS, at);
  return Object.freeze({
    authorPubkey: eventId(object, "authorPubkey", at),
    source: word<CodingSessionObservationSource>(object, "source", SOURCES, at),
    eventIds: eventIds(object, "eventIds", at),
    droppedEventIds: count(object, "droppedEventIds", at),
    assignmentRef: nullableEventId(object, "assignmentRef", at),
    findingId: text(object, "findingId", at),
    title: text(object, "title", at),
    disposition: word<CodingSessionObservationDisposition>(
      object,
      "disposition",
      DISPOSITIONS,
      at,
    ),
    detail: nullableText(object, "detail", at),
    refs: eventIds(object, "refs", at),
    decisionRef: nullableEventId(object, "decisionRef", at),
  });
}

const PHASE_KEYS = [
  "eventId",
  "authorPubkey",
  "source",
  "assignmentRef",
  "phase",
  "startedAtMs",
  "endedAtMs",
  "durationMs",
] as const;

function decodePhase(
  value: unknown,
  at: string,
): CodingSessionObservationPhaseRow {
  const object = exact(value, PHASE_KEYS, at);
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    authorPubkey: eventId(object, "authorPubkey", at),
    source: word<CodingSessionObservationSource>(object, "source", SOURCES, at),
    assignmentRef: nullableEventId(object, "assignmentRef", at),
    phase: text(object, "phase", at),
    startedAtMs: count(object, "startedAtMs", at),
    endedAtMs: nullableCount(object, "endedAtMs", at),
    durationMs: nullableCount(object, "durationMs", at),
  });
}

const GATE_START_KEYS = [
  "eventId",
  "authorPubkey",
  "gate",
  "startedAtMs",
  "assignmentRef",
  "closeEventId",
  "endedAtMs",
  "durationMs",
] as const;

function decodeGateStart(
  value: unknown,
  at: string,
): CodingSessionObservationGateStartRow {
  const object = exact(value, GATE_START_KEYS, at);
  const closeEventId = nullableEventId(object, "closeEventId", at);
  const endedAtMs = nullableCount(object, "endedAtMs", at);
  const durationMs = nullableCount(object, "durationMs", at);
  // A close is all-or-nothing: an end with no closing row (or the reverse)
  // would let a reader call a start finished on half a record.
  if ((closeEventId === null) !== (endedAtMs === null)) {
    refuse(`${at} must carry "closeEventId" and "endedAtMs" together`);
  }
  if (endedAtMs === null && durationMs !== null) {
    refuse(`${at} field "durationMs" must be null while the start is open`);
  }
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    authorPubkey: eventId(object, "authorPubkey", at),
    gate: text(object, "gate", at),
    startedAtMs: count(object, "startedAtMs", at),
    assignmentRef: nullableEventId(object, "assignmentRef", at),
    closeEventId,
    endedAtMs,
    durationMs,
  });
}

function decodeUnresolved(
  value: unknown,
  at: string,
): CodingSessionObservationUnresolvedRow {
  const object = exact(value, ["eventId", "assignmentRef"], at);
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    assignmentRef: eventId(object, "assignmentRef", at),
  });
}

function decodeMisclaimed(
  value: unknown,
  at: string,
): CodingSessionObservationMisclaimedRow {
  const object = exact(value, ["eventId", "authorPubkey"], at);
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    authorPubkey: eventId(object, "authorPubkey", at),
  });
}

function decodeIgnored(
  value: unknown,
  at: string,
): CodingSessionObservationIgnoredRow {
  const object = exact(value, ["eventId", "reason"], at);
  return Object.freeze({
    eventId: eventId(object, "eventId", at),
    reason: text(object, "reason", at),
  });
}

const TRUNCATION_KEYS = [
  "checkpoints",
  "gates",
  "findings",
  "phases",
  "unresolved",
  "ignored",
  "misclaimedObserved",
  "entryEventIds",
  "displacedGates",
  "displacedFindings",
  "gateStarts",
  "gateStartClosesUnmatched",
] as const;

function decodeTruncation(value: unknown): CodingSessionObservationTruncation {
  const at = "truncated";
  const object = exact(value, TRUNCATION_KEYS, at);
  return Object.freeze({
    checkpoints: count(object, "checkpoints", at),
    gates: count(object, "gates", at),
    findings: count(object, "findings", at),
    phases: count(object, "phases", at),
    unresolved: count(object, "unresolved", at),
    ignored: count(object, "ignored", at),
    misclaimedObserved: count(object, "misclaimedObserved", at),
    entryEventIds: count(object, "entryEventIds", at),
    displacedGates: count(object, "displacedGates", at),
    displacedFindings: count(object, "displacedFindings", at),
    gateStarts: count(object, "gateStarts", at),
    gateStartClosesUnmatched: count(object, "gateStartClosesUnmatched", at),
  });
}

const RESPONSE_KEYS = [
  "schema",
  "implementation",
  "inputEventIds",
  "sessionRef",
  "genesisRef",
  "checkpoints",
  "gates",
  "findings",
  "phases",
  "gateStarts",
  "gateStartStaleAfterMs",
  "unresolved",
  "ignored",
  "misclaimedObserved",
  "provenanceChecked",
  "truncated",
  "disclosure",
] as const;

/**
 * Decode one native adapter response, or throw naming the offending key.
 *
 * Exported so a test can drive it with a fixture generated by the Rust adapter
 * itself — the only kind of fixture that can prove the two sides still agree.
 */
export function decodeCodingSessionObservationFold(
  value: unknown,
): CodingSessionObservationFold {
  const object = exact(value, RESPONSE_KEYS, "response");
  if (object.schema !== CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA) {
    refuse(
      `response field "schema" must be ${quote(CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA)}`,
    );
  }
  if (object.implementation !== "buzz-core") {
    refuse('response field "implementation" must be "buzz-core"');
  }
  return Object.freeze({
    schema: CODING_SESSION_OBSERVATION_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: eventIds(object, "inputEventIds", "response"),
    sessionRef: text(object, "sessionRef", "response"),
    genesisRef: eventId(object, "genesisRef", "response"),
    checkpoints: rows(object, "checkpoints", decodeCheckpoint),
    gates: rows(object, "gates", decodeGate),
    findings: rows(object, "findings", decodeFinding),
    phases: rows(object, "phases", decodePhase),
    gateStarts: rows(object, "gateStarts", decodeGateStart),
    gateStartStaleAfterMs: count(object, "gateStartStaleAfterMs", "response"),
    unresolved: rows(object, "unresolved", decodeUnresolved),
    ignored: rows(object, "ignored", decodeIgnored),
    misclaimedObserved: rows(object, "misclaimedObserved", decodeMisclaimed),
    provenanceChecked: bool(object, "provenanceChecked", "response"),
    truncated: decodeTruncation(object.truncated),
    disclosure: text(object, "disclosure", "response"),
  } satisfies CodingSessionObservationFold);
}
