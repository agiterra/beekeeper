import { expect, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
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

// SV-16/SV-17 moved every boundary row out of the transcript and onto the
// composer's sandbox chip. The umbrella (mission) view renders the same
// transcript with its own composer, so it must carry the chip too: a teammate
// on another machine watching a seat that runs with full access has no other
// place to learn it. This spec signs a resumed execution — two generations,
// which routes to the umbrella surface — whose running generation discloses
// full access, exactly as `execution_scope::boundary_status_item` does.

const SHOTS = "test-results/coding-session-umbrella-sandbox";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const base = {
  driver: "claude-agent-acp",
  instanceId: "0fedcba987654321",
  sessionId: "cccccccc-dddd-eeee-ffff-000000000000",
};
const FULL_ACCESS_TEXT = "Sandbox off — this session was granted full access";

function generation(n: number) {
  return { ...base, generation: n };
}

function signed(kind: number, at: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_300_000 + at,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(n: number, status: string): RelayEvent {
  const session = generation(n);
  return signed(
    KIND_CODING_SESSION_METADATA,
    n * 10,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Ship the reconnect fix",
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
}

function transcript(
  n: number,
  seq: number,
  turnId: string | null,
  item: unknown,
): RelayEvent {
  const session = generation(n);
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    n * 10 + seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: (1_800_300_000 + n * 10 + seq) * 1_000,
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
}

function events(): RelayEvent[] {
  return [
    metadata(1, "completed"),
    transcript(1, 1, null, {
      kind: "status",
      status: "execution_boundary_enforced",
      reason: "macos-seatbelt",
    }),
    transcript(1, 2, "turn-1", {
      kind: "user_prompt",
      content: "Fix the reconnect bug.",
    }),
    transcript(1, 3, "turn-1", {
      kind: "assistant_text",
      text: "Reconnect now recovers cleanly.",
    }),
    metadata(2, "running"),
    // The resumed generation was granted full access: the newest disclosure
    // is the one that describes the agent running now.
    transcript(2, 1, null, {
      kind: "status",
      status: "execution_boundary_not_enforced",
      reason: "full-access",
    }),
    transcript(2, 2, "turn-2", {
      kind: "user_prompt",
      content: "Now run the full suite.",
    }),
    transcript(2, 3, "turn-2", {
      kind: "assistant_text",
      text: "Running the full suite outside the sandbox.",
    }),
  ];
}

test("the umbrella view keeps a full-access seat's warning on its composer", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Another computer" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();

  const workspace = page.getByTestId("coding-session-umbrella-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  const timeline = page.getByTestId("coding-session-umbrella-timeline");
  await expect(timeline).toContainText(
    "Running the full suite outside the sandbox.",
  );
  // The boundary rows left the transcript...
  await expect(timeline).not.toContainText("Project boundary");
  await expect(timeline).not.toContainText(FULL_ACCESS_TEXT);

  // ...so the composer is where full access must be said, in the warning
  // tone, on the chip itself — and it describes the running generation,
  // not the sandboxed one before it.
  const composer = page.getByTestId("coding-session-umbrella-composer");
  const chip = composer.getByTestId("coding-session-control-sandbox");
  await expect(chip).toHaveCount(1);
  await expect(chip).toContainText("Full access");
  await expect(chip).toHaveAttribute("data-tone", "warning");
  await waitForAnimations(page);
  await composer.screenshot({ path: `${SHOTS}/01-umbrella-full-access.png` });

  await chip.click();
  await expect(
    page.getByTestId("coding-session-sandbox-boundary"),
  ).toContainText(FULL_ACCESS_TEXT);
  // This computer does not run the agent, so the chip offers no toggle.
  await expect(
    page.getByTestId("coding-session-sandbox-full-access-toggle"),
  ).toHaveCount(0);
});
