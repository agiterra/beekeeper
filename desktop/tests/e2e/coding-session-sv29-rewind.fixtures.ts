/**
 * SV-29 rewind fixtures: real signed events for the mock relay. The provider
 * key signs metadata, transcript, checkpoints and receipts; a separate person
 * key signs the 44221 rewind a seeded "after" state answers. Nothing here is
 * mocked IPC — a rewind is pure relay traffic — so the spec needs no bridge
 * handler and the Wave B registry is untouched.
 */
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  CODING_SESSION_CHECKPOINT_SCHEMA,
  CODING_SESSION_CHECKPOINT_TAG_VERSION,
  codingSessionCheckpointSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionCheckpoints";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionRewindEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CHECKPOINT,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

export const SV29_CHANNEL_NAME = "engineering";
export const SV29_CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

const providerSecret = generateSecretKey();
export const SV29_PROVIDER = getPublicKey(providerSecret);
const personSecret = generateSecretKey();
export const SV29_PERSON = getPublicKey(personSecret);

export const SV29_HEAD = "e".repeat(40);
const COMMIT = "c".repeat(40);

export type Sv29Target = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

export function sv29Target(generation: number): Sv29Target {
  return {
    driver: "claude-agent-acp",
    instanceId: "b1b2c3d4e5f60829",
    sessionId: "da8d6582-0000-4000-8000-000000000829",
    generation,
  };
}

let clock = 1_800_900_000;

function sign(
  secret: Uint8Array,
  kind: number,
  content: string,
  tags: string[][],
): RelayEvent {
  clock += 1;
  return finalizeEvent(
    { kind, created_at: clock, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

export function sv29Metadata(
  session: Sv29Target,
  status: "idle" | "running" | "disconnected" = "idle",
): RelayEvent {
  return sign(
    providerSecret,
    KIND_CODING_SESSION_METADATA,
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Tidy the parser",
      agentRef: null,
      provider: session.driver,
      runtime: session.driver,
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
    }),
    [
      ["h", SV29_CHANNEL_ID],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

export function sv29Transcript(
  session: Sv29Target,
  seq: number,
  turnId: string | null,
  item: unknown,
): RelayEvent {
  return sign(
    providerSecret,
    KIND_CODING_SESSION_TRANSCRIPT,
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_900_000_000 + session.generation * 100_000 + seq * 1_000,
      turnId,
      item,
    }),
    [
      ["h", SV29_CHANNEL_ID],
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

/** One prompted turn: prompt, answer, result, at `seq`..`seq + 2`. */
export function sv29Turn(
  session: Sv29Target,
  seq: number,
  turnId: string,
  prompt: string,
  answer: string,
): RelayEvent[] {
  return [
    sv29Transcript(session, seq, turnId, {
      kind: "user_prompt",
      content: prompt,
    }),
    sv29Transcript(session, seq + 1, turnId, {
      kind: "assistant_text",
      text: answer,
    }),
    sv29Transcript(session, seq + 2, turnId, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 6_000,
      result: "",
    }),
  ];
}

/** A turn checkpoint; `restorable: false` with git trees is an older build's. */
export function sv29Checkpoint(
  session: Sv29Target,
  turnId: string,
  fromSeq: number,
  throughSeq: number,
  restorable: boolean,
): RelayEvent {
  return sign(
    providerSecret,
    KIND_CODING_SESSION_CHECKPOINT,
    JSON.stringify({
      schema: CODING_SESSION_CHECKPOINT_SCHEMA,
      session,
      turnId,
      reason: "turn",
      coverage: { fromSeq, throughSeq },
      git: {
        head: SV29_HEAD,
        branch: "main",
        baseTree: String(fromSeq).repeat(40).slice(0, 40),
        tree: String(throughSeq).repeat(40).slice(0, 40),
        commit: COMMIT,
        outsideTurn: false,
        complete: true,
        omitted: [],
        omittedNotListed: 0,
      },
      files: [],
      filesNotListed: 0,
      restorable,
      unavailable: null,
      summary: null,
    }),
    [
      ["h", SV29_CHANNEL_ID],
      ["csck-v", CODING_SESSION_CHECKPOINT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["csck-seq", String(throughSeq)],
      [
        "csck-key",
        codingSessionCheckpointSemanticKey(session, "turn", throughSeq),
      ],
    ],
  );
}

/** A provider-signed 44224 for one lifecycle command. */
export function sv29Receipt(
  commandId: string,
  body: Record<string, unknown>,
): RelayEvent {
  return sign(
    providerSecret,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    JSON.stringify({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId,
      ...body,
    }),
    [
      ["h", SV29_CHANNEL_ID],
      ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
      ["csl-command", commandId],
      ["csl-key", lifecycleReceiptSemanticKey(commandId)],
    ],
  );
}

/** The person's signed rewind of generation `from` to before `checkpoint`. */
export function sv29RewindCommand(
  commandId: string,
  from: Sv29Target,
  checkpoint: string,
  files: "keep" | "restore",
): RelayEvent {
  const built = buildCodingSessionRewindEvent({
    channelId: SV29_CHANNEL_ID,
    commandId,
    target: from,
    providerAuthorityPubkey: SV29_PROVIDER,
    checkpoint,
    files,
  });
  return sign(personSecret, built.kind, built.content, built.tags);
}

/** Generation 1: three turns; turns 1–2 restorable, turn 3 an older build's. */
export function sv29BeforeEvents(): {
  events: RelayEvent[];
  turn2Checkpoint: RelayEvent;
} {
  const g1 = sv29Target(1);
  const turn2Checkpoint = sv29Checkpoint(g1, "turn-2", 4, 6, true);
  return {
    turn2Checkpoint,
    events: [
      sv29Metadata(g1),
      ...sv29Turn(
        g1,
        1,
        "turn-1",
        "Add a failing test for the parser.",
        "Added parser.test.ts.",
      ),
      sv29Checkpoint(g1, "turn-1", 1, 3, true),
      ...sv29Turn(
        g1,
        4,
        "turn-2",
        "Fix the parser so it passes.",
        "Rewrote parse().",
      ),
      turn2Checkpoint,
      ...sv29Turn(
        g1,
        7,
        "turn-3",
        "Now inline every helper.",
        "Inlined five helpers.",
      ),
      sv29Checkpoint(g1, "turn-3", 7, 9, false),
    ],
  };
}
