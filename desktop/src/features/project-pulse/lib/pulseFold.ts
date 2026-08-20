/**
 * The Project Pulse digest fold — the TypeScript twin of the Rust fold in
 * `buzz pulse digest` (Slice 1) and of kind 39011 (Slice 2).
 *
 * Both languages bind to `conformance/project-pulse-fold/fixtures/fold-vectors.json`
 * and must produce byte-identical digests for the same events and clock; that
 * corpus, not this file, is the source of truth for every rule here
 * (`conformance/project-pulse-fold/CONTRACT.md`). A fold rule that exists in
 * only one language is a defect.
 *
 * The fold produces facts. It never renders: no humanized ages, no advisory,
 * no "who should wait" — a surface computes those from `verifiedAt`,
 * `observedAgeSeconds`, and `activity` at paint time.
 *
 * LOAD-BEARING CONSTRAINT: this module and `pulseEntry.ts` must keep ZERO
 * runtime imports outside this directory and use erasable TypeScript syntax
 * only (no enums, no namespaces, no parameter properties).
 * `conformance/project-pulse-fold/implementation.test.mjs` imports them under
 * plain `node --test` with Node's native type stripping, so an aliased (`@/…`)
 * or non-erasable construct breaks the conformance gate.
 *
 * The 44223 shape this module reads is deliberately minimal — only the fields
 * the digest carries. In the product path the caller validates every metadata
 * event with `parseBuzzCodingSessionMetadata`
 * (`desktop/src/features/coding-sessions/lib/codingSessionIngressPayloads.ts`)
 * *before* handing it here, so the strict all-four-or-none fact discipline and
 * the `relayReachable`/`verifiedAt` null coupling are still enforced by the one
 * shared decoder; see `useProjectPulseDigest` in `pulseQueries.ts`.
 *
 * KNOWN, NARROWED DIVERGENCE: the Rust fold decodes 44223 with
 * `buzz_core::coding_session_payload::decode_coding_session_metadata` (exact
 * key set, four accepted shapes), while the desktop path uses the more
 * permissive `parseBuzzCodingSessionMetadata`. A 44223 carrying a key
 * buzz-core does not know is therefore dropped by the CLI and kept here. The
 * *session identity* no longer diverges — `codingSessionTargetKey` below
 * derives it from the same content field, by the same algorithm — but the
 * admissibility rule still has two owners. Closing it means picking one
 * decoder as the contract; until then, do not add a conformance vector whose
 * outcome depends on it.
 */

import {
  PULSE_ENTRY_KIND,
  normalizePulseProjectCoordinate,
  pulseEntryProjectCoordinate,
  pulseEntrySessionRef,
  validatePulseEntryEnvelope,
  type PulseEntryType,
  type PulseEvent,
} from "./pulseEntry.ts";

/** Exact `schema` value of the digest envelope (kind 39011's content). */
export const PULSE_DIGEST_SCHEMA = "buzz-project-pulse-digest/v1";

/** The only session reach v1 has: the project's own channels, never wider. */
export const PULSE_SESSIONS_SCOPE = "project channels";

/**
 * A session counts as Active work only within this many seconds of its newest
 * 44223 observation. Pinned against the corpus's `activeWindowSeconds`.
 *
 * The absence of a closure is not evidence of life: a machine that dies
 * mid-turn leaves the last 44223 saying `running` forever, and answering "who
 * is working on this project?" with that ghost tells a new worker to wait on
 * it indefinitely.
 */
export const PULSE_ACTIVE_WINDOW_SECONDS = 1800;

/** Coding-session wire statuses that can carry a session into Active work. */
export const PULSE_ACTIVE_STATUSES = [
  "starting",
  "running",
  "idle",
  "waiting_for_input",
] as const;

/** Kinds the session half of the fold consumes, in wire order. */
export const PULSE_SESSION_KINDS = [44223, 44227, 44229, 44230] as const;

/** The fixed tri-state commit-confirmation strings every surface emits. */
export const PULSE_COMMIT_CONFIRMED = "Commit confirmed on relay";
export const PULSE_COMMIT_NOT_FOUND = "Commit not found on relay";
export const PULSE_COMMIT_NOT_CHECKED = "Commit not checked";

/** Why a `supersedes` claim was not honored; `null` on an honored claim. */
export type PulseSupersessionReason =
  | "cross-author"
  | "unresolved"
  | "out-of-order";

/** One supersession claim, from the perspective of the entry that carries it. */
export type PulseSupersessionClaim = {
  eventId: string;
  /** The other entry's author, or `null` when it is not in the result set. */
  pubkey: string | null;
  honored: boolean;
  reason: PulseSupersessionReason | null;
};

/** One folded Pulse entry. `claimedAreas` are claims, never observed facts. */
export type PulseDigestEntry = {
  eventId: string;
  pubkey: string;
  createdAt: number;
  type: PulseEntryType;
  text: string;
  claimedAreas: string[];
  branch: string | null;
  sessionRef: string | null;
  supersedes: string | null;
  supersededBy: PulseSupersessionClaim[];
  active: boolean;
};

/** Whether a session passed the positive freshness test, per §5.4. */
export type PulseSessionActivity = "active" | "stale";

/** One folded coding session, with every observation kept tri-state. */
export type PulseDigestSession = {
  targetKey: string;
  sessionRef: string | null;
  name: string | null;
  goal: string | null;
  status: string;
  statusAt: number;
  closed: boolean;
  activity: PulseSessionActivity;
  branch: string | null;
  observedCommit: string | null;
  dirty: boolean | null;
  relayReachable: boolean | null;
  verifiedAt: number | null;
  commitConfirmation: string;
  observedAgeSeconds: number;
  sourceEventIds: string[];
};

/** One source query that failed, was truncated, or yielded a bad event. */
export type PulseDigestError = { scope: string; message: string };

/** The complete digest envelope — the §6 kind-39011 content object. */
export type ProjectPulseDigest = {
  schema: typeof PULSE_DIGEST_SCHEMA;
  source: string;
  project: string;
  asOf: number;
  complete: boolean;
  sessionsScope: typeof PULSE_SESSIONS_SCOPE;
  sessions: PulseDigestSession[];
  entries: PulseDigestEntry[];
  errors: PulseDigestError[];
};

/** Everything the fold consumes. `now` is read once, after the last query. */
export type ProjectPulseFoldInput = {
  project: string;
  now: number;
  events: readonly PulseEvent[];
  /** One entry per source query that failed or was truncated by `limit`. */
  sourceErrors?: readonly PulseDigestError[];
  /** `client-composed` here; a relay-side fold substitutes `relay-digest`. */
  source?: string;
};

type SessionMetadataFacts = {
  targetKey: string;
  projectRef: string | null;
  status: string;
  branch: string | null;
  sessionRef: string | null;
  observedCommit: string | null;
  dirty: boolean | null;
  relayReachable: boolean | null;
  verifiedAt: number | null;
};

function tagValue(event: PulseEvent, key: string): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === key);
  return tag?.[1] ?? null;
}

/**
 * Byte-wise string order, matching Rust's `Ord for str`/`String`.
 *
 * `String.prototype.localeCompare` is ICU collation, which is **not** code-unit
 * order — e.g. `"…|6:claude".localeCompare("…|60claude")` is `-1` while the
 * byte comparison is `+1`. The Rust fold
 * (`crates/buzz-cli/src/commands/pulse.rs`) sorts with `Ord`, so every string
 * tiebreak in this fold must use this instead, or the two digests stop being
 * byte-identical for arbitrary `targetKey` / `errors[].message` values (R5).
 */
function byteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** `(created_at, event id)` — the total order every fold in this repo uses. */
function isNewer(candidate: PulseEvent, incumbent: PulseEvent): boolean {
  if (candidate.created_at !== incumbent.created_at) {
    return candidate.created_at > incumbent.created_at;
  }
  return candidate.id > incumbent.id;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nullableString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function nullableBoolean(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function nullableInteger(value: unknown): number | null {
  return Number.isSafeInteger(value) ? (value as number) : null;
}

/** UTF-8 byte length, matching Rust's `str::len()`. */
function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

/**
 * The session identity, derived from the metadata **content**, byte-identical
 * to `buzz_core::coding_session_command::coding_session_target_key`.
 *
 * Derived, never read off the `cs-target` tag. Nothing validates that tag for
 * a 44223 — the relay re-derives `cs-target` only for 44220 — so a 44223 whose
 * tag disagreed with its content would give the two folds different session
 * identities (and a different sort tiebreak) for the same event, and a 44223
 * with no tag at all would be folded by Rust and dropped here. The content is
 * the claim; the tag is only a filter affordance.
 */
function codingSessionTargetKey(session: unknown): string | null {
  if (!isPlainObject(session)) return null;
  const { driver, instanceId, sessionId, generation } = session;
  if (
    typeof driver !== "string" ||
    typeof instanceId !== "string" ||
    typeof sessionId !== "string" ||
    !Number.isSafeInteger(generation)
  ) {
    return null;
  }
  const fields = [driver, instanceId, sessionId, String(generation)];
  let key = "coding-session/v1|";
  for (const field of fields) {
    key += `${utf8Length(field)}:${field}`;
  }
  return key;
}

/**
 * Read the digest-bearing fields off a 44223 metadata event, or `null` when it
 * is not one. Deliberately minimal: the product path has already run the event
 * through the shared strict decoder (see the module doc comment).
 */
function readSessionMetadata(event: PulseEvent): SessionMetadataFacts | null {
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (!isPlainObject(content) || typeof content.status !== "string") {
    return null;
  }
  const targetKey = codingSessionTargetKey(content.session);
  if (targetKey === null) return null;
  return {
    targetKey,
    projectRef: nullableString(content.projectRef),
    status: content.status,
    branch: nullableString(content.branch),
    sessionRef: nullableString(content.sessionRef),
    observedCommit: nullableString(content.observedCommit),
    dirty: nullableBoolean(content.dirty),
    relayReachable: nullableBoolean(content.relayReachable),
    verifiedAt: nullableInteger(content.verifiedAt),
  };
}

/** The fixed tri-state string for one session's commit confirmation. */
export function pulseCommitConfirmation(
  relayReachable: boolean | null,
): string {
  if (relayReachable === true) return PULSE_COMMIT_CONFIRMED;
  if (relayReachable === false) return PULSE_COMMIT_NOT_FOUND;
  return PULSE_COMMIT_NOT_CHECKED;
}

/**
 * Active work requires a positive freshness signal — all three of: no closure,
 * a live-ish wire status, and an observation inside
 * {@link PULSE_ACTIVE_WINDOW_SECONDS}. Anything else is `stale` and renders
 * under **Last seen**, never as somebody currently working.
 */
export function pulseSessionActivity(input: {
  status: string;
  statusAt: number;
  closed: boolean;
  now: number;
}): PulseSessionActivity {
  if (input.closed) return "stale";
  if (!(PULSE_ACTIVE_STATUSES as readonly string[]).includes(input.status)) {
    return "stale";
  }
  return input.now - input.statusAt <= PULSE_ACTIVE_WINDOW_SECONDS
    ? "active"
    : "stale";
}

function foldNewestByDTag(
  events: readonly PulseEvent[],
  kind: number,
): Map<string, PulseEvent> {
  const newest = new Map<string, PulseEvent>();
  for (const event of events) {
    if (event.kind !== kind) continue;
    const key = tagValue(event, "d");
    if (!key) continue;
    const incumbent = newest.get(key);
    if (!incumbent || isNewer(event, incumbent)) newest.set(key, event);
  }
  return newest;
}

function closureIsClosed(event: PulseEvent): boolean {
  try {
    const content: unknown = JSON.parse(event.content);
    return isPlainObject(content) && content.action === "closed";
  } catch {
    return false;
  }
}

function foldSessions(
  events: readonly PulseEvent[],
  project: string,
  now: number,
): PulseDigestSession[] {
  const winners = new Map<
    string,
    { event: PulseEvent; facts: SessionMetadataFacts }
  >();
  for (const event of events) {
    if (event.kind !== 44223) continue;
    const facts = readSessionMetadata(event);
    if (!facts) continue;
    // 44223 carries no `a` tag; the project join is the content `projectRef`,
    // normalized, exactly as the CLI's session retrieval does it.
    if (
      facts.projectRef === null ||
      normalizePulseProjectCoordinate(facts.projectRef) !== project
    ) {
      continue;
    }
    const incumbent = winners.get(facts.targetKey);
    if (!incumbent || isNewer(event, incumbent.event)) {
      winners.set(facts.targetKey, { event, facts });
    }
  }

  const goals = foldNewestByDTag(events, 44227);
  const names = foldNewestByDTag(events, 44229);
  const closures = foldNewestByDTag(events, 44230);

  const sessions: PulseDigestSession[] = [];
  for (const { event, facts } of winners.values()) {
    const sessionRef = facts.sessionRef;
    const goal = sessionRef ? (goals.get(sessionRef) ?? null) : null;
    const name = sessionRef ? (names.get(sessionRef) ?? null) : null;
    const closure = sessionRef ? (closures.get(sessionRef) ?? null) : null;
    const closed = closure ? closureIsClosed(closure) : false;
    const sourceEventIds = [event.id, goal?.id, name?.id, closure?.id]
      .filter((id): id is string => typeof id === "string")
      .sort();
    sessions.push({
      targetKey: facts.targetKey,
      sessionRef,
      name: name ? name.content : null,
      goal: goal ? goal.content : null,
      status: facts.status,
      statusAt: event.created_at,
      closed,
      activity: pulseSessionActivity({
        status: facts.status,
        statusAt: event.created_at,
        closed,
        now,
      }),
      branch: facts.branch,
      observedCommit: facts.observedCommit,
      dirty: facts.dirty,
      relayReachable: facts.relayReachable,
      verifiedAt: facts.verifiedAt,
      commitConfirmation: pulseCommitConfirmation(facts.relayReachable),
      // Never derived from `verifiedAt`: that is null whenever the
      // reachability probe did not complete, and a session with perfectly
      // fresh 44223 observations would then render "age: unknown".
      observedAgeSeconds: now - event.created_at,
      sourceEventIds,
    });
  }
  return sessions.sort((left, right) =>
    left.statusAt !== right.statusAt
      ? right.statusAt - left.statusAt
      : byteOrder(left.targetKey, right.targetKey),
  );
}

/**
 * Fold every readable Pulse entry and coding-session fact for one project into
 * the digest envelope.
 *
 * Supersession is a **single-pass marking, never a traversal**, so cycles are
 * structurally impossible: an honored claim is recorded on its target, and a
 * refused claim (cross-author, unresolved, or out-of-order) is recorded on the
 * claimant so the attempt stays visible. Superseded entries are never dropped.
 */
export function foldProjectPulseDigest(
  input: ProjectPulseFoldInput,
): ProjectPulseDigest {
  const project =
    normalizePulseProjectCoordinate(input.project) ?? input.project;
  const sourceErrors = input.sourceErrors ?? [];
  const errors: PulseDigestError[] = sourceErrors.map((error) => ({
    scope: error.scope,
    message: error.message,
  }));

  const entries: PulseDigestEntry[] = [];
  const claimsById = new Map<string, PulseSupersessionClaim[]>();
  for (const event of input.events) {
    if (event.kind !== PULSE_ENTRY_KIND) continue;
    // Project scope is decided BEFORE envelope validation, matching the Rust
    // fold's order (`fold_pulse_digest`). A mixed result set is a caller's
    // shape, not a defect: another project's entry — well-formed or not — is
    // not this digest's business and must not appear in its `errors[]`. An
    // unresolvable coordinate *is* this digest's business, because the gate
    // closes rather than opening.
    const coordinate = pulseEntryProjectCoordinate(event);
    if (coordinate === null) {
      errors.push({
        scope: "invalid-entry",
        message: `entry ${event.id} failed validation and was excluded`,
      });
      continue;
    }
    if (coordinate !== project) continue;
    const decoded = validatePulseEntryEnvelope(event);
    if (!decoded.ok) {
      // Ingest rejects these shapes, so only a smuggled or legacy event gets
      // here. It is excluded from the claims but never silently dropped.
      errors.push({
        scope: "invalid-entry",
        message: `entry ${event.id} failed validation and was excluded`,
      });
      continue;
    }
    entries.push({
      eventId: event.id,
      pubkey: event.pubkey,
      createdAt: event.created_at,
      type: decoded.entry.type,
      text: decoded.entry.text,
      claimedAreas: [...decoded.entry.codeAreas],
      // The content field is the claim; the `branch` tag exists so a relay
      // filter can see it, and the validator already proved the two agree
      // whenever both are present. Falling back to the tag keeps this fold
      // byte-identical to the Rust one (`crates/buzz-cli/src/commands/pulse.rs`
      // `branch: entry.branch.or_else(tag "branch")`) for an entry that
      // carries only the tag.
      branch: decoded.entry.branch ?? tagValue(event, "branch"),
      sessionRef: pulseEntrySessionRef(event),
      supersedes: decoded.entry.supersedes,
      supersededBy: [],
      active: true,
    });
    claimsById.set(event.id, []);
  }

  const byId = new Map(entries.map((entry) => [entry.eventId, entry]));
  for (const claimant of entries) {
    const targetId = claimant.supersedes;
    if (targetId === null) continue;
    const target = byId.get(targetId);
    if (!target) {
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: null,
        honored: false,
        reason: "unresolved",
      });
      errors.push({
        scope: "unresolved-supersedes",
        message: `entry ${claimant.eventId} supersedes ${targetId}, which is not in the visible result set`,
      });
      continue;
    }
    if (target.pubkey !== claimant.pubkey) {
      // A peer's claim never removes your entry from the set that drives
      // wait|consult|proceed — both entries render, the claim stays visible.
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: target.pubkey,
        honored: false,
        reason: "cross-author",
      });
      continue;
    }
    const wins =
      claimant.createdAt > target.createdAt ||
      (claimant.createdAt === target.createdAt &&
        claimant.eventId > target.eventId);
    if (!wins) {
      claimsById.get(claimant.eventId)?.push({
        eventId: targetId,
        pubkey: target.pubkey,
        honored: false,
        reason: "out-of-order",
      });
      continue;
    }
    claimsById.get(target.eventId)?.push({
      eventId: claimant.eventId,
      pubkey: claimant.pubkey,
      honored: true,
      reason: null,
    });
    target.active = false;
  }

  for (const entry of entries) {
    entry.supersededBy = (claimsById.get(entry.eventId) ?? []).sort(
      (left, right) => byteOrder(left.eventId, right.eventId),
    );
  }
  entries.sort((left, right) =>
    left.createdAt !== right.createdAt
      ? right.createdAt - left.createdAt
      : byteOrder(right.eventId, left.eventId),
  );

  errors.sort((left, right) =>
    left.scope !== right.scope
      ? byteOrder(left.scope, right.scope)
      : byteOrder(left.message, right.message),
  );

  return {
    schema: PULSE_DIGEST_SCHEMA,
    source: input.source ?? "client-composed",
    project,
    asOf: input.now,
    // `complete:false` plus a non-empty `errors[]` is the only representation
    // of a partial read: an invalid entry or a dangling supersession is a fact
    // about one event, not a failed query, and never flips this.
    complete: sourceErrors.length === 0,
    sessionsScope: PULSE_SESSIONS_SCOPE,
    sessions: foldSessions(input.events, project, input.now),
    entries,
    errors,
  };
}

/** The distinct branch values across a digest, `null` kept as its own group. */
export function pulseDigestBranches(
  digest: ProjectPulseDigest,
): Array<string | null> {
  const named = new Set<string>();
  let hasNull = false;
  for (const branch of [
    ...digest.entries.map((entry) => entry.branch),
    ...digest.sessions.map((session) => session.branch),
  ]) {
    if (branch === null) hasNull = true;
    else named.add(branch);
  }
  const groups: Array<string | null> = [...named].sort(byteOrder);
  // A null branch is its own group: it is never merged into a named branch and
  // never rendered as one. Desktop's "no branch" chip maps to `--branch -`.
  if (hasNull) groups.push(null);
  return groups;
}
