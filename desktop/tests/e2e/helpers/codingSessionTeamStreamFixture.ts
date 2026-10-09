import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

/**
 * A synthetic, sanitized team umbrella for the SV-100 stream harness: one
 * founder-signed genesis, SEATS executions each created and receipted into
 * it, and a history of real-shaped turns per seat (prompt, Bash calls and
 * results, an answer, a result). Every event is really signed.
 */

/** The mock bridge's `engineering` channel. */
export const TEAM_CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b100000-0000-4000-8000-0000000000aa";
export const TEAM_TITLE = "SV-100 team stream";

const PROVIDER_SECRET = new Uint8Array(32).fill(21);
const FOUNDER_SECRET = new Uint8Array(32).fill(23);
export const TEAM_PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
/** The mock identity (`DEFAULT_MOCK_IDENTITY`). */
const ME = "deadbeef".repeat(8);

type Target = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

function sign(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

function seatTarget(index: number): Target {
  return {
    driver: "claude-agent-acp",
    instanceId: `5b10${String(index).padStart(12, "0")}`,
    sessionId: `5b100000-0000-4000-8000-${String(index).padStart(12, "0")}`,
    generation: 1,
  };
}

function metadata(target: Target, createdAt: number, status: string) {
  return sign(
    KIND_CODING_SESSION_METADATA,
    createdAt,
    [
      ["h", CHANNEL_ID],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(target)],
      ["csm-key", codingSessionMetadataSemanticKey(target)],
    ],
    JSON.stringify({
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      projectRef: null,
      repoRef: null,
      title: TEAM_TITLE,
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
        promptImage: false,
        context: false,
        diff: false,
        plan: false,
      },
      sessionRef: SESSION_REF,
    }),
    PROVIDER_SECRET,
  );
}

function createAndReceipt(
  genesisRef: string,
  index: number,
  createdAt: number,
): RelayEvent[] {
  const commandId = `5b100000-0000-4000-9000-${String(index).padStart(12, "0")}`;
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: TEAM_PROVIDER_PUBKEY,
    model: "sonnet",
    title: TEAM_TITLE,
    initialTurn: null,
  });
  return [
    sign(built.kind, createdAt, built.tags, built.content, FOUNDER_SECRET),
    sign(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      createdAt + 1,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "created",
        session: seatTarget(index),
        error: null,
      }),
      PROVIDER_SECRET,
    ),
  ];
}

/** One seat's transcript writer; `seq` numbering is its own. */
export function teamSeatWriter(index: number, baseSeconds: number) {
  const target = seatTarget(index);
  const targetKey = buildCodingSessionTargetKey(target);
  let seq = 0;
  return (turnId: string, item: unknown): RelayEvent => {
    seq += 1;
    return sign(
      KIND_CODING_SESSION_TRANSCRIPT,
      baseSeconds + seq,
      [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", targetKey],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(target, seq)],
      ],
      JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: target,
        eventSeq: seq,
        timestamp: (baseSeconds + seq) * 1_000,
        turnId,
        item,
      }),
      PROVIDER_SECRET,
    );
  };
}

/**
 * The umbrella: `seats` executions with `turnsPerSeat` settled turns each,
 * interleaved by time. Returns the events and, for the LAST seat, a writer
 * positioned after its history plus that seat's open turn id — the harness
 * streams into it.
 */
export function teamStreamFixture(seats: number, turnsPerSeat: number) {
  const base = Math.floor(Date.now() / 1_000) - 6 * 3_600;
  const genesisBuilt = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = sign(
    genesisBuilt.kind,
    base - 20,
    genesisBuilt.tags,
    genesisBuilt.content,
    FOUNDER_SECRET,
  );
  const events: RelayEvent[] = [genesis];
  const writers = [];
  for (let seat = 0; seat < seats; seat += 1) {
    events.push(...createAndReceipt(genesis.id, seat, base - 10 + seat * 2));
    writers.push(teamSeatWriter(seat, base + seat * 7));
  }
  for (let turn = 1; turn <= turnsPerSeat; turn += 1) {
    for (let seat = 0; seat < seats; seat += 1) {
      const write = writers[seat];
      if (!write) continue;
      const turnId = `team-${seat}-${turn}`;
      events.push(
        write(turnId, {
          kind: "user_prompt",
          content: `Seat ${seat} task ${turn}`,
          operatorPubkey: ME,
        }),
      );
      for (let tool = 0; tool < 3; tool += 1) {
        const toolId = `${turnId}-t${tool}`;
        events.push(
          write(turnId, {
            kind: "tool_call",
            tool: {
              toolName: "Bash",
              toolId,
              input: {
                command: `cd /work && cargo test -p crate${tool} -- --nocapture | tail -${10 + tool}`,
              },
            },
          }),
          write(turnId, {
            kind: "tool_result",
            toolId,
            toolName: "Bash",
            content: `test result: ok. ${tool + 5} passed; 0 failed`,
            isError: false,
          }),
        );
      }
      events.push(
        write(turnId, {
          kind: "assistant_text",
          text: `Seat ${seat}, turn ${turn}: the suites pass.`,
        }),
        write(turnId, {
          kind: "result",
          subtype: "success",
          isError: false,
          durationMs: 20_000,
          result: "",
          costUsd: 0.01,
        }),
      );
    }
  }
  const last = seats - 1;
  const liveTurn = `team-${last}-live`;
  const liveWrite = writers[last];
  if (!liveWrite) throw new Error("no seats");
  events.push(
    liveWrite(liveTurn, {
      kind: "user_prompt",
      content: "Live streamed turn",
      operatorPubkey: ME,
    }),
  );
  for (let seat = 0; seat < seats; seat += 1) {
    events.push(
      metadata(
        seatTarget(seat),
        base + 9_000 + seat,
        seat === last ? "running" : "idle",
      ),
    );
  }
  return { events, liveWrite, liveTurn };
}
