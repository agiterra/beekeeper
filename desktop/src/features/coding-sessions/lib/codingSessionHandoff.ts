/**
 * Operator-mediated handoff v1 (design §D): "Send to ⟨execution B⟩" prefills
 * an ordinary 44220 to B with an editable provenance block quoting execution
 * A's signed fact and a `buzz://coding-session` deep link to it.
 *
 * Nothing here is wire schema. The provenance block is plain prompt text
 * signed by the operator; recognition is presentation-only. If parsing fails
 * (edited text, old client), the message renders as a plain prompt containing
 * a visible quote — nothing on the wire pretends to be structured provenance,
 * so nothing can lie about it.
 */
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionUmbrellaTurnBlock } from "./codingSessionUmbrellaTimeline";

/** Client-side deep-link convention pinning a quoted signed transcript fact. */
export const CODING_SESSION_HANDOFF_LINK_PREFIX = "buzz://coding-session";

/** Coordinates of one exact signed 44225 fact, as the deep link carries them. */
export type CodingSessionHandoffLink = {
  channelId: string;
  /** The exact `cs-target` key (generation included) of the quoted stream. */
  targetKey: string;
  /** The quoted fact's `eventSeq` within that stream. */
  eventSeq: number;
};

/** A recognized handoff prefill, split back into its parts. */
export type CodingSessionHandoffProvenance = {
  /** Human label of the quoted execution, e.g. "Claude · claude-opus-5". */
  sourceLabel: string;
  linkUrl: string;
  link: CodingSessionHandoffLink | null;
  /** The quoted excerpt, without the blockquote markers. */
  quote: string;
  /** The operator's own instruction following the provenance block. */
  instruction: string;
};

/** Build the `buzz://coding-session?channel=…&target=…&seq=…` deep link. */
export function buildCodingSessionHandoffLink(
  link: CodingSessionHandoffLink,
): string {
  const params = new URLSearchParams({
    channel: link.channelId,
    target: link.targetKey,
    seq: String(link.eventSeq),
  });
  return `${CODING_SESSION_HANDOFF_LINK_PREFIX}?${params.toString()}`;
}

/** Parse a handoff deep link, or null for anything that is not one. */
export function parseCodingSessionHandoffLink(
  url: string,
): CodingSessionHandoffLink | null {
  if (!url.startsWith(`${CODING_SESSION_HANDOFF_LINK_PREFIX}?`)) return null;
  const params = new URLSearchParams(
    url.slice(CODING_SESSION_HANDOFF_LINK_PREFIX.length + 1),
  );
  const channelId = params.get("channel");
  const targetKey = params.get("target");
  const seq = params.get("seq");
  if (!channelId || !targetKey || !seq || !/^\d+$/.test(seq)) return null;
  const eventSeq = Number.parseInt(seq, 10);
  if (!Number.isSafeInteger(eventSeq) || eventSeq <= 0) return null;
  return { channelId, targetKey, eventSeq };
}

/**
 * Build the editable prefill for the composer: provenance block first, then a
 * blank line where the operator writes the actual instruction.
 */
export function buildCodingSessionHandoffPrefill(input: {
  sourceLabel: string;
  link: CodingSessionHandoffLink;
  quote: string;
}): string {
  const quotedLines = input.quote
    .trim()
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
  return `> From ${input.sourceLabel} (this session) — ${buildCodingSessionHandoffLink(
    input.link,
  )}\n${quotedLines}\n\n`;
}

const HANDOFF_HEADER_PATTERN =
  /^> From (.+) \(this session\) — (buzz:\/\/coding-session\?\S+)$/;

/**
 * Recognize a handoff provenance block at the head of an operator-signed
 * prompt. Returns null when the text is not in the exact built shape — the
 * caller then renders the message as a plain prompt with a visible quote,
 * which is the honest degraded form.
 */
export function parseCodingSessionHandoffPrefill(
  content: string,
): CodingSessionHandoffProvenance | null {
  const lines = content.split("\n");
  const header = lines[0]?.match(HANDOFF_HEADER_PATTERN);
  if (!header) return null;
  const quoteLines: string[] = [];
  let index = 1;
  while (index < lines.length && lines[index].startsWith("> ")) {
    quoteLines.push(lines[index].slice(2));
    index += 1;
  }
  if (quoteLines.length === 0) return null;
  return {
    sourceLabel: header[1],
    linkUrl: header[2],
    link: parseCodingSessionHandoffLink(header[2]),
    quote: quoteLines.join("\n"),
    instruction: lines.slice(index).join("\n").trim(),
  };
}

/** A block the current view could scroll to, as fact resolution needs it. */
export type CodingSessionHandoffFactCandidate = {
  /** The block's render key, returned as the resolved location. */
  key: string;
  /** Exact `cs-target` key (generation included) of the block's stream. */
  targetKey: string | null;
  items: readonly { id: string }[];
};

/**
 * Locate a handoff link's quoted fact among the blocks this view has already
 * ingested.
 *
 * The `buzz://coding-session` link is a client-side convention, not a scheme
 * anything registers: nothing in the app (or the OS) resolves it, so a bare
 * anchor is a dead control. Resolution is therefore local and total — the fact
 * is either already in the rendered timeline, in which case the chip can
 * scroll to it, or it is not, in which case the caller must render
 * non-interactive provenance instead of a link that goes nowhere.
 *
 * Matching is exact on all three coordinates the link carries: same channel,
 * same signed target (generation included), same `eventSeq`. A cross-channel
 * or cross-generation link never resolves to a lookalike block.
 */
export function resolveCodingSessionHandoffFactLocation(input: {
  channelId: string;
  link: CodingSessionHandoffLink | null;
  candidates: readonly CodingSessionHandoffFactCandidate[];
}): string | null {
  const link = input.link;
  if (link === null || link.channelId !== input.channelId) return null;
  for (const candidate of input.candidates) {
    if (
      candidate.targetKey === null ||
      candidate.targetKey !== link.targetKey
    ) {
      continue;
    }
    const found = candidate.items.some(
      (item) =>
        readCodingSessionTranscriptItemEventSeq(item.id) === link.eventSeq,
    );
    if (found) return candidate.key;
  }
  return null;
}

/**
 * Whether a turn block has finished and may be handed to another execution.
 * A block still streaming has no stable result to quote yet.
 */
export function isCompletedCodingSessionTurnBlock(
  block: Pick<CodingSessionUmbrellaTurnBlock, "items">,
): boolean {
  const last = block.items[block.items.length - 1];
  return (
    last !== undefined &&
    last.type === "lifecycle" &&
    (last.title === "Turn result" || last.title === "Interrupted")
  );
}

/**
 * The quotable substance of a completed block: the latest assistant text or
 * plan. Tool telemetry and lifecycle rows are not quotable claims.
 */
export function resolveCodingSessionHandoffSource(
  block: Pick<CodingSessionUmbrellaTurnBlock, "items">,
): { quote: string; eventSeq: number | null } | null {
  for (let index = block.items.length - 1; index >= 0; index -= 1) {
    const item = block.items[index];
    if (
      (item.type === "message" && item.role === "assistant") ||
      item.type === "plan"
    ) {
      return {
        quote: item.text,
        eventSeq: readCodingSessionTranscriptItemEventSeq(item.id),
      };
    }
  }
  return null;
}

/**
 * Recover the `eventSeq` embedded as the final field of a projected
 * coding-session transcript item id (`coding-session-transcript-item/v1`
 * structured key). Returns null for foreign or fallback ids — the handoff
 * then degrades to a link-less quote rather than pointing at a guessed fact.
 */
export function readCodingSessionTranscriptItemEventSeq(
  itemId: string,
): number | null {
  const prefix = "coding-session-transcript-item/v1|";
  if (!itemId.startsWith(prefix)) return null;
  const fields = decodeStructuredKeyFields(itemId.slice(prefix.length));
  const seqField = fields?.[fields.length - 1];
  if (fields?.length !== 3 || seqField === undefined) return null;
  if (!/^\d+$/.test(seqField)) return null;
  const seq = Number.parseInt(seqField, 10);
  return Number.isSafeInteger(seq) && seq > 0 ? seq : null;
}

/**
 * Walk a `<byteLen>:<field>` sequence from the front — the only direction the
 * length-prefixed encoding can be read, since field values may themselves
 * contain digits and colons. Returns null on any malformed run.
 */
function decodeStructuredKeyFields(encoded: string): string[] | null {
  const bytes = new TextEncoder().encode(encoded);
  const decoder = new TextDecoder();
  const fields: string[] = [];
  let offset = 0;
  while (offset < bytes.length) {
    let length = 0;
    let sawDigit = false;
    while (offset < bytes.length) {
      const byte = bytes[offset];
      if (byte === 0x3a /* ":" */) break;
      if (byte < 0x30 || byte > 0x39) return null;
      length = length * 10 + (byte - 0x30);
      sawDigit = true;
      offset += 1;
      if (length > bytes.length) return null;
    }
    if (!sawDigit || bytes[offset] !== 0x3a) return null;
    offset += 1;
    if (offset + length > bytes.length) return null;
    fields.push(decoder.decode(bytes.subarray(offset, offset + length)));
    offset += length;
  }
  return fields;
}

/** The first user prompt of a block, when the block opens with one. */
export function readCodingSessionTurnBlockPrompt(
  block: Pick<CodingSessionUmbrellaTurnBlock, "items">,
): Extract<TranscriptItem, { type: "message" }> | null {
  const first = block.items[0];
  return first !== undefined &&
    first.type === "message" &&
    first.role === "user"
    ? first
    : null;
}
