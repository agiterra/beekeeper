import { expect, test, type Page, type Locator } from "@playwright/test";
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
  codingSessionReceiptSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/** Short-window layout acceptance; signed fixtures use the normal ingress. */
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const BASE_CREATED_AT = 1_800_000_000;
const BASE_TIMESTAMP_MS = 1_800_000_000_000;
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CREATE_COMMAND_ID = "csl-native-steer-session";
const RUNNING_TURN_ID = "turn-running-1";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function genesisEvent(): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: BASE_CREATED_AT - 2,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
}

function createAndReceiptEvents(genesisRef: string): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: CREATE_COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Steer the running turn",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: built.kind,
      created_at: BASE_CREATED_AT - 1,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: BASE_CREATED_AT,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", CREATE_COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(CREATE_COMMAND_ID)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: CREATE_COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  return [create, receipt];
}

/** A working execution whose runtime advertised native steering. */
function workingMetadataEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE_CREATED_AT,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: "Steer the running turn",
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude",
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
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/**
 * A live 24223 lease: the provider is reachable *now*. Without it the
 * composer reads the execution as unreachable and disables the editor —
 * correctly — so this is what makes the working execution steerable.
 */
function liveLeaseEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: Math.floor(Date.now() / 1_000) - 5,
      tags: [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", TARGET_KEY],
        ["csl-command", CREATE_COMMAND_ID],
        ["cslease-seq", "1"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target: TARGET,
        state: "live",
        leaseSequence: 1,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function transcriptEvent(
  eventSeq: number,
  item: unknown,
  turnId = RUNNING_TURN_ID,
): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: BASE_CREATED_AT + eventSeq,
      tags: [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["cst-seq", String(eventSeq)],
        ["cst-key", codingSessionTranscriptSemanticKey(TARGET, eventSeq)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: TARGET,
        eventSeq,
        timestamp: BASE_TIMESTAMP_MS + eventSeq * 1_000,
        turnId,
        item,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/** A per-stage turn receipt, keyed exactly as the provider's outbox keys it. */
function turnReceiptEvent(input: {
  commandId: string;
  status: "turn_injected" | "turn_delivery_unknown" | "turn_degraded";
  createdAt: number;
  /** Overrides the default error for statuses that carry one. */
  error?: { code: string; message: string };
}): RelayEvent {
  const defaultError =
    input.status === "turn_delivery_unknown"
      ? {
          code: "STEER_ACK_LOST",
          message: "the prompt ended before the acknowledgement arrived",
        }
      : input.status === "turn_degraded"
        ? {
            code: "STEER_TURN_ENDED",
            message: "the turn ended before the steer reached it",
          }
        : null;
  const content: Record<string, unknown> = {
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: input.commandId,
    status: input.status,
    session: TARGET,
    error: input.error ?? defaultError,
  };
  if (input.status === "turn_injected") content.turnId = RUNNING_TURN_ID;
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: input.createdAt,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", input.commandId],
        [
          "csl-key",
          codingSessionReceiptSemanticKey(input.commandId, input.status),
        ],
      ],
      content: JSON.stringify(content),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function seededEvents(): RelayEvent[] {
  const genesis = genesisEvent();
  return [
    genesis,
    ...createAndReceiptEvents(genesis.id),
    workingMetadataEvent(),
    liveLeaseEvent(),
    transcriptEvent(1, {
      kind: "user_prompt",
      content: "Fix the reconnect bug",
      commandId: CREATE_COMMAND_ID,
    }),
    transcriptEvent(2, {
      kind: "assistant_text",
      text: "Looking at the reconnect path now.",
    }),
  ];
}

async function seed(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events },
  );
}

/** Send one instruction while the execution works, and return the command it signed. */
async function steerFromComposer(page: Page, text: string) {
  const before = await page.evaluate(
    () => (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).length,
  );
  const editor = page.getByLabel("Coding-session instruction");
  await expect(editor).toBeEnabled();
  await editor.fill(text);
  // The deck's send control is keyed by what it will do: a working execution
  // that advertised steering renders `…-steer` (title "Steer current turn"),
  // a boundary send renders `…-queue`. Only the steer control is accepted,
  // so a boundary send cannot pass this step.
  const steer = page.getByTestId("coding-session-composer-steer");
  await expect(steer).toBeVisible();
  await expect(steer).toHaveAttribute("title", "Steer current turn");
  await reveal(page, steer);
  await steer.click();

  const signed = await page.waitForFunction((count) => {
    const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    return events.slice(count).find((event) => event.kind === 44220) ?? null;
  }, before);
  const value = (await signed.jsonValue()) as { content: string };
  const command = JSON.parse(value.content) as {
    commandId: string;
    action: { type: string; text: string; deliver?: string };
  };
  expect(command.action.type).toBe("thread.turn.start");
  expect(command.action.text).toBe(text);
  expect(command.action.deliver).toBe("steer");
  return command.commandId;
}

test.use({ viewport: { width: 900, height: 720 } });

test.beforeEach(async ({ page }) => {
  // Before the bridge: React reads the identity on mount, and the bridge
  // triggers mount. The viewer is this session's founder.
  await page.addInitScript(
    (founderIdentity) => {
      window.localStorage.setItem(
        "buzz:e2e-identity-override.v1",
        JSON.stringify(founderIdentity),
      );
    },
    {
      privateKey: hex(FOUNDER_SECRET),
      pubkey: FOUNDER_PUBKEY,
      username: "tyler",
    },
  );
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [{ pubkey: FOUNDER_PUBKEY, displayName: "Tyler" }],
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toBeVisible();
  await seed(page, seededEvents());
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-composer")).toBeVisible();
});

/** Visibility alone does not detect an overflow-hidden ancestor clipping a control. */
async function reveal(page: Page, locator: Locator) {
  await locator.scrollIntoViewIfNeeded();
  await waitForAnimations(page);
  await expect
    .poll(() =>
      locator.evaluate((element) => {
        const rect = element.getBoundingClientRect();
        const workspace = element.closest(
          '[data-testid="coding-session-shell"]',
        );
        if (!workspace) return false;
        const clip = workspace.getBoundingClientRect();
        const hit = document.elementFromPoint(
          rect.x + rect.width / 2,
          rect.y + rect.height / 2,
        );
        return (
          rect.width > 0 &&
          rect.height > 0 &&
          rect.left >= Math.max(0, clip.left) - 1 &&
          rect.right <= Math.min(innerWidth, clip.right) + 1 &&
          rect.top >= Math.max(0, clip.top) - 1 &&
          rect.bottom <= Math.min(innerHeight, clip.bottom) + 1 &&
          hit !== null &&
          element.contains(hit)
        );
      }),
    )
    .toBe(true);
}

async function enlargeText(page: Page) {
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "250%";
  });
  await expect(page.getByTestId("coding-session-workspace")).toHaveAttribute(
    "data-reflow",
    "true",
  );
  expect(page.viewportSize()).toEqual({ width: 900, height: 720 });
}

test("at 250% in a 720px window, scroll to recover, send and use header controls", async ({
  page,
}) => {
  // Wave B (SV-20): a Working status dot pulses (motion-safe:animate-pulse),
  // so every waitForAnimations here runs to its 1s ceiling. Fourteen of them
  // took this test from 18s to 31s alone, past the 30s default.
  test.setTimeout(60_000);
  await enlargeText(page);
  const editor = page.getByLabel("Coding-session instruction");
  const notice = page.getByTestId("coding-session-handover-error");
  await expect(notice).toContainText("could not be read");
  await reveal(page, notice);
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/screenshots/short-window-handover-250.png",
  });
  await reveal(page, editor);
  const commandId = await steerFromComposer(
    page,
    "Stop after the first failure",
  );
  await seed(page, [
    turnReceiptEvent({
      commandId,
      status: "turn_delivery_unknown",
      createdAt: BASE_CREATED_AT + 10,
    }),
  ]);
  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveAttribute("data-pending-state", "unknown");
  await editor.fill("meanwhile, check CI");
  const signedBefore = await page.evaluate(
    () => (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).length,
  );
  const copy = row.getByTestId("coding-session-pending-turn-copy-draft");
  await reveal(page, copy);
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/screenshots/short-window-recovery-250.png",
  });
  await copy.click();
  const recovered = "Stop after the first failure\n\nmeanwhile, check CI";
  await expect(editor).toHaveValue(recovered);
  await expect(row).toHaveAttribute("data-pending-state", "unknown");
  const dismiss = row.getByTestId("coding-session-pending-turn-dismiss");
  await reveal(page, dismiss);
  await dismiss.click();
  await expect(row).toHaveCount(0);
  expect(
    await page.evaluate(() => (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).length),
  ).toBe(signedBefore);

  const provenance = page.getByTestId("coding-session-provenance-toggle");
  await reveal(page, provenance);
  const title = page.getByTestId("coding-session-header").getByRole("heading");
  await reveal(page, title);
  // Wave B (SV-20): the header is one breadcrumb row, and in a narrow window
  // the title truncates first so the status word is never squeezed. The full
  // title stays one hover away in its tooltip, and the status word stays whole.
  await expect(title).toHaveAttribute("title", /Steer the running turn/);
  const statusBadge = page
    .getByTestId("coding-session-header")
    .getByTestId("coding-session-status-badge");
  await expect(statusBadge).toBeVisible();
  expect(
    await statusBadge.evaluate(
      (element) => element.scrollWidth <= element.clientWidth,
    ),
  ).toBe(true);
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/screenshots/short-window-header-250.png",
  });
  await provenance.click();
  await expect(
    page.getByTestId("coding-session-provenance-details"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(
    page.getByTestId("coding-session-provenance-details"),
  ).toBeHidden();
  // Radix restores trigger focus on exit; let that scroll finish before ours.
  await expect(provenance).toBeFocused();
  await waitForAnimations(page);
  const queue = page.getByTestId("coding-session-composer-queue-next");
  await reveal(page, queue);
  await waitForAnimations(page);
  await page.screenshot({
    path: "test-results/screenshots/short-window-send-250.png",
  });
  await queue.click();
  await expect
    .poll(async () =>
      page.evaluate((count) => {
        const event = (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [])
          .slice(count)
          .find((event) => event.kind === 44220);
        if (!event) return null;
        const action = JSON.parse(event.content).action;
        // NIP-CSC omits the default boundary delivery class on the wire.
        return { ...action, deliver: action.deliver ?? "boundary" };
      }, signedBefore),
    )
    .toMatchObject({
      type: "thread.turn.start",
      text: recovered,
      deliver: "boundary",
    });
  await reveal(page, page.getByTestId("coding-session-composer-interrupt"));

  // Resizing back restores the ordinary dock without losing the editor state.
  await editor.fill("Keep this draft across resize");
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "100%";
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(
    page.getByTestId("coding-session-workspace"),
  ).not.toHaveAttribute("data-reflow", "true");
  await expect(page.getByTestId("coding-session-composer-dock")).toHaveCSS(
    "position",
    "absolute",
  );
  await expect(editor).toHaveValue("Keep this draft across resize");
});

test("short-window reflow retains a bounded virtualized transcript and follow-latest", async ({
  page,
}) => {
  const events: RelayEvent[] = [];
  for (let index = 0; index < 120; index++) {
    const turnId = `history-turn-${index}`;
    events.push(
      transcriptEvent(
        3 + index * 2,
        {
          kind: "user_prompt",
          content: `History prompt ${index}`,
          commandId: `history-command-${index}`,
        },
        turnId,
      ),
    );
    events.push(
      transcriptEvent(
        4 + index * 2,
        { kind: "assistant_text", text: `History answer ${index}` },
        turnId,
      ),
    );
  }
  await seed(page, events);
  const transcript = page.getByTestId("coding-session-transcript");
  await expect(transcript).toHaveAttribute(
    "data-transcript-renderer",
    "virtualized",
  );
  const scroller = page.getByTestId("coding-session-transcript-scroll");
  await scroller.evaluate((element) => {
    element.dataset.layoutProbe = "same-scroll-element";
  });
  await enlargeText(page);
  await expect(scroller).toHaveAttribute(
    "data-layout-probe",
    "same-scroll-element",
  );
  const workspace = page.getByTestId("coding-session-shell");
  const workspaceHeight = await workspace.evaluate(
    (element) => element.clientHeight,
  );
  const dimensions = await scroller.evaluate((element) => ({
    height: element.clientHeight,
    content: element.scrollHeight,
  }));
  expect(dimensions.height).toBeGreaterThan(0);
  expect(dimensions.height).toBeLessThanOrEqual(workspaceHeight);
  expect(dimensions.content).toBeGreaterThan(dimensions.height * 2);
  await expect
    .poll(() => transcript.locator("[data-index]").count())
    .toBeLessThan(60);
  // A reader's jump to the top: an upward wheel tick, then the scroll. A bare
  // scrollTop write is not reader input, and a bottom-pinned transcript holds.
  await scroller.evaluate((element) => {
    element.dispatchEvent(new WheelEvent("wheel", { deltaY: -1 }));
    element.scrollTop = 0;
  });
  await expect(transcript).toContainText("Fix the reconnect bug");
  await scroller.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(transcript).toContainText("History prompt 119");
  await seed(page, [
    transcriptEvent(
      243,
      {
        kind: "user_prompt",
        content: "Latest after reflow",
        commandId: "latest-after-reflow",
      },
      "latest-turn",
    ),
  ]);
  await expect(transcript).toContainText("Latest after reflow");
  await expect
    .poll(() =>
      scroller.evaluate(
        (element) =>
          element.scrollHeight - element.clientHeight - element.scrollTop,
      ),
    )
    .toBeLessThan(32);
  await expect
    .poll(() => transcript.locator("[data-index]").count())
    .toBeLessThan(60);
  expect(
    await page.evaluate(() => document.documentElement.scrollHeight),
  ).toBeLessThanOrEqual(720);
});

test("follow-latest survives virtualizer corrections and resumes after the reader scrolls back down", async ({
  page,
}) => {
  const events: RelayEvent[] = [];
  for (let index = 0; index < 120; index++) {
    const turnId = `history-turn-${index}`;
    events.push(
      transcriptEvent(
        3 + index * 2,
        {
          kind: "user_prompt",
          content: `History prompt ${index}`,
          commandId: `history-command-${index}`,
        },
        turnId,
      ),
    );
    events.push(
      transcriptEvent(
        4 + index * 2,
        { kind: "assistant_text", text: `History answer ${index}` },
        turnId,
      ),
    );
  }
  await seed(page, events);
  const transcript = page.getByTestId("coding-session-transcript");
  await expect(transcript).toHaveAttribute(
    "data-transcript-renderer",
    "virtualized",
  );
  const scroller = page.getByTestId("coding-session-transcript-scroll");
  const gap = () =>
    scroller.evaluate(
      (element) =>
        element.scrollHeight - element.clientHeight - element.scrollTop,
    );
  const latest = page.getByTestId("coding-session-scroll-to-latest");
  // The burst virtualizes the transcript; the virtualizer's scrollTop
  // corrections must not read as the reader leaving the bottom.
  await expect.poll(gap).toBeLessThan(32);
  await expect(latest).toHaveCount(0);

  const box = await scroller.boundingBox();
  if (!box) throw new Error("transcript scroller has no box");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  for (let tick = 0; tick < 10; tick++) await page.mouse.wheel(0, -400);
  await expect(latest).toHaveCount(1);
  await expect.poll(gap).toBeGreaterThan(1_000);
  for (let tick = 0; tick < 40; tick++) await page.mouse.wheel(0, 400);
  await expect.poll(gap).toBeLessThan(32);
  await expect(latest).toHaveCount(0);

  await seed(page, [
    transcriptEvent(
      243,
      {
        kind: "user_prompt",
        content: "Latest after scrolling back",
        commandId: "latest-after-scrolling-back",
      },
      "latest-turn",
    ),
    transcriptEvent(
      244,
      { kind: "assistant_text", text: "Answer line\n".repeat(30) },
      "latest-turn",
    ),
  ]);
  await expect(transcript).toContainText("Latest after scrolling back");
  await expect.poll(gap).toBeLessThan(32);
  await expect(latest).toHaveCount(0);
});

test("returning to a long coding session opens at the latest content and holds there", async ({
  page,
}) => {
  const events: RelayEvent[] = [];
  for (let index = 0; index < 120; index++) {
    const turnId = `history-turn-${index}`;
    events.push(
      transcriptEvent(
        3 + index * 2,
        {
          kind: "user_prompt",
          content: `History prompt ${index}`,
          commandId: `history-command-${index}`,
        },
        turnId,
      ),
    );
    events.push(
      transcriptEvent(
        4 + index * 2,
        { kind: "assistant_text", text: `History answer ${index}` },
        turnId,
      ),
    );
  }
  await seed(page, events);
  const scroller = page.getByTestId("coding-session-transcript-scroll");
  const gap = () =>
    scroller.evaluate(
      (element) =>
        element.scrollHeight - element.clientHeight - element.scrollTop,
    );
  await expect(page.getByTestId("coding-session-transcript")).toHaveAttribute(
    "data-transcript-renderer",
    "virtualized",
  );
  await expect.poll(gap).toBeLessThan(32);

  await page.getByTestId("channel-general").click();
  await expect(scroller).toHaveCount(0);
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(scroller).toHaveCount(1);
  await expect.poll(gap).toBeLessThan(32);
  await expect(page.getByTestId("coding-session-scroll-to-latest")).toHaveCount(
    0,
  );

  // The virtualizer scrolls its element to 0 when it attaches to it. Any
  // scroll to the top that no reader made must not strand a pinned view there.
  await scroller.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect.poll(gap).toBeLessThan(32);
});
