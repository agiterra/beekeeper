import { hexToBytes } from "@noble/hashes/utils.js";
import { expect, type Locator, type Page, test } from "@playwright/test";
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
 * SV-31 on the desktop surfaces: a provider's generated title (kind 44252).
 *
 * Every event is signed with a real key and seeded through the mock relay;
 * readers verify signatures and standing exactly as they do live. Two
 * sessions share the `engineering` channel, both founded by the signed-in
 * identity:
 *
 * - **Titled** — started: a genesis and create by the founder, the provider's
 *   created receipt and 44223, a first turn, and a 44252 signed by that same
 *   provider key. No 44229 exists, so the generated title is the effective
 *   name, marked "Auto-named" wherever a row shows it.
 *   The titled session also carries a second 44252 for the very same
 *   `cs-target`, signed by a key with no 44223 there and written *earlier*
 *   than the genuine one. A reader that only checked "some 44223 exists for
 *   this target", and never compared the signer with the execution's
 *   provider key, would pick it by "earliest wins"; every surface must still
 *   show the generated title and never the impostor's text.
 * - **Foreign** — founded, never started, with a 44252 signed by a key that
 *   runs nothing in the session. Readers set it aside, so it stays
 *   "Untitled session".
 *
 * The header must show the effective name too (S2: "and the title in the
 * header"). That check is soft: it fails the test, but the shots after it —
 * the header as it actually renders, the rename dialog, the renamed sidebar —
 * are still taken, so one header defect does not hide the rest of the run.
 * Whether the header carries the "Auto-named" marker is not asserted here:
 * the marker belongs to Wave B's header breadcrumb, and the rename dialog
 * carries the attribution one click away in any case.
 */

const SHOTS = "test-results/sv31";

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
const PROVIDER_NAME = "Brian's Mac";
const FOREIGN_SECRET = generateSecretKey();

const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const TITLED_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOREIGN_REF = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
const COMMAND_ID = "csl-sv31-titled";
const BASE = 1_800_300_000;

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "31313131-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const FOREIGN_TARGET_KEY = buildCodingSessionTargetKey({
  ...TARGET,
  instanceId: "not-in-this-session",
  sessionId: "99999999-8888-7777-6666-555555555555",
});

const GENERATED_TITLE = "Fix reconnect after sleep";
/** A foreign key's title on the started session's real target. */
const IMPOSTOR_TITLE = "Impostor title from a stranger";
const TITLE_MODEL = "claude-haiku-4-5";
const PERSON_NAME = "Reconnect survives sleep";
const FIRST_MESSAGE =
  "After the laptop sleeps, the relay reconnect drops the first message. Fix it.";

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

function genesis(sessionRef: string, createdAt: number): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef,
  });
  return sign(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    FOUNDER_SECRET,
  );
}

function titledSessionEvents(): RelayEvent[] {
  const root = genesis(TITLED_REF, BASE);
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: TITLED_REF,
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
        sessionRef: TITLED_REF,
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
      sessionRef: TITLED_REF,
      targetKey: TARGET_KEY,
      title: GENERATED_TITLE,
      model: TITLE_MODEL,
      createEventId: create.id,
      sourceCommand: null,
      createdAt: BASE + 4,
    },
    PROVIDER_SECRET,
  );
  // Same session, same real `cs-target`, one second earlier, but signed by a
  // key that is not the execution's provider: only the signer check stands
  // between it and the name every row shows.
  const impostorTitle = signMockGeneratedTitle(
    {
      channelId: CHANNEL_ID,
      sessionRef: TITLED_REF,
      targetKey: TARGET_KEY,
      title: IMPOSTOR_TITLE,
      model: TITLE_MODEL,
      createEventId: create.id,
      sourceCommand: null,
      createdAt: BASE + 3,
    },
    FOREIGN_SECRET,
  );
  return [
    root,
    create,
    receipt,
    metadata,
    transcript(1, { kind: "user_prompt", content: FIRST_MESSAGE }),
    transcript(2, {
      kind: "assistant_text",
      text: "Reconnect now replays the queued message after sleep.",
    }),
    impostorTitle,
    title,
  ];
}

function foreignTitledFoundedEvents(): RelayEvent[] {
  const root = genesis(FOREIGN_REF, BASE + 10);
  // A key with no execution here: a title nobody with standing wrote.
  const title = signMockGeneratedTitle(
    {
      channelId: CHANNEL_ID,
      sessionRef: FOREIGN_REF,
      targetKey: FOREIGN_TARGET_KEY,
      title: "Not your session's name",
      model: TITLE_MODEL,
      createEventId: "e".repeat(64),
      sourceCommand: null,
      createdAt: BASE + 11,
    },
    FOREIGN_SECRET,
  );
  return [root, title];
}

async function openApp(page: Page) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
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
    searchProfiles: [{ pubkey: PROVIDER_PUBKEY, displayName: PROVIDER_NAME }],
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  // The names read is a bounded history read plus a live watch. A real relay
  // replays stored events when that watch opens (its filter carries a
  // `limit`); the mock relay's live REQ replays nothing. So a title seeded
  // after the history read but before the watch is registered is never seen
  // by the sidebar, and the row keeps its fallback label. Seed only once the
  // watches that carry the title and the session's metadata are live.
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
    {
      channelName: CHANNEL_NAME,
      events: [...titledSessionEvents(), ...foreignTitledFoundedEvents()],
    },
  );
}

function sidebarRow(page: Page, text: string): Locator {
  return page
    .getByTestId("project-coding-session-row")
    .filter({ hasText: text })
    .first();
}

/** One screenshot of a trigger and the tooltip it opened, cropped to both. */
async function shotWithTooltip(
  page: Page,
  anchor: Locator,
  tooltip: Locator,
  path: string,
) {
  const a = await anchor.boundingBox();
  const b = await tooltip.boundingBox();
  if (!a || !b) throw new Error("nothing to crop");
  const pad = 8;
  const x = Math.max(0, Math.min(a.x, b.x) - pad);
  const y = Math.max(0, Math.min(a.y, b.y) - pad);
  await page.screenshot({
    path,
    clip: {
      x,
      y,
      width: Math.max(a.x + a.width, b.x + b.width) + pad - x,
      height: Math.max(a.y + a.height, b.y + b.height) + pad - y,
    },
  });
}

test("sv31: a generated title is marked where rows show it, a rename removes the marker, a foreign title is ignored", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await openApp(page);

  // Sidebar: the generated title with its marker.
  const row = sidebarRow(page, GENERATED_TITLE);
  await expect(row).toBeVisible({ timeout: 20_000 });
  const rowMarker = row.getByTestId("coding-session-title-origin");
  await expect(rowMarker).toHaveText("Auto-named");
  await waitForAnimations(page);
  await row.screenshot({ path: `${SHOTS}/sv31-auto-named-sidebar.png` });

  // The tooltip names the provider and the model.
  await rowMarker.hover();
  const detail = page.getByTestId("coding-session-title-origin-detail");
  await expect(detail).toBeVisible();
  await expect(detail).toContainText(
    "Named automatically from the first message by",
  );
  await expect(detail).toContainText(PROVIDER_NAME);
  await expect(detail).toContainText(TITLE_MODEL);
  await waitForAnimations(page);
  await shotWithTooltip(
    page,
    row,
    detail,
    `${SHOTS}/sv31-auto-named-tooltip.png`,
  );
  // Radix keeps a hoverable tooltip open across its pointer grace area until
  // a later pointermove lands outside it; a one-step move sends none, so walk
  // the pointer out in steps.
  await page.mouse.move(0, 0, { steps: 10 });
  await expect(detail).toBeHidden();

  // A title from a key with no execution here is ignored on the shelf.
  const foreignRow = sidebarRow(page, "Untitled session");
  await expect(foreignRow).toBeVisible();
  await expect(
    foreignRow.getByTestId("coding-session-title-origin"),
  ).toHaveCount(0);
  await expect(page.getByText("Not your session's name")).toHaveCount(0);
  // The earlier title on the real target, by a key that is not the
  // execution's provider, is ignored too: the row above kept the genuine one.
  await expect(page.getByText(IMPOSTOR_TITLE)).toHaveCount(0);

  // Channel Sessions menu: the same title and marker; the foreign one untitled.
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toBeVisible();
  await trigger.click();
  const entry = page
    .getByTestId("channel-coding-session-entry")
    .filter({ hasText: GENERATED_TITLE });
  await expect(entry).toBeVisible();
  await expect(entry.getByTestId("coding-session-title-origin")).toHaveText(
    "Auto-named",
  );
  await waitForAnimations(page);
  await entry.screenshot({ path: `${SHOTS}/sv31-auto-named-channel-menu.png` });
  const foreignEntry = page.getByTestId("channel-founded-coding-session-entry");
  await expect(foreignEntry).toContainText("Untitled session");
  await expect(
    foreignEntry.getByTestId("coding-session-title-origin"),
  ).toHaveCount(0);
  await expect(page.getByText(IMPOSTOR_TITLE)).toHaveCount(0);
  await waitForAnimations(page);
  await foreignEntry.screenshot({
    path: `${SHOTS}/sv31-foreign-signer-untitled.png`,
  });

  // The header: the effective name, the generated title here (S2).
  //
  // SV-57: at 1280x720 with the handover banner, a header layout bug squeezes
  // the title column to zero width, so the h1 is in the DOM with the right
  // text but not visible. That bug belongs to Wave B's header rewrite; this
  // spec asserts the title's TEXT (what SV-31 owns) without requiring
  // visibility, and does not widen the viewport to hide the bug.
  await entry.getByTestId("channel-coding-session-open").click();
  const header = page.getByTestId("coding-session-header");
  const headerTitle = header.locator("h1");
  await expect(
    headerTitle,
    "S2: the header shows the generated title",
  ).toHaveText(GENERATED_TITLE, { timeout: 15_000 });
  await expect(
    header,
    "S2: the header never shows a foreign signer's title",
  ).not.toContainText(IMPOSTOR_TITLE);
  await expect(page.getByText(IMPOSTOR_TITLE)).toHaveCount(0);
  await waitForAnimations(page);
  // The header shot is of the h1 itself when it can be seen; while SV-57
  // collapses it there is nothing to photograph, so that one PNG is skipped.
  if (await headerTitle.isVisible()) {
    await headerTitle.screenshot({
      path: `${SHOTS}/sv31-generated-title-header.png`,
    });
  } else {
    test.info().annotations.push({
      type: "skipped-screenshot",
      description:
        "sv31-generated-title-header.png: header title column collapsed (SV-57)",
    });
  }

  // Rename: prefilled with the generated title, attribution one line away.
  // The rename control is reached by keyboard focus, which reveals it the
  // same as hovering the title does and stays in the tab order; hovering a
  // zero-width h1 (SV-57) is not possible.
  const renameButton = page.getByTestId("coding-session-rename");
  await renameButton.focus();
  await renameButton.press("Enter");
  const dialog = page.getByTestId("coding-session-name-dialog");
  await expect(dialog).toBeVisible();
  const input = page.getByTestId("coding-session-name-input");
  await expect(input).toHaveValue(GENERATED_TITLE);
  const attribution = page.getByTestId(
    "coding-session-name-dialog-attribution",
  );
  await expect(attribution).toContainText(
    "Named automatically from the first message by",
  );
  await expect(attribution).toContainText(TITLE_MODEL);
  await waitForAnimations(page);
  await dialog.screenshot({
    path: `${SHOTS}/sv31-rename-dialog-attribution.png`,
  });

  await input.fill(PERSON_NAME);
  await page.getByTestId("coding-session-name-save").click();
  await expect(dialog).toBeHidden();

  // The person's name wins by tier: no marker anywhere it shows.
  const renamed = sidebarRow(page, PERSON_NAME);
  await expect(renamed).toBeVisible({ timeout: 15_000 });
  await expect(renamed.getByTestId("coding-session-title-origin")).toHaveCount(
    0,
  );
  await expect(sidebarRow(page, GENERATED_TITLE)).toHaveCount(0);
  await expect(header.locator("h1")).toHaveText(PERSON_NAME);
  await waitForAnimations(page);
  await renamed.screenshot({ path: `${SHOTS}/sv31-renamed-sidebar.png` });
});
