/**
 * The session-coordination fold: the app's one answer to "which coding
 * sessions exist, and which of them can be reached right now?".
 *
 * This module owns the whole chain and nothing else owns any part of it:
 *
 * - **Authority.** A generation exists only when exactly one 44221 lifecycle
 *   command and exactly one 44224 receipt agree under one command id, the
 *   receipt is signed by the command's declared `providerAuthorityPubkey`, and
 *   the receipt's status is a success for that action. Two commands, two
 *   receipts, or a receipt from anyone else proves nothing.
 * - **Generations.** `session.create` opens generation 1; each `session.resume`
 *   extends the chain by exactly one, against the same execution key, from an
 *   already-accepted predecessor. An execution may span several resumed
 *   generations; `executionKey` is the identity callers count.
 * - **Leases.** Reachability comes from kind 24223 and from nothing else. The
 *   winning lease is the unique event at the highest sequence for that exact
 *   target, signed by the same provider authority, under the same command id.
 * - **Expiry.** A live lease is live only until `created_at +
 *   {@link SESSION_LEASE_TTL_SECONDS}`, measured against the caller's single
 *   `now`. The relay's accepted-time expiry is longer, so this under-claims.
 * - **Closure.** A human 44230 closure outranks every provider signal: a closed
 *   session reads `closed` even while its provider still holds a live lease.
 * - **Ambiguity.** Every refusal is reported, never swallowed.
 * - **Completeness.** Callers supply source read failures alongside the events;
 *   the fold carries them into one shared `complete` / `errors` result.
 *
 * **Metadata recency is history, not liveness.** A 44223 that says `running`
 * keeps saying `running` after the machine that signed it dies, so no age
 * threshold over `statusAt` may ever stand in for `coordinationState`. Reported
 * status and coordination state are two axes and stay two axes.
 *
 * Constraints (load-bearing): no runtime imports outside this directory, and
 * erasable TypeScript syntax only — `conformance/project-pulse-fold` loads this
 * file transitively under plain `node --test` with native type stripping.
 */

import {
  KIND_SESSION_LEASE,
  SESSION_COMMIT_CONFIRMED,
  SESSION_COMMIT_NOT_CHECKED,
  SESSION_COMMIT_NOT_FOUND,
  SESSION_LEASE_TTL_SECONDS,
  type CoordinatedGeneration,
  type CoordinatedSession,
  type CoordinationEvent,
  type SessionCoordinationAmbiguity,
  type SessionCoordinationFold,
  type SessionCoordinationFoldInput,
  type SessionCoordinationState,
  type SessionLifecycle,
  type SessionReachability,
} from "./sessionCoordinationTypes.ts";
import {
  hasStrictClosureJson,
  hasStrictLeaseJson,
  hasStrictLeaseValues,
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
  hasStrictLifecycleReceiptJson,
  hasStrictLifecycleReceiptValues,
  hasStrictMetadataJson,
  hasStrictSessionTargetValues,
} from "./sessionCoordinationStrictJson.ts";

export * from "./sessionCoordinationTypes.ts";

type SessionMetadataFacts = {
  channelId: string;
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

type CodingSessionTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

type LifecycleCommandFacts = {
  event: CoordinationEvent;
  channelId: string;
  commandId: string;
  action: "create" | "resume";
  providerAuthorityPubkey: string;
  projectRef: string | null;
  sessionRef: string | null;
  previousTarget: CodingSessionTarget | null;
};

type LifecycleReceiptFacts = {
  event: CoordinationEvent;
  channelId: string;
  commandId: string;
  status: string;
  target: CodingSessionTarget;
};

type AcceptedGeneration = {
  command: LifecycleCommandFacts;
  receipt: LifecycleReceiptFacts;
  target: CodingSessionTarget;
  targetKey: string;
  executionKey: string;
  sessionRef: string | null;
  channelId: string;
};

type LeaseFacts = {
  event: CoordinationEvent;
  channelId: string;
  targetKey: string;
  commandId: string;
  state: "live" | "released";
  sequence: number;
};

function tagValue(event: CoordinationEvent, key: string): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === key);
  return tag?.[1] ?? null;
}

function hasExactOrderedTwoFieldTags(
  event: CoordinationEvent,
  expected: ReadonlyArray<readonly [string, string | null]>,
): boolean {
  return (
    event.tags.length === expected.length &&
    event.tags.every((tag, index) => {
      const [key, value] = expected[index];
      return (
        tag.length === 2 &&
        tag[0] === key &&
        (value === null ? tag[1].length > 0 : tag[1] === value)
      );
    })
  );
}

/** Byte-wise order matching Rust `Ord`; never substitute ICU collation. */
export function coordinationByteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** `(created_at, event id)` — the total order every fold in this repo uses. */
function isNewer(
  candidate: CoordinationEvent,
  incumbent: CoordinationEvent,
): boolean {
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

/** Session identity is derived from metadata content, never a `cs-target`. */
function readCodingSessionTarget(session: unknown): CodingSessionTarget | null {
  if (!hasStrictSessionTargetValues(session)) {
    return null;
  }
  const { driver, instanceId, sessionId, generation } = session;
  if (
    typeof driver !== "string" ||
    typeof instanceId !== "string" ||
    typeof sessionId !== "string" ||
    !Number.isSafeInteger(generation) ||
    (generation as number) <= 0
  ) {
    return null;
  }
  return {
    driver,
    instanceId,
    sessionId,
    generation: generation as number,
  };
}

function lengthPrefixedKey(prefix: string, fields: readonly string[]): string {
  let key = prefix;
  for (const field of fields) key += `${utf8Length(field)}:${field}`;
  return key;
}

function codingSessionTargetKey(session: unknown): string | null {
  const target = readCodingSessionTarget(session);
  if (!target) return null;
  const { driver, instanceId, sessionId, generation } = target;
  const fields = [driver, instanceId, sessionId, String(generation)];
  return lengthPrefixedKey("coding-session/v1|", fields);
}

function codingExecutionKey(target: CodingSessionTarget): string {
  return lengthPrefixedKey("coding-execution/v1|", [
    target.driver,
    target.instanceId,
    target.sessionId,
  ]);
}

function channelFactKey(channelId: string, factKey: string): string {
  return lengthPrefixedKey("channel-fact/v1|", [channelId, factKey]);
}

/**
 * Read the digest-bearing fields off a 44223 metadata event, or `null` when it
 * is not one. Deliberately minimal: the product path has already run the event
 * through the shared strict decoder (see the module doc comment).
 */
function readSessionMetadata(
  event: CoordinationEvent,
): SessionMetadataFacts | null {
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (!hasStrictMetadataJson(event.content, content)) {
    return null;
  }
  const targetKey = codingSessionTargetKey(content.session);
  const channelId = tagValue(event, "h");
  if (targetKey === null || !channelId) return null;
  return {
    channelId,
    targetKey,
    projectRef: nullableString(content.projectRef),
    status: content.status as string,
    branch: nullableString(content.branch),
    sessionRef: nullableString(content.sessionRef),
    observedCommit: nullableString(content.observedCommit),
    dirty: nullableBoolean(content.dirty),
    relayReachable: nullableBoolean(content.relayReachable),
    verifiedAt: nullableInteger(content.verifiedAt),
  };
}

/** The fixed tri-state string for one session's commit confirmation. */
export function sessionCommitConfirmation(
  relayReachable: boolean | null,
): string {
  if (relayReachable === true) return SESSION_COMMIT_CONFIRMED;
  if (relayReachable === false) return SESSION_COMMIT_NOT_FOUND;
  return SESSION_COMMIT_NOT_CHECKED;
}

function parseObjectContent(
  event: CoordinationEvent,
): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(event.content);
    return isPlainObject(value) ? value : null;
  } catch {
    return null;
  }
}

function isLowerHexPubkey(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function readLifecycleCommand(
  event: CoordinationEvent,
): LifecycleCommandFacts | null {
  if (event.kind !== 44221) return null;
  const content = parseObjectContent(event);
  if (
    !hasStrictLifecycleCommandJson(event.content, content) ||
    !hasStrictLifecycleCommandValues(content) ||
    content?.schema !== "buzz-coding-session-lifecycle-command/v1" ||
    typeof content.commandId !== "string" ||
    !hasExactOrderedTwoFieldTags(event, [
      ["h", null],
      ["csl-v", "csl1-1"],
      ["csl-command", content.commandId],
    ]) ||
    !isPlainObject(content.action)
  ) {
    return null;
  }
  const action = content.action;
  const channelId = tagValue(event, "h");
  if (!channelId) return null;
  if (!isLowerHexPubkey(action.providerAuthorityPubkey)) return null;
  if (action.type === "session.create") {
    return {
      event,
      channelId,
      commandId: content.commandId,
      action: "create",
      providerAuthorityPubkey: action.providerAuthorityPubkey,
      projectRef: nullableString(action.projectRef),
      sessionRef: nullableString(action.sessionRef),
      previousTarget: null,
    };
  }
  if (action.type === "session.resume") {
    const previousTarget = readCodingSessionTarget(action.session);
    if (!previousTarget) return null;
    return {
      event,
      channelId,
      commandId: content.commandId,
      action: "resume",
      providerAuthorityPubkey: action.providerAuthorityPubkey,
      projectRef: null,
      sessionRef: null,
      previousTarget,
    };
  }
  return null;
}

function readLifecycleReceipt(
  event: CoordinationEvent,
): LifecycleReceiptFacts | null {
  if (event.kind !== 44224) return null;
  const content = parseObjectContent(event);
  if (
    !hasStrictLifecycleReceiptJson(event.content, content) ||
    !hasStrictLifecycleReceiptValues(content) ||
    content?.schema !== "buzz-coding-session-lifecycle-receipt/v1" ||
    typeof content.commandId !== "string" ||
    typeof content.status !== "string" ||
    tagValue(event, "csl-command") !== content.commandId
  ) {
    return null;
  }
  const target = readCodingSessionTarget(content.session);
  if (!target) return null;
  const receiptKey = lengthPrefixedKey("coding-session-lifecycle-receipt/v1|", [
    content.commandId,
  ]);
  if (
    !hasExactOrderedTwoFieldTags(event, [
      ["h", null],
      ["cslr-v", "cslr1-1"],
      ["csl-command", content.commandId],
      ["csl-key", receiptKey],
    ])
  ) {
    return null;
  }
  const channelId = tagValue(event, "h");
  if (!channelId) return null;
  return {
    event,
    channelId,
    commandId: content.commandId,
    status: content.status,
    target,
  };
}

function lifecycleSucceeded(command: LifecycleCommandFacts, status: string) {
  return command.action === "create"
    ? status === "created" || status === "created_with_failed_initial_turn"
    : status === "resumed" || status === "resumed_without_context";
}

/**
 * Decode one kind-24223 lease.
 *
 * Exported because reachability is the fact most likely to be re-derived
 * badly: a caller that wants "is there a live lease" must decode it here, with
 * the exact tag shape and the strict content decoder, rather than sniffing
 * `content.state` off an event it did not validate.
 */
export function readSessionLease(event: CoordinationEvent): LeaseFacts | null {
  if (event.kind !== KIND_SESSION_LEASE) return null;
  const content = parseObjectContent(event);
  if (
    !hasStrictLeaseJson(event.content, content) ||
    !hasStrictLeaseValues(content)
  ) {
    return null;
  }
  const targetKey = codingSessionTargetKey(content.target);
  const commandId = tagValue(event, "csl-command");
  if (
    targetKey === null ||
    !commandId ||
    !hasExactOrderedTwoFieldTags(event, [
      ["h", null],
      ["cslease-v", "cslease1-1"],
      ["cs-target", targetKey],
      ["csl-command", commandId],
      ["cslease-seq", String(content.leaseSequence)],
    ])
  ) {
    return null;
  }
  const channelId = tagValue(event, "h");
  if (!channelId) return null;
  return {
    event,
    channelId,
    targetKey,
    commandId,
    state: content.state as "live" | "released",
    sequence: content.leaseSequence as number,
  };
}

function foldNewestByDTag(
  events: readonly CoordinationEvent[],
  kind: number,
): Map<string, CoordinationEvent> {
  const newest = new Map<string, CoordinationEvent>();
  for (const event of events) {
    if (event.kind !== kind) continue;
    const sessionRef = tagValue(event, "d");
    const channelId = tagValue(event, "h");
    if (!sessionRef || !channelId) continue;
    const key = channelFactKey(channelId, sessionRef);
    const incumbent = newest.get(key);
    if (!incumbent || isNewer(event, incumbent)) newest.set(key, event);
  }
  return newest;
}

function closureIsClosed(event: CoordinationEvent): boolean {
  try {
    const content: unknown = JSON.parse(event.content);
    return (
      hasStrictClosureJson(event.content, content) &&
      content.action === "closed"
    );
  } catch {
    return false;
  }
}

function sortedUnique(values: readonly string[]): string[] {
  return [...new Set(values)].sort(coordinationByteOrder);
}

function acceptedGenerations(
  events: readonly CoordinationEvent[],
  acceptProjectRef: (projectRef: string | null) => boolean,
  ambiguities: SessionCoordinationAmbiguity[],
): Map<string, AcceptedGeneration> {
  const commands = new Map<string, LifecycleCommandFacts[]>();
  const receipts = new Map<string, LifecycleReceiptFacts[]>();
  for (const event of events) {
    const command = readLifecycleCommand(event);
    if (command) {
      const key = channelFactKey(command.channelId, command.commandId);
      const held = commands.get(key) ?? [];
      if (!held.some((row) => row.event.id === command.event.id)) {
        commands.set(key, [...held, command]);
      }
    }
    const receipt = readLifecycleReceipt(event);
    if (receipt) {
      const key = channelFactKey(receipt.channelId, receipt.commandId);
      const held = receipts.get(key) ?? [];
      if (!held.some((row) => row.event.id === receipt.event.id)) {
        receipts.set(key, [...held, receipt]);
      }
    }
  }

  const pairs: Array<{
    command: LifecycleCommandFacts;
    receipt: LifecycleReceiptFacts;
  }> = [];
  for (const [channelCommandKey, candidates] of commands) {
    const answers = receipts.get(channelCommandKey) ?? [];
    if (candidates.length !== 1 || answers.length !== 1) {
      // Zero answers is an unanswered command, not a conflict: the provider has
      // simply not replied yet, and a surface that reported that as ambiguous
      // evidence would cry wolf on every in-flight create.
      if (candidates.length > 1 || answers.length > 1) {
        ambiguities.push({
          scope: "authority",
          message: `command ${candidates[0]?.commandId ?? answers[0]?.commandId ?? "?"} has ${candidates.length} commands and ${answers.length} receipts; no generation was accepted`,
        });
      }
      continue;
    }
    const [command] = candidates;
    const [receipt] = answers;
    if (
      receipt.event.pubkey !== command.providerAuthorityPubkey ||
      !lifecycleSucceeded(command, receipt.status)
    ) {
      continue;
    }
    pairs.push({ command, receipt });
  }

  const accepted = new Map<string, AcceptedGeneration>();
  const add = (
    command: LifecycleCommandFacts,
    receipt: LifecycleReceiptFacts,
    sessionRef: string | null,
  ) => {
    const targetKey = codingSessionTargetKey(receipt.target);
    const acceptedKey = targetKey
      ? channelFactKey(command.channelId, targetKey)
      : null;
    if (!targetKey || !acceptedKey || accepted.has(acceptedKey)) return false;
    accepted.set(acceptedKey, {
      command,
      receipt,
      target: receipt.target,
      targetKey,
      executionKey: codingExecutionKey(receipt.target),
      sessionRef,
      channelId: command.channelId,
    });
    return true;
  };

  type ProofCandidate = {
    command: LifecycleCommandFacts;
    receipt: LifecycleReceiptFacts;
    sessionRef: string | null;
  };
  const acceptUnique = (candidates: Map<string, ProofCandidate[]>) => {
    let additions = 0;
    for (const rows of candidates.values()) {
      const unique = new Map(
        rows.map((row) => [
          `${row.command.event.id}:${row.receipt.event.id}`,
          row,
        ]),
      );
      if (unique.size !== 1) {
        ambiguities.push({
          scope: "authority",
          message: `target ${rows[0]?.receipt.target.sessionId ?? "?"} generation ${rows[0]?.receipt.target.generation ?? "?"} is claimed by ${unique.size} distinct lifecycle proofs; none was accepted`,
        });
        continue;
      }
      const candidate = unique.values().next().value as ProofCandidate;
      additions += add(
        candidate.command,
        candidate.receipt,
        candidate.sessionRef,
      )
        ? 1
        : 0;
    }
    return additions;
  };

  const createCandidates = new Map<string, ProofCandidate[]>();
  for (const { command, receipt } of pairs) {
    if (
      command.action === "create" &&
      receipt.target.generation === 1 &&
      acceptProjectRef(command.projectRef)
    ) {
      const targetKey = codingSessionTargetKey(receipt.target);
      if (!targetKey) continue;
      const key = channelFactKey(command.channelId, targetKey);
      createCandidates.set(key, [
        ...(createCandidates.get(key) ?? []),
        { command, receipt, sessionRef: command.sessionRef },
      ]);
    }
  }
  acceptUnique(createCandidates);

  let changed = true;
  while (changed) {
    changed = false;
    const resumeCandidates = new Map<string, ProofCandidate[]>();
    for (const { command, receipt } of pairs) {
      if (command.action !== "resume" || !command.previousTarget) continue;
      const previousKey = codingSessionTargetKey(command.previousTarget);
      const previous = previousKey
        ? accepted.get(channelFactKey(command.channelId, previousKey))
        : null;
      if (
        !previous ||
        codingExecutionKey(receipt.target) !== previous.executionKey ||
        receipt.target.generation !== previous.target.generation + 1
      ) {
        continue;
      }
      const targetKey = codingSessionTargetKey(receipt.target);
      if (!targetKey) continue;
      const key = channelFactKey(command.channelId, targetKey);
      resumeCandidates.set(key, [
        ...(resumeCandidates.get(key) ?? []),
        { command, receipt, sessionRef: previous.sessionRef },
      ]);
    }
    changed = acceptUnique(resumeCandidates) > 0;
  }
  return accepted;
}

/**
 * Fold every readable coding-session fact into durable sessions, their
 * authority-proven generations, and one coordination state each.
 *
 * `acceptProjectRef` scopes which *creates* found a session; a resume inherits
 * its predecessor's scope and is never re-tested, because a resume command
 * carries no `projectRef` and re-deciding scope there would silently drop the
 * later generations of an in-scope session.
 */
export function foldSessionCoordination(
  input: SessionCoordinationFoldInput,
): SessionCoordinationFold {
  const now = input.now;
  const events = input.events;
  const acceptProjectRef = input.acceptProjectRef ?? (() => true);
  const errors = [...(input.sourceErrors ?? [])].sort(
    (left, right) =>
      coordinationByteOrder(left.scope, right.scope) ||
      coordinationByteOrder(left.message, right.message),
  );
  const rawAmbiguities: SessionCoordinationAmbiguity[] = [];
  const accepted = acceptedGenerations(
    events,
    acceptProjectRef,
    rawAmbiguities,
  );

  const metadata = new Map<
    string,
    { event: CoordinationEvent; facts: SessionMetadataFacts }
  >();
  for (const event of events) {
    if (event.kind !== 44223) continue;
    const facts = readSessionMetadata(event);
    const authority = facts
      ? accepted.get(channelFactKey(facts.channelId, facts.targetKey))
      : null;
    if (
      !facts ||
      !authority ||
      event.pubkey !== authority.command.providerAuthorityPubkey ||
      !acceptProjectRef(facts.projectRef)
    ) {
      continue;
    }
    const metadataKey = channelFactKey(facts.channelId, facts.targetKey);
    const incumbent = metadata.get(metadataKey);
    if (!incumbent || isNewer(event, incumbent.event)) {
      metadata.set(metadataKey, { event, facts });
    }
  }

  const leasesByTarget = new Map<string, LeaseFacts[]>();
  for (const event of events) {
    const lease = readSessionLease(event);
    const authority = lease
      ? accepted.get(channelFactKey(lease.channelId, lease.targetKey))
      : null;
    if (
      !lease ||
      !authority ||
      lease.commandId !== authority.command.commandId ||
      event.pubkey !== authority.command.providerAuthorityPubkey
    ) {
      continue;
    }
    const leaseKey = channelFactKey(lease.channelId, lease.targetKey);
    leasesByTarget.set(leaseKey, [
      ...(leasesByTarget.get(leaseKey) ?? []),
      lease,
    ]);
  }

  const currentGeneration = new Map<string, number>();
  for (const generation of accepted.values()) {
    const currentKey = channelFactKey(
      generation.channelId,
      generation.executionKey,
    );
    currentGeneration.set(
      currentKey,
      Math.max(
        currentGeneration.get(currentKey) ?? 0,
        generation.target.generation,
      ),
    );
  }

  const generationsBySession = new Map<string, CoordinatedGeneration[]>();
  const sessionRefs = new Map<string, string | null>();
  const sessionChannels = new Map<string, Set<string>>();
  for (const acceptedGeneration of accepted.values()) {
    const generationFactKey = channelFactKey(
      acceptedGeneration.channelId,
      acceptedGeneration.targetKey,
    );
    const observed = metadata.get(generationFactKey) ?? null;
    const leases = leasesByTarget.get(generationFactKey) ?? [];
    const highestSequence = leases.reduce(
      (highest, lease) => Math.max(highest, lease.sequence),
      0,
    );
    const highest = leases.filter(
      (lease) => lease.sequence === highestSequence,
    );
    const uniqueHighest = [
      ...new Map(highest.map((lease) => [lease.event.id, lease])).values(),
    ];
    const winningLease = uniqueHighest.length === 1 ? uniqueHighest[0] : null;
    if (uniqueHighest.length > 1) {
      // Two distinct leases at one sequence: the provider's own sequence is
      // supposed to break that tie, so a tie is evidence the snapshot cannot be
      // trusted. Neither wins, and the generation reads unverified.
      rawAmbiguities.push({
        scope: "lease",
        message: `target ${acceptedGeneration.target.sessionId} generation ${acceptedGeneration.target.generation} has ${uniqueHighest.length} distinct leases at sequence ${highestSequence}; none proves reachability`,
      });
    }
    const current =
      currentGeneration.get(
        channelFactKey(
          acceptedGeneration.channelId,
          acceptedGeneration.executionKey,
        ),
      ) === acceptedGeneration.target.generation;
    const terminal =
      observed?.facts.status === "stopped" ||
      observed?.facts.status === "disconnected";
    const leaseExpiresAt = winningLease
      ? winningLease.event.created_at + SESSION_LEASE_TTL_SECONDS
      : null;
    const leaseIsLive =
      winningLease?.state === "live" &&
      leaseExpiresAt !== null &&
      now < leaseExpiresAt;
    const reachability: SessionReachability = terminal
      ? "terminal"
      : current && leaseIsLive
        ? "provider_reachable"
        : "unverified";
    const leaseSourceIds = uniqueHighest.map((lease) => lease.event.id);
    const sourceEventIds = sortedUnique([
      acceptedGeneration.command.event.id,
      acceptedGeneration.receipt.event.id,
      ...(observed ? [observed.event.id] : []),
      ...leaseSourceIds,
    ]);
    const generation: CoordinatedGeneration = {
      targetKey: acceptedGeneration.targetKey,
      executionKey: acceptedGeneration.executionKey,
      providerAuthorityPubkey:
        acceptedGeneration.command.providerAuthorityPubkey,
      current,
      reachability,
      status: observed?.facts.status ?? null,
      statusAt: observed?.event.created_at ?? null,
      branch: observed?.facts.branch ?? null,
      observedCommit: observed?.facts.observedCommit ?? null,
      dirty: observed?.facts.dirty ?? null,
      relayReachable: observed?.facts.relayReachable ?? null,
      verifiedAt: observed?.facts.verifiedAt ?? null,
      commitConfirmation: sessionCommitConfirmation(
        observed?.facts.relayReachable ?? null,
      ),
      leaseState: winningLease?.state ?? null,
      leaseIssuedAt: winningLease?.event.created_at ?? null,
      leaseAcceptedAt: null,
      leaseExpiresAt,
      leaseSigner: winningLease?.event.pubkey ?? null,
      leaseSourceEventId: winningLease?.event.id ?? null,
      leaseSequence: highestSequence > 0 ? highestSequence : null,
      lifecycleCommandEventId: acceptedGeneration.command.event.id,
      lifecycleReceiptEventId: acceptedGeneration.receipt.event.id,
      sourceEventIds,
    };
    const sessionKey =
      acceptedGeneration.sessionRef ??
      `implicit:${acceptedGeneration.executionKey}`;
    generationsBySession.set(sessionKey, [
      ...(generationsBySession.get(sessionKey) ?? []),
      generation,
    ]);
    sessionRefs.set(sessionKey, acceptedGeneration.sessionRef);
    const channels = sessionChannels.get(sessionKey) ?? new Set<string>();
    channels.add(acceptedGeneration.channelId);
    sessionChannels.set(sessionKey, channels);
  }

  const goals = foldNewestByDTag(events, 44227);
  const names = foldNewestByDTag(events, 44229);
  const closures = foldNewestByDTag(events, 44230);
  const sessions: CoordinatedSession[] = [];
  for (const [sessionKey, generations] of generationsBySession) {
    generations.sort(
      (left, right) =>
        Number(right.current) - Number(left.current) ||
        coordinationByteOrder(left.executionKey, right.executionKey) ||
        coordinationByteOrder(left.targetKey, right.targetKey),
    );
    const sessionRef = sessionRefs.get(sessionKey) ?? null;
    const channels = sessionChannels.get(sessionKey) ?? new Set<string>();
    const channelId = channels.size === 1 ? [...channels][0] : null;
    const sessionFactKey =
      channelId && sessionRef ? channelFactKey(channelId, sessionRef) : null;
    const goal = sessionFactKey ? (goals.get(sessionFactKey) ?? null) : null;
    const name = sessionFactKey ? (names.get(sessionFactKey) ?? null) : null;
    const closure = sessionFactKey
      ? (closures.get(sessionFactKey) ?? null)
      : null;
    const lifecycle: SessionLifecycle =
      closure && closureIsClosed(closure) ? "closed" : "open";
    const coordinationState: SessionCoordinationState =
      lifecycle === "closed"
        ? "closed"
        : generations.some(
              (generation) =>
                generation.current &&
                generation.reachability === "provider_reachable",
            )
          ? "provider_reachable"
          : "open_unverified";
    const observedTimes = generations
      .map((generation) => generation.statusAt)
      .filter((value): value is number => value !== null);
    const latestObservationAt =
      observedTimes.length > 0 ? Math.max(...observedTimes) : null;
    const sourceEventIds = sortedUnique([
      ...generations.flatMap((generation) => generation.sourceEventIds),
      ...(goal ? [goal.id] : []),
      ...(name ? [name.id] : []),
      ...(closure ? [closure.id] : []),
    ]);
    sessions.push({
      sessionKey,
      sessionRef,
      name: name?.content ?? null,
      goal: goal?.content ?? null,
      lifecycle,
      coordinationState,
      latestObservationAt,
      observedAgeSeconds:
        latestObservationAt === null ? null : now - latestObservationAt,
      generations,
      sourceEventIds,
    });
  }
  sessions.sort((left, right) => {
    if (left.latestObservationAt !== right.latestObservationAt) {
      if (left.latestObservationAt === null) return 1;
      if (right.latestObservationAt === null) return -1;
      return right.latestObservationAt - left.latestObservationAt;
    }
    return coordinationByteOrder(left.sessionKey, right.sessionKey);
  });

  // The resume loop re-derives its candidate set on every pass, so one
  // conflicting proof is seen more than once. Dedupe and order so an adapter's
  // disclosure is stable across renders of the same events.
  const ambiguities = [
    ...new Map(
      rawAmbiguities.map((row) => [`${row.scope}\u0000${row.message}`, row]),
    ).values(),
  ].sort(
    (left, right) =>
      coordinationByteOrder(left.scope, right.scope) ||
      coordinationByteOrder(left.message, right.message),
  );

  const channelsBySession = new Map<string, string[]>();
  for (const [sessionKey, channels] of sessionChannels) {
    channelsBySession.set(
      sessionKey,
      [...channels].sort(coordinationByteOrder),
    );
  }

  return {
    complete: errors.length === 0,
    errors,
    sessions,
    channelsBySession,
    providerReachableSessions: sessions
      .filter((session) => session.coordinationState === "provider_reachable")
      .map((session) => session.sessionKey),
    openUnverifiedSessions: sessions
      .filter((session) => session.coordinationState === "open_unverified")
      .map((session) => session.sessionKey),
    closedSessions: sessions
      .filter((session) => session.coordinationState === "closed")
      .map((session) => session.sessionKey),
    ambiguities,
  };
}

/**
 * Milliseconds until the earliest currently reachable lease expires, or `null`
 * when nothing is reachable.
 *
 * Both adapters schedule their re-read on this: a lease that lapses with no new
 * event arriving must take the claim of reachability down with it, and only a
 * timer can do that.
 */
export function sessionLeaseExpiryDelayMs(
  sessions: readonly CoordinatedSession[],
  nowMs: number,
): number | null {
  const expiries = sessions.flatMap((session) =>
    session.coordinationState !== "provider_reachable"
      ? []
      : session.generations
          .filter(
            (generation) =>
              generation.current &&
              generation.reachability === "provider_reachable" &&
              generation.leaseExpiresAt !== null,
          )
          .map((generation) => generation.leaseExpiresAt as number),
  );
  if (expiries.length === 0) return null;
  return Math.max(0, Math.min(...expiries) * 1_000 - nowMs);
}
