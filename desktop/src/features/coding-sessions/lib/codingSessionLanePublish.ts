/**
 * Publish path for conversation-lane messages: sign the kind:9 lane event
 * with the OS keystore and publish it natively, mirroring the 44220 command
 * path in `codingSessionCommand.ts`. The lane is ordinary chat — every relay
 * rejection is a failure of the write, never silently retried.
 */
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  buildCodingSessionLaneMessageEvent,
  type CodingSessionLaneMessageEventInput,
} from "./codingSessionConversationLane";

type LanePublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

type LaneSigner = (
  input: CodingSessionLaneMessageEventInput,
) => Promise<RelayEvent>;

/** Sign and publish one session-lane message; returns the accepted event id. */
export async function publishCodingSessionLaneMessage(
  input: Parameters<typeof buildCodingSessionLaneMessageEvent>[0],
  dependencies: {
    publisher?: LanePublisher;
    signer?: LaneSigner;
  } = {},
): Promise<{ eventId: string }> {
  const publisher = dependencies.publisher ?? relayClient;
  const signer = dependencies.signer ?? signRelayEvent;
  const event = await signer(buildCodingSessionLaneMessageEvent(input));
  const accepted = await publisher.publishEvent(
    event,
    "Timed out while sending the session message.",
    "Failed to send the session message.",
  );
  return { eventId: accepted.id };
}
