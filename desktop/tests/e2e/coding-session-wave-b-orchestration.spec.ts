import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Session-view parity Wave B, lane B6: the Agents surface's orchestration
// view (SV-40). A three-phase mission — Builder ●● › Gate ● › Reviewer ● —
// seeded as signed kind-44223 metadata and kind-44225 transcripts, the same
// relay events a teammate's client reads. Kind 44200 is not seeded: it is
// encrypted to the agent's owner, carries no channel, and the coding-session
// provider never publishes it, so the view reads tokens from the signed
// transcript's turn results instead. Every shot is scoped to
// its subject and the set is gated on distinct hashes.

const SHOTS = "test-results/session-parity-b";
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "b6b6b6b6-0000-4000-8000-000000000040";

function hexToBytes(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

type SeatStatus = "running" | "completed" | "failed";

type Seat = {
  title: string;
  role: string;
  model: string;
  status: SeatStatus;
  secret: Uint8Array;
  actor: string;
  target: CodingSessionCommandTarget;
  /** Transcript items in order; `turnId` overrides the first turn. */
  items: ReadonlyArray<{ turnId?: string; item: unknown }>;
};

// Pinned provider keys, one per seat, so the run is reproducible.
const SECRETS = ["b6b6b6b6", "b6b6b6b7", "b6b6b6b8", "b6b6b6b9"].map((seed) =>
  hexToBytes(seed.repeat(8)),
);
// One seat actor per seat. A metadata `role` travels with its `agentRef`: a
// role without an actor is malformed and the whole 44223 is refused (the
// Rust decoder agrees), which once dropped every `sessionRef` here and split
// the mission into four unrelated sessions (SV-62).
const ACTORS = ["c6c6c6c6", "c6c6c6c7", "c6c6c6c8", "c6c6c6c9"].map((seed) =>
  getPublicKey(hexToBytes(seed.repeat(8))),
);

function seatTarget(instanceId: string, index: number) {
  return {
    driver: "claude-agent-acp",
    instanceId,
    sessionId: `b6000000-0000-4000-8000-00000000000${index}`,
    generation: 1,
  } satisfies CodingSessionCommandTarget;
}

const call = (toolId: string, toolName: string, input: unknown = {}) => ({
  item: {
    kind: "tool_call",
    tool: { toolName, toolKind: "execute", toolId, input },
  },
});
const answer = (toolId: string, toolName: string, content = "ok") => ({
  item: { kind: "tool_result", toolId, toolName, content, isError: false },
});
const turnResult = (usage?: Record<string, number>) => ({
  item: {
    kind: "result",
    subtype: "success",
    isError: false,
    durationMs: 4_000,
    result: "Done.",
    ...(usage ? { usage } : {}),
  },
});

const SEATS: readonly Seat[] = [
  {
    title: "build r1",
    role: "builder",
    model: "claude-opus-5-5[1m]",
    status: "completed",
    secret: SECRETS[0],
    actor: ACTORS[0],
    target: seatTarget("b6-build-1", 1),
    items: [
      call("task-1", "Task", {
        description: "Survey agent memory papers",
        prompt: "Read the literature",
        subagent_type: "general-purpose",
      }),
      answer(
        "task-1",
        "Task",
        "I found measured numbers for all five of your questions.\nDetails follow.",
      ),
      call("edit-1", "Edit", { file_path: "src/a.ts" }),
      answer("edit-1", "Edit"),
      turnResult({ inputTokens: 150_000, outputTokens: 9_000 }),
    ],
  },
  {
    title: "build r2",
    role: "builder",
    model: "claude-opus-5-5[1m]",
    status: "running",
    secret: SECRETS[1],
    actor: ACTORS[1],
    target: seatTarget("b6-build-2", 2),
    items: [
      call("read-1", "Read", { file_path: "src/b.ts" }),
      answer("read-1", "Read"),
      turnResult({ inputTokens: 100_000, outputTokens: 5_000 }),
      { turnId: "turn-2", item: { kind: "user_prompt", content: "Continue." } },
      { turnId: "turn-2", ...call("bash-1", "Bash", { command: "just ci" }) },
    ],
  },
  {
    title: "gate r1",
    role: "gate",
    model: "gpt-6",
    status: "running",
    secret: SECRETS[2],
    actor: ACTORS[2],
    target: seatTarget("b6-gate-1", 3),
    items: [call("out-1", "StructuredOutput", { schema: "gate" })],
  },
  {
    title: "review r1",
    role: "reviewer",
    model: "claude-sonnet-5-5",
    status: "failed",
    secret: SECRETS[3],
    actor: ACTORS[3],
    target: seatTarget("b6-review-1", 4),
    items: [
      call("grep-1", "Grep", { pattern: "TODO" }),
      answer("grep-1", "Grep"),
      turnResult(),
    ],
  },
];

function metadata(seat: Seat, createdAt: number): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(seat.target)],
        ["csm-key", codingSessionMetadataSemanticKey(seat.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: seat.target,
        projectRef: null,
        repoRef: null,
        title: seat.title,
        agentRef: seat.actor,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: seat.model,
        status: seat.status,
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: false,
          plan: false,
        },
        sessionRef: SESSION_REF,
        role: seat.role,
      }),
    },
    seat.secret,
  ) as unknown as RelayEvent;
}

function transcript(seat: Seat, nowSeconds: number): RelayEvent[] {
  return seat.items.map((entry, index) => {
    const eventSeq = index + 1;
    const createdAt = nowSeconds - 60 + index;
    return finalizeEvent(
      {
        kind: KIND_CODING_SESSION_TRANSCRIPT,
        created_at: createdAt,
        tags: [
          ["h", CHANNEL_ID],
          ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(seat.target)],
          ["cst-seq", String(eventSeq)],
          [
            "cst-key",
            codingSessionTranscriptSemanticKey(seat.target, eventSeq),
          ],
        ],
        content: JSON.stringify({
          schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
          session: seat.target,
          eventSeq,
          timestamp: createdAt * 1_000,
          turnId: entry.turnId ?? "turn-1",
          item: entry.item,
        }),
      },
      seat.secret,
    ) as unknown as RelayEvent;
  });
}

function missionEvents(): RelayEvent[] {
  const now = Math.floor(Date.now() / 1_000);
  // Seats act in hire order, ten seconds apart. With no lifecycle create to
  // date a hire, the umbrella orders executions by their catalog times, so a
  // seat that went quiet early would otherwise lead the pipeline.
  return SEATS.flatMap((seat, index) => [
    metadata(seat, now - 90 + index),
    // The last seat's last item is `now - 28`: never in the future.
    ...transcript(seat, now + index * 10),
  ]);
}

async function openAgentsSurface(page: Page): Promise<Locator> {
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: missionEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({
    timeout: 15_000,
  });
  const orchestration = page.getByTestId("coding-session-agents-orchestration");
  // A wide multi-execution view opens Agents by itself; give it the chance
  // before reaching for the launcher, so the chord never closes a panel that
  // was about to show the view.
  const opened = await orchestration
    .waitFor({ state: "visible", timeout: 5_000 })
    .then(
      () => true,
      () => false,
    );
  if (!opened) {
    const launcher = page.getByTestId("coding-session-surface-launcher");
    if (!(await launcher.isVisible())) {
      await page.keyboard.press("ControlOrMeta+Alt+KeyB");
    }
    await page
      .getByTestId("coding-session-surface-launcher-row-agents")
      .click();
  }
  await expect(orchestration).toBeVisible();
  return orchestration;
}

test("SV-40: a mission's phase pipeline, an expanded phase and the direct spawns", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const hashes = new Map<string, string>();
  const shoot = async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };

  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": SEATS.map((seat) => ({
        pubkey: getPublicKey(seat.secret),
        label: `${seat.title} provider`,
      })),
    },
  });
  const orchestration = await openAgentsSurface(page);

  // SV-40 pipeline: one card, N/M settled over executions whose newest state
  // is terminal (the completed builder and the failed reviewer), the failure
  // counted in its own word.
  const card = orchestration.getByTestId("coding-session-agents-workflow-card");
  await expect(card).toHaveCount(1);
  await expect(
    card.getByTestId("coding-session-agents-workflow-settled"),
  ).toHaveText("2/4 settled · 1 failed");
  const pipeline = card.getByTestId("coding-session-agents-phase-pipeline");
  await expect(pipeline.locator("li")).toHaveCount(3);
  await expect(pipeline.locator("li").nth(0)).toContainText("Builder");
  await expect(pipeline.locator("li").nth(1)).toContainText("Gate");
  await expect(pipeline.locator("li").nth(2)).toContainText("Reviewer");
  const builderChip = card.getByTestId(
    "coding-session-agents-phase-chip-role:builder",
  );
  await expect(
    builderChip.getByTestId("coding-session-agents-dot"),
  ).toHaveCount(2);
  await expect(
    builderChip.locator(
      '[data-testid="coding-session-agents-dot"][data-state="done"]',
    ),
  ).toHaveCount(1);
  await expect(
    builderChip.locator(
      '[data-testid="coding-session-agents-dot"][data-state="running"]',
    ),
  ).toHaveCount(1);
  const reviewerChip = card.getByTestId(
    "coding-session-agents-phase-chip-role:reviewer",
  );
  await expect(
    reviewerChip.locator(
      '[data-testid="coding-session-agents-dot"][data-state="failed"]',
    ),
  ).toHaveCount(1);
  // A phase whose only seat failed is never a green check.
  await expect(reviewerChip).toHaveAttribute("data-state", "failed");
  await expect(reviewerChip).not.toContainText("✓");
  // Collapse the running phases so the card shot is the pipeline at rest.
  for (const key of ["role:builder", "role:gate"]) {
    const toggle = card.getByTestId(
      `coding-session-agents-phase-toggle-${key}`,
    );
    if ((await toggle.getAttribute("aria-expanded")) === "true")
      await toggle.click();
  }
  await shoot("SV40-workflow", card);

  // SV-40 expand: the builder phase lists each agent with its current tool,
  // model, tokens and tool count; the gate's missing usage reads "not reported".
  const builderToggle = card.getByTestId(
    "coding-session-agents-phase-toggle-role:builder",
  );
  await builderToggle.click();
  await expect(builderToggle).toHaveAttribute("aria-expanded", "true");
  const rows = card.getByTestId("coding-session-agents-agent-row");
  const running = rows.filter({ hasText: "build r2" });
  await expect(
    running.getByTestId("coding-session-agents-agent-tool"),
  ).toHaveText("▸ Bash");
  await expect(
    running.getByTestId("coding-session-agents-agent-meta"),
  ).toContainText("claude-opus-5-5[1m]");
  await expect(
    running.getByTestId("coding-session-agents-agent-meta"),
  ).toContainText("105k tok");
  await expect(
    running.getByTestId("coding-session-agents-agent-meta"),
  ).toContainText("2 tools");
  await expect(
    rows
      .filter({ hasText: "build r1" })
      .getByTestId("coding-session-agents-agent-meta"),
  ).toContainText("159k tok");
  const gateToggle = card.getByTestId(
    "coding-session-agents-phase-toggle-role:gate",
  );
  if ((await gateToggle.getAttribute("aria-expanded")) !== "true")
    await gateToggle.click();
  const gate = rows.filter({ hasText: "gate r1" });
  await expect(gate.getByTestId("coding-session-agents-agent-tool")).toHaveText(
    "▸ StructuredOutput",
  );
  await expect(
    gate.getByTestId("coding-session-agents-agent-meta"),
  ).toContainText("tokens not reported");
  await shoot("SV40-phase-expanded", card);

  // SV-40 direct spawns: the Task subagent with its duration and the first
  // line of its result.
  const spawns = orchestration.getByTestId(
    "coding-session-agents-direct-spawns",
  );
  const spawn = spawns.getByTestId("coding-session-agents-spawn-row");
  await expect(spawn).toHaveCount(1);
  await expect(spawn).toContainText("Survey agent memory papers");
  await expect(spawn).toContainText("general-purpose");
  await expect(
    spawn.getByTestId("coding-session-agents-spawn-preview"),
  ).toHaveText("I found measured numbers for all five of your questions.");
  await expect(
    spawn.getByTestId("coding-session-agents-spawn-duration"),
  ).toBeVisible();
  await spawns.scrollIntoViewIfNeeded();
  await shoot("SV40-direct-spawns", spawns);

  // Every shot captured a distinct state.
  const values = [...hashes.values()];
  expect(new Set(values).size, JSON.stringify([...hashes])).toBe(values.length);
});
