import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { hexToBytes } from "@noble/hashes/utils.js";
import {
  expect,
  type Locator,
  type Page,
  type TestInfo,
  test,
} from "@playwright/test";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SURFACE_FRAME } from "@/shared/constants/kinds";
import type { SurfaceObservationShareState } from "@/testing/e2eBridgeSurfaceObservation";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";
import {
  PREVIEW_FRAME_A,
  PREVIEW_SNAPSHOT_COMMIT_PNG,
  PREVIEW_SNAPSHOT_PNG,
} from "./fixtures/surface-observation/images";
import {
  CHANNEL_ID,
  CHANNEL_NAME,
  nowSecs,
  previewAnnounce,
  publishedWatches,
  routeSnapshotBlobs,
  seedSurfaceEvents,
  sessionEvents,
  surfaceFrame,
  surfaceSnapshot,
  waitForLiveKind,
} from "./fixtures/surface-observation/surfaceEvents";

// SV-33 S3/S4 (C5 lane V): sharing the session's Browser, in the mock
// bridge. A second machine is NOT here: the "remote" desktop is this one,
// reading a 30626 announce, 44253 snapshots and 24321 frames that the spec
// signs with another identity (alice, the host) and injects into the mock
// relay. The host strip reads the mock `session_preview_share_*` state
// (`e2eBridgeSurfaceObservation.ts`); no WKWebView renders or captures here.

test.describe.configure({ mode: "serial" });

const SHOTS = "test-results/session-preview-share";
const SHOT_NAMES = [
  "session-preview-snapshot-card",
  "session-preview-remote-snapshot",
  "session-preview-live-remote",
  "session-preview-paused",
  "session-preview-no-browser",
  "session-preview-host-sharing",
] as const;

/** The hosting desktop (signs 30626, 44253 and 24321). */
const HOST_SECRET = hexToBytes(TEST_IDENTITIES.alice.privateKey);
const HOST_PUBKEY = TEST_IDENTITIES.alice.pubkey;
/** The session's provider on the host's machine. */
const REMOTE_PROVIDER_SECRET = generateSecretKey();
const REMOTE_PROVIDER_PUBKEY = getPublicKey(REMOTE_PROVIDER_SECRET);
/** This computer's provider: runs the host-side session, not the remote one. */
const LOCAL_PROVIDER_SECRET = generateSecretKey();
const LOCAL_PROVIDER_PUBKEY = getPublicKey(LOCAL_PROVIDER_SECRET);
const REQUESTER_PUBKEY = TEST_IDENTITIES.bob.pubkey;
const SESSION_REF = "c5c5c5c5-2222-4000-8000-000000000033";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "c5c5c5c5c5c50033",
  sessionId: "c5000000-0000-4000-8000-000000000033",
  generation: 1,
};
const COMMIT = "1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b";
const PREVIEW_DIM = "160x100";
const SERVERS = [
  {
    port: 5173,
    url: "http://localhost:5173/",
    address: "127.0.0.1",
    pid: 4242,
    process: "node",
    title: "Vite App",
  },
];

function base(): number {
  return nowSecs() - 3_600;
}

function announce(stream: "frames" | "snapshots", createdAt: number) {
  return previewAnnounce({
    sessionRef: SESSION_REF,
    status: "open",
    csTarget: buildCodingSessionTargetKey(TARGET),
    providerPubkey: REMOTE_PROVIDER_PUBKEY,
    page: "local:/settings",
    title: "Settings",
    viewport: "1280x800",
    stream,
    createdAt,
    secret: HOST_SECRET,
  });
}

function previewSnapshot(input: {
  blob: string;
  takenAtMs: number;
  commit?: boolean;
  requestedBy?: string;
}): RelayEvent {
  return surfaceSnapshot({
    surface: "preview",
    d: SESSION_REF,
    blob: input.blob,
    dim: PREVIEW_DIM,
    takenAtMs: input.takenAtMs,
    providerPubkey: REMOTE_PROVIDER_PUBKEY,
    requestedBy: input.requestedBy,
    commit: input.commit ? { sha: COMMIT, state: "dirty" } : undefined,
    page: "local:/settings",
    title: "Settings",
    alt: "Settings page after save",
    secret: HOST_SECRET,
  });
}

function previewFrame(
  t: "frame" | "paused",
  seq: number,
  jpeg?: string,
): RelayEvent {
  return surfaceFrame({
    surface: "preview",
    d: SESSION_REF,
    t,
    seq,
    epoch: 1_791_374_400_456,
    cadenceMs: 2_000,
    dim: PREVIEW_DIM,
    capturedAtMs: Date.now(),
    jpeg,
    secret: HOST_SECRET,
  });
}

/**
 * Open the session's Browser surface from the launcher. `host: true` runs
 * the session on this computer (its provider is this machine's) and declares
 * the mock share state; otherwise the session runs on the host's machine.
 */
async function openBrowser(
  page: Page,
  input: {
    host: boolean;
    surfaceEvents: RelayEvent[];
    share?: Partial<SurfaceObservationShareState>;
  },
): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(
    ({ servers, share, channelId }) => {
      // The Browser is macOS-only; pin the platform (as C4's spec does).
      Object.defineProperty(window.navigator, "platform", {
        configurable: true,
        get: () => "MacIntel",
      });
      window.__BEEKEEPER_E2E_SESSION_PREVIEW__ = { servers };
      window.__BEEKEEPER_E2E_SURFACE_OBSERVATION__ = {
        share: share
          ? { [channelId]: { ...share, lastFrameAt: Date.now() } }
          : {},
      };
    },
    {
      servers: input.host ? SERVERS : [],
      share: input.share ?? null,
      channelId: CHANNEL_ID,
    },
  );
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: LOCAL_PROVIDER_PUBKEY,
    },
  });
  await routeSnapshotBlobs(page, [
    PREVIEW_SNAPSHOT_PNG,
    PREVIEW_SNAPSHOT_COMMIT_PNG,
  ]);
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await seedSurfaceEvents(page, [
    ...sessionEvents({
      target: TARGET,
      sessionRef: SESSION_REF,
      title: "Shared preview",
      secret: input.host ? LOCAL_PROVIDER_SECRET : REMOTE_PROVIDER_SECRET,
      createdAt: base(),
    }),
    ...input.surfaceEvents,
  ]);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-workspace")).toContainText(
    "Shared preview",
  );
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  const row = page.getByTestId("coding-session-surface-launcher-row-browser");
  await expect(row).toHaveAttribute("data-available", "true");
  await row.click();
  const panel = page.getByTestId("coding-session-surface-panel-browser");
  await expect(panel).toBeVisible();
  return panel;
}

async function shoot(
  page: Page,
  testInfo: TestInfo,
  name: string,
  locator: Locator,
) {
  await expect(locator).toBeVisible();
  await page.mouse.move(2, 2);
  await waitForAnimations(page);
  await locator.screenshot({ path: shotPath(testInfo, SHOTS, name) });
}

async function shareCalls(page: Page) {
  return page.evaluate(
    () => window.__BEEKEEPER_E2E_SURFACE_OBSERVATION__?.calls ?? [],
  );
}

test("S3: snapshot cards, and the remote surface reads Snapshot · not live", async ({
  page,
}, testInfo) => {
  const t = base();
  const plain = previewSnapshot({
    blob: PREVIEW_SNAPSHOT_PNG,
    takenAtMs: (t + 30) * 1_000,
  });
  const withCommit = previewSnapshot({
    blob: PREVIEW_SNAPSHOT_COMMIT_PNG,
    takenAtMs: (t + 60) * 1_000,
    commit: true,
    requestedBy: REQUESTER_PUBKEY,
  });
  const panel = await openBrowser(page, {
    host: false,
    surfaceEvents: [announce("snapshots", t + 20), plain, withCommit],
  });
  const remote = page.getByTestId("session-preview-remote");
  await expect(remote).toHaveAttribute("data-state", "snapshot", {
    timeout: 15_000,
  });
  await expect(page.getByTestId("session-preview-remote-status")).toHaveText(
    /^Snapshot · \d{1,2}:\d{2} · not live$/,
  );

  const card = (event: RelayEvent) =>
    page
      .locator(
        `[data-testid="surface-snapshot-card"][data-snapshot-id="${event.id}"]`,
      )
      .first();
  const plainCard = card(plain);
  await expect(plainCard).toHaveAttribute("data-surface", "preview");
  await expect(
    plainCard.getByTestId("surface-snapshot-card-signer"),
  ).toHaveText(/\S/);
  await expect(
    plainCard.getByTestId("surface-snapshot-card-machine"),
  ).toHaveText(/\S/);
  await expect(plainCard.getByTestId("surface-snapshot-card-time")).toHaveText(
    /\d{1,2}:\d{2}/,
  );
  await expect(
    plainCard.getByTestId("surface-snapshot-card-commit"),
  ).toHaveText("commit not recorded");
  await expect(plainCard.getByTestId("surface-snapshot-card-page")).toHaveText(
    "local:/settings",
  );
  await expect(
    plainCard.getByTestId("surface-snapshot-card-requested-by"),
  ).toHaveCount(0);

  const commitCard = card(withCommit);
  await expect(
    commitCard.getByTestId("surface-snapshot-card-commit"),
  ).toHaveText("commit 1a2b3c4 · dirty");
  await expect(
    commitCard.getByTestId("surface-snapshot-card-requested-by"),
  ).toHaveText(/^requested by .+$/);

  await shoot(page, testInfo, "session-preview-snapshot-card", plainCard);
  await shoot(page, testInfo, "session-preview-remote-snapshot", panel);

  // Request snapshot sends a 24320 `snapshot` to the announce's owner.
  await page.getByTestId("session-preview-request-snapshot").click();
  await expect
    .poll(async () =>
      (await publishedWatches(page)).some(
        (watch) =>
          watch.action === "snapshot" &&
          watch.tags.some((tag) => tag[0] === "p" && tag[1] === HOST_PUBKEY) &&
          watch.tags.some((tag) => tag[0] === "d" && tag[1] === SESSION_REF),
      ),
    )
    .toBe(true);
});

test("S4: a teammate sees Live from the host's computer, then Paused", async ({
  page,
}, testInfo) => {
  const panel = await openBrowser(page, {
    host: false,
    surfaceEvents: [announce("frames", base() + 20)],
  });
  await waitForLiveKind(page, KIND_SURFACE_FRAME);
  await expect
    .poll(async () =>
      (await publishedWatches(page)).some(
        (watch) =>
          watch.action === "watch" &&
          JSON.stringify(watch.tags.slice(1, 4)) ===
            JSON.stringify([
              ["surface", "preview"],
              ["d", SESSION_REF],
              ["p", HOST_PUBKEY],
            ]),
      ),
    )
    .toBe(true);
  await seedSurfaceEvents(page, [previewFrame("frame", 1, PREVIEW_FRAME_A)]);
  const remote = page.getByTestId("session-preview-remote");
  const statusLine = page.getByTestId("session-preview-remote-status");
  await expect(remote).toHaveAttribute("data-state", "live");
  await expect(statusLine).toHaveText(
    /^Live from .+'s computer · every 2 s · \d+ s ago$/,
  );
  await expect(page.getByTestId("session-preview-remote-frame")).toBeVisible();
  await shoot(page, testInfo, "session-preview-live-remote", panel);

  // The host turns Share off: the producer sends `t=paused`.
  await seedSurfaceEvents(page, [previewFrame("paused", 2)]);
  await expect(remote).toHaveAttribute("data-state", "paused");
  await expect(statusLine).toHaveText(/^Paused by .+$/);
  await expect(statusLine).not.toContainText("Live");
  await shoot(page, testInfo, "session-preview-paused", panel);
});

test("S3: no open announce reads the no-browser sentence", async ({
  page,
}, testInfo) => {
  const panel = await openBrowser(page, { host: false, surfaceEvents: [] });
  const remote = page.getByTestId("session-preview-remote");
  await expect(remote).toHaveAttribute("data-state", "none", {
    timeout: 15_000,
  });
  await expect(page.getByTestId("session-preview-remote-status")).toHaveText(
    "No preview is shared for this session.",
  );
  await expect(remote).toContainText(
    "The Browser runs in the Beekeeper app on the machine running the agent.",
  );
  await shoot(page, testInfo, "session-preview-no-browser", panel);
});

test("S4: the host strip shows sharing and watchers; camera and Share toggle call Rust", async ({
  page,
}, testInfo) => {
  const panel = await openBrowser(page, {
    host: true,
    surfaceEvents: [],
    share: {
      sessionRef: SESSION_REF,
      share: true,
      announced: "open",
      stream: "frames",
      watchers: [REQUESTER_PUBKEY],
      cadenceMs: 2_000,
      framesLastMinute: 12,
    },
  });
  await panel.getByTestId("session-preview-server").first().click();
  await expect(panel.getByTestId("session-preview-surface")).toHaveAttribute(
    "data-placement",
    "docked",
  );
  const strip = panel.getByTestId("session-preview-share-strip");
  await expect(strip).toHaveText(
    "Live on this computer · shared with the session · 1 watching",
  );
  const toggle = panel.getByTestId("session-preview-share-toggle");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await shoot(page, testInfo, "session-preview-host-sharing", panel);

  // The camera publishes a snapshot through Rust.
  await panel.getByTestId("session-preview-camera").click();
  await expect
    .poll(async () =>
      (await shareCalls(page)).some(
        (call) =>
          call.command === "session_preview_share_snapshot" &&
          call.payload.channelId === CHANNEL_ID,
      ),
    )
    .toBe(true);

  // Share off: configure with share:false, and the strip says so.
  await toggle.click();
  await expect
    .poll(async () =>
      (await shareCalls(page)).some(
        (call) =>
          call.command === "session_preview_share_configure" &&
          call.payload.channelId === CHANNEL_ID &&
          call.payload.share === false,
      ),
    )
    .toBe(true);
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(strip).toHaveText("Local only · not shared");
});

test("every shot is a distinct state", async ({ page: _page }, testInfo) => {
  const hashes = SHOT_NAMES.map((name) =>
    createHash("sha256")
      .update(readFileSync(shotPath(testInfo, SHOTS, name)))
      .digest("hex"),
  );
  expect(new Set(hashes).size).toBe(SHOT_NAMES.length);
});
