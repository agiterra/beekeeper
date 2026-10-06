import {
  CODING_SESSION_TEAM_FOLD_COMMAND,
  CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
} from "@/features/coding-sessions/lib/invokeCodingSessionTeamFold";
import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock Tauri commands for the transcript minimap (SV-26, SV-27), owned by
 * lane B5.
 *
 * Off unless a spec sets `window.__BEEKEEPER_E2E_WAVE_B_MINIMAP_TEAM_FOLD__ = true`
 * before the app loads. Then the kind-44244 team fold answers from the
 * signed transactions it is handed, so the minimap's waiting-ruling mark
 * (DB8) can be driven by a real signed `decision.request`: every input is
 * included, each request is listed in `decisions[]`, and it stays open
 * (`answeredBy: null`) until a `decision.answer` names it.
 *
 * Deliberately not `buzz-core`'s fold — assignments, conflicts and
 * terminals are left empty. The fold's semantics are tested in Rust; this
 * exists so the screen can be driven by a signed event.
 */
type MockEvent = {
  id: string;
  pubkey: string;
  content: string;
};

function enabled(): boolean {
  return (
    typeof window !== "undefined" &&
    (
      window as Window & {
        __BEEKEEPER_E2E_WAVE_B_MINIMAP_TEAM_FOLD__?: boolean;
      }
    ).__BEEKEEPER_E2E_WAVE_B_MINIMAP_TEAM_FOLD__ === true
  );
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : {};
}

function parse(event: MockEvent): Record<string, unknown> {
  try {
    return record(JSON.parse(event.content) as unknown);
  } catch {
    return {};
  }
}

/** The team fold's rulings, from the transactions it was handed. */
function foldTeamRulings(request: Record<string, unknown>) {
  const context = record(request.context);
  const events = (request.events as MockEvent[] | undefined) ?? [];
  const inputEventIds = (request.inputEventIds as string[] | undefined) ?? [];
  const typed = events.map((event) => {
    const content = parse(event);
    return { event, type: content.type, body: record(content.body) };
  });
  const decisions = typed
    .filter((entry) => entry.type === "decision.request")
    .map((entry) => {
      const answer = typed.find(
        (candidate) =>
          candidate.type === "decision.answer" &&
          candidate.body.requestRef === entry.event.id,
      );
      return {
        requestId: entry.event.id,
        heldOn:
          typeof entry.body.heldOn === "string" ? entry.body.heldOn : "founder",
        blocks: Array.isArray(entry.body.blocks) ? entry.body.blocks : [],
        answeredBy: answer ? answer.event.pubkey : null,
        answerId: answer ? answer.event.id : null,
      };
    });
  const waiting = decisions.find((decision) => decision.answerId === null);
  return {
    schema: CODING_SESSION_TEAM_FOLD_RESPONSE_SCHEMA,
    implementation: "buzz-core",
    inputEventIds,
    context: {
      channelRef: context.channelRef,
      sessionRef: context.sessionRef,
      genesisRef: context.genesisRef,
      founderPubkey: context.founderPubkey,
      authorityHeadEventId: context.authorityHeadEventId ?? null,
      authorityHeadSeq: context.authorityHeadSeq ?? 0,
      verifierRequired: context.verifierRequired === true,
    },
    includedEventIds: inputEventIds,
    excluded: [],
    conflicts: [],
    assignments: [],
    unseatedReports: [],
    notes: [],
    decisions,
    waitingOnDecision: waiting
      ? { requestId: waiting.requestId, heldOn: waiting.heldOn }
      : null,
    pendingCompletion: null,
    canonicalTerminal: null,
  };
}

export async function handleWaveBMinimapMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (!enabled() || command !== CODING_SESSION_TEAM_FOLD_COMMAND) return null;
  return {
    handled: true,
    value: foldTeamRulings(record(record(payload).request)),
  };
}
