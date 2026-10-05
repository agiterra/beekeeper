import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  buildCodingSessionHandoverContent,
  buildCodingSessionHandoverTags,
} from "@/features/coding-sessions/lib/codingSessionHandoverWire";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
} from "@/features/coding-sessions/lib/codingSessionTeamTransactionWire";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_HANDOVER,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_OBSERVATION,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

/**
 * Fixtures for the transcript minimap spec (SV-26, SV-27), lane B5.
 *
 * Two sessions:
 * - a **single-layout** session of 40 prompted turns plus one turn with no
 *   prompt, so the transcript holds 41 rows and virtualizes, while the
 *   minimap draws exactly 40 dashes;
 * - a **governed umbrella** of two executions (15 prompted turns), so the
 *   render window of ten hides the earliest turns, with one failed turn, one
 *   gate row signed inside a turn's window, changed files on that turn, and
 *   one handover checkpoint (kind 44247) signed inside another turn's window,
 *   and one open ruling request (`decision.request`, kind 44244, DB8) signed
 *   inside a third turn's window.
 */

export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
/** The mock identity (`DEFAULT_MOCK_IDENTITY`): "your" prompts. */
export const ME = "deadbeef".repeat(8);

const PROVIDER_SECRET = generateSecretKey();
export const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
/** A teammate whose prompts take their identity hue (DB12). */
export const TEAMMATE = getPublicKey(generateSecretKey());

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

function metadataEvent(
  target: Target,
  title: string,
  createdAt: number,
  sessionRef: string | null,
): RelayEvent {
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
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      projectRef: null,
      repoRef: null,
      title,
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "completed",
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
      ...(sessionRef === null ? {} : { sessionRef }),
    }),
    PROVIDER_SECRET,
  );
}

/** Builds one target's transcript events, numbering `eventSeq` itself. */
function transcriptWriter(target: Target, baseCreatedAt: number) {
  let seq = 0;
  const targetKey = buildCodingSessionTargetKey(target);
  return (turnId: string, timestampMs: number, item: unknown): RelayEvent => {
    seq += 1;
    return sign(
      KIND_CODING_SESSION_TRANSCRIPT,
      baseCreatedAt + seq,
      [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", targetKey],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(target, seq)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: target,
        eventSeq: seq,
        timestamp: timestampMs,
        turnId,
        item,
      }),
      PROVIDER_SECRET,
    );
  };
}

type TurnSpec = {
  turnId: string;
  prompt: string | null;
  reply: string;
  operator: string;
  startMs: number;
  failed?: boolean;
  edits?: readonly string[];
};

function turnEvents(
  write: ReturnType<typeof transcriptWriter>,
  turn: TurnSpec,
): RelayEvent[] {
  const events: RelayEvent[] = [];
  let at = turn.startMs;
  const next = () => {
    at += 1_000;
    return at;
  };
  if (turn.prompt !== null) {
    events.push(
      write(turn.turnId, at, {
        kind: "user_prompt",
        content: turn.prompt,
        operatorPubkey: turn.operator,
      }),
    );
  }
  for (const [index, path] of (turn.edits ?? []).entries()) {
    const toolId = `${turn.turnId}-edit-${index}`;
    events.push(
      write(turn.turnId, next(), {
        kind: "tool_call",
        tool: {
          toolName: "Edit",
          toolKind: "edit",
          toolId,
          input: {
            file_path: path,
            old_string: "a",
            new_string: "b",
          },
        },
      }),
      write(turn.turnId, next(), {
        kind: "tool_result",
        toolId,
        toolName: "Edit",
        content: `Edited ${path}`,
        isError: false,
      }),
    );
  }
  events.push(
    write(turn.turnId, next(), { kind: "assistant_text", text: turn.reply }),
    write(
      turn.turnId,
      next(),
      turn.failed
        ? {
            kind: "result",
            subtype: "error",
            isError: true,
            durationMs: 9_000,
            result: "The suite crashed.",
          }
        : {
            kind: "result",
            subtype: "success",
            isError: false,
            durationMs: 42_000,
            result: "Done.",
          },
    ),
  );
  return events;
}

// --- Single layout -------------------------------------------------------

const SINGLE_TARGET: Target = {
  driver: "claude-agent-acp",
  instanceId: "b5b5b5b5b5b5b5b5",
  sessionId: "b5000000-0000-4000-8000-000000000026",
  generation: 1,
};
export const SINGLE_TITLE = "Forty prompts deep";
export const SINGLE_TURNS = 40;

/** The prompt text of single-layout turn `n` (1-based). */
export function singlePrompt(n: number): string {
  return `Turn ${n} prompt: tighten step ${n} of the retry plan`;
}

/** Every 5th turn is a teammate's; the rest are yours. */
export function singleTurnIsTeammate(n: number): boolean {
  return n % 5 === 0;
}

export function singleSessionEvents(): RelayEvent[] {
  const base = 1_800_600_000;
  const write = transcriptWriter(SINGLE_TARGET, base);
  const startMs = base * 1_000;
  const events: RelayEvent[] = [
    metadataEvent(SINGLE_TARGET, SINGLE_TITLE, base, null),
  ];
  for (let n = 1; n <= SINGLE_TURNS; n += 1) {
    events.push(
      ...turnEvents(write, {
        turnId: `single-turn-${n}`,
        prompt: singlePrompt(n),
        reply: `Reply ${n}: step ${n} now backs off and the counter resets.`,
        operator: singleTurnIsTeammate(n) ? TEAMMATE : ME,
        startMs: startMs + n * 60_000,
      }),
    );
  }
  // A turn the producer began with no prompt: a 41st row, no dash.
  events.push(
    ...turnEvents(write, {
      turnId: "single-turn-autonomous",
      prompt: null,
      reply: "Continuing on my own: the flaky test is quarantined.",
      operator: ME,
      startMs: startMs + (SINGLE_TURNS + 1) * 60_000,
    }),
  );
  return events;
}

// --- Governed umbrella ---------------------------------------------------

export const UMBRELLA_TITLE = "Minimap marks";
const SESSION_REF = "b5e1c2a0-90d4-4b0e-a1f3-7c2d8e6f4a26";
const TARGET_A: Target = {
  driver: "claude-agent-acp",
  instanceId: "0b5a0b5a0b5a0b5a",
  sessionId: "b5a00000-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_B: Target = {
  driver: "claude-agent-acp",
  instanceId: "0b5b0b5b0b5b0b5b",
  sessionId: "b5b00000-3333-4444-5555-666666666666",
  generation: 1,
};
export const UMBRELLA_TURNS_A = 14;
/** The failed turn and the turn whose window holds the gate row. */
export const UMBRELLA_FAILED_TURN = 13;
export const UMBRELLA_GATE_TURN = 12;
/** The turn whose window holds the founder's handover checkpoint. */
export const UMBRELLA_HANDOVER_TURN = 10;
/** The turn whose window holds the open `decision.request` (DB8). */
export const UMBRELLA_RULING_TURN = 11;
/** The relay's NIP-11 `self`, so the handover read trusts its receipts. */
const RELAY_SECRET = generateSecretKey();
export const RELAY_PUBKEY = getPublicKey(RELAY_SECRET);
export const UMBRELLA_GATE_FILES = [
  "src/retry.ts",
  "src/backoff.ts",
  "src/socket.ts",
  "src/counter.ts",
] as const;

export function umbrellaPrompt(n: number): string {
  return `Umbrella turn ${n} prompt: harden reconnect stage ${n}`;
}
export const UMBRELLA_TEAMMATE_PROMPT =
  "Teammate prompt: review the reconnect stages";

const OBSERVATION_SCHEMA = "buzz-coding-session-observation/v1";
const FOLD_SCHEMA = "buzz-coding-session-observation-fold-adapter/v2";
const DISCLOSURE =
  "an observation is something its author saw, not a decision: it settles nothing, authorizes nothing and excludes nothing, and every duration in it is the author's own measurement";

function createAndReceipt(
  genesisRef: string,
  commandId: string,
  target: Target,
  createdAt: number,
): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: UMBRELLA_TITLE,
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
        session: target,
        error: null,
      }),
      PROVIDER_SECRET,
    ),
  ];
}

/** The umbrella's events and the fold answer for its one gate row. */
export function umbrellaFixture() {
  // Real time, an hour back, so nothing sits in the future.
  const base = Math.floor(Date.now() / 1_000) - 3_600;
  const genesisBuilt = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = sign(
    genesisBuilt.kind,
    base - 10,
    genesisBuilt.tags,
    genesisBuilt.content,
    FOUNDER_SECRET,
  );
  const writeA = transcriptWriter(TARGET_A, base);
  const writeB = transcriptWriter(TARGET_B, base + 500);
  const turnStart = (n: number) => (base + n * 120) * 1_000;
  const transcripts: RelayEvent[] = [];
  for (let n = 1; n <= UMBRELLA_TURNS_A; n += 1) {
    transcripts.push(
      ...turnEvents(writeA, {
        turnId: `umbrella-a-${n}`,
        prompt: umbrellaPrompt(n),
        reply: `Stage ${n} answered: the socket now drains before closing.`,
        operator: ME,
        startMs: turnStart(n),
        failed: n === UMBRELLA_FAILED_TURN,
        edits: n === UMBRELLA_GATE_TURN ? UMBRELLA_GATE_FILES : undefined,
      }),
    );
  }
  // The teammate's turn on the second execution, last in the narrative.
  transcripts.push(
    ...turnEvents(writeB, {
      turnId: "umbrella-b-1",
      prompt: UMBRELLA_TEAMMATE_PROMPT,
      reply: "Reviewed: stages 1 to 14 hold.",
      operator: TEAMMATE,
      startMs: turnStart(UMBRELLA_TURNS_A + 1),
    }),
  );
  // A gate row signed thirty seconds into the gate turn's window.
  const gateCreatedAt = base + UMBRELLA_GATE_TURN * 120 + 30;
  const gate = sign(
    KIND_CODING_SESSION_OBSERVATION,
    gateCreatedAt,
    [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["csob-v", OBSERVATION_SCHEMA],
      ["csob-genesis", genesis.id],
      ["csob-type", "gate"],
    ],
    JSON.stringify({
      schema: OBSERVATION_SCHEMA,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      type: "gate",
      source: "observed",
      assignmentRef: null,
      body: {
        rows: [
          {
            gate: "cargo test",
            outcome: "passed",
            command: "cargo test -p buzz-core",
            summary: "test result: ok. 40 passed; 0 failed",
            durationMs: 30_000,
            headSha: "b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5",
            dirty: false,
          },
        ],
      },
    }),
    PROVIDER_SECRET,
  );
  // A founder's checkpoint forty seconds into the handover turn's window.
  const checkpoint = sign(
    KIND_CODING_SESSION_HANDOVER,
    base + UMBRELLA_HANDOVER_TURN * 120 + 40,
    buildCodingSessionHandoverTags({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      type: "checkpoint",
    }),
    buildCodingSessionHandoverContent({
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      type: "checkpoint",
      body: {
        prevCheckpointRef: null,
        task: "Harden the reconnect stages",
        assignmentRefs: [],
        decisions: [],
        revision: {
          repoRef: "30617:aa/beekeeper",
          baseSha: "1a".repeat(20),
          headSha: "2b".repeat(20),
          branch: "work/reconnect",
          dirty: false,
          preserved: "partial",
        },
        artifacts: [
          {
            kind: "wip-ref",
            repoRef: "30617:aa/beekeeper",
            ref: "refs/heads/wip/reconnect",
            sha: "2b".repeat(20),
          },
        ],
        tests: [],
        unresolved: [],
        nextAction: "Finish stages 11 to 14",
        missing: [],
      },
    }),
    FOUNDER_SECRET,
  );
  // An open ruling request, held on the founder, fifty seconds into the
  // ruling turn's window. Nothing answers it, so it is still waiting.
  const rulingRequest = sign(
    KIND_CODING_SESSION_TEAM_TRANSACTION,
    base + UMBRELLA_RULING_TURN * 120 + 50,
    [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", genesis.id],
      ["cstx-type", "decision.request"],
    ],
    JSON.stringify({
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      type: "decision.request",
      supersedes: null,
      deliveryCommandId: null,
      body: {
        question: "Keep the reconnect backoff capped at thirty seconds?",
        options: ["Yes", "No"],
        heldOn: "founder",
        blocks: [],
        recommendation: "Yes",
      },
    }),
    FOUNDER_SECRET,
  );
  const events: RelayEvent[] = [
    genesis,
    ...createAndReceipt(
      genesis.id,
      "b5c0ffee-3d05-4b0a-9f4e-8c2b1d6a7e30",
      TARGET_A,
      base - 8,
    ),
    ...createAndReceipt(
      genesis.id,
      "b5c0ffee-6b21-4f88-9a0c-2e4f8b1d7c55",
      TARGET_B,
      base - 6,
    ),
    metadataEvent(TARGET_A, UMBRELLA_TITLE, base - 4, SESSION_REF),
    metadataEvent(TARGET_B, UMBRELLA_TITLE, base - 3, SESSION_REF),
    ...transcripts,
    gate,
    checkpoint,
    rulingRequest,
  ];
  const foldResponse = {
    schema: FOLD_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: [gate.id],
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    checkpoints: [],
    gates: [
      {
        authorPubkey: PROVIDER_PUBKEY,
        source: "observed",
        eventIds: [gate.id],
        droppedEventIds: 0,
        assignmentRef: null,
        gate: "cargo test",
        outcome: "passed",
        command: "cargo test -p buzz-core",
        summary: "test result: ok. 40 passed; 0 failed",
        durationMs: 30_000,
        headSha: "b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5b5",
        dirty: false,
      },
    ],
    findings: [],
    phases: [],
    gateStarts: [],
    gateStartStaleAfterMs: 1_800_000,
    unresolved: [],
    ignored: [],
    misclaimedObserved: [],
    provenanceChecked: true,
    truncated: {
      checkpoints: 0,
      gates: 0,
      findings: 0,
      phases: 0,
      unresolved: 0,
      ignored: 0,
      misclaimedObserved: 0,
      entryEventIds: 0,
      displacedGates: 0,
      displacedFindings: 0,
      gateStarts: 0,
      gateStartClosesUnmatched: 0,
    },
    disclosure: DISCLOSURE,
  };
  return { events, foldResponse, founderPubkey: FOUNDER_PUBKEY };
}
