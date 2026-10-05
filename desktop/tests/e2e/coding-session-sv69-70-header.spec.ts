import { hexToBytes } from "@noble/hashes/utils.js";
import { expect, type Page, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
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
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { signMockGeneratedTitle } from "@/testing/e2eBridgeSessionTitles";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * SV-69 and SV-70, on the session view's header row at 1280x720.
 *
 * - **SV-69** — the right panel's surface tab strip follows the WAI-ARIA tabs
 *   pattern: a tab's badge is part of its accessible name, the per-tab close
 *   buttons are out of the tab order, the arrows rove across tabs only, and
 *   Delete on a focused tab closes it.
 * - **SV-70** — a provider's generated title in the header breadcrumb carries
 *   the same "Auto-named" marker the rows show (SV-31), and the title keeps
 *   its readable, unclipped width beside it (SV-57).
 *
 * One started session, founded by the signed-in identity, with no 44229 and a
 * 44252 from the execution's own provider key: the generated title is the
 * effective name. Every event is signed with a real key and seeded through
 * the mock relay, so readers verify signatures and standing as they do live.
 */

const SHOTS = "test-results/sv69-70";

/** The bridge's own identity, so the founder is the signed-in person. */
const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const FOUNDER_SECRET = hexToBytes(FOUNDER_IDENTITY.privateKey);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "7d9e3f4c-12f6-4d20-93b5-9e4fa08b6c32";
const COMMAND_ID = "csl-sv70-titled";
const BASE = 1_800_700_000;

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "7070707070707070",
  sessionId: "70707070-6969-4000-8000-000000000070",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);

const GENERATED_TITLE = "Fix reconnect after sleep";
const TITLE_MODEL = "claude-haiku-4-5";

function sign(
  template: {
    kind: number;
    created_at: number;
    tags: string[][];
    content: string;
  },
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(template, secret) as unknown as RelayEvent;
}

function sessionEvents(): RelayEvent[] {
  const genesisTemplate = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const root = sign(
    {
      kind: genesisTemplate.kind,
      created_at: BASE,
      tags: genesisTemplate.tags,
      content: genesisTemplate.content,
    },
    FOUNDER_SECRET,
  );
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: root.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: null,
    initialTurn: null,
  });
  const create = sign(
    {
      kind: built.kind,
      created_at: BASE + 1,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
  const receipt = sign(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: BASE + 2,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  );
  const metadata = sign(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE + 2,
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
        // No operator title: the generated title is the only name there is.
        title: null,
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
    },
    PROVIDER_SECRET,
  );
  const transcript = (seq: number, item: unknown) =>
    sign(
      {
        kind: KIND_CODING_SESSION_TRANSCRIPT,
        created_at: BASE + 2 + seq,
        tags: [
          ["h", CHANNEL_ID],
          ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
          ["cs-target", TARGET_KEY],
          ["cst-seq", String(seq)],
          ["cst-key", codingSessionTranscriptSemanticKey(TARGET, seq)],
        ],
        content: JSON.stringify({
          schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
          session: TARGET,
          eventSeq: seq,
          timestamp: (BASE + 2 + seq) * 1_000,
          turnId: "turn-1",
          item,
        }),
      },
      PROVIDER_SECRET,
    );
  // Signed by the execution's own provider key: standing in this session.
  const title = signMockGeneratedTitle(
    {
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      targetKey: TARGET_KEY,
      title: GENERATED_TITLE,
      model: TITLE_MODEL,
      createEventId: create.id,
      sourceCommand: null,
      createdAt: BASE + 4,
    },
    PROVIDER_SECRET,
  );
  return [
    root,
    create,
    receipt,
    metadata,
    transcript(1, {
      kind: "user_prompt",
      content: "After sleep, the relay reconnect drops the first message.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "Reconnect now replays the queued message after sleep.",
    }),
    title,
  ];
}

/** Sign in as the founder, seed the session, and open it from the channel. */
async function openSession(page: Page) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
    // A test-only surface with an activity badge, available, so its tab can
    // open and carry the badge (SV-38's e2e hook; no product file names it).
    (
      window as Window & { __BUZZ_E2E_EXTRA_SURFACES__?: unknown }
    ).__BUZZ_E2E_EXTRA_SURFACES__ = [
      {
        id: "memory",
        label: "Memory",
        shortcut: "Y",
        order: 950,
        badgeCount: 2,
      },
    ];
  }, FOUNDER_IDENTITY);
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  // The mock relay's live REQ replays nothing, so seed only once the watches
  // that carry the title and the metadata are live (see the SV-31 spec).
  for (const kind of [
    KIND_CODING_SESSION_GENERATED_TITLE,
    KIND_CODING_SESSION_METADATA,
  ]) {
    await expect
      .poll(
        () =>
          page.evaluate(
            ({ channelName, kind }) =>
              window.__BUZZ_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
                channelName,
                kind,
              }) ?? false,
            { channelName: CHANNEL_NAME, kind },
          ),
        {
          message: `a live watch for kind ${kind} in ${CHANNEL_NAME}`,
          timeout: 20_000,
        },
      )
      .toBe(true);
  }
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: sessionEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toBeVisible();
  await trigger.click();
  const entry = page
    .getByTestId("channel-coding-session-entry")
    .filter({ hasText: GENERATED_TITLE });
  await expect(entry).toBeVisible({ timeout: 20_000 });
  await entry.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-workspace")).toBeVisible();
}

/** The focused element's testid, or null. */
function focusedTestId(page: Page) {
  return page.evaluate(
    () => document.activeElement?.getAttribute("data-testid") ?? null,
  );
}

test("sv70: the header breadcrumb marks a generated title and keeps its width", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await openSession(page);
  expect(page.viewportSize()).toEqual({ width: 1280, height: 720 });

  const header = page.getByTestId("coding-session-header");
  const breadcrumb = page.getByTestId("coding-session-breadcrumb");
  const headerTitle = header.locator("h1");
  await expect(headerTitle).toHaveText(GENERATED_TITLE, { timeout: 15_000 });

  // The rows' marker, beside the title, with the rows' attribution sentence.
  const marker = breadcrumb.getByTestId("coding-session-header-title-origin");
  await expect(marker).toHaveText("Auto-named");
  await expect(marker).toHaveAccessibleName(
    /^Auto-named: Named automatically from the first message by .+ · claude-haiku-4-5$/,
  );
  const titleBox = await headerTitle.boundingBox();
  const markerBox = await marker.boundingBox();
  expect(titleBox && markerBox, "title and marker are laid out").toBeTruthy();
  if (!titleBox || !markerBox) return;
  expect(
    markerBox.x,
    "the marker follows the title on the same row",
  ).toBeGreaterThanOrEqual(titleBox.x + titleBox.width - 1);

  // SV-57 still holds with the marker beside it: readable and unclipped.
  await expect(headerTitle).toBeVisible();
  expect(titleBox.width).toBeGreaterThanOrEqual(120);
  const clip = await headerTitle.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  expect(
    clip.scrollWidth,
    `"${GENERATED_TITLE}" is shown whole beside its marker (${JSON.stringify(clip)})`,
  ).toBeLessThanOrEqual(clip.clientWidth + 1);

  // Its tooltip names the model, as the rows' does.
  await marker.hover();
  const detail = page.getByTestId("coding-session-header-title-origin-detail");
  await expect(detail).toContainText(
    "Named automatically from the first message by",
  );
  await expect(detail).toContainText(TITLE_MODEL);
  await page.mouse.move(0, 0, { steps: 10 });
  await expect(detail).toBeHidden();
  await waitForAnimations(page);
  await breadcrumb.screenshot({
    path: `${SHOTS}/sv70-header-auto-named.png`,
  });
});

test("sv69: tabs name their badge, close buttons leave the tab order, arrows rove and Delete closes", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await openSession(page);

  // Open Diff from the launcher, then Memory (badged) from "+".
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  await page.keyboard.press("d");
  const diffTab = page.getByTestId("coding-session-surface-tab-diff");
  await expect(diffTab).toHaveAttribute("aria-selected", "true");
  await page.getByTestId("coding-session-surface-add").click();
  await page.getByTestId("coding-session-surface-add-memory").click();
  const memoryTab = page.getByTestId("coding-session-surface-tab-memory");
  await expect(memoryTab).toHaveAttribute("aria-selected", "true");

  // The badge is part of the tab's accessible name; a tab with none is just
  // its label.
  await expect(
    page.getByTestId("coding-session-surface-badge-memory").first(),
  ).toHaveText("2");
  await expect(memoryTab).toHaveAccessibleName("Memory 2 memory items active");
  await expect(diffTab).toHaveAccessibleName("Diff");
  const tablist = page.getByRole("tablist", { name: "Session surface tabs" });
  await expect(tablist.getByRole("tab")).toHaveCount(2);
  await expect(
    tablist.getByRole("tab", { name: "Memory 2 memory items active" }),
  ).toBeVisible();

  // Close buttons are out of the tab order; the active tab is the one stop.
  for (const id of ["diff", "memory"]) {
    await expect(
      page.getByTestId(`coding-session-surface-tab-close-${id}`),
    ).toHaveAttribute("tabindex", "-1");
  }
  await expect(memoryTab).toHaveAttribute("tabindex", "0");
  await expect(diffTab).toHaveAttribute("tabindex", "-1");

  // Arrows rove across the tabs alone, activating as they go.
  await memoryTab.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(diffTab).toBeFocused();
  await expect(diffTab).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowRight");
  await expect(memoryTab).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(diffTab).toBeFocused();

  // Tab leaves the strip for "+" without stopping on any close button.
  await page.keyboard.press("Tab");
  expect(await focusedTestId(page)).toBe("coding-session-surface-add");
  await page.keyboard.press("Shift+Tab");
  await expect(diffTab).toBeFocused();

  await waitForAnimations(page);
  await page
    .getByTestId("coding-session-surface-tabbar")
    .screenshot({ path: `${SHOTS}/sv69-tab-strip-focused.png` });

  // Delete closes the focused tab; focus lands on the tab that is left.
  await page.keyboard.press("Delete");
  await expect(diffTab).toHaveCount(0);
  await expect(memoryTab).toHaveAttribute("aria-selected", "true");
  await expect(memoryTab).toBeFocused();
});
