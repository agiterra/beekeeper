import { expect, type Page, test } from "@playwright/test";
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
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";

/**
 * SV-32 S2: a path the agent wrote is a chip on the computer that ran the
 * agent, and plain code with the reason everywhere else.
 *
 * The bridge's `codingSessionFileRefs` answers the host lookup per provider
 * session id (`src/testing/e2eBridgeFileRefs.ts`) and records every open and
 * reveal, so the spec can assert that the renderer sent the candidate as the
 * agent wrote it and never an absolute path.
 */

const SHOTS = "test-results/coding-session-file-chips";
const secret = generateSecretKey();
const signer = getPublicKey(secret);
const OTHER_PROVIDER = "cd".repeat(32);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "fedcba9876543210",
  sessionId: "5a1d2c3b-0000-4000-8000-00000000c032",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);
const CANDIDATE = "desktop/src/app/App.tsx:42";
const ANSWER =
  "The entry point is `desktop/src/app/App.tsx:42`; the remote is `origin/main` and the module is `node.meta`.";

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_200_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Find the app entry point",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "completed",
      branch: "feature/entry",
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: true,
        plan: true,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function transcript(seq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_200_000_000 + seq * 1_000,
      turnId: "turn-1",
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", targetKey],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

function events(): RelayEvent[] {
  return [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "Where does the app start?",
    }),
    transcript(2, { kind: "assistant_text", text: ANSWER }),
    transcript(3, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 4_000,
      result: "Found it.",
    }),
  ];
}

type FileRefsAnswer = {
  where: string;
  reason?: string | null;
  refs?: Record<string, unknown>;
};

const LOCAL: FileRefsAnswer = {
  where: "thisComputer",
  refs: {
    [CANDIDATE]: {
      exists: true,
      isDir: false,
      relativePath: "desktop/src/app/App.tsx",
      fullPath: "/Users/e2e/worktrees/entry/desktop/src/app/App.tsx",
      line: 42,
    },
  },
};

async function openSession(
  page: Page,
  {
    answer,
    providerPubkey,
  }: { answer: FileRefsAnswer; providerPubkey: string },
) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey: signer, label: "Chip provider" }],
    },
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey,
    },
    // Not declared on MockBridgeOptions; the bridge reads it by name.
    ...({
      codingSessionFileRefs: { bySession: { [session.sessionId]: answer } },
    } as Record<string, unknown>),
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ name, signedEvents }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { name: channelName, signedEvents: events() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  // The result body ("Found it.") differs from the answer, so it renders as
  // its own assistant row after this one: pick the answer by its text.
  const message = page
    .getByTestId("coding-session-assistant-message")
    .filter({ hasText: "The entry point is" });
  await expect(message).toContainText("The entry point is");
  return message;
}

async function fileRefCalls(page: Page) {
  return page.evaluate(
    () =>
      ((window as unknown as { __BEEKEEPER_E2E_FILE_REF_CALLS__?: unknown[] })
        .__BEEKEEPER_E2E_FILE_REF_CALLS__ ?? []) as {
        command: string;
        request: Record<string, unknown>;
      }[],
  );
}

test("a local path is a chip that opens the candidate as written", async ({
  page,
}, testInfo) => {
  const message = await openSession(page, {
    answer: LOCAL,
    providerPubkey: signer,
  });
  const chip = message.getByTestId("coding-session-file-chip");
  await expect(chip).toHaveCount(1);
  await expect(chip).toHaveText("App.tsx · L42");
  await expect(chip).toHaveAttribute("data-file-type", "react");
  // Not paths: these stay inline code, untouched.
  for (const text of ["origin/main", "node.meta"]) {
    const code = message.locator("code", { hasText: text });
    await expect(code).toHaveCount(1);
    await expect(code).not.toHaveAttribute("data-file-ref-plain", "");
  }
  await waitForAnimations(page);
  await message.screenshot({
    path: shotPath(testInfo, SHOTS, "sv32-chip-rest"),
  });

  await chip.hover();
  const tooltip = page.getByTestId("coding-session-file-chip-tooltip");
  await expect(tooltip).toContainText("desktop/src/app/App.tsx");
  await expect(tooltip).toContainText("On this computer");
  await expect(tooltip).toContainText("line 42 not selected");
  await waitForAnimations(page);
  await shotAround(
    page,
    message,
    tooltip,
    shotPath(testInfo, SHOTS, "sv32-chip-hover-tooltip"),
  );

  await page.mouse.move(0, 0);
  await chip.click({ button: "right" });
  const menu = page.locator("[data-file-chip-menu]");
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("button")).toHaveText([
    "Open",
    /Reveal in Finder|Show in Explorer|Show in file manager/,
    "Copy relative path",
    "Copy full path",
  ]);
  await waitForAnimations(page);
  await shotAround(
    page,
    message,
    menu,
    shotPath(testInfo, SHOTS, "sv32-chip-menu"),
  );
  await menu.getByRole("button", { name: /Reveal|Show in/ }).click();

  await chip.click();
  const calls = await fileRefCalls(page);
  const actions = calls.filter(
    (call) => call.command !== "coding_session_file_refs",
  );
  expect(actions.map((call) => call.command)).toEqual([
    "coding_session_reveal_file_ref",
    "coding_session_open_file_ref",
  ]);
  for (const call of actions) {
    expect(call.request.candidate).toBe(CANDIDATE);
    expect(JSON.stringify(call.request)).not.toContain("/Users/");
  }
});

test("another computer's path stays plain code with the reason", async ({
  page,
}, testInfo) => {
  const message = await openSession(page, {
    answer: {
      where: "notLocal",
      reason: "Written on another computer — open it there",
    },
    providerPubkey: OTHER_PROVIDER,
  });
  await expect(message.getByTestId("coding-session-file-chip")).toHaveCount(0);
  const plain = message.getByTestId("coding-session-file-ref-plain");
  await expect(plain).toHaveText(CANDIDATE);
  await plain.hover();
  const reason = page.getByTestId("coding-session-file-ref-reason");
  await expect(reason).toHaveText(
    "Written on another computer — open it there",
  );
  await waitForAnimations(page);
  await shotAround(
    page,
    message,
    reason,
    shotPath(testInfo, SHOTS, "sv32-remote-plain"),
  );
});

test("a worktree whose folder is gone says so", async ({ page }, testInfo) => {
  const message = await openSession(page, {
    answer: {
      where: "folderGone",
      reason: "This computer ran this agent, but its folder is gone",
    },
    providerPubkey: signer,
  });
  await expect(message.getByTestId("coding-session-file-chip")).toHaveCount(0);
  const plain = message.getByTestId("coding-session-file-ref-plain");
  await plain.hover();
  const reason = page.getByTestId("coding-session-file-ref-reason");
  await expect(reason).toHaveText(
    "This computer ran this agent, but its folder is gone",
  );
  await waitForAnimations(page);
  await shotAround(
    page,
    message,
    reason,
    shotPath(testInfo, SHOTS, "sv32-folder-gone"),
  );
});

/**
 * The message plus an overlay (tooltip or menu) portalled outside it. A
 * `locator.screenshot()` of the message alone would miss the overlay, so this
 * clips the page to the union of the two boxes.
 */
async function shotAround(
  page: Page,
  message: ReturnType<Page["getByTestId"]>,
  overlay: ReturnType<Page["locator"]>,
  path: string,
) {
  const a = await message.boundingBox();
  const b = await overlay.boundingBox();
  if (!a || !b) throw new Error("nothing to capture");
  const x = Math.max(0, Math.min(a.x, b.x) - 8);
  const y = Math.max(0, Math.min(a.y, b.y) - 8);
  const right = Math.max(a.x + a.width, b.x + b.width) + 8;
  const bottom = Math.max(a.y + a.height, b.y + b.height) + 8;
  await page.screenshot({
    path,
    clip: { x, y, width: right - x, height: bottom - y },
  });
}
