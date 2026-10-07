import type { Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

/** The mock bridge's `engineering` channel. */
export const STREAM_CHANNEL_NAME = "engineering";
const STREAM_CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

export type StreamTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/**
 * Signed 44223/44225 builders for one provider key (SV-118 specs). Every
 * event is really signed, so the app verifies it exactly as it would a
 * relay's.
 */
export function codingSessionStreamSigner(secret: Uint8Array) {
  const pubkey = getPublicKey(secret);
  const signed = (
    kind: number,
    createdAt: number,
    content: unknown,
    tags: string[][],
  ) =>
    finalizeEvent(
      {
        kind,
        created_at: createdAt,
        tags: [["h", STREAM_CHANNEL_ID], ...tags],
        content: JSON.stringify(content),
      },
      secret,
    ) as unknown as RelayEvent;

  return {
    pubkey,
    metadata(
      session: StreamTarget,
      createdAt: number,
      title: string,
      status = "running",
    ): RelayEvent {
      return signed(
        KIND_CODING_SESSION_METADATA,
        createdAt,
        {
          schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
          session,
          projectRef: null,
          repoRef: null,
          title,
          agentRef: null,
          provider: "claude-agent-acp",
          runtime: "claude-agent-acp",
          model: "sonnet",
          status,
          branch: null,
          capabilities: {
            threadTurnStart: true,
            threadTurnInterrupt: true,
            threadSteer: true,
            promptImage: true,
            context: false,
            diff: false,
            plan: false,
          },
        },
        [
          ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(session)],
          ["csm-key", codingSessionMetadataSemanticKey(session)],
        ],
      );
    },
    transcript(
      session: StreamTarget,
      seq: number,
      turnId: string | null,
      item: unknown,
      createdAt = 1_800_600_000 + seq,
    ): RelayEvent {
      return signed(
        KIND_CODING_SESSION_TRANSCRIPT,
        createdAt,
        {
          schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
          session,
          eventSeq: seq,
          timestamp: 1_800_600_000_000 + seq * 1_000,
          turnId,
          item,
        },
        [
          ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(session)],
          ["cst-seq", String(seq)],
          ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
        ],
      );
    },
  };
}

/** Deliver signed events into the mock relay, as a live subscription sees them. */
export async function seedStreamEvents(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, events: signedEvents }) => {
      const seedEvent = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seedEvent) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seedEvent({ channelName, event });
    },
    { channelName: STREAM_CHANNEL_NAME, events },
  );
}
