import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import {
  expect,
  type Locator,
  type Page,
  type TestInfo,
  test,
} from "@playwright/test";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";

import type { RelayEvent } from "@/shared/api/types";
import { KIND_SURFACE_FRAME } from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { shotPath } from "../helpers/shotPath";
import {
  DEVICE_FRAME_A,
  DEVICE_FRAME_B,
  DEVICE_FRAME_STRANGER,
  DEVICE_SNAPSHOT_PNG,
} from "./fixtures/surface-observation/images";
import {
  CHANNEL_NAME,
  type DeviceContext,
  deviceAvailability,
  deviceState,
  nowSecs,
  publishedWatches,
  routeSnapshotBlobs,
  seedSurfaceEvents,
  sessionEvents,
  surfaceFrame,
  surfaceSnapshot,
  waitForLiveKind,
} from "./fixtures/surface-observation/surfaceEvents";

// SV-34 S1/S2 (C5 lane V): the session's Device surface in the mock bridge.
// The simulator, `simctl` and the provider are NOT here: the specs sign the
// provider's 44255 records, 44253 snapshots and 24321 frames themselves and
// inject them into the mock relay (`e2eBridgeSurfaceObservation.ts`). These
// shots prove the surface's states and honesty text, never a real device,
// a real capture loop or a second machine watching.

test.describe.configure({ mode: "serial" });

const SHOTS = "test-results/session-device";
const SHOT_NAMES = [
  "sv34-device-snapshot",
  "sv34-device-closed",
  "sv34-device-not-offered",
  "sv34-device-no-xcode",
  "sv34-device-agent-device-unavailable",
  "sv34-device-live",
  "sv34-device-stalled",
] as const;

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const PROVIDER_NAME = "Studio Mac";
const STRANGER_SECRET = generateSecretKey();
const SEAT_PUBKEY = TEST_IDENTITIES.bob.pubkey;
const SESSION_REF = "c5c5c5c5-0000-4000-8000-000000000034";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "c5c5c5c5c5c5c5c5",
  sessionId: "c5000000-0000-4000-8000-000000000034",
  generation: 1,
};
const DEVICE: DeviceContext = {
  target: TARGET,
  lifecycleCommand: "c5c5c5c5-1111-4000-8000-000000000034",
  slot: "9f2c4e1a7b3d5c80",
  providerSecret: PROVIDER_SECRET,
};
const DEVICE_DIM = "60x130";
const AGENT_DEVICE_REASON = "Node 22.12 or newer is required (found 20.11.0)";

/** Base of every fixture's `created_at`: an hour ago, then later rows. */
function base(): number {
  return nowSecs() - 3_600;
}

function available(createdAt: number): RelayEvent {
  return deviceAvailability(DEVICE, createdAt, {
    ios: { available: true },
    agentDevice: { installed: true, version: "0.21.12" },
  });
}

function snapshot(takenAtMs: number): RelayEvent {
  return surfaceSnapshot({
    surface: "device",
    d: DEVICE.slot,
    blob: DEVICE_SNAPSHOT_PNG,
    dim: DEVICE_DIM,
    takenAtMs,
    providerPubkey: PROVIDER_PUBKEY,
    requestedBy: SEAT_PUBKEY,
    alt: "Settings screen",
    secret: PROVIDER_SECRET,
  });
}

function frame(
  seq: number,
  jpeg: string,
  secret: Uint8Array = PROVIDER_SECRET,
): RelayEvent {
  return surfaceFrame({
    surface: "device",
    d: DEVICE.slot,
    t: "frame",
    seq,
    epoch: 1_791_374_400_123,
    cadenceMs: 3_000,
    dim: DEVICE_DIM,
    capturedAtMs: Date.now(),
    jpeg,
    secret,
  });
}

/**
 * Open the session with its Device tab showing. The tab is restored from the
 * stored panel state rather than the launcher, because a not-offered or
 * unavailable device may be listed dimmed — and the panel must still say why.
 */
async function openDevice(page: Page, deviceEvents: RelayEvent[]) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    const original = Storage.prototype.getItem;
    Storage.prototype.getItem = function getItem(key: string) {
      const stored = original.call(this, key);
      if (stored === null && key.startsWith("beekeeper:session-panels:v1:")) {
        return JSON.stringify({
          rightOpen: true,
          tabs: ["device"],
          active: "device",
          expanded: false,
          bottomOpen: false,
          userActed: true,
        });
      }
      return stored;
    };
  });
  await installMockBridge(page, {
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
    },
    searchProfiles: [{ pubkey: PROVIDER_PUBKEY, displayName: PROVIDER_NAME }],
  });
  await routeSnapshotBlobs(page, [DEVICE_SNAPSHOT_PNG]);
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await seedSurfaceEvents(page, [
    ...sessionEvents({
      target: TARGET,
      sessionRef: SESSION_REF,
      title: "Device check",
      secret: PROVIDER_SECRET,
      createdAt: base(),
    }),
    ...deviceEvents,
  ]);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-workspace")).toContainText(
    "Device check",
  );
  const panel = page.getByTestId("coding-session-surface-panel-device");
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

function status(panel: Locator): Locator {
  return panel.getByTestId("device-surface-status");
}

const HEADER = /^iPhone 17 · iOS 27\.0 · on .+$/;

test("S1: an open device with a snapshot reads Snapshot · HH:MM by the seat", async ({
  page,
}, testInfo) => {
  const t = base();
  const panel = await openDevice(page, [
    available(t + 10),
    deviceState(DEVICE, t + 20, "open", "open-1"),
    snapshot(Date.now() - 120_000),
  ]);
  await expect(panel.getByTestId("device-surface-header")).toHaveText(HEADER);
  // No watcher frames arrive, so the newest 44253 is what shows.
  await expect(status(panel)).toHaveAttribute("data-state", "snapshot", {
    timeout: 15_000,
  });
  await expect(status(panel)).toHaveText(/^Snapshot · \d{1,2}:\d{2} by .+$/);
  await expect(status(panel)).not.toContainText("Live");
  await expect(panel.getByTestId("device-surface-snapshot")).toBeVisible();
  await expect(panel.getByTestId("device-surface-frame")).toHaveCount(0);
  const card = page.getByTestId("surface-snapshot-card").first();
  await expect(card).toHaveAttribute("data-surface", "device");
  await expect(card.getByTestId("surface-snapshot-card-commit")).toHaveText(
    "commit not recorded",
  );
  await expect(
    card.getByTestId("surface-snapshot-card-requested-by"),
  ).toHaveText(/^requested by .+$/);
  await expect(
    page.getByTestId("coding-session-surface-badge-device"),
  ).toHaveText("1");
  await shoot(page, testInfo, "sv34-device-snapshot", panel);
});

test("S1: a closed device reads Closed", async ({ page }, testInfo) => {
  const t = base();
  const panel = await openDevice(page, [
    available(t + 10),
    deviceState(DEVICE, t + 20, "open", "open-1"),
    snapshot((t + 30) * 1_000),
    deviceState(DEVICE, t + 40, "closed", "close-1"),
  ]);
  await expect(status(panel)).toHaveAttribute("data-state", "closed", {
    timeout: 15_000,
  });
  await expect(status(panel)).toHaveText("Closed");
  await expect(panel.getByTestId("device-surface-frame")).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-surface-badge-device"),
  ).toHaveCount(0);
  await shoot(page, testInfo, "sv34-device-closed", panel);
});

test("S1: no availability record reads not offered, never 'no devices'", async ({
  page,
}, testInfo) => {
  const panel = await openDevice(page, []);
  await expect(status(panel)).toHaveAttribute("data-state", "not-offered", {
    timeout: 15_000,
  });
  await expect(status(panel)).toContainText(/^Not offered by .+/);
  await expect(panel).toContainText(
    "This machine's provider does not offer devices.",
  );
  await expect(page.locator("body")).not.toContainText(/no devices/i);
  await shoot(page, testInfo, "sv34-device-not-offered", panel);
  await expect(page.locator("body")).not.toContainText(/no devices/i);
});

test("S1: iOS unavailable names the machine and the reason verbatim", async ({
  page,
}, testInfo) => {
  const panel = await openDevice(page, [
    deviceAvailability(DEVICE, base() + 10, {
      ios: { available: false, reason: "Xcode not found" },
      agentDevice: { installed: true, version: "0.21.12" },
    }),
  ]);
  await expect(status(panel)).toHaveAttribute("data-state", "unavailable", {
    timeout: 15_000,
  });
  await expect(status(panel)).toHaveText(
    /^Not offered by .+: Xcode not found$/,
  );
  await expect(page.locator("body")).not.toContainText(/no devices/i);
  await shoot(page, testInfo, "sv34-device-no-xcode", panel);
});

test("S1: agent-device unavailable still shows snapshots", async ({
  page,
}, testInfo) => {
  const t = base();
  const panel = await openDevice(page, [
    deviceAvailability(DEVICE, t + 10, {
      ios: { available: true },
      agentDevice: { installed: false, reason: AGENT_DEVICE_REASON },
    }),
    deviceState(DEVICE, t + 20, "open", "open-1"),
    snapshot(Date.now() - 60_000),
  ]);
  await expect(
    panel.getByTestId("device-surface-agent-device-note"),
  ).toHaveText(
    `agent-device unavailable: ${AGENT_DEVICE_REASON}. Snapshots still work.`,
    { timeout: 15_000 },
  );
  await expect(panel.getByTestId("device-surface-snapshot")).toBeVisible();
  await expect(status(panel)).toHaveAttribute("data-state", "snapshot", {
    timeout: 15_000,
  });
  await shoot(page, testInfo, "sv34-device-agent-device-unavailable", panel);
});

test("S2: a watcher sees Live · every 3 s from the authority's frames", async ({
  page,
}, testInfo) => {
  const t = base();
  const panel = await openDevice(page, [
    available(t + 10),
    deviceState(DEVICE, t + 20, "open", "open-1"),
    snapshot((t + 30) * 1_000),
  ]);
  await waitForLiveKind(page, KIND_SURFACE_FRAME);
  // Mounting the surface sends a watch to the slot's producer.
  await expect
    .poll(async () =>
      (await publishedWatches(page)).some(
        (watch) =>
          watch.action === "watch" &&
          JSON.stringify(watch.tags.slice(1, 4)) ===
            JSON.stringify([
              ["surface", "device"],
              ["d", DEVICE.slot],
              ["p", PROVIDER_PUBKEY],
            ]),
      ),
    )
    .toBe(true);
  await seedSurfaceEvents(page, [frame(1, DEVICE_FRAME_A)]);
  await expect(status(panel)).toHaveAttribute("data-state", "live");
  await expect(status(panel)).toHaveText("Live · every 3 s");
  await expect(panel.getByTestId("device-surface-frame")).toBeVisible();
  await seedSurfaceEvents(page, [frame(2, DEVICE_FRAME_B)]);
  await expect(status(panel)).toHaveText("Live · every 3 s");
  await shoot(page, testInfo, "sv34-device-live", panel);
});

test("S2: a last frame older than 30 s reads Stalled, never Live", async ({
  page,
}, testInfo) => {
  const t = base();
  await page.clock.install();
  const panel = await openDevice(page, [
    available(t + 10),
    deviceState(DEVICE, t + 20, "open", "open-1"),
  ]);
  await waitForLiveKind(page, KIND_SURFACE_FRAME);
  await seedSurfaceEvents(page, [frame(1, DEVICE_FRAME_A)]);
  await expect(status(panel)).toHaveAttribute("data-state", "live");
  // The producer goes quiet; 40 s pass with no frame.
  await page.clock.fastForward(40_000);
  await expect(status(panel)).toHaveAttribute("data-state", "stalled");
  await expect(status(panel)).toHaveText(/^Stalled · last frame \d+ s ago$/);
  await expect(status(panel)).not.toContainText("Live");
  await shoot(page, testInfo, "sv34-device-stalled", panel);
});

test("S2: a frame signed by a key that is not the authority is never shown", async ({
  page,
}) => {
  const t = base();
  const panel = await openDevice(page, [
    available(t + 10),
    deviceState(DEVICE, t + 20, "open", "open-1"),
  ]);
  await waitForLiveKind(page, KIND_SURFACE_FRAME);
  await seedSurfaceEvents(page, [
    frame(1, DEVICE_FRAME_STRANGER, STRANGER_SECRET),
  ]);
  // Give the stranger's frame time to land if anything would accept it.
  await page.waitForTimeout(1_500);
  await expect(status(panel)).not.toHaveAttribute("data-state", "live");
  await expect(panel.getByTestId("device-surface-frame")).toHaveCount(0);
  // The authority's frame is accepted on the same subscription.
  await seedSurfaceEvents(page, [frame(1, DEVICE_FRAME_A)]);
  await expect(status(panel)).toHaveAttribute("data-state", "live");
  const src = await panel
    .getByTestId("device-surface-frame")
    .getAttribute("src");
  expect(src ?? "").not.toContain(DEVICE_FRAME_STRANGER);
});

test("every shot is a distinct state", async ({ page: _page }, testInfo) => {
  const hashes = SHOT_NAMES.map((name) =>
    createHash("sha256")
      .update(readFileSync(shotPath(testInfo, SHOTS, name)))
      .digest("hex"),
  );
  expect(new Set(hashes).size).toBe(SHOT_NAMES.length);
});
