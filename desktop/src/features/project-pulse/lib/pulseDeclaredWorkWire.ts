/**
 * The `pulse_declared_work` native contract, as TypeScript reads it.
 *
 * Types first (this file is imported by the read, the projection and the
 * surface), then a strict decoder in the `pulseMissionWire.ts` discipline:
 * exact key sets, closed vocabularies, pinned hex widths, and a failure that
 * names the field. A producer that adds a key is refused, not absorbed —
 * the same fixture pin `pulseDeclaredWork.fixture.json` carries from Rust.
 *
 * Contract: `docs/DECLARED_WORK_PULSE_IMPL.md` §3.
 */

import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";

/** The Tauri command name. */
export const PULSE_DECLARED_WORK_COMMAND = "pulse_declared_work";

/** Exact request schema the native command accepts. */
export const PULSE_DECLARED_WORK_REQUEST_SCHEMA =
  "buzz-pulse-declared-work-request/v1";

/** Exact response schema. A different one is refused, not upgraded. */
export const PULSE_DECLARED_WORK_SCHEMA = "buzz-pulse-declared-work/v1";

/** The native command's own per-request session cap. */
export const MAX_PULSE_DECLARED_WORK_SESSIONS = 8;

/** A signed relay event exactly as the relay handed it back. */
export type PulseDeclaredWorkSignedEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

/** One active seat from the verified authority projection. */
export type PulseDeclaredWorkSeatInput = {
  actorPubkey: string;
  role: string;
};

/** One active signed operator grant from the verified authority projection. */
export type PulseDeclaredWorkGrantInput = {
  actorPubkey: string;
  grantEventRef: string;
  maySteer: boolean;
  acceptedAt: number;
  granted: boolean;
};

/** Durable lifecycle of an umbrella session, as the digest proved it. */
export type PulseDeclaredWorkLifecycle = "open" | "closed";

/** One umbrella the caller gathered, with only its signed 44244 set. */
export type PulseDeclaredWorkSessionInput = {
  sessionKey: string;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
  name: string | null;
  lifecycle: PulseDeclaredWorkLifecycle;
  latestObservationAt: number | null;
  activeSeats: PulseDeclaredWorkSeatInput[];
  activeGrants: PulseDeclaredWorkGrantInput[];
  teamEvents: PulseDeclaredWorkSignedEvent[];
};

/** One read that failed or was bounded before, or inside, the projection. */
export type PulseDeclaredWorkError = {
  readonly scope: string;
  readonly message: string;
};

/** What one page of the read sends to the native command. */
export type PulseDeclaredWorkRequest = {
  schema: typeof PULSE_DECLARED_WORK_REQUEST_SCHEMA;
  project: string;
  channelIds: string[];
  nowUnix: number;
  viewerPubkey: string | null;
  sessions: PulseDeclaredWorkSessionInput[];
  readErrors: PulseDeclaredWorkError[];
};

/** The closed disposition vocabulary, exactly as kind 44244 spells it. */
export type PulseDeclaredDispositionDecision =
  | "approve"
  | "approve-with-notes"
  | "changes-requested"
  | "reject"
  | "blocked";

/** The closed terminal vocabulary. */
export type PulseDeclaredTerminalType = "mission.completed" | "mission.blocked";

/** The one word Rust derives per assignment. */
export type PulseDeclaredAssignmentStatus =
  | "unresolved"
  | "reported"
  | "settled";

/** One canonically included report answering an assignment. */
export type PulseDeclaredReport = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly createdAt: number;
  readonly summary: string;
  readonly branch: string | null;
  readonly baseSha: string | null;
  readonly headSha: string | null;
  readonly files: readonly string[];
  readonly testCount: number;
  readonly deviations: readonly string[];
  readonly residuals: readonly string[];
  /** The fold's `unseated_reports` disclosure for this report's author. */
  readonly authorUnseated: boolean;
};

/** One canonically included disposition governing a report of an assignment. */
export type PulseDeclaredDisposition = {
  readonly eventId: string;
  readonly authorPubkey: string;
  readonly createdAt: number;
  readonly decision: PulseDeclaredDispositionDecision;
  readonly reportRef: string;
};

/** The canonical fold's settlement row, verbatim. */
export type PulseDeclaredSettlement = {
  readonly settled: boolean;
  readonly governedReportEventId: string | null;
  readonly dispositionEventId: string | null;
  readonly acknowledgementEventId: string | null;
  /**
   * Which rule settled it, `null` exactly when `settled` is false (lane 210).
   *
   * `acknowledgement` is the assignee's explicit receipt.
   * `approving_disposition_without_ask` is an approving disposition that asks
   * the assignee for nothing, which settles with no receipt at all — so a
   * null `acknowledgementEventId` on a settled row is an answer, not a gap.
   * Required rather than optional: a build that has not shipped the rule must
   * fail this decoder rather than read as "acknowledged".
   */
  readonly settledBy:
    | "acknowledgement"
    | "approving_disposition_without_ask"
    | null;
};

/** One canonically included assignment with its verified source fields. */
export type PulseDeclaredAssignment = {
  readonly sourceEventId: string;
  readonly createdAt: number;
  readonly assignerPubkey: string;
  readonly assigneeActor: string;
  readonly assigneeRole: string;
  readonly objective: string;
  readonly brief: string;
  readonly branch: string | null;
  readonly baseSha: string | null;
  readonly fileOwnership: readonly string[];
  readonly acceptanceSteps: readonly string[];
  readonly supersedes: string | null;
  readonly reports: readonly PulseDeclaredReport[];
  readonly dispositions: readonly PulseDeclaredDisposition[];
  readonly settlement: PulseDeclaredSettlement;
  readonly status: PulseDeclaredAssignmentStatus;
};

/** The canonical newest authorized terminal, when the fold established one. */
export type PulseDeclaredTerminal = {
  readonly eventId: string;
  readonly type: PulseDeclaredTerminalType;
  readonly at: number;
};

/** One umbrella's declared work, as the native projection wrote it. */
export type PulseDeclaredWorkSession = {
  readonly sessionKey: string;
  readonly sessionRef: string;
  /** The umbrella's immutable genesis event id — the record its 44244 set names. */
  readonly genesisRef: string;
  readonly channelId: string;
  readonly name: string | null;
  readonly lifecycle: PulseDeclaredWorkLifecycle;
  readonly latestObservationAt: number | null;
  readonly founderPubkey: string;
  readonly terminal: PulseDeclaredTerminal | null;
  /** The fold's own error sentence when its 44244 set could not be read. */
  readonly unreadable: string | null;
  /** How many 44244 events the fold excluded — a count, never their content. */
  readonly excludedCount: number;
  readonly assignments: readonly PulseDeclaredAssignment[];
};

/** One page of the read, decoded. */
export type PulseDeclaredWorkResponse = {
  readonly schema: typeof PULSE_DECLARED_WORK_SCHEMA;
  readonly viewerPubkey: string | null;
  readonly sessions: readonly PulseDeclaredWorkSession[];
  readonly errors: readonly PulseDeclaredWorkError[];
};

/**
 * Lowercase 64-hex: an event id, a pubkey, a genesis ref.
 *
 * Pinned rather than "looks hex-ish": every id in this response is either a
 * source a reader opens or an actor a row attributes work to, and a widened
 * value would make the decoder the place a producer's defect became normal.
 */
const HEX64 = /^[0-9a-f]{64}$/;

/** A git object id, 40- or 64-hex, exactly as kind 44244 validates one. */
const GIT_SHA = /^[0-9a-f]{40}$|^[0-9a-f]{64}$/;

/**
 * A canonical UUID shape — channel and umbrella refs.
 *
 * Shape, not RFC 4122 version and variant: these are opaque keys this reader
 * echoes back for row identity and derives nothing from, and the fold that
 * produced them has already validated them canonically.
 */
const CANONICAL_UUID = /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/;

/** The closed lifecycle vocabulary. */
const LIFECYCLES: readonly PulseDeclaredWorkLifecycle[] = ["open", "closed"];

/** The closed disposition vocabulary, exactly as kind 44244 spells it. */
const DECISIONS: readonly PulseDeclaredDispositionDecision[] = [
  "approve",
  "approve-with-notes",
  "changes-requested",
  "reject",
  "blocked",
];

/** The closed terminal vocabulary. */
const TERMINAL_TYPES: readonly PulseDeclaredTerminalType[] = [
  "mission.completed",
  "mission.blocked",
];

/** The closed status vocabulary Rust derives, once, per assignment. */
const STATUSES: readonly PulseDeclaredAssignmentStatus[] = [
  "unresolved",
  "reported",
  "settled",
];

/** A decode failure. The message names the field or token that caused it. */
function fail(what: string): never {
  throw new Error(`pulse declared work: ${what}`);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Exactly these keys, and say which one broke it.
 *
 * A surface that can only say "malformed" makes a one-key producer change
 * indistinguishable from a relay outage, so the offending key is named.
 */
function requireExactFields(
  value: unknown,
  fields: readonly string[],
  where: string,
): Record<string, unknown> {
  if (!isPlainObject(value)) fail(`${where} is not an object`);
  if (!hasExactFields(value, [fields])) {
    const keys = Object.keys(value);
    const extra = keys.filter((key) => !fields.includes(key));
    const missing = fields.filter((key) => !Object.hasOwn(value, key));
    if (extra.length > 0)
      fail(`${where} has an unknown key ${extra.join(", ")}`);
    fail(`${where} is missing ${missing.join(", ")}`);
  }
  return value;
}

function requireString(
  value: unknown,
  where: string,
  pattern?: RegExp,
): string {
  if (typeof value !== "string" || value.trim().length === 0) {
    fail(`${where} is not a non-empty string`);
  }
  if (pattern && !pattern.test(value))
    fail(`${where} is not the expected shape`);
  return value;
}

function requireNullableString(
  value: unknown,
  where: string,
  pattern?: RegExp,
): string | null {
  return value === null ? null : requireString(value, where, pattern);
}

function requireStrings(value: unknown, where: string): readonly string[] {
  return requireArray(value, where).map((item, index) =>
    requireString(item, `${where}[${index}]`),
  );
}

function requireSeconds(value: unknown, where: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    fail(`${where} is not a whole number of seconds`);
  }
  return value as number;
}

function requireNullableSeconds(value: unknown, where: string): number | null {
  return value === null ? null : requireSeconds(value, where);
}

function requireCount(value: unknown, where: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    fail(`${where} is not a count`);
  }
  return value as number;
}

function requireBoolean(value: unknown, where: string): boolean {
  if (typeof value !== "boolean") fail(`${where} is not a boolean`);
  return value;
}

function requireArray(value: unknown, where: string): readonly unknown[] {
  if (!Array.isArray(value)) fail(`${where} is not an array`);
  return value;
}

function requireToken<T extends string>(
  value: unknown,
  allowed: readonly T[],
  where: string,
): T {
  if (typeof value !== "string" || !allowed.includes(value as T)) {
    fail(`${where} carries the unknown token ${JSON.stringify(value)}`);
  }
  return value as T;
}

function decodeReport(value: unknown, where: string): PulseDeclaredReport {
  const fields = requireExactFields(
    value,
    [
      "eventId",
      "authorPubkey",
      "createdAt",
      "summary",
      "branch",
      "baseSha",
      "headSha",
      "files",
      "testCount",
      "deviations",
      "residuals",
      "authorUnseated",
    ],
    where,
  );
  return {
    eventId: requireString(fields.eventId, `${where}.eventId`, HEX64),
    authorPubkey: requireString(
      fields.authorPubkey,
      `${where}.authorPubkey`,
      HEX64,
    ),
    createdAt: requireSeconds(fields.createdAt, `${where}.createdAt`),
    summary: requireString(fields.summary, `${where}.summary`),
    branch: requireNullableString(fields.branch, `${where}.branch`),
    baseSha: requireNullableString(fields.baseSha, `${where}.baseSha`, GIT_SHA),
    headSha: requireNullableString(fields.headSha, `${where}.headSha`, GIT_SHA),
    files: requireStrings(fields.files, `${where}.files`),
    // A count, not a verdict: the tests themselves live in the source event,
    // and a passing count is never independent verification.
    testCount: requireCount(fields.testCount, `${where}.testCount`),
    deviations: requireStrings(fields.deviations, `${where}.deviations`),
    residuals: requireStrings(fields.residuals, `${where}.residuals`),
    authorUnseated: requireBoolean(
      fields.authorUnseated,
      `${where}.authorUnseated`,
    ),
  };
}

function decodeDisposition(
  value: unknown,
  where: string,
): PulseDeclaredDisposition {
  const fields = requireExactFields(
    value,
    ["eventId", "authorPubkey", "createdAt", "decision", "reportRef"],
    where,
  );
  return {
    eventId: requireString(fields.eventId, `${where}.eventId`, HEX64),
    authorPubkey: requireString(
      fields.authorPubkey,
      `${where}.authorPubkey`,
      HEX64,
    ),
    createdAt: requireSeconds(fields.createdAt, `${where}.createdAt`),
    decision: requireToken(fields.decision, DECISIONS, `${where}.decision`),
    reportRef: requireString(fields.reportRef, `${where}.reportRef`, HEX64),
  };
}

function decodeSettlement(
  value: unknown,
  where: string,
): PulseDeclaredSettlement {
  const fields = requireExactFields(
    value,
    [
      "settled",
      "governedReportEventId",
      "dispositionEventId",
      "acknowledgementEventId",
      "settledBy",
    ],
    where,
  );
  return {
    settled: requireBoolean(fields.settled, `${where}.settled`),
    governedReportEventId: requireNullableString(
      fields.governedReportEventId,
      `${where}.governedReportEventId`,
      HEX64,
    ),
    dispositionEventId: requireNullableString(
      fields.dispositionEventId,
      `${where}.dispositionEventId`,
      HEX64,
    ),
    acknowledgementEventId: requireNullableString(
      fields.acknowledgementEventId,
      `${where}.acknowledgementEventId`,
      HEX64,
    ),
    settledBy: decodeSettledBy(fields.settledBy, `${where}.settledBy`),
  };
}

/** The closed settlement-rule vocabulary, or `null`. */
function decodeSettledBy(
  value: unknown,
  where: string,
): PulseDeclaredSettlement["settledBy"] {
  if (value === null) return null;
  if (
    value === "acknowledgement" ||
    value === "approving_disposition_without_ask"
  ) {
    return value;
  }
  fail(`${where} is not a settlement rule this build knows`);
}

function decodeAssignment(
  value: unknown,
  where: string,
): PulseDeclaredAssignment {
  const fields = requireExactFields(
    value,
    [
      "sourceEventId",
      "createdAt",
      "assignerPubkey",
      "assigneeActor",
      "assigneeRole",
      "objective",
      "brief",
      "branch",
      "baseSha",
      "fileOwnership",
      "acceptanceSteps",
      "supersedes",
      "reports",
      "dispositions",
      "settlement",
      "status",
    ],
    where,
  );
  return {
    sourceEventId: requireString(
      fields.sourceEventId,
      `${where}.sourceEventId`,
      HEX64,
    ),
    createdAt: requireSeconds(fields.createdAt, `${where}.createdAt`),
    assignerPubkey: requireString(
      fields.assignerPubkey,
      `${where}.assignerPubkey`,
      HEX64,
    ),
    assigneeActor: requireString(
      fields.assigneeActor,
      `${where}.assigneeActor`,
      HEX64,
    ),
    assigneeRole: requireString(fields.assigneeRole, `${where}.assigneeRole`),
    objective: requireString(fields.objective, `${where}.objective`),
    brief: requireString(fields.brief, `${where}.brief`),
    branch: requireNullableString(fields.branch, `${where}.branch`),
    baseSha: requireNullableString(fields.baseSha, `${where}.baseSha`, GIT_SHA),
    // Declared paths, verbatim. Nothing here normalises, splits or compares
    // them: an ambiguous path is uncomparable, never an inferred match.
    fileOwnership: requireStrings(
      fields.fileOwnership,
      `${where}.fileOwnership`,
    ),
    acceptanceSteps: requireStrings(
      fields.acceptanceSteps,
      `${where}.acceptanceSteps`,
    ),
    supersedes: requireNullableString(
      fields.supersedes,
      `${where}.supersedes`,
      HEX64,
    ),
    reports: requireArray(fields.reports, `${where}.reports`).map(
      (report, index) => decodeReport(report, `${where}.reports[${index}]`),
    ),
    dispositions: requireArray(
      fields.dispositions,
      `${where}.dispositions`,
    ).map((disposition, index) =>
      decodeDisposition(disposition, `${where}.dispositions[${index}]`),
    ),
    settlement: decodeSettlement(fields.settlement, `${where}.settlement`),
    status: requireToken(fields.status, STATUSES, `${where}.status`),
  };
}

function decodeTerminal(value: unknown, where: string): PulseDeclaredTerminal {
  const fields = requireExactFields(value, ["eventId", "type", "at"], where);
  return {
    eventId: requireString(fields.eventId, `${where}.eventId`, HEX64),
    type: requireToken(fields.type, TERMINAL_TYPES, `${where}.type`),
    at: requireSeconds(fields.at, `${where}.at`),
  };
}

function decodeSession(
  value: unknown,
  where: string,
): PulseDeclaredWorkSession {
  const fields = requireExactFields(
    value,
    [
      "sessionKey",
      "sessionRef",
      "genesisRef",
      "channelId",
      "name",
      "lifecycle",
      "latestObservationAt",
      "founderPubkey",
      "terminal",
      "unreadable",
      "excludedCount",
      "assignments",
    ],
    where,
  );
  return {
    sessionKey: requireString(fields.sessionKey, `${where}.sessionKey`),
    sessionRef: requireString(
      fields.sessionRef,
      `${where}.sessionRef`,
      CANONICAL_UUID,
    ),
    genesisRef: requireString(fields.genesisRef, `${where}.genesisRef`, HEX64),
    channelId: requireString(
      fields.channelId,
      `${where}.channelId`,
      CANONICAL_UUID,
    ),
    name: requireNullableString(fields.name, `${where}.name`),
    lifecycle: requireToken(fields.lifecycle, LIFECYCLES, `${where}.lifecycle`),
    latestObservationAt: requireNullableSeconds(
      fields.latestObservationAt,
      `${where}.latestObservationAt`,
    ),
    founderPubkey: requireString(
      fields.founderPubkey,
      `${where}.founderPubkey`,
      HEX64,
    ),
    terminal:
      fields.terminal === null
        ? null
        : decodeTerminal(fields.terminal, `${where}.terminal`),
    unreadable: requireNullableString(fields.unreadable, `${where}.unreadable`),
    // A count, never their content: an excluded record is not a declaration.
    excludedCount: requireCount(fields.excludedCount, `${where}.excludedCount`),
    assignments: requireArray(fields.assignments, `${where}.assignments`).map(
      (assignment, index) =>
        decodeAssignment(assignment, `${where}.assignments[${index}]`),
    ),
  };
}

function decodeError(value: unknown, where: string): PulseDeclaredWorkError {
  const fields = requireExactFields(value, ["scope", "message"], where);
  return {
    scope: requireString(fields.scope, `${where}.scope`),
    message: requireString(fields.message, `${where}.message`),
  };
}

function deepFreeze<T>(value: T): T {
  if (Array.isArray(value)) {
    for (const item of value) deepFreeze(item);
    return Object.freeze(value);
  }
  if (isPlainObject(value)) {
    for (const item of Object.values(value)) deepFreeze(item);
    return Object.freeze(value) as T;
  }
  return value;
}

/**
 * Decode one `pulse_declared_work` payload, or throw naming what broke.
 *
 * A rejection is an error, never an empty section: silently returning nothing
 * would turn a producer/reader disagreement into "no declared work", which is
 * exactly the coordination lie this feature exists to prevent.
 *
 * The result is deep-frozen — these bytes are a signed-fact projection, and a
 * renderer able to edit one could change what the surface says a seat owes.
 */
export function decodePulseDeclaredWork(
  raw: unknown,
): PulseDeclaredWorkResponse {
  const fields = requireExactFields(
    raw,
    ["schema", "viewerPubkey", "sessions", "errors"],
    "response",
  );
  if (fields.schema !== PULSE_DECLARED_WORK_SCHEMA) {
    fail(
      `response.schema is ${JSON.stringify(fields.schema)}, not ${PULSE_DECLARED_WORK_SCHEMA}`,
    );
  }
  return deepFreeze({
    schema: PULSE_DECLARED_WORK_SCHEMA,
    viewerPubkey: requireNullableString(
      fields.viewerPubkey,
      "response.viewerPubkey",
      HEX64,
    ),
    sessions: requireArray(fields.sessions, "response.sessions").map(
      (session, index) => decodeSession(session, `response.sessions[${index}]`),
    ),
    errors: requireArray(fields.errors, "response.errors").map((error, index) =>
      decodeError(error, `response.errors[${index}]`),
    ),
  });
}
