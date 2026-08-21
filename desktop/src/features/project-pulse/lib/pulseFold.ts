/**
 * Project Pulse's fact-only TypeScript fold. Shared vectors are the source of
 * truth and must remain byte-identical to buzz-core. Session JSON crosses the
 * strict decoder seam; this module uses only local, erasable TypeScript so the
 * plain Node conformance binder can import it.
 */

import {
  PULSE_ENTRY_KIND,
  normalizePulseProjectCoordinate,
  pulseEntryProjectCoordinate,
  pulseEntrySessionRef,
  validatePulseEntryEnvelope,
  type PulseEvent,
} from "./pulseEntry.ts";
import {
  PULSE_COMMIT_CONFIRMED,
  PULSE_COMMIT_NOT_CHECKED,
  PULSE_COMMIT_NOT_FOUND,
  PULSE_DIGEST_SCHEMA,
  PULSE_LEASE_TTL_SECONDS,
  PULSE_SESSIONS_SCOPE,
  type ProjectPulseDigest,
  type ProjectPulseFoldInput,
  type PulseCoordinationState,
  type PulseDigestError,
  type PulseDigestEntry,
  type PulseDigestGeneration,
  type PulseDigestSession,
  type PulseGenerationReachability,
  type PulseSessionLifecycle,
  type PulseSupersessionClaim,
} from "./pulseFoldTypes.ts";
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
} from "./pulseFoldStrictJson.ts";

export * from "./pulseFoldTypes.ts";

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
  event: PulseEvent;
  channelId: string;
  commandId: string;
  action: "create" | "resume";
  providerAuthorityPubkey: string;
  projectRef: string | null;
  sessionRef: string | null;
  previousTarget: CodingSessionTarget | null;
};

type LifecycleReceiptFacts = {
  event: PulseEvent;
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
  event: PulseEvent;
  channelId: string;
  targetKey: string;
  commandId: string;
  state: "live" | "released";
  sequence: number;
};

function tagValue(event: PulseEvent, key: string): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === key);
  return tag?.[1] ?? null;
}

function hasExactOrderedTwoFieldTags(
  event: PulseEvent,
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
function readSessionMetadata(event: PulseEvent): SessionMetadataFacts | null {
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
export function pulseCommitConfirmation(
  relayReachable: boolean | null,
): string {
  if (relayReachable === true) return PULSE_COMMIT_CONFIRMED;
  if (relayReachable === false) return PULSE_COMMIT_NOT_FOUND;
  return PULSE_COMMIT_NOT_CHECKED;
}

function parseObjectContent(event: PulseEvent): Record<string, unknown> | null {
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

function readLifecycleCommand(event: PulseEvent): LifecycleCommandFacts | null {
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

function readLifecycleReceipt(event: PulseEvent): LifecycleReceiptFacts | null {
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

function readLease(event: PulseEvent): LeaseFacts | null {
  if (event.kind !== 24223) return null;
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
  events: readonly PulseEvent[],
  kind: number,
): Map<string, PulseEvent> {
  const newest = new Map<string, PulseEvent>();
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

function closureIsClosed(event: PulseEvent): boolean {
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

type FoldedSessions = {
  sessions: PulseDigestSession[];
  providerReachableSessions: string[];
  openUnverifiedSessions: string[];
  closedSessions: string[];
};

function sortedUnique(values: readonly string[]): string[] {
  return [...new Set(values)].sort(byteOrder);
}

function acceptedGenerations(
  events: readonly PulseEvent[],
  project: string,
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
    if (candidates.length !== 1 || answers.length !== 1) continue;
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
      if (unique.size !== 1) continue;
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
      command.projectRef !== null &&
      normalizePulseProjectCoordinate(command.projectRef) === project
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

function foldSessions(
  events: readonly PulseEvent[],
  project: string,
  now: number,
): FoldedSessions {
  const accepted = acceptedGenerations(events, project);
  const metadata = new Map<
    string,
    { event: PulseEvent; facts: SessionMetadataFacts }
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
      facts.projectRef === null ||
      normalizePulseProjectCoordinate(facts.projectRef) !== project
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
    const lease = readLease(event);
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

  const generationsBySession = new Map<string, PulseDigestGeneration[]>();
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
      ? winningLease.event.created_at + PULSE_LEASE_TTL_SECONDS
      : null;
    const leaseIsLive =
      winningLease?.state === "live" &&
      leaseExpiresAt !== null &&
      now < leaseExpiresAt;
    const reachability: PulseGenerationReachability = terminal
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
    const generation: PulseDigestGeneration = {
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
      commitConfirmation: pulseCommitConfirmation(
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
  const sessions: PulseDigestSession[] = [];
  for (const [sessionKey, generations] of generationsBySession) {
    generations.sort(
      (left, right) =>
        Number(right.current) - Number(left.current) ||
        byteOrder(left.executionKey, right.executionKey) ||
        byteOrder(left.targetKey, right.targetKey),
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
    const lifecycle: PulseSessionLifecycle =
      closure && closureIsClosed(closure) ? "closed" : "open";
    const coordinationState: PulseCoordinationState =
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
    return byteOrder(left.sessionKey, right.sessionKey);
  });
  return {
    sessions,
    providerReachableSessions: sessions
      .filter((session) => session.coordinationState === "provider_reachable")
      .map((session) => session.sessionKey),
    openUnverifiedSessions: sessions
      .filter((session) => session.coordinationState === "open_unverified")
      .map((session) => session.sessionKey),
    closedSessions: sessions
      .filter((session) => session.coordinationState === "closed")
      .map((session) => session.sessionKey),
  };
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

  const foldedSessions = foldSessions(input.events, project, input.now);
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
    sessions: foldedSessions.sessions,
    providerReachableSessions: foldedSessions.providerReachableSessions,
    openUnverifiedSessions: foldedSessions.openUnverifiedSessions,
    closedSessions: foldedSessions.closedSessions,
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
    ...digest.sessions.flatMap((session) =>
      session.generations.map((generation) => generation.branch),
    ),
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
