import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";

/**
 * SV-36 S5 — a paragraph-streamed answer reads as one message, and says it is
 * still being written only while a live lease vouches for its producer
 * (`conformance/transcript-prose-join/CONTRACT.md` rules 3 and 7).
 *
 * The provider is real signed facts, not a stub: a create command and its
 * receipt (so the coordination fold accepts the generation), running
 * metadata, an unexpired kind:24223 lease, and three kind:44225
 * `assistant_text` pieces with no `result`. Then the `result` arrives.
 * Lease and authority shape: `coding-session-reachability.spec.ts`.
 *
 * Run in the solo route (one execution) and in the umbrella route (two
 * executions under one sessionRef). In the umbrella, the sibling execution
 * has an open turn too but holds no lease: its answer must not read
 * "Writing…".
 */

const SHOTS = "test-results/coding-session-paragraph-streaming";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const OPERATOR_SECRET = generateSecretKey();
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const CHANNEL_NAME = "engineering";
const SESSION_REF = "5b3f0e2a-6c1d-4e8f-9a7b-2d4c6e8f0a1b";
const COMMAND_ID = `csl-${SESSION_REF}`;
const WRITER = {
  driver: "claude-agent-acp",
  instanceId: "sv36-paragraph-instance",
  sessionId: "5b3f0e2a-0000-4000-8000-000000000001",
  generation: 1,
};
const SIBLING = {
  ...WRITER,
  sessionId: "5b3f0e2a-0000-4000-8000-000000000002",
};
const TURN = "sv36-turn";
const PIECES = [
  "The reconnect loop retried without a bound.\n\n",
  "I capped it at five attempts with jittered backoff.\n\n",
  "Next I am checking the recovery path under load.",
];
const SIBLING_TEXT = "Sibling draft that nobody is vouching for.";

type Target = typeof WRITER;

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

function provider(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: unknown,
): RelayEvent {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: [["h", CHANNEL_ID], ...tags],
      content: JSON.stringify(content),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/** The founder's create command and the provider's receipt for it. */
function authorityEvents(): RelayEvent[] {
  const command = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    providerInstanceRef: WRITER.instanceId,
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Bound the reconnect loop",
    initialTurn: null,
  });
  return [
    finalizeEvent(
      {
        kind: command.kind,
        created_at: nowSeconds() - 120,
        tags: command.tags,
        content: command.content,
      },
      OPERATOR_SECRET,
    ) as unknown as RelayEvent,
    provider(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      nowSeconds() - 110,
      [
        ["cslr-v", "cslr1-1"],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      {
        schema: "buzz-coding-session-lifecycle-receipt/v1",
        commandId: COMMAND_ID,
        status: "created",
        session: WRITER,
        error: null,
      },
    ),
  ];
}

function metadata(target: Target, createdAt: number): RelayEvent {
  return provider(
    KIND_CODING_SESSION_METADATA,
    createdAt,
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(target)],
      ["csm-key", codingSessionMetadataSemanticKey(target)],
    ],
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      sessionRef: SESSION_REF,
      projectRef: null,
      repoRef: null,
      title: "Bound the reconnect loop",
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
        plan: false,
      },
    },
  );
}

/** An unexpired `live` lease on the writer's exact generation. */
function liveLease(): RelayEvent {
  return provider(
    KIND_CODING_SESSION_LEASE,
    nowSeconds() - 5,
    [
      ["cslease-v", "cslease1-1"],
      ["cs-target", buildCodingSessionTargetKey(WRITER)],
      ["csl-command", COMMAND_ID],
      ["cslease-seq", "1"],
    ],
    {
      schema: "buzz-coding-session-lease/v1",
      target: WRITER,
      state: "live",
      leaseSequence: 1,
    },
  );
}

function transcript(
  target: Target,
  seq: number,
  item: unknown,
  atMs: number,
): RelayEvent {
  return provider(
    KIND_CODING_SESSION_TRANSCRIPT,
    Math.floor(atMs / 1_000),
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(target)],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(target, seq)],
    ],
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: target,
      eventSeq: seq,
      timestamp: atMs,
      turnId: TURN,
      item,
    },
  );
}

/** The writer's open turn: a prompt, then three paragraph pieces, no result. */
function writerEvents(): RelayEvent[] {
  const start = Date.now() - 10_000;
  return [
    metadata(WRITER, nowSeconds() - 30),
    liveLease(),
    transcript(
      WRITER,
      1,
      { kind: "user_prompt", content: "Bound the reconnect loop." },
      start,
    ),
    ...PIECES.map((text, index) =>
      transcript(
        WRITER,
        index + 2,
        { kind: "assistant_text", text },
        start + (index + 1) * 1_000,
      ),
    ),
  ];
}

/** The sibling: an open turn of its own, no create proof and no lease. */
function siblingEvents(): RelayEvent[] {
  const start = Date.now() - 20_000;
  return [
    metadata(SIBLING, nowSeconds() - 40),
    transcript(
      SIBLING,
      1,
      { kind: "user_prompt", content: "Draft the sibling note." },
      start,
    ),
    transcript(
      SIBLING,
      2,
      { kind: "assistant_text", text: SIBLING_TEXT },
      start + 1_000,
    ),
  ];
}

function resultEvent(): RelayEvent {
  return transcript(
    WRITER,
    PIECES.length + 2,
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 9_000,
      result: PIECES.join(""),
      costUsd: 0.02,
    },
    Date.now(),
  );
}

async function seed(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, seeds }) => {
      const signed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!signed) throw new Error("mock signed-event seam is missing");
      for (const event of seeds as never[]) signed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, seeds: events as never },
  );
}

async function openSession(page: Page, events: RelayEvent[]) {
  await page.setViewportSize({ width: 1280, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "Another computer" },
      ],
    },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await seed(page, events);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute(
    "aria-label",
    /Coding sessions \(\d\)/,
    {
      timeout: 15_000,
    },
  );
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
}

/** The writer's answer: one row, every paragraph, in order. */
async function expectOneJoinedAnswer(scope: Locator) {
  const answer = scope
    .getByTestId("coding-session-assistant-message")
    .filter({ hasText: "The reconnect loop retried" });
  await expect(answer).toHaveCount(1, { timeout: 15_000 });
  for (const piece of PIECES) await expect(answer).toContainText(piece.trim());
  return answer;
}

async function capture(locator: Locator, name: string): Promise<string> {
  await waitForAnimations(locator.page());
  const png = await locator.screenshot({
    path: shotPath(test.info(), SHOTS, name),
  });
  return createHash("sha256").update(png).digest("hex");
}

async function arrivingThenSettled(page: Page, scope: Locator, prefix: string) {
  const answer = await expectOneJoinedAnswer(scope);
  await expect(
    answer.getByTestId("coding-session-assistant-writing"),
  ).toHaveText("Writing…");
  // `has` is resolved inside each turn, so it must not start from `scope`
  // (that would look for a nested `body` / timeline): root it at the page.
  const turn = scope.getByTestId("coding-session-turn").filter({
    has: page
      .getByTestId("coding-session-assistant-message")
      .filter({ hasText: "The reconnect loop retried" }),
  });
  await expect(turn.getByTestId("coding-session-turn-copy")).toHaveCount(0);
  const arriving = await capture(turn, `${prefix}sv36-arriving`);

  await seed(page, [resultEvent()]);
  await expect(turn.getByTestId("coding-session-turn-copy")).toBeVisible({
    timeout: 15_000,
  });
  await expect(
    scope.getByTestId("coding-session-assistant-writing"),
  ).toHaveCount(0);
  await expectOneJoinedAnswer(scope);
  const settled = await capture(turn, `${prefix}sv36-settled`);
  expect(settled, "arriving and settled are different pixels").not.toBe(
    arriving,
  );
}

test("solo: three paragraph pieces read as one answer, Writing… until the result", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await openSession(page, [...authorityEvents(), ...writerEvents()]);
  await expect(page.getByTestId("coding-session-header")).toBeVisible({
    timeout: 15_000,
  });
  await arrivingThenSettled(page, page.locator("body"), "");
});

test("umbrella: only the execution with a live lease reads Writing…", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await openSession(page, [
    ...authorityEvents(),
    ...writerEvents(),
    ...siblingEvents(),
  ]);
  const timeline = page.getByTestId("coding-session-umbrella-timeline");
  await expect(timeline).toBeVisible({ timeout: 15_000 });
  const sibling = timeline
    .getByTestId("coding-session-assistant-message")
    .filter({ hasText: SIBLING_TEXT });
  await expect(sibling).toHaveCount(1, { timeout: 15_000 });
  await expect(
    sibling.getByTestId("coding-session-assistant-writing"),
  ).toHaveCount(0);
  await arrivingThenSettled(page, timeline, "umbrella-");
  await expect(
    sibling.getByTestId("coding-session-assistant-writing"),
  ).toHaveCount(0);
});
