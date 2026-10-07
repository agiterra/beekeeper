import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// SV-99: a strip docked above the composer while the agent waits on its own
// work — "Waiting on 1 subagent" and what — drawn only from live evidence,
// absent for an idle session. SV-104: the running tool's label shimmers only
// while the provider is fresh; a quiet provider and reduced motion stop it.

const SHOTS = "test-results/coding-session-liveness";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "d1b2c3d4e5f60719",
  sessionId: "eeeeeeee-ffff-0000-1111-444444444444",
  generation: 1,
};
/** The lit copy that sweeps across live action text (SV-104). */
const OVERLAY = '[data-testid="coding-session-live-shimmer-overlay"]';
const SUBAGENT = "Review the backoff bounds";
const COMMAND = "cargo test -p beekeeper-core retry";

function signed(
  kind: number,
  createdAt: number,
  content: unknown,
  tags: string[][],
) {
  return finalizeEvent(
    {
      kind,
      created_at: createdAt,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(
  status: string,
  createdAt: number,
  target: typeof session = session,
  sessionRef: string | null = null,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    createdAt,
    {
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session: target,
      ...(sessionRef ? { sessionRef } : {}),
      projectRef: null,
      repoRef: null,
      title: "Liveness strip and shimmer",
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
      ["cs-target", buildCodingSessionTargetKey(target)],
      ["csm-key", codingSessionMetadataSemanticKey(target)],
    ],
  );
}

/** A live 24223 lease: the provider is reachable now, so "working" holds. */
function liveLease(target: typeof session = session): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: Math.floor(Date.now() / 1_000) - 5,
      tags: [
        ["h", channelId],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csl-command", "csl-liveness-session"],
        ["cslease-seq", "1"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target,
        state: "live",
        leaseSequence: 1,
      }),
    },
    secret,
  ) as unknown as RelayEvent;
}

function transcript(
  seq: number,
  timestampMs: number,
  turnId: string,
  item: unknown,
  target: typeof session = session,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    Math.floor(timestampMs / 1_000),
    {
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: target,
      eventSeq: seq,
      timestamp: timestampMs,
      turnId,
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(target)],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(target, seq)],
    ],
  );
}

/**
 * A working turn with one open subagent and one running command, every event
 * `ageMs` old: 0 is fresh, five minutes is a quiet provider.
 */
function workingEvents(ageMs: number): RelayEvent[] {
  const turn = "liveness-turn";
  const last = Date.now() - ageMs;
  const at = (seq: number) => last - (4 - seq) * 1_000;
  return [
    metadata("running", Math.floor(last / 1_000)),
    liveLease(),
    transcript(1, at(1), turn, {
      kind: "user_prompt",
      content: "Bound the backoff",
    }),
    transcript(2, at(2), turn, {
      kind: "tool_call",
      tool: {
        toolName: "Task",
        toolKind: "think",
        toolId: "task-1",
        input: {
          description: SUBAGENT,
          prompt: "Look at the retry loop",
          subagent_type: "Explore",
        },
      },
    }),
    transcript(3, at(3), turn, {
      kind: "assistant_text",
      text: "Reading the retry loop.",
      parentToolId: "task-1",
    }),
    transcript(4, at(4), turn, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolKind: "execute",
        toolId: "bash-1",
        input: { command: COMMAND },
      },
    }),
  ];
}

/** A finished turn and nothing outstanding. */
function idleEvents(): RelayEvent[] {
  const turn = "idle-turn";
  const now = Date.now();
  return [
    metadata("idle", Math.floor(now / 1_000)),
    liveLease(),
    transcript(1, now - 3_000, turn, {
      kind: "user_prompt",
      content: "Say hi",
    }),
    transcript(2, now - 2_000, turn, {
      kind: "assistant_text",
      text: "Hi — nothing left running.",
    }),
    transcript(3, now - 1_000, turn, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 2_000,
      result: "Done.",
      costUsd: 0.01,
    }),
  ];
}

async function openSession(page: Page, events: RelayEvent[], text: string) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Liveness provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText(text);
  return workspace;
}

const hashes = new Map<string, string>();

async function shoot(page: Page, name: string, locator: Locator) {
  await expect(locator).toBeVisible();
  await page.mouse.move(2, 2);
  await waitForAnimations(page);
  const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
  hashes.set(name, createHash("sha256").update(png).digest("hex"));
}

test.describe.configure({ mode: "serial" });

test("SV-99/SV-104: a fresh working turn shows the strip and shimmers its running tool", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await openSession(page, workingEvents(0), "Reading the retry loop.");

  const dock = page.getByTestId("coding-session-composer-dock");
  const strip = dock.getByTestId("coding-session-waiting-strip");
  await expect(strip).toHaveCount(1);
  await expect(
    strip.getByTestId("coding-session-waiting-strip-headline"),
  ).toHaveText("Waiting on 1 subagent");
  await expect(
    strip.getByTestId("coding-session-waiting-strip-brief"),
  ).toHaveText(SUBAGENT);
  await expect(strip).toHaveAttribute("data-pulse", "on");
  await expect(strip).toHaveAttribute("data-quiet", "false");
  // No Stop of its own: the composer below carries the session's Stop.
  await expect(strip.getByRole("button", { name: "Stop" })).toHaveCount(0);
  await shoot(page, "SV99-strip-fresh", strip);

  // The info affordance lists each line with the evidence behind it.
  await strip.getByTestId("coding-session-waiting-strip-info").click();
  const details = page.getByTestId("coding-session-waiting-strip-details");
  await expect(details).toContainText(`Subagent: ${SUBAGENT}`);
  await expect(details).toContainText("its turn is live");
  await shoot(page, "SV99-strip-details", details);
  await page.keyboard.press("Escape");

  // SV-104: the running command's label shimmers while the provider is fresh.
  const running = page
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: COMMAND });
  await expect(running).toHaveCount(1);
  await expect(running.locator('[data-live-shimmer="on"]')).toHaveCount(1);
  await expect(running.locator(OVERLAY)).toHaveCount(1);
  await shoot(page, "SV104-tool-shimmer-fresh", running);
});

test("SV-99/SV-104: a quiet provider holds still and says how long", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await openSession(
    page,
    workingEvents(5 * 60_000 + 10_000),
    "Reading the retry loop.",
  );
  const strip = page.getByTestId("coding-session-waiting-strip");
  await expect(strip).toHaveAttribute("data-pulse", "off");
  await expect(strip).toHaveAttribute("data-quiet", "true");
  await expect(
    strip.getByTestId("coding-session-waiting-strip-quiet"),
  ).toContainText("no update for 5m");
  await shoot(page, "SV99-strip-quiet", strip);

  const running = page
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: COMMAND });
  await expect(running.locator(OVERLAY)).toHaveCount(0);
  await expect(page.getByTestId("coding-session-working")).toContainText(
    "no update for 5m",
  );
  await expect(
    page.getByTestId("coding-session-working").locator(OVERLAY),
  ).toHaveCount(0);
});

test("SV-104: reduced motion stops the shimmer and the pulse", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openSession(page, workingEvents(0), "Reading the retry loop.");
  const strip = page.getByTestId("coding-session-waiting-strip");
  await expect(strip).toHaveAttribute("data-pulse", "off");
  const running = page
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: COMMAND });
  await expect(running).toHaveCount(1);
  await expect(running.locator(OVERLAY)).toHaveCount(0);
});

test("SV-99: an idle session with nothing running shows no strip", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await openSession(page, idleEvents(), "nothing left running");
  await expect(page.getByTestId("coding-session-composer-dock")).toBeVisible();
  await expect(page.getByTestId("coding-session-waiting-strip")).toHaveCount(0);
  await expect(page.locator(OVERLAY)).toHaveCount(0);
});

/**
 * A team session: two executions under one session reference, the first
 * finished, the second fresh and running a command. Two executions route to
 * the umbrella surface, and its Mission lens bundles the running call.
 */
const SESSION_REF = "7d1c9a40-3b2e-4f5a-9c8d-1e2f3a4b5c6d";
const lead = { ...session, sessionId: "eeeeeeee-ffff-0000-1111-555555555555" };
const builder = {
  ...session,
  sessionId: "eeeeeeee-ffff-0000-1111-666666666666",
};
const TEAM_COMMAND = "python3 -c 'import time; time.sleep(90)'";

function teamEvents(): RelayEvent[] {
  const now = Date.now();
  const sec = (ms: number) => Math.floor(ms / 1_000);
  return [
    metadata("idle", sec(now - 60_000), lead, SESSION_REF),
    liveLease(lead),
    transcript(
      1,
      now - 62_000,
      "lead-turn",
      {
        kind: "user_prompt",
        content: "Plan the backoff work",
      },
      lead,
    ),
    transcript(
      2,
      now - 61_000,
      "lead-turn",
      {
        kind: "assistant_text",
        text: "Handing the build to the builder.",
      },
      lead,
    ),
    transcript(
      3,
      now - 60_000,
      "lead-turn",
      {
        kind: "result",
        subtype: "success",
        isError: false,
        durationMs: 2_000,
        result: "Done.",
        costUsd: 0.01,
      },
      lead,
    ),
    metadata("running", sec(now - 3_000), builder, SESSION_REF),
    liveLease(builder),
    transcript(
      1,
      now - 3_000,
      "builder-turn",
      {
        kind: "user_prompt",
        content: "Build the backoff",
      },
      builder,
    ),
    transcript(
      2,
      now - 2_000,
      "builder-turn",
      {
        kind: "assistant_text",
        text: "Running the slow check.",
      },
      builder,
    ),
    transcript(
      3,
      now - 1_000,
      "builder-turn",
      {
        kind: "tool_call",
        tool: {
          toolName: "Bash",
          toolKind: "execute",
          toolId: "bash-team",
          input: { command: TEAM_COMMAND },
        },
      },
      builder,
    ),
  ];
}

test("SV-104: a team session's running call shimmers in both lenses", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Liveness provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: teamEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute(
    "aria-label",
    /Coding sessions \(\d+\)/,
    {
      timeout: 15_000,
    },
  );
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  const timeline = page.getByTestId("coding-session-umbrella-timeline");
  await expect(timeline).toContainText("Running the slow check.", {
    timeout: 15_000,
  });

  // Conversation lens: the builder's turn, its running call, its working line.
  const running = timeline
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: "time.sleep(90)" });
  await expect(running).toHaveCount(1);
  await expect(running.locator('[data-live-shimmer="on"]')).toHaveCount(1);
  await expect(running.locator(OVERLAY)).toHaveCount(1);
  await expect(
    timeline.getByTestId("coding-session-working").locator(OVERLAY),
  ).toHaveCount(1);
  await shoot(page, "SV104-team-tool-shimmer", running);

  // Mission lens: the running call moves into the execution bundle, which
  // is its own transcript and must be told the last event time too.
  await page.getByRole("button", { name: "Mission lens" }).click();
  const toggle = page
    .getByTestId("coding-session-mission-execution-bundle-toggle")
    .first();
  await expect(toggle).toBeVisible({ timeout: 15_000 });
  if ((await toggle.getAttribute("aria-expanded")) !== "true") {
    await toggle.click();
  }
  const bundled = page
    .getByTestId("coding-session-mission-execution-bundle")
    .getByTestId("coding-session-active-tool")
    .filter({ hasText: "time.sleep(90)" });
  await expect(bundled).toHaveCount(1);
  await expect(bundled.locator('[data-live-shimmer="on"]')).toHaveCount(1);
});

test("liveness shots are hash-distinct", () => {
  test.skip(hashes.size === 0, "runs after the shot tests in this file");
  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
  expect(hashes.size).toBe(5);
});
