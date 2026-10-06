import { expect, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_OBSERVATION,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../../helpers/bridge";

/**
 * The observer's screen, end to end: kind 44246 from the relay, through the
 * native fold, onto the Audit tab and the Structured tests card.
 *
 * The fixture is deliberately small — one governed session, five signed
 * observations — because what this spec is about is the *reading*, not the
 * governance stream the Mission lens spec already covers.
 */

const PROVIDER_SECRET = generateSecretKey();
export const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
export const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const SEAT_SECRET = generateSecretKey();
export const SEAT_PUBKEY = getPublicKey(SEAT_SECRET);

export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
export const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "9f2f0e12-3d05-4b0a-9f4e-8c2b1d6a7e30";
// Two executions on purpose. `umbrellaHasCollapsedHistory`
// (`lib/codingSessionUmbrellaModel.ts:158`) is what routes a session to the
// **umbrella** workspace — the one that has a Mission lens and therefore an
// Audit tab — and it is true only for more than one execution or a resumed
// one. A single-seat session renders the solo workspace, which has no Mission.
const SECOND_COMMAND_ID = "3a1c5d70-6b21-4f88-9a0c-2e4f8b1d7c55";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const SECOND_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "fedcba9876543210",
  sessionId: "22222222-3333-4444-5555-666666666666",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const SECOND_TARGET_KEY = buildCodingSessionTargetKey(SECOND_TARGET);
const BASE_CREATED_AT = 1_800_000_000;
/**
 * Observations are stamped a few minutes before the wall clock, not at
 * `BASE_CREATED_AT`.
 *
 * The Route rail's Now rule is real time, and a sign stamped months in the
 * future would be placed past Now — correctly, and uselessly for a screenshot.
 * The governance fixture keeps its fixed base; only the 44246 events move.
 */
const OBSERVED_AT = Math.floor(Date.now() / 1_000) - 600;

const OBSERVATION_SCHEMA = "buzz-coding-session-observation/v1";
const FOLD_SCHEMA = "buzz-coding-session-observation-fold-adapter/v2";
const DISCLOSURE =
  "an observation is something its author saw, not a decision: it settles nothing, authorizes nothing and excludes nothing, and every duration in it is the author's own measurement";

function signed(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as RelayEvent;
}

function genesisEvent(): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    founderPubkey: FOUNDER_PUBKEY,
  });
  return signed(
    built.kind,
    BASE_CREATED_AT - 2,
    built.tags,
    built.content,
    FOUNDER_SECRET,
  );
}

/** Where the seeded session says it works; both null unless a spec asks. */
type SessionRefs = { projectRef: string | null; repoRef: string | null };
const NO_REFS: SessionRefs = { projectRef: null, repoRef: null };

function createAndReceiptEvents(
  genesisRef: string,
  commandId: string = COMMAND_ID,
  target: typeof TARGET = TARGET,
  refs: SessionRefs = NO_REFS,
): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId,
    projectRef: refs.projectRef,
    repoRef: refs.repoRef,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Render kind 44246",
    initialTurn: null,
  });
  return [
    signed(
      built.kind,
      BASE_CREATED_AT - 1,
      built.tags,
      built.content,
      FOUNDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT,
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

function metadataEvent(
  target: typeof TARGET = TARGET,
  targetKey: string = TARGET_KEY,
  refs: SessionRefs = NO_REFS,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    BASE_CREATED_AT,
    [
      ["h", CHANNEL_ID],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(target)],
    ],
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      projectRef: refs.projectRef,
      repoRef: refs.repoRef,
      title: "Render kind 44246",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "running",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
      sessionRef: SESSION_REF,
    }),
    PROVIDER_SECRET,
  );
}

/** One signed kind-44246 with NIP-CSOB's exact five-tag envelope. */
function observationEvent(input: {
  genesisRef: string;
  type: "checkpoint" | "gate" | "finding" | "phase";
  source: "observed" | "declared";
  assignmentRef: string | null;
  body: unknown;
  secret: Uint8Array;
  createdAt: number;
}): RelayEvent {
  return signed(
    KIND_CODING_SESSION_OBSERVATION,
    input.createdAt,
    [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["csob-v", OBSERVATION_SCHEMA],
      ["csob-genesis", input.genesisRef],
      ["csob-type", input.type],
    ],
    JSON.stringify({
      schema: OBSERVATION_SCHEMA,
      sessionRef: SESSION_REF,
      genesisRef: input.genesisRef,
      type: input.type,
      source: input.source,
      assignmentRef: input.assignmentRef,
      body: input.body,
    }),
    input.secret,
  );
}

const DANGLING_ASSIGNMENT = "cd".repeat(32);

/** The five observations this spec reads, and the fold they produce. */
export function observationFixture(genesisRef: string) {
  const declaredGate = observationEvent({
    genesisRef,
    type: "gate",
    source: "declared",
    assignmentRef: null,
    createdAt: OBSERVED_AT + 1,
    secret: SEAT_SECRET,
    body: {
      rows: [
        {
          gate: "cargo test",
          outcome: "passed",
          command: "cargo test -p beekeeper-cli",
          summary: "test result: ok. 13 passed; 0 failed",
          durationMs: 12_000,
          // The shape every gate row on the wire carried before 2026-09-03:
          // the seat's own claim, naming no commit. It must still render.
          headSha: null,
          dirty: null,
        },
      ],
    },
  });
  // Live-run finding 26, on the wire: the seat says green, the mechanism that
  // watched the command run says red. Nobody had to ask either of them.
  const observedGate = observationEvent({
    genesisRef,
    type: "gate",
    source: "observed",
    assignmentRef: null,
    createdAt: OBSERVED_AT + 2,
    secret: PROVIDER_SECRET,
    body: {
      rows: [
        {
          gate: "cargo test",
          outcome: "failed",
          command: "cargo test -p beekeeper-cli",
          summary:
            "running 2 tests\nfailures:\n  subcommand_names_are_stable\ntest result: FAILED. 0 passed; 2 failed; 0 ignored",
          durationMs: 41_000,
          // The provider resolved the seat's own HEAD when the gate closed.
          headSha: "07c470be007c470be007c470be007c470be007c4",
          dirty: false,
        },
      ],
    },
  });
  const checkpoint = observationEvent({
    genesisRef,
    type: "checkpoint",
    source: "declared",
    assignmentRef: null,
    createdAt: OBSERVED_AT + 3,
    secret: SEAT_SECRET,
    body: {
      phase: "gates",
      testsWritten: 6,
      testsRed: 6,
      testsGreen: 4,
      lastCommand: "cargo test -p beekeeper-cli",
      lastSummary: "test result: ok. 13 passed; 0 failed",
      note: null,
    },
  });
  const finding = observationEvent({
    genesisRef,
    type: "finding",
    source: "declared",
    assignmentRef: null,
    createdAt: OBSERVED_AT + 4,
    secret: SEAT_SECRET,
    body: {
      findingId: "A3",
      title:
        "the Structured tests card described the wire instead of reading it",
      disposition: "fixed",
      detail: null,
      refs: [],
      decisionRef: null,
    },
  });
  const phase = observationEvent({
    genesisRef,
    type: "phase",
    source: "declared",
    assignmentRef: DANGLING_ASSIGNMENT,
    createdAt: OBSERVED_AT + 5,
    secret: SEAT_SECRET,
    body: {
      phase: "green",
      startedAtMs: 1_756_800_000_000,
      endedAtMs: 1_756_800_180_000,
      durationMs: 180_000,
    },
  });

  const events = [declaredGate, observedGate, checkpoint, finding, phase];
  // The canonical adapter answer for exactly these five events. The real
  // adapter is exercised by `coding_session_observation_fold_tests.rs`, which
  // also generates the decoder's fixture; here it is stubbed so the spec is
  // about the screen.
  const foldResponse = {
    schema: FOLD_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: events.map((event) => event.id),
    sessionRef: SESSION_REF,
    genesisRef,
    checkpoints: [
      {
        eventId: checkpoint.id,
        authorPubkey: SEAT_PUBKEY,
        source: "declared",
        assignmentRef: null,
        phase: "gates",
        testsWritten: 6,
        testsRed: 6,
        testsGreen: 4,
        lastCommand: "cargo test -p beekeeper-cli",
        lastSummary: "test result: ok. 13 passed; 0 failed",
        note: null,
      },
    ],
    gates: [
      {
        authorPubkey: SEAT_PUBKEY,
        source: "declared",
        eventIds: [declaredGate.id],
        droppedEventIds: 0,
        assignmentRef: null,
        gate: "cargo test",
        outcome: "passed",
        command: "cargo test -p beekeeper-cli",
        summary: "test result: ok. 13 passed; 0 failed",
        durationMs: 12_000,
        // The seat's own claim names no commit — the shape every gate row on
        // the wire carried before 2026-09-03. It must render as "no commit
        // named" rather than borrowing the observed row's (L22).
        headSha: null,
        dirty: null,
      },
      {
        authorPubkey: PROVIDER_PUBKEY,
        source: "observed",
        eventIds: [observedGate.id],
        droppedEventIds: 0,
        assignmentRef: null,
        gate: "cargo test",
        outcome: "failed",
        command: "cargo test -p beekeeper-cli",
        summary:
          "running 2 tests\nfailures:\n  subcommand_names_are_stable\ntest result: FAILED. 0 passed; 2 failed; 0 ignored",
        durationMs: 41_000,
        // The provider resolved the seat's own HEAD when the gate closed.
        headSha: "07c470be007c470be007c470be007c470be007c4",
        dirty: false,
      },
    ],
    findings: [
      {
        authorPubkey: SEAT_PUBKEY,
        source: "declared",
        eventIds: [finding.id],
        droppedEventIds: 0,
        assignmentRef: null,
        findingId: "A3",
        title:
          "the Structured tests card described the wire instead of reading it",
        disposition: "fixed",
        detail: null,
        refs: [],
        decisionRef: null,
      },
    ],
    phases: [
      {
        eventId: phase.id,
        authorPubkey: SEAT_PUBKEY,
        source: "declared",
        assignmentRef: DANGLING_ASSIGNMENT,
        phase: "green",
        startedAtMs: 1_756_800_000_000,
        endedAtMs: 1_756_800_180_000,
        durationMs: 180_000,
      },
    ],
    gateStarts: [],
    gateStartStaleAfterMs: 1_800_000,
    unresolved: [{ eventId: phase.id, assignmentRef: DANGLING_ASSIGNMENT }],
    ignored: [],
    misclaimedObserved: [],
    // The provider signed the observed row, so this caller's provider set
    // honours it (REVIEW-L5 F2).
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
  return { events, foldResponse };
}

/** The fold answer for a session on which nobody has observed anything. */
export function emptyFoldResponse(genesisRef: string) {
  return {
    schema: FOLD_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: [],
    sessionRef: SESSION_REF,
    genesisRef,
    checkpoints: [],
    gates: [],
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
}

/**
 * Seed one governed session, with or without observations, and open it.
 *
 * `refs` names a project and repository for the session (SV-20's header spec
 * needs a repository so Landing can open). `foldThroughWaveBHeader` serves
 * the fold from the header lane's bridge module (`e2eBridgeWaveBHeader.ts`)
 * instead of the built-in mock: the same response, through the seam that
 * spec exercises.
 */
export async function openObservedSession(
  page: Page,
  input: {
    withObservations: boolean;
    refs?: SessionRefs;
    foldThroughWaveBHeader?: boolean;
  },
): Promise<void> {
  const genesis = genesisEvent();
  const fixture = observationFixture(genesis.id);
  const refs = input.refs ?? NO_REFS;
  const fold = input.withObservations
    ? fixture.foldResponse
    : emptyFoldResponse(genesis.id);
  // Not a field of the typed options: the Wave B seam reads its own keys.
  const waveB = input.foldThroughWaveBHeader
    ? { waveBHeader: { observationFold: fold } }
    : {};
  await installMockBridge(page, {
    ...waveB,
    codingSessionObservationFoldResponse: input.foldThroughWaveBHeader
      ? emptyFoldResponse(genesis.id)
      : fold,
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [
      { pubkey: SEAT_PUBKEY, displayName: "Bob" },
      { pubkey: PROVIDER_PUBKEY, displayName: "This computer" },
    ],
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: [
        genesis,
        ...createAndReceiptEvents(genesis.id, COMMAND_ID, TARGET, refs),
        ...createAndReceiptEvents(
          genesis.id,
          SECOND_COMMAND_ID,
          SECOND_TARGET,
          refs,
        ),
        metadataEvent(TARGET, TARGET_KEY, refs),
        metadataEvent(SECOND_TARGET, SECOND_TARGET_KEY, refs),
        ...(input.withObservations ? fixture.events : []),
      ],
    },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
}
