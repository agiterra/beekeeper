import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionProjectedTranscriptItem } from "@/features/coding-sessions/lib/codingSessionTranscriptItems";
import type { CodingSessionProseFields } from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";

/**
 * Joining a turn's consecutive prose items into one message — the desktop
 * reader of `conformance/transcript-prose-join/CONTRACT.md` (NIP-CST
 * amendment 3, **Join key**), bound to its vectors by
 * `codingSessionTranscriptProseJoin.conformance.test.mjs`.
 *
 * The provider publishes an answer in pieces: at a size cap, at a tool
 * boundary, and (with paragraph flush) at each paragraph. Each piece is a
 * separate signed item, but the agent wrote — and the reader should read —
 * one message. Rendering the pieces as separate Markdown blocks breaks any
 * list or code fence that straddles a cut.
 *
 * The pieces are exact slices of one stream (`flush_text` neither trims nor
 * pads), so the join is plain concatenation and nothing is ever inserted
 * (rule 3). Two pieces the provider names as *different* messages (rule 5:
 * a `messageId` present on the later piece and unequal to the one the
 * message already carries) are two messages, not one with a break between.
 *
 * Callers hand this items already deduplicated by event and ordered by
 * `eventSeq` within each exact target: the trusted ingress store does both
 * (`codingSessionTrustedIngress.ts`, one bucket per target + signer +
 * `eventSeq`, records keyed by event id, `snapshot` sorted by `eventSeq`).
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

/** A piece the join reads as prose: an assistant message or a thought. */
export function isCodingSessionProsePiece(item: TranscriptItem): boolean {
  return (
    (item.type === "message" && item.role === "assistant") ||
    item.type === "thought"
  );
}

/** The id of the last piece a (possibly joined) prose item holds. */
export function codingSessionProseLastPieceId(item: TranscriptItem): string {
  return (item as CodingSessionProseFields).proseLastPieceId ?? item.id;
}

/** The signed event a (possibly joined) prose item currently ends at. */
export function codingSessionProseLastEventId(
  item: TranscriptItem,
): string | undefined {
  return (
    (item as CodingSessionProseFields).proseLastEventId ??
    (item as CodingSessionProjectedTranscriptItem).sourceEventId
  );
}

/** Rule 7, as the model resolved it: is this message still being written? */
export function isCodingSessionProseArriving(item: TranscriptItem): boolean {
  return (item as CodingSessionProseFields).arriving === true;
}

/** `item` with rule 7's verdict applied; the same object when unchanged. */
export function withCodingSessionProseArriving(
  item: TranscriptItem,
  arriving: boolean,
): TranscriptItem {
  if (isCodingSessionProseArriving(item) === arriving) return item;
  if (arriving) {
    const marked: TranscriptItem & CodingSessionProseFields = {
      ...item,
      arriving: true,
    };
    return marked;
  }
  const { arriving: _settled, ...rest } = item as TranscriptItem &
    CodingSessionProseFields;
  return rest as TranscriptItem;
}

/**
 * Assistant prose joins assistant prose; a thought joins a thought — and only
 * when both pieces are attributed identically and name no different message.
 *
 * Turns are grouped by `turnId` alone, so two signers or two generations that
 * happen to share a turn id can land in one turn. The joined item keeps the
 * first piece's attribution, so joining across a difference in who produced
 * the words would re-attribute the second piece's prose. The exact target
 * (`sourceTargetKey`: driver + instanceId + sessionId + generation), the
 * display generation (`sessionId`), the provider session, author, bridge
 * signer and owning subagent call must all agree.
 */
function areJoinableProse(left: TranscriptItem, right: TranscriptItem) {
  const sameKind =
    left.type === "message" && right.type === "message"
      ? left.role === "assistant" && right.role === "assistant"
      : left.type === "thought" && right.type === "thought";
  return (
    sameKind &&
    haveSameAttribution(left, right) &&
    !namesAnotherMessage(left, right)
  );
}

function haveSameAttribution(left: TranscriptItem, right: TranscriptItem) {
  return (
    (left.turnId ?? null) === (right.turnId ?? null) &&
    (left.sessionId ?? null) === (right.sessionId ?? null) &&
    targetKeyOf(left) === targetKeyOf(right) &&
    (left.providerSessionId ?? null) === (right.providerSessionId ?? null) &&
    authorOf(left) === authorOf(right) &&
    (left.bridgeSource?.pubkey ?? null) ===
      (right.bridgeSource?.pubkey ?? null) &&
    (left.parentToolId ?? null) === (right.parentToolId ?? null)
  );
}

/**
 * Rule 5: the message's `messageId` is the first one any of its pieces
 * carries; a piece with a different one starts a new message, a piece with
 * none joins.
 */
function namesAnotherMessage(message: TranscriptItem, piece: TranscriptItem) {
  const current = messageIdOf(message);
  const next = messageIdOf(piece);
  return current !== null && next !== null && current !== next;
}

function messageIdOf(item: TranscriptItem): string | null {
  const value = (item as { messageId?: unknown }).messageId;
  return typeof value === "string" && value.length > 0 ? value : null;
}

function targetKeyOf(item: TranscriptItem): string | null {
  return (item as CodingSessionProjectedTranscriptItem).sourceTargetKey ?? null;
}

function authorOf(item: TranscriptItem): string | null {
  return item.type === "message" ? (item.authorPubkey ?? null) : null;
}

function joinProseItems(
  left: TranscriptItem,
  right: TranscriptItem,
): TranscriptItem {
  if (
    !(left.type === "message" || left.type === "thought") ||
    !(right.type === "message" || right.type === "thought")
  ) {
    return left;
  }
  // The first piece's identity is the message's identity: its id (and its
  // `sourceEventId`, the contract's `firstEventId`) keys the row, so the row
  // stays mounted as later paragraphs arrive.
  const { arriving: _left, ...first } = left as TranscriptItem &
    CodingSessionProseFields;
  const lastEventId = codingSessionProseLastEventId(right);
  const messageId = messageIdOf(left) ?? messageIdOf(right);
  const joined = {
    ...first,
    text: left.text + right.text,
    ...(messageId !== null && left.type === "message" ? { messageId } : {}),
    proseLastPieceId: codingSessionProseLastPieceId(right),
    ...(lastEventId === undefined ? {} : { proseLastEventId: lastEventId }),
    ...(isCodingSessionProseArriving(right) ? { arriving: true as const } : {}),
  };
  return joined as TranscriptItem;
}

/**
 * Whether two prose items are the same rendered message.
 *
 * A joined item is rebuilt on every derivation, so identity cannot be the
 * test; its id, words, end and arriving verdict can. Only prose qualifies —
 * other items carry fields (attribution, status) a value check here would
 * miss.
 */
export function codingSessionProseItemsEqual(
  left: TranscriptItem,
  right: TranscriptItem,
): boolean {
  if (left === right) return true;
  if (left.id !== right.id || left.type !== right.type) return false;
  if (
    isCodingSessionProseArriving(left) !==
      isCodingSessionProseArriving(right) ||
    codingSessionProseLastPieceId(left) !== codingSessionProseLastPieceId(right)
  ) {
    return false;
  }
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
