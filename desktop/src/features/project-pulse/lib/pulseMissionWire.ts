/**
 * The `pulse_mission_rows` wire: types, and a decoder that refuses by name.
 *
 * This module decodes. It does not fold, re-word, or compose. Every human
 * sentence on the Missions surface arrives here as a `text` string written by
 * `buzz-core` and is rendered verbatim; TypeScript's whole job is to prove the
 * bytes are the shape Rust promised and to hand them to React unchanged.
 *
 * Two disciplines are load-bearing:
 *
 * - **Closed sets, refused rather than coerced.** `state`, `kind`, and every
 *   line `id` is a closed vocabulary per family. An unknown token is a decode
 *   failure naming the token, never a row rendered with a shrug. A reader that
 *   passed an unknown line id through would paint a sentence under a heading
 *   it does not belong to, which on this surface is a coordination lie.
 * - **A rejection is an error, never an empty Pulse.** The decoder throws; the
 *   caller renders that failure. Silently returning nothing would turn a
 *   producer/reader disagreement into "this project is quiet", which is the
 *   exact dishonesty this feature exists to prevent.
 *
 * The response is a **sibling** of `ProjectPulseDigest`, not a replacement: it
 * adds eight top-level keys and retypes nothing, so a reader built before it
 * existed ignores eight keys it does not know.
 */

import { hasExactFields } from "@/shared/coordination/sessionCoordinationStrictJson";

/** Exact `missionsSchema` value. A different one is refused, not upgraded. */
export const PULSE_MISSION_ROWS_SCHEMA = "buzz-pulse-mission-rows/v1";

/** Every mission state Rust may report. Closed. */
export const PULSE_MISSION_STATES = [
  "running",
  "blocked",
  "completed",
  "unreadable",
] as const;
export type PulseMissionState = (typeof PULSE_MISSION_STATES)[number];

/** Line ids that may appear on a mission row itself. Closed. */
export const PULSE_MISSION_LINE_IDS = [
  "waiting",
  "state",
  "verdict",
  "excluded-completion",
  "policy",
  "seat-claims-refused",
  "ref-state",
  "wip-window",
  "not-read",
  "unreadable",
  "landing",
] as const;
export type PulseMissionLineId = (typeof PULSE_MISSION_LINE_IDS)[number];

/** Line ids that may appear on a seat. Closed, and disjoint from the rest. */
export const PULSE_SEAT_LINE_IDS = [
  "live",
  "gate",
  "gate-truncated",
  "gate-missing",
  "checkpoint",
  "owed",
  "wip",
] as const;
export type PulseSeatLineId = (typeof PULSE_SEAT_LINE_IDS)[number];

/** The one line id a moved-commit row may carry. */
export const PULSE_MOVED_LINE_IDS = ["moved"] as const;
export type PulseMovedLineId = (typeof PULSE_MOVED_LINE_IDS)[number];

/** Line ids on the timing/cost tail of a mission row. Closed. */
export const PULSE_TIMING_LINE_IDS = [
  "timing",
  "timing-missing",
  "cost",
] as const;
export type PulseTimingLineId = (typeof PULSE_TIMING_LINE_IDS)[number];

/** Line ids on an overlap row. Closed. */
export const PULSE_OVERLAP_LINE_IDS = ["overlap", "overlap-truncated"] as const;
export type PulseOverlapLineId = (typeof PULSE_OVERLAP_LINE_IDS)[number];

/** What kinds of commit a mission's `moved` rows describe. Closed. */
export const PULSE_MOVED_KINDS = ["wip", "landing"] as const;
export type PulseMovedKind = (typeof PULSE_MOVED_KINDS)[number];

/** One rendered sentence and the closed id that says where it belongs. */
export type PulseLine<Id extends string> = {
  readonly id: Id;
  readonly text: string;
};

export type PulseMissionLine = PulseLine<PulseMissionLineId>;
export type PulseSeatLine = PulseLine<PulseSeatLineId>;
export type PulseMovedLine = PulseLine<PulseMovedLineId>;
export type PulseTimingLine = PulseLine<PulseTimingLineId>;
export type PulseOverlapLine = PulseLine<PulseOverlapLineId>;

/** One seat on a mission, with the lines Rust wrote about it. */
export type PulseMissionSeat = {
  readonly pubkey: string;
  readonly role: string | null;
  readonly lines: readonly PulseSeatLine[];
};

/** One commit that moved: a wip-ref commit, or a landing on a tracked ref. */
export type PulseMissionMoved = {
  readonly kind: PulseMovedKind;
  readonly sha: string;
  readonly ref: string;
  readonly authorPubkey: string;
  readonly subject: string | null;
  readonly ageSeconds: number | null;
  readonly lines: readonly PulseMovedLine[];
};

/** One mission row. `lines`, `seats`, `moved`, `timing` render in order. */
export type PulseMissionRow = {
  readonly sessionKey: string;
  readonly sessionRef: string | null;
  readonly channelId: string;
  readonly name: string | null;
  readonly state: PulseMissionState;
  readonly latestObservationAt: number | null;
  readonly lines: readonly PulseMissionLine[];
  readonly seats: readonly PulseMissionSeat[];
  readonly moved: readonly PulseMissionMoved[];
  readonly timing: readonly PulseTimingLine[];
};

/** One open ruling: a question a named person still holds. */
export type PulseMissionRuling = {
  readonly sessionKey: string;
  readonly requestId: string;
  /** Exactly `"founder"`, or a 64-hex actor pubkey. */
  readonly heldOn: string;
  readonly askedBy: string;
  readonly askedAt: number | null;
  readonly question: string | null;
};

/** One seat's side of a path two missions are both touching. */
export type PulseOverlapSeat = {
  readonly sessionKey: string;
  readonly authorPubkey: string;
  readonly sha: string;
  readonly ageSeconds: number | null;
};

/** Two missions touching the same paths. Carries no action and no wake. */
export type PulseMissionOverlap = {
  readonly paths: readonly string[];
  readonly pathsTruncated: number;
  readonly seats: readonly PulseOverlapSeat[];
  readonly lines: readonly PulseOverlapLine[];
};

/** One read that failed or was bounded. Never rendered as a quiet project. */
export type PulseMissionError = {
  readonly scope: string;
  readonly message: string;
};

/** The eight keys `pulse_mission_rows` adds beside the existing digest. */
export type PulseMissionRowsResponse = {
  readonly missionsSchema: typeof PULSE_MISSION_ROWS_SCHEMA;
  readonly missionScope: string;
  readonly missions: readonly PulseMissionRow[];
  readonly missionErrors: readonly PulseMissionError[];
  readonly openRulings: readonly PulseMissionRuling[];
  readonly rulingsWaitingOnViewer: readonly PulseMissionRuling[];
  readonly overlaps: readonly PulseMissionOverlap[];
  /** The reader's own pubkey, or null when this client does not know it. */
  readonly viewerPubkey: string | null;
};

const HEX64 = /^[0-9a-f]{64}$/;
const HEX40 = /^[0-9a-f]{40}$/;
/**
 * An actor pubkey: lowercase 64-hex, pinned.
 *
 * Sub-lane T found the contract fixture carrying two 66-character actor values
 * and, rather than absorb them, left the width unpinned with a test naming the
 * defect. The lane owner corrected the fixture's generator
 * (`crates/buzz-core/src/pulse_mission_fixture_tests.rs`), so the pin is back
 * where every other reader in this app has it. A producer that widens an actor
 * pubkey now fails at the one bad field instead of quietly becoming the
 * decoder's new normal.
 */
const ACTOR_HEX = HEX64;
/**
 * A session ref is matched on shape, not on RFC 4122 version and variant.
 *
 * `isCanonicalSessionRef` in the shared strict-JSON module pins those nibbles.
 * This reader deliberately does not: a `sessionRef` here is an opaque key it
 * echoes back for row identity and derives nothing from, and refusing a whole
 * surface over a version nibble would be this reader disagreeing with its
 * producer about a field neither of them reads.
 */
const SESSION_REF = /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/;

/** A decode failure. The message names the field or token that caused it. */
function fail(what: string): never {
  throw new Error(`pulse mission rows: ${what}`);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Exactly these keys, and say which one broke it.
 *
 * `hasExactFields` answers yes or no; a surface that can only say "malformed"
 * makes a one-key producer change indistinguishable from a relay outage, so
 * the offending key is named here.
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

function requireWholeSeconds(value: unknown, where: string): number | null {
  if (value === null) return null;
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    fail(`${where} is not a whole number of seconds`);
  }
  return value as number;
}

function requireCount(value: unknown, where: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    fail(`${where} is not a count`);
  }
  return value as number;
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

/** One `{id, text}` line, with `id` closed to this family's vocabulary. */
function decodeLines<Id extends string>(
  value: unknown,
  allowed: readonly Id[],
  where: string,
): readonly PulseLine<Id>[] {
  return requireArray(value, where).map((line, index) => {
    const at = `${where}[${index}]`;
    const fields = requireExactFields(line, ["id", "text"], at);
    return {
      id: requireToken(fields.id, allowed, `${at}.id`),
      text: requireString(fields.text, `${at}.text`),
    };
  });
}

function decodeSeat(value: unknown, where: string): PulseMissionSeat {
  const fields = requireExactFields(value, ["pubkey", "role", "lines"], where);
  return {
    pubkey: requireString(fields.pubkey, `${where}.pubkey`, ACTOR_HEX),
    role: requireNullableString(fields.role, `${where}.role`),
    lines: decodeLines(fields.lines, PULSE_SEAT_LINE_IDS, `${where}.lines`),
  };
}

function decodeMoved(value: unknown, where: string): PulseMissionMoved {
  const fields = requireExactFields(
    value,
    ["kind", "sha", "ref", "authorPubkey", "subject", "ageSeconds", "lines"],
    where,
  );
  return {
    kind: requireToken(fields.kind, PULSE_MOVED_KINDS, `${where}.kind`),
    sha: requireString(fields.sha, `${where}.sha`, HEX40),
    ref: requireString(fields.ref, `${where}.ref`),
    authorPubkey: requireString(
      fields.authorPubkey,
      `${where}.authorPubkey`,
      ACTOR_HEX,
    ),
    subject: requireNullableString(fields.subject, `${where}.subject`),
    ageSeconds: requireWholeSeconds(fields.ageSeconds, `${where}.ageSeconds`),
    lines: decodeLines(fields.lines, PULSE_MOVED_LINE_IDS, `${where}.lines`),
  };
}

function decodeMission(value: unknown, where: string): PulseMissionRow {
  const fields = requireExactFields(
    value,
    [
      "sessionKey",
      "sessionRef",
      "channelId",
      "name",
      "state",
      "latestObservationAt",
      "lines",
      "seats",
      "moved",
      "timing",
    ],
    where,
  );
  return {
    sessionKey: requireString(fields.sessionKey, `${where}.sessionKey`),
    sessionRef: requireNullableString(
      fields.sessionRef,
      `${where}.sessionRef`,
      SESSION_REF,
    ),
    channelId: requireString(fields.channelId, `${where}.channelId`, HEX64),
    name: requireNullableString(fields.name, `${where}.name`),
    state: requireToken(fields.state, PULSE_MISSION_STATES, `${where}.state`),
    latestObservationAt: requireWholeSeconds(
      fields.latestObservationAt,
      `${where}.latestObservationAt`,
    ),
    lines: decodeLines(fields.lines, PULSE_MISSION_LINE_IDS, `${where}.lines`),
    seats: requireArray(fields.seats, `${where}.seats`).map((seat, index) =>
      decodeSeat(seat, `${where}.seats[${index}]`),
    ),
    moved: requireArray(fields.moved, `${where}.moved`).map((moved, index) =>
      decodeMoved(moved, `${where}.moved[${index}]`),
    ),
    timing: decodeLines(
      fields.timing,
      PULSE_TIMING_LINE_IDS,
      `${where}.timing`,
    ),
  };
}

/** `"founder"` or a 64-hex actor. Anything else is a holder nobody can find. */
function decodeHolder(value: unknown, where: string): string {
  if (value === "founder") return value;
  if (typeof value !== "string" || !ACTOR_HEX.test(value)) {
    fail(`${where} names no holder this client can resolve`);
  }
  return value;
}

function decodeRuling(value: unknown, where: string): PulseMissionRuling {
  const fields = requireExactFields(
    value,
    ["sessionKey", "requestId", "heldOn", "askedBy", "askedAt", "question"],
    where,
  );
  return {
    sessionKey: requireString(fields.sessionKey, `${where}.sessionKey`),
    requestId: requireString(fields.requestId, `${where}.requestId`, HEX64),
    heldOn: decodeHolder(fields.heldOn, `${where}.heldOn`),
    askedBy: requireString(fields.askedBy, `${where}.askedBy`, ACTOR_HEX),
    askedAt: requireWholeSeconds(fields.askedAt, `${where}.askedAt`),
    question: requireNullableString(fields.question, `${where}.question`),
  };
}

function decodeOverlapSeat(value: unknown, where: string): PulseOverlapSeat {
  const fields = requireExactFields(
    value,
    ["sessionKey", "authorPubkey", "sha", "ageSeconds"],
    where,
  );
  return {
    sessionKey: requireString(fields.sessionKey, `${where}.sessionKey`),
    authorPubkey: requireString(
      fields.authorPubkey,
      `${where}.authorPubkey`,
      ACTOR_HEX,
    ),
    sha: requireString(fields.sha, `${where}.sha`, HEX40),
    ageSeconds: requireWholeSeconds(fields.ageSeconds, `${where}.ageSeconds`),
  };
}

function decodeOverlap(value: unknown, where: string): PulseMissionOverlap {
  const fields = requireExactFields(
    value,
    ["paths", "pathsTruncated", "seats", "lines"],
    where,
  );
  return {
    paths: requireArray(fields.paths, `${where}.paths`).map((path, index) =>
      requireString(path, `${where}.paths[${index}]`),
    ),
    pathsTruncated: requireCount(
      fields.pathsTruncated,
      `${where}.pathsTruncated`,
    ),
    seats: requireArray(fields.seats, `${where}.seats`).map((seat, index) =>
      decodeOverlapSeat(seat, `${where}.seats[${index}]`),
    ),
    lines: decodeLines(fields.lines, PULSE_OVERLAP_LINE_IDS, `${where}.lines`),
  };
}

function decodeError(value: unknown, where: string): PulseMissionError {
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
 * Decode one `pulse_mission_rows` payload, or throw naming what broke.
 *
 * The result is deep-frozen: these bytes are a signed-fact projection, and a
 * renderer that could edit one would be able to change what the surface says a
 * seat did.
 */
export function decodePulseMissionRows(
  value: unknown,
): PulseMissionRowsResponse {
  const fields = requireExactFields(
    value,
    [
      "missionsSchema",
      "missionScope",
      "missions",
      "missionErrors",
      "openRulings",
      "rulingsWaitingOnViewer",
      "overlaps",
      "viewerPubkey",
    ],
    "response",
  );
  if (fields.missionsSchema !== PULSE_MISSION_ROWS_SCHEMA) {
    fail(
      `response.missionsSchema is ${JSON.stringify(fields.missionsSchema)}, not ${PULSE_MISSION_ROWS_SCHEMA}`,
    );
  }
  return deepFreeze({
    missionsSchema: PULSE_MISSION_ROWS_SCHEMA,
    missionScope: requireString(fields.missionScope, "response.missionScope"),
    missions: requireArray(fields.missions, "response.missions").map(
      (mission, index) => decodeMission(mission, `response.missions[${index}]`),
    ),
    missionErrors: requireArray(
      fields.missionErrors,
      "response.missionErrors",
    ).map((error, index) =>
      decodeError(error, `response.missionErrors[${index}]`),
    ),
    openRulings: requireArray(fields.openRulings, "response.openRulings").map(
      (ruling, index) => decodeRuling(ruling, `response.openRulings[${index}]`),
    ),
    rulingsWaitingOnViewer: requireArray(
      fields.rulingsWaitingOnViewer,
      "response.rulingsWaitingOnViewer",
    ).map((ruling, index) =>
      decodeRuling(ruling, `response.rulingsWaitingOnViewer[${index}]`),
    ),
    overlaps: requireArray(fields.overlaps, "response.overlaps").map(
      (overlap, index) => decodeOverlap(overlap, `response.overlaps[${index}]`),
    ),
    viewerPubkey: requireNullableString(
      fields.viewerPubkey,
      "response.viewerPubkey",
      ACTOR_HEX,
    ),
  });
}

/**
 * The mission lines that say this client could not read who the viewer is.
 *
 * With `viewerPubkey === null` the waiting list is not empty — it is *unknown*,
 * and the two must never render the same. The sentence for that state is
 * written by Rust and carried on a `not-read` mission line; this selector only
 * finds it. When Rust supplied none, the caller renders no waiting claim at
 * all rather than inventing one.
 */
export function pulseMissionNotReadLines(
  response: PulseMissionRowsResponse,
): readonly PulseMissionLine[] {
  // De-duplicated by text: the model composes this line on **every** mission
  // row, because a row is where it composes lines, but it is one statement
  // about the reader and repeating it once per session would read as a list of
  // separate problems.
  const seen = new Set<string>();
  const lines: PulseMissionLine[] = [];
  for (const mission of response.missions) {
    for (const line of mission.lines) {
      if (line.id !== "not-read" || seen.has(line.text)) continue;
      seen.add(line.text);
      lines.push(line);
    }
  }
  return lines;
}

/** The mission rows for one channel, in the order Rust returned them. */
export function pulseMissionRowsForChannel(
  response: PulseMissionRowsResponse | null,
  channelId: string,
): readonly PulseMissionRow[] {
  return (
    response?.missions.filter((mission) => mission.channelId === channelId) ??
    []
  );
}

/** The mission row for one session key, or null when this read has none. */
export function pulseMissionRowForSession(
  response: PulseMissionRowsResponse | null,
  sessionKey: string | null,
): PulseMissionRow | null {
  if (!response || sessionKey === null) return null;
  return (
    response.missions.find((mission) => mission.sessionKey === sessionKey) ??
    null
  );
}
