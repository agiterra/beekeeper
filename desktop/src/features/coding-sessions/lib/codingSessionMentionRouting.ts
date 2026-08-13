/**
 * `@mention` sugar over the umbrella composer's participant selection.
 *
 * Typing `@claude` at the start of a prompt addresses the Claude execution in
 * this umbrella: it moves the existing participant selection and the handle is
 * removed from the text that is actually sent. There is no wire concept here —
 * the send path stays the 44220 turn command against the selected execution's
 * governed target. This module is the whole rule set:
 *
 * - Handles come from the executions *present in this umbrella* (runtime slug,
 *   provider instance ref, and — only when neither is published — the target
 *   driver), plus the leading token of each, all slugified and compared
 *   case-insensitively. Nothing is invented from a global provider list, so a
 *   handle that resolves always names something on screen.
 * - Only a *leading* mention routes. `@codex` in the middle of a sentence is
 *   ordinary prose about Codex, and prose must never move the target.
 * - An unknown handle is ordinary text. A handle two executions answer to is
 *   ordinary text too: the ambiguity is reported, never guessed, and the
 *   explicit selector decides.
 * - A resolved execution that cannot take a turn (no published command target,
 *   failed, disconnected) does not get routed to silently; the mention stays
 *   text and the caller can say why.
 * - At one execution there are no handles at all, so a single-execution
 *   session behaves exactly as it did before mentions existed — `@claude` is
 *   just the first word of the prompt.
 */
import type { CodingSessionUmbrellaParticipant } from "./codingSessionUmbrellaModel";
import { codingSessionUmbrellaParticipantKey } from "./codingSessionUmbrellaComposerModel";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
} from "./codingSessionTypes";

type ExecutionParticipant = Extract<
  CodingSessionUmbrellaParticipant,
  { kind: "execution" }
>;

/** One usable handle: unambiguous, and addressable right now. */
export type CodingSessionMentionTarget = {
  /** Slugified handle, without the leading `@`. */
  handle: string;
  /** `codingSessionUmbrellaParticipantKey` of the execution it addresses. */
  participantKey: string;
  /** The participant's selector label, for hints and confirmations. */
  label: string;
};

export type CodingSessionLeadingMention = {
  /** Slugified, lowercased handle. */
  handle: string;
  /** Exactly what the person typed after `@`, for echoing back. */
  raw: string;
  /** The prompt with the mention (and its delimiter) removed. */
  rest: string;
};

export type CodingSessionMentionResolution =
  | { kind: "none" }
  | { kind: "unknown"; handle: string; raw: string }
  | { kind: "ambiguous"; handle: string; raw: string; labels: string[] }
  | {
      kind: "unavailable";
      handle: string;
      raw: string;
      label: string;
      reason: string;
    }
  | {
      kind: "match";
      handle: string;
      raw: string;
      participantKey: string;
      label: string;
      /** The prompt to send: the handle stripped, nothing else changed. */
      text: string;
    };

/** Handle characters: a slug, never ending in a separator. */
const LEADING_MENTION_PATTERN =
  /^\s*@([A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)/;
/** Punctuation allowed to close a mention: `@codex, do the thing`. */
const MENTION_TERMINATORS = new Set([",", ".", ":", ";", "!", "?"]);

/** Lowercase, separator-normalized form used for every comparison. */
export function normalizeCodingSessionMentionHandle(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/**
 * Parse a *leading* `@handle`, or null.
 *
 * Null covers everything that must stay prose: no `@`, a mention that is not
 * first, and a handle butted against something that is not whitespace or
 * closing punctuation (`@codex/foo`, `@codex's`) — those are tokens the person
 * meant literally.
 */
export function parseLeadingCodingSessionMention(
  text: string,
): CodingSessionLeadingMention | null {
  const match = LEADING_MENTION_PATTERN.exec(text);
  if (!match) return null;
  const raw = match[1];
  const after = text.slice(match[0].length);
  const next = after.slice(0, 1);
  let rest = after;
  if (next.length === 0) {
    rest = "";
  } else if (MENTION_TERMINATORS.has(next)) {
    const following = after.slice(1, 2);
    if (following.length !== 0 && !/\s/.test(following)) return null;
    rest = after.slice(1);
  } else if (!/\s/.test(next)) {
    return null;
  }
  const handle = normalizeCodingSessionMentionHandle(raw);
  if (handle.length === 0) return null;
  return { handle, raw, rest: rest.replace(/^\s+/, "") };
}

/**
 * Every unambiguous, addressable handle in this umbrella.
 *
 * Ambiguity is computed over *all* executions, healthy or not: two Claude
 * executions make `@claude` ambiguous even when only one of them could take
 * the turn, because "the live one" is a guess about intent, not a fact.
 */
export function listCodingSessionMentionTargets(
  participants: readonly CodingSessionUmbrellaParticipant[],
): CodingSessionMentionTarget[] {
  const index = buildHandleIndex(participants);
  const targets: CodingSessionMentionTarget[] = [];
  for (const [handle, claimants] of index) {
    if (claimants.length !== 1) continue;
    const [participant] = claimants;
    if (executionMentionUnavailability(participant.execution) !== null)
      continue;
    targets.push({
      handle,
      participantKey: codingSessionUmbrellaParticipantKey(participant),
      label: participant.label,
    });
  }
  return targets;
}

/**
 * One suggested handle per addressable execution — the shortest unambiguous
 * one — for the composer's discoverability hint.
 */
export function suggestCodingSessionMentionHandles(
  participants: readonly CodingSessionUmbrellaParticipant[],
): CodingSessionMentionTarget[] {
  const byParticipant = new Map<string, CodingSessionMentionTarget>();
  for (const target of listCodingSessionMentionTargets(participants)) {
    const existing = byParticipant.get(target.participantKey);
    if (
      !existing ||
      target.handle.length < existing.handle.length ||
      (target.handle.length === existing.handle.length &&
        target.handle < existing.handle)
    ) {
      byParticipant.set(target.participantKey, target);
    }
  }
  // Selector order, so the hint reads in the order the chips are shown.
  return participants
    .filter(
      (participant): participant is ExecutionParticipant =>
        participant.kind === "execution",
    )
    .map((participant) =>
      byParticipant.get(codingSessionUmbrellaParticipantKey(participant)),
    )
    .filter((target): target is CodingSessionMentionTarget => Boolean(target));
}

/** Classify the leading mention of a draft against this umbrella. */
export function resolveCodingSessionMention(input: {
  text: string;
  participants: readonly CodingSessionUmbrellaParticipant[];
}): CodingSessionMentionResolution {
  const mention = parseLeadingCodingSessionMention(input.text);
  if (mention === null) return { kind: "none" };
  const index = buildHandleIndex(input.participants);
  // No handles exist here at all (the N=1 case): mentions are not a concept in
  // this session, so `@claude` is the first word of the prompt and nothing else.
  if (index.size === 0) return { kind: "none" };
  const claimants = index.get(mention.handle);
  if (!claimants || claimants.length === 0) {
    return { kind: "unknown", handle: mention.handle, raw: mention.raw };
  }
  if (claimants.length > 1) {
    return {
      kind: "ambiguous",
      handle: mention.handle,
      raw: mention.raw,
      labels: claimants.map((participant) => participant.label),
    };
  }
  const [participant] = claimants;
  const unavailable = executionMentionUnavailability(participant.execution);
  if (unavailable !== null) {
    return {
      kind: "unavailable",
      handle: mention.handle,
      raw: mention.raw,
      label: participant.label,
      reason: unavailable,
    };
  }
  return {
    kind: "match",
    handle: mention.handle,
    raw: mention.raw,
    participantKey: codingSessionUmbrellaParticipantKey(participant),
    label: participant.label,
    text: mention.rest,
  };
}

/**
 * The text to actually send: the leading handle removed *only* when it routes
 * to the participant the message is going to. A mention that resolves
 * elsewhere, or to nothing, is left in the prompt verbatim — stripping text
 * that did not do any routing would be editing someone's words for no reason.
 */
export function stripCodingSessionMentionForTarget(input: {
  text: string;
  participants: readonly CodingSessionUmbrellaParticipant[];
  participantKey: string | null;
}): string {
  const resolution = resolveCodingSessionMention({
    text: input.text,
    participants: input.participants,
  });
  if (
    resolution.kind !== "match" ||
    input.participantKey === null ||
    resolution.participantKey !== input.participantKey
  ) {
    return input.text;
  }
  return resolution.text;
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/**
 * handle -> the executions claiming it, in selector order.
 *
 * Empty below two executions: mention sugar exists to disambiguate between
 * participants, and at N=1 there is no selector to move.
 */
function buildHandleIndex(
  participants: readonly CodingSessionUmbrellaParticipant[],
): Map<string, ExecutionParticipant[]> {
  const executions = participants.filter(
    (participant): participant is ExecutionParticipant =>
      participant.kind === "execution",
  );
  const index = new Map<string, ExecutionParticipant[]>();
  if (executions.length < 2) return index;
  for (const participant of executions) {
    for (const handle of executionHandleCandidates(
      participant.execution.activeGeneration,
    )) {
      const claimants = index.get(handle);
      if (claimants) {
        if (!claimants.includes(participant)) claimants.push(participant);
      } else {
        index.set(handle, [participant]);
      }
    }
  }
  return index;
}

/**
 * The handles one execution answers to: its runtime and provider instance ref
 * (and the driver only when the provider published neither), each contributing
 * its full slug plus its leading token — so `claude-primary` is reachable as
 * `@claude-primary` and as `@claude` when nothing else claims `claude`.
 */
function executionHandleCandidates(
  record: CodingSessionCatalogRecord,
): string[] {
  const sources: string[] = [];
  if (record.runtime) sources.push(record.runtime);
  if (record.provider) sources.push(record.provider);
  if (sources.length === 0 && record.commandTarget) {
    sources.push(record.commandTarget.driver);
  }
  const handles: string[] = [];
  for (const source of sources) {
    const slug = normalizeCodingSessionMentionHandle(source);
    if (slug.length === 0) continue;
    if (!handles.includes(slug)) handles.push(slug);
    const [leading] = slug.split("-");
    if (leading.length > 0 && !handles.includes(leading)) {
      handles.push(leading);
    }
  }
  return handles;
}

/** Why a mention must not route here, or null when it may. */
function executionMentionUnavailability(
  execution: CodingSessionExecution,
): string | null {
  const record = execution.activeGeneration;
  if (!record.commandTarget) {
    return "it has not published a governed command target yet";
  }
  if (record.status === "failed") return "it failed";
  if (record.status === "disconnected") return "it is disconnected";
  if (record.capabilities?.threadTurnStart === false) {
    return "it does not accept turns";
  }
  return null;
}
