/**
 * What a host says about a hire, and to whom.
 *
 * Moved out of `hooks/useCodingSessionHire.ts` unchanged when that file
 * reached the repository's 1000-line ceiling, the same way
 * `codingSessionHireOutcomeStore.ts` was. It is a coherent unit: the two
 * places every answer is published, the refusal that goes through them, and
 * the outcome every non-seated answer is recorded as.
 */
import type {
  CodingSessionHireDeps,
  CodingSessionHireOutcome,
  UseCodingSessionHireInput,
} from "../hooks/useCodingSessionHire";
import { publishCodingSessionCommand } from "./codingSessionCommand";
import {
  codingSessionHireRefusalNotice,
  type CodingSessionHireAnswer,
} from "./codingSessionHireAnswer";
import { codingSessionHireRequesterLabel } from "./codingSessionHireSeat";
import {
  codingSessionHireRequesterStanding,
  type CodingSessionHireRequest,
} from "./codingSessionHireWire";
import { publishCodingSessionLaneMessage } from "./codingSessionLanePublish";

/**
 * Say one fact twice: privately to the seat that asked, and in the umbrella.
 *
 * To the requester so it can act, and to the umbrella so the person sees it.
 * Neither is allowed to fail the other: a lead that heard nothing would wait
 * out its whole turn budget on an answer that was published and dropped.
 */
export async function discloseCodingSessionHire(
  disclosure: {
    channelId: string;
    sessionRef: string;
    requesterPubkey: string;
    /** The 44220 text the requesting seat receives. */
    text: string;
    /** The umbrella's own line for the same fact. */
    notice: string;
  },
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  const target = input.targetForActor(
    disclosure.channelId,
    disclosure.requesterPubkey,
  );
  if (target) {
    await publishCodingSessionCommand(
      {
        channelId: disclosure.channelId,
        commandId: deps.newTurnCommandId(),
        target,
        text: disclosure.text,
        deliver: "boundary",
      },
      { publisher: deps.publisher, signer: deps.signer },
    ).catch(() => {});
  }
  await publishCodingSessionLaneMessage(
    {
      channelId: disclosure.channelId,
      sessionRef: disclosure.sessionRef,
      content: disclosure.notice,
    },
    { publisher: deps.publisher, signer: deps.signer },
  ).catch(() => {});
}

export async function publishRefusal(
  request: CodingSessionHireRequest,
  answer: Extract<CodingSessionHireAnswer, { kind: "refused" }>,
  input: UseCodingSessionHireInput,
  deps: CodingSessionHireDeps,
): Promise<void> {
  await discloseCodingSessionHire(
    {
      channelId: request.channelId,
      sessionRef: request.action.sessionRef,
      requesterPubkey: request.requesterPubkey,
      text: answer.text,
      notice: codingSessionHireRefusalNotice({
        role: request.action.role,
        // Named, not "A seat". The hire carries `requestedBy`, this host
        // compares it with the signer itself — the relay does not
        // (POLICY.md 5) — and the three answers are three different
        // sentences, including the one that says the claim is disputed.
        requesterLabel: codingSessionHireRequesterLabel({
          standing: codingSessionHireRequesterStanding(request),
          nameFor: (pubkey) =>
            input.agents.find((agent) => agent.pubkey === pubkey)?.name ?? null,
        }),
        text: answer.text,
      }),
    },
    input,
    deps,
  );
}

export function outcomeOf(
  request: CodingSessionHireRequest,
  state: CodingSessionHireOutcome["state"],
  detail: string | null,
): CodingSessionHireOutcome {
  return {
    commandId: request.commandId,
    channelId: request.channelId,
    sessionRef: request.action.sessionRef,
    role: request.action.role,
    state,
    detail,
    seatCommandId: null,
    granted: false,
    seatActor: null,
    requesterLabel: null,
    hostPubkey: null,
  };
}
