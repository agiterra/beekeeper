import { truncatePubkey } from "@/shared/lib/pubkey";
import type { CodingSessionMissionDecisionModel } from "@/features/coding-sessions/lib/codingSessionMissionDecisions";

/** Kind 44244, the NIP-CSTX team transaction. */
export const KIND_CODING_SESSION_TEAM_TRANSACTION = 44244;

const TEAM_TRANSACTION_SCHEMA = "buzz-coding-session-team-transaction/v1";
const HEX64 = /^[0-9a-f]{64}$/;

/** One `decision.request` as the inbox reads it (ledger 249(A)). */
export type InboxDecisionRequest = {
  requestId: string;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  question: string;
  options: readonly string[];
  heldOn: string;
  recommendation: string | null;
  blocks: readonly string[];
  askerPubkey: string;
};

type EventLike = {
  id?: string;
  kind: number;
  pubkey?: string;
  tags: readonly (readonly string[])[];
  content: string;
};

function tag(event: EventLike, name: string): string | null {
  const found = event.tags.find((entry) => entry[0] === name);
  return typeof found?.[1] === "string" ? found[1] : null;
}

function isStringArray(value: unknown): value is string[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === "string")
  );
}

/**
 * Read a kind:44244 `decision.request`, or `null` for anything else.
 *
 * Strict in the same places the card depends on: a request this reader does
 * not fully recognise carries no answer control and keeps its ordinary body.
 * The desktop's Rust feed has already run `buzz-core`'s envelope validator
 * before admitting it; this is the renderer's own guard, not a second rule.
 */
export function readInboxDecisionRequest(
  event: EventLike,
): InboxDecisionRequest | null {
  if (event.kind !== KIND_CODING_SESSION_TEAM_TRANSACTION) return null;
  if (tag(event, "cstx-type") !== "decision.request") return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const record = parsed as Record<string, unknown>;
  const body = record.body as Record<string, unknown> | null | undefined;
  const channelRef = tag(event, "h");
  if (
    record.schema !== TEAM_TRANSACTION_SCHEMA ||
    record.type !== "decision.request" ||
    typeof record.sessionRef !== "string" ||
    typeof record.genesisRef !== "string" ||
    !HEX64.test(record.genesisRef) ||
    channelRef === null ||
    typeof body !== "object" ||
    body === null ||
    typeof body.question !== "string" ||
    typeof body.heldOn !== "string" ||
    !isStringArray(body.options) ||
    !isStringArray(body.blocks ?? []) ||
    !(body.recommendation === null || typeof body.recommendation === "string")
  ) {
    return null;
  }
  const requestId = event.id ?? "";
  if (!HEX64.test(requestId)) return null;
  return {
    requestId,
    channelRef,
    sessionRef: record.sessionRef,
    genesisRef: record.genesisRef,
    question: body.question,
    options: body.options,
    heldOn: body.heldOn,
    recommendation: (body.recommendation as string | null) ?? null,
    blocks: (body.blocks as string[] | undefined) ?? [],
    askerPubkey: event.pubkey ?? "",
  };
}

/**
 * The answer form's model for an inbox request.
 *
 * The feed admitted this request because it is held on the viewer — as the
 * session's founder, or by the viewer's own key — or on an agent the viewer
 * owns. Only the first two may answer here: an agent's ruling is the agent's,
 * so its owner sees it (disabled, never hidden) and says whose it is.
 */
export function inboxDecisionModel(
  request: InboxDecisionRequest,
  viewerPubkey: string | null,
): CodingSessionMissionDecisionModel {
  const viewerIsHolder =
    request.heldOn === "founder"
      ? true
      : viewerPubkey === null
        ? null
        : request.heldOn === viewerPubkey;
  return {
    requestId: request.requestId,
    shortId: request.requestId.slice(0, 8),
    question: request.question,
    state: "open",
    stateWord:
      request.heldOn === "founder"
        ? "Open · held on you, as this session's founder"
        : `Open · held on ${truncatePubkey(request.heldOn)}`,
    blocksWord:
      request.blocks.length === 0
        ? "holds up no assignment yet"
        : `holds up ${request.blocks.length} assignment${request.blocks.length === 1 ? "" : "s"}`,
    blocks: request.blocks.map((id) => id.slice(0, 8)),
    askedAtMs: null,
    answerCondition: null,
    options: request.options,
    recommendation: request.recommendation,
    heldOn: request.heldOn,
    viewerIsHolder,
    heldElsewhereSentence:
      viewerIsHolder === true
        ? null
        : `This ruling is held on your agent ${truncatePubkey(request.heldOn)}; it answers it from its own session.`,
    answerChoiceWord: null,
    channelRef: request.channelRef,
    sessionRef: request.sessionRef,
    genesisRef: request.genesisRef,
  };
}
