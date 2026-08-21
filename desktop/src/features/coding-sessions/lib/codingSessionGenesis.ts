import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_GENESIS } from "@/shared/constants/kinds";
import { isCodingSessionSessionRef } from "./codingSessionWireDecode";
import { fanOutObservedCodingSessionEvents } from "./codingSessionObservedEvents";

export const CODING_SESSION_GENESIS_TAG_VERSION = "csg1-1" as const;
export const CODING_SESSION_GENESIS_SCHEMA_VERSION = 1 as const;
export const MAX_CODING_SESSION_GENESIS_CONTENT_BYTES = 1024;

export type CodingSessionGenesisEventInput = {
  content: string;
  kind: number;
  tags: string[][];
};

/** Build the immutable fresh-founding record for one umbrella session. */
export function buildCodingSessionGenesisEvent(input: {
  channelId: string;
  sessionRef: string;
}): CodingSessionGenesisEventInput {
  if (input.channelId.trim().length === 0) {
    throw new Error("channelId must not be empty");
  }
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error("sessionRef must be a canonical lowercase hyphenated UUID");
  }
  const content = JSON.stringify({
    sessionRef: input.sessionRef,
    v: CODING_SESSION_GENESIS_SCHEMA_VERSION,
  });
  if (
    new TextEncoder().encode(content).byteLength >
    MAX_CODING_SESSION_GENESIS_CONTENT_BYTES
  ) {
    throw new Error("coding-session genesis content exceeds 1024 bytes");
  }
  return {
    kind: KIND_CODING_SESSION_GENESIS,
    content,
    tags: [
      ["h", input.channelId],
      ["csg-v", CODING_SESSION_GENESIS_TAG_VERSION],
      ["csg-session", input.sessionRef],
    ],
  };
}

/** Sign and publish a fresh genesis, returning the relay-accepted event id. */
export async function publishCodingSessionGenesis(
  input: Parameters<typeof buildCodingSessionGenesisEvent>[0],
  dependencies: {
    publisher?: {
      publishEvent: (
        event: RelayEvent,
        timeoutMessage: string,
        sendErrorMessage: string,
      ) => Promise<RelayEvent>;
    };
    signer?: (input: CodingSessionGenesisEventInput) => Promise<RelayEvent>;
  } = {},
): Promise<{ eventId: string; kind: number }> {
  const event = await (dependencies.signer ?? signRelayEvent)(
    buildCodingSessionGenesisEvent(input),
  );
  const accepted = await (dependencies.publisher ?? relayClient).publishEvent(
    event,
    "Timed out while founding the coding session.",
    "Failed to found the coding session.",
  );
  fanOutObservedCodingSessionEvents([accepted]);
  return { eventId: accepted.id, kind: accepted.kind };
}
