import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

/**
 * Joining a turn's consecutive prose items into one message.
 *
 * The provider publishes an answer in pieces: at a size cap, at a tool
 * boundary, and (since the paragraph-flush change in
 * `crates/beekeeper-session-provider/src/transcript.rs`) at each paragraph. Each
 * piece is a separate signed item, but the reader wrote — and should read —
 * one message. Rendering the pieces as separate Markdown blocks breaks any
 * list or code fence that straddles a cut, and spaces paragraphs as if they
 * were separate replies.
 *
 * The pieces are exact slices of one stream (`flush_text` neither trims nor
 * pads), so the faithful join is plain concatenation: whatever blank line
 * separated two paragraphs is already in the text. The one exception is two
 * items the provider itself names as *different* messages (`messageId` set on
 * both and unequal): those are separate replies that happen to be adjacent,
 * and gluing "Done." to "Next" would invent a word, so they get a paragraph
 * break if they lack one.
 *
 * Split out of `codingSessionTranscriptModel.ts` for the 1000-line ceiling.
 */
export function joinConsecutiveCodingSessionProse(
  items: readonly TranscriptItem[],
  keepSeparate: ReadonlySet<TranscriptItem> = new Set(),
): TranscriptItem[] {
  const joined: TranscriptItem[] = [];
  for (const item of items) {
    const previous = joined.at(-1);
    if (
      previous &&
      !keepSeparate.has(previous) &&
      !keepSeparate.has(item) &&
      areJoinableProse(previous, item)
    ) {
      joined[joined.length - 1] = joinProseItems(previous, item);
      continue;
    }
    joined.push(item);
  }
  return joined;
}

/**
 * Assistant prose joins assistant prose; a thought joins a thought — and only
 * when both pieces are attributed identically.
 *
 * Turns are grouped by `turnId` alone, so two signers or two generations that
 * happen to share a turn id land in one turn. The joined item keeps the first
 * piece's attribution, so joining across a difference in who produced the
 * words would re-attribute the second piece's prose (invariant 5). Any
 * difference in turn, generation (`sessionId`), author, bridge signer or
 * owning subagent call keeps the pieces apart.
 */
function areJoinableProse(left: TranscriptItem, right: TranscriptItem) {
  const sameKind =
    left.type === "message" && right.type === "message"
      ? left.role === "assistant" && right.role === "assistant"
      : left.type === "thought" && right.type === "thought";
  return sameKind && haveSameAttribution(left, right);
}

function haveSameAttribution(left: TranscriptItem, right: TranscriptItem) {
  return (
    (left.turnId ?? null) === (right.turnId ?? null) &&
    (left.sessionId ?? null) === (right.sessionId ?? null) &&
    authorOf(left) === authorOf(right) &&
    (left.bridgeSource?.pubkey ?? null) ===
      (right.bridgeSource?.pubkey ?? null) &&
    (left.parentToolId ?? null) === (right.parentToolId ?? null)
  );
}

function authorOf(item: TranscriptItem): string | null {
  return item.type === "message" ? (item.authorPubkey ?? null) : null;
}

function joinProseItems(
  left: TranscriptItem,
  right: TranscriptItem,
): TranscriptItem {
  if (left.type === "message" && right.type === "message") {
    const distinct =
      typeof left.messageId === "string" &&
      typeof right.messageId === "string" &&
      left.messageId !== right.messageId;
    // The first piece's identity is the message's identity: its id keys the
    // row, so the row stays mounted as later paragraphs arrive.
    return {
      ...left,
      text: joinCodingSessionProseText(left.text, right.text, distinct),
    };
  }
  if (left.type === "thought" && right.type === "thought") {
    return {
      ...left,
      text: joinCodingSessionProseText(left.text, right.text, false),
    };
  }
  return left;
}

/** Concatenate two prose slices; see the module comment for when a break is added. */
export function joinCodingSessionProseText(
  left: string,
  right: string,
  distinctMessages: boolean,
): string {
  if (!distinctMessages) return left + right;
  if (/\n\s*\n\s*$/.test(left) || /^\s*\n\s*\n/.test(right))
    return left + right;
  return `${left.replace(/\s+$/, "")}\n\n${right.replace(/^\s+/, "")}`;
}

/**
 * Whether two prose items are the same rendered message.
 *
 * A joined item is rebuilt on every derivation, so identity cannot be the
 * test; its id, words and timestamp can. Only prose qualifies — other items
 * carry fields (attribution, status) a value check here would miss.
 */
export function codingSessionProseItemsEqual(
  left: TranscriptItem,
  right: TranscriptItem,
): boolean {
  if (left === right) return true;
  if (left.id !== right.id || left.type !== right.type) return false;
  if (left.type === "message" && right.type === "message") {
    return (
      left.role === "assistant" &&
      right.role === "assistant" &&
      left.text === right.text &&
      left.timestamp === right.timestamp &&
      left.turnId === right.turnId
    );
  }
  if (left.type === "thought" && right.type === "thought") {
    return (
      left.text === right.text &&
      left.title === right.title &&
      left.timestamp === right.timestamp
    );
  }
  return false;
}
