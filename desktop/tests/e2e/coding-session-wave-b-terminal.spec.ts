import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { expect, test, type Locator, type Page } from "@playwright/test";
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

// Session-view parity Wave B, lane B4: the session terminal drawer (SV-25)
// and the Terminal badge (SV-22). The local half opens, splits, manages,
// closes and resizes terminals in the session's tree; the remote half is a
// teammate's view — no tree, no "New terminal", the owner's shared terminal
// watched read-only and labelled by owner and liveness, never by host.
// Every shot is scoped to its subject; the last test gates on distinct
// hashes across the whole set.

test.describe.configure({ mode: "serial" });

const SHOTS = "test-results/session-parity-b";
const SHOT_NAMES = [
  "SV25-drawer",
  "SV25-controls",
  "SV25-split",
  "SV25-manager",
  "SV22-terminal-running",
  "SV25-remote-no-tree",
  "SV25-watching-teammate",
] as const;

const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const projectRef = `30621:${pubkey}:wave-b-terminal`;
const session = {
  driver: "claude-agent-acp",
  instanceId: "b4b4b4b4b4b4b4b4",
  sessionId: "b4000000-0000-4000-8000-000000000025",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_700_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function events(): RelayEvent[] {
  const metadata = signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef,
      repoRef: null,
      title: "Terminal drawer",
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
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
  const transcript = (seq: number, item: unknown) =>
    signed(
      KIND_CODING_SESSION_TRANSCRIPT,
      seq,
      {
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session,
        eventSeq: seq,
        timestamp: 1_800_700_000_000 + seq * 1_000,
        turnId: "terminal-turn",
        item,
      },
      [
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", targetKey],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
      ],
    );
  return [
    metadata,
    transcript(1, {
      kind: "user_prompt",
      content: "Run the reconnect tests and tell me what fails.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "Two reconnect tests fail on a stale retry counter.",
    }),
    transcript(3, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 9_000,
      result: "Found the failing tests.",
    }),
  ];
}

/** The terminal mock's live state, declared before the app loads. */
async function declareTerminalMock(page: Page, tree: "local" | "elsewhere") {
  await page.addInitScript((value) => {
    window.__BUZZ_E2E_WAVE_B_TERMINAL__ = {
      tree: value,
      runningShellIds: [],
      announces: [],
    };
  }, tree);
}

async function openSession(page: Page): Promise<Locator> {
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
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Terminal drawer");
  return workspace;
}

/** Open the drawer with ⌘J unless a stored panel state already has it open. */
async function ensureDrawerOpen(page: Page) {
  const host = page.getByTestId("coding-session-drawer-host");
  try {
    await host.waitFor({ state: "visible", timeout: 2_000 });
  } catch {
    await page.keyboard.press("ControlOrMeta+KeyJ");
  }
  await expect(host).toBeVisible();
}

async function shoot(page: Page, name: string, locator: Locator) {
  await expect(locator).toBeVisible();
  await waitForAnimations(page);
  await locator.screenshot({ path: `${SHOTS}/${name}.png` });
}

async function rest(page: Page) {
  await page.mouse.move(2, 2);
  await page.mouse.move(3, 3);
}

async function setRunning(page: Page, ids: string[]) {
  await page.evaluate((running) => {
    const state = window.__BUZZ_E2E_WAVE_B_TERMINAL__;
    if (!state) throw new Error("terminal mock is missing");
    state.runningShellIds = running;
  }, ids);
}

test("SV-25 and SV-22: the drawer opens in the session's tree, splits, manages, closes and keeps its height", async ({
  page,
}) => {
  test.setTimeout(150_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await declareTerminalMock(page, "local");
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: pubkey,
    },
  });
  await openSession(page);
  const transcriptPane = page.getByTestId("coding-session-transcript-pane");

  // Opening: ⌘J opens the drawer under the composer, in the session's tree.
  await page.keyboard.press("ControlOrMeta+KeyJ");
  const drawer = page.getByTestId("coding-session-terminal-drawer");
  await expect(drawer).toBeVisible();
  await expect(page.getByTestId("coding-session-terminal-header")).toHaveText(
    "Your shell · not sandboxed · this session's worktree",
  );
  const first = page.getByTestId("coding-session-terminal-pane-wave-b-shell-1");
  await expect(first).toBeVisible();
  // The renderer named the session, never a directory.
  const creates = await page.evaluate(() =>
    (window.__BUZZ_E2E_COMMAND_PAYLOADS__ ?? []).filter(
      (entry) => entry.command === "create_shell_session",
    ),
  );
  expect(creates).toHaveLength(1);
  const createPayload = creates[0]?.payload as {
    cwd: unknown;
    codingSession: Record<string, unknown>;
  };
  expect(createPayload.cwd).toBeNull();
  expect(createPayload.codingSession).toMatchObject({
    sessionId: session.sessionId,
    channelId,
    projectRef,
    isLocalProvider: true,
  });
  expect(typeof createPayload.codingSession.sessionRef).toBe("string");
  expect(createPayload.codingSession).not.toHaveProperty("cwd");
  // The default height is T3's 280 px.
  const handle = page.getByTestId("coding-session-terminal-resize");
  await expect(handle).toHaveAttribute("aria-valuenow", "280");
  // ⌘J on a session route is the drawer's: the channel terminal stays shut.
  await expect(
    page.locator(
      '[data-terminal-owner="terminal"][data-terminal-mode="docked"]',
    ),
  ).toHaveCount(0);
  await rest(page);
  await shoot(page, "SV25-drawer", transcriptPane);
  await shoot(
    page,
    "SV25-controls",
    page.getByTestId("coding-session-terminal-actions"),
  );

  // Split: two terminals side by side; the manager appears at two.
  await expect(page.getByTestId("coding-session-terminal-manager")).toHaveCount(
    0,
  );
  await page.getByTestId("coding-session-terminal-split").click();
  await expect(
    page.getByTestId("coding-session-terminal-pane-wave-b-shell-2"),
  ).toBeVisible();
  await expect(first).toBeVisible();
  const manager = page.getByTestId("coding-session-terminal-manager");
  await expect(manager).toBeVisible();
  await rest(page);
  await shoot(page, "SV25-split", drawer);

  // New: a third terminal in its own group; the manager lists both groups.
  await page.getByTestId("coding-session-terminal-new").click();
  await expect(
    page.getByTestId("coding-session-terminal-row-wave-b-shell-3"),
  ).toHaveAttribute("data-active", "true");
  await expect(manager).toContainText("Side by side");
  await expect(manager).toContainText("Single");
  await expect(first).toHaveCount(0);
  await rest(page);
  await shoot(page, "SV25-manager", manager);

  // SV-22: the badge counts a command running now, on this computer.
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  const launcherRow = page.getByTestId(
    "coding-session-surface-launcher-row-terminal",
  );
  await expect(launcherRow).toBeVisible();
  const badge = page.getByTestId("coding-session-surface-badge-terminal");
  await expect(badge).toHaveCount(0);
  await setRunning(page, ["wave-b-shell-1"]);
  await expect(badge.first()).toHaveText("1", { timeout: 10_000 });
  await expect(badge.first()).toHaveAttribute(
    "title",
    "1 command running on this computer",
  );
  await rest(page);
  await shoot(page, "SV22-terminal-running", launcherRow);
  // Back at the prompt, the badge clears.
  await setRunning(page, []);
  await expect(badge).toHaveCount(0, { timeout: 10_000 });
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");

  // Close asks first while a command runs, and not at the prompt.
  await setRunning(page, ["wave-b-shell-1"]);
  await expect(
    page.getByTestId("coding-session-terminal-row-wave-b-shell-1"),
  ).toHaveAttribute("data-foreground", "running", { timeout: 10_000 });
  await page
    .getByTestId("coding-session-terminal-row-close-wave-b-shell-1")
    .click();
  const dialog = page.getByTestId("coding-session-terminal-close-dialog");
  await expect(dialog).toBeVisible();
  await page.getByTestId("coding-session-terminal-close-confirm").click();
  await expect(
    page.getByTestId("coding-session-terminal-row-wave-b-shell-1"),
  ).toHaveCount(0);
  // At the prompt (read as idle), a close does not ask.
  await expect(
    page.getByTestId("coding-session-terminal-row-wave-b-shell-2"),
  ).toHaveAttribute("data-foreground", "idle", { timeout: 10_000 });
  await page
    .getByTestId("coding-session-terminal-row-close-wave-b-shell-2")
    .click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-terminal-row-wave-b-shell-2"),
  ).toHaveCount(0);
  // One terminal left: the manager goes, the actions float again.
  await expect(manager).toHaveCount(0);
  const closes = await page.evaluate(() =>
    (window.__BUZZ_E2E_COMMANDS__ ?? []).filter(
      (command) => command === "close_shell_session",
    ),
  );
  expect(closes).toHaveLength(2);

  // Resize: arrow keys move the edge 16 px a press; a drag moves it too.
  await handle.focus();
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect(handle).toHaveAttribute("aria-valuenow", "328");
  const box = await handle.boundingBox();
  if (!box) throw new Error("resize handle geometry missing");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2 - 40, {
    steps: 4,
  });
  await page.mouse.up();
  await expect(handle).toHaveAttribute("aria-valuenow", "368");
  // Never below 180 px.
  await handle.focus();
  await page.keyboard.press("Home");
  await expect(handle).toHaveAttribute("aria-valuenow", "180");
  await page.keyboard.press("ArrowDown");
  await expect(handle).toHaveAttribute("aria-valuenow", "180");
  // Never above 75% of the window.
  await page.keyboard.press("End");
  await expect(handle).toHaveAttribute("aria-valuenow", "675");
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await expect(handle).toHaveAttribute("aria-valuenow", "212");
  const drawerBox = await drawer.boundingBox();
  expect(Math.round(drawerBox?.height ?? 0)).toBe(212);

  // The height survives a reload.
  await page.reload();
  await openSession(page);
  await ensureDrawerOpen(page);
  await expect(
    page.getByTestId("coding-session-terminal-resize"),
  ).toHaveAttribute("aria-valuenow", "212");
});

test("SV-25 remote: no tree here, no New terminal; a teammate's shared terminal is watched read-only by owner and liveness", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await declareTerminalMock(page, "elsewhere");
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      // Another provider than the one that signed the session: not local.
      providerPubkey: getPublicKey(generateSecretKey()),
    },
  });
  await openSession(page);
  await ensureDrawerOpen(page);
  const host = page.getByTestId("coding-session-drawer-host");

  // No tree here and nothing shared: the reason, and no way to open one.
  const unavailable = page.getByTestId("coding-session-terminal-unavailable");
  await expect(unavailable).toBeVisible();
  await expect(
    page.getByTestId("coding-session-surface-reason-terminal"),
  ).toContainText("no terminal there is shared");
  await expect(page.getByTestId("coding-session-terminal-new")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "New terminal" })).toHaveCount(
    0,
  );
  await rest(page);
  await shoot(page, "SV25-remote-no-tree", host);
  const sessionKey = await unavailable.getAttribute("data-session-key");
  if (!sessionKey) throw new Error("session key missing");

  // The session's owner shares a terminal for this session (30623 with the
  // `session` tag); the drawer lists it and watches it.
  const shellId = "teammate-shell-1";
  const announce = finalizeEvent(
    {
      kind: 30623,
      created_at: Math.floor(Date.now() / 1000),
      tags: [
        ["d", shellId],
        ["a", projectRef],
        ["title", "Terminal 1"],
        ["status", "open"],
        ["dims", "24x80"],
        ["session", sessionKey],
      ],
      content: "",
    },
    secret,
  ) as unknown as RelayEvent;
  await page.evaluate((event) => {
    const state = window.__BUZZ_E2E_WAVE_B_TERMINAL__;
    if (!state) throw new Error("terminal mock is missing");
    state.announces = [event];
  }, announce);
  // The announce reaches the drawer through its live 30623 subscription,
  // which only wakes a re-read. A relay replays a stored announce to a
  // subscription opened after it (`since`); the mock relay replays nothing,
  // so the announce is re-published until the drawer has read it (SV-66).
  const watch = page.getByTestId("coding-session-terminal-watch");
  await expect(async () => {
    await page.evaluate(
      ({ event, name }) =>
        window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__?.({
          channelName: name,
          event,
        }),
      { event: announce, name: channelName },
    );
    await expect(watch).toBeVisible({ timeout: 1_500 });
  }).toPass({ timeout: 20_000 });
  await expect(watch).toContainText("read-only");
  await expect(page.getByTestId("coding-session-terminal-new")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "New terminal" })).toHaveCount(
    0,
  );

  // Frames arrive (24311 from the owner): the label reads live.
  const label = page.getByTestId("coding-session-terminal-watch-label");
  const screen = Buffer.from(
    "teammate % cargo test reconnect\r\n   Compiling relay\r\ntest reconnect_resets ... FAILED\r\n",
  ).toString("base64");
  await expect(async () => {
    const seq = Date.now() % 1_000_000;
    const frame = finalizeEvent(
      {
        kind: 24311,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ["d", shellId],
          ["t", "snap"],
          ["seq", String(seq)],
          ["epoch", `epoch-${seq}`],
          ["dims", "24x80"],
        ],
        content: screen,
      },
      secret,
    ) as unknown as RelayEvent;
    await page.evaluate(
      ({ event, name }) =>
        window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__?.({
          channelName: name,
          event,
        }),
      { event: frame, name: channelName },
    );
    await expect(label).toHaveText(/'s computer · live$/, { timeout: 1_000 });
  }).toPass({ timeout: 20_000 });
  // Owner and liveness, never a host name; and no input path.
  await expect(label).not.toContainText(".local");
  const commands = await page.evaluate(
    () => window.__BUZZ_E2E_COMMANDS__ ?? [],
  );
  expect(commands).toContain("build_shell_watch_event");
  expect(commands).not.toContain("build_shell_input_event");
  expect(commands).not.toContain("create_shell_session");
  await rest(page);
  await shoot(page, "SV25-watching-teammate", host);
});

test("SV-25 and SV-22 screenshots are hash-distinct", () => {
  const hashes = new Map<string, string>();
  for (const name of SHOT_NAMES) {
    const png = readFileSync(`${SHOTS}/${name}.png`);
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  }
  const unique = new Set(hashes.values());
  expect(
    unique.size,
    `screenshot hashes must be distinct: ${JSON.stringify([...hashes])}`,
  ).toBe(SHOT_NAMES.length);
});
