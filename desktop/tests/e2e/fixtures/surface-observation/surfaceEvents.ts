import { createHash } from "node:crypto";

import type { Page } from "@playwright/test";
import { finalizeEvent } from "nostr-tools/pure";

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
  KIND_SESSION_DEVICE_RECORD,
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_FRAME,
  KIND_SURFACE_SNAPSHOT,
} from "@/shared/constants/kinds";

/**
 * Signed fixtures for the C5 surface-observation specs. Every builder writes
 * the WIRE-C5 tag layout in its exact order (optional tags skipped, never
 * reordered), so a reader that is strict about the wire accepts them.
 */

/** The `engineering` mock channel (the bridge's seeded id). */
export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

export type SessionTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

export function signed(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

export function nowSecs(): number {
  return Math.floor(Date.now() / 1_000);
}

/** sha256 hex of a base64 blob's bytes. */
export function sha256OfBase64(base64: string): string {
  return createHash("sha256")
    .update(Buffer.from(base64, "base64"))
    .digest("hex");
}

/** Metadata (carrying `sessionRef`) plus two transcript rows. */
export function sessionEvents(input: {
  target: SessionTarget;
  sessionRef: string;
  title: string;
  secret: Uint8Array;
  createdAt: number;
}): RelayEvent[] {
  const { target, secret, createdAt } = input;
  const targetKey = buildCodingSessionTargetKey(target);
  const transcript = (seq: number, item: unknown) =>
    signed(
      KIND_CODING_SESSION_TRANSCRIPT,
      createdAt + seq,
      [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", targetKey],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(target, seq)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: target,
        eventSeq: seq,
        timestamp: (createdAt + seq) * 1_000,
        turnId: "surface-turn",
        item,
      }),
      secret,
    );
  return [
    signed(
      KIND_CODING_SESSION_METADATA,
      createdAt,
      [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", targetKey],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: null,
        repoRef: null,
        title: input.title,
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
          promptImage: false,
          context: false,
          diff: false,
          plan: false,
        },
        sessionRef: input.sessionRef,
      }),
      secret,
    ),
    transcript(1, {
      kind: "user_prompt",
      content: "Open the app and show me the settings screen.",
    }),
    transcript(2, {
      kind: "assistant_text",
      text: "The settings screen is up.",
    }),
  ];
}

// ---- Device (44255 record, 44253 snapshot, 24321 frame) ------------------

export type DeviceContext = {
  target: SessionTarget;
  /** The lifecycle command id that minted the generation (`csl-command`). */
  lifecycleCommand: string;
  /** 16 lowercase hex: the opaque slot id. */
  slot: string;
  providerSecret: Uint8Array;
};

function deviceRecord(
  device: DeviceContext,
  createdAt: number,
  type: "availability" | "state" | "refused" | "shot",
  extra: { slot?: boolean; command?: string },
  content: Record<string, unknown>,
): RelayEvent {
  const tags: string[][] = [
    ["h", CHANNEL_ID],
    ["sdv-v", "sdv1"],
    ["cs-target", buildCodingSessionTargetKey(device.target)],
    ["csl-command", device.lifecycleCommand],
    ["sdv-type", type],
  ];
  if (extra.slot) tags.push(["sdv-slot", device.slot]);
  if (extra.command) tags.push(["sdv-cmd", extra.command]);
  return signed(
    KIND_SESSION_DEVICE_RECORD,
    createdAt,
    tags,
    JSON.stringify({ type, ...content }),
    device.providerSecret,
  );
}

/** 44255 `availability` for the generation. */
export function deviceAvailability(
  device: DeviceContext,
  createdAt: number,
  input: {
    ios: { available: boolean; reason?: string };
    agentDevice: { installed: boolean; version?: string; reason?: string };
  },
): RelayEvent {
  return deviceRecord(
    device,
    createdAt,
    "availability",
    {},
    {
      platforms: {
        ios: input.ios,
        android: { available: false, reason: "Android SDK not found" },
      },
      agentDevice: input.agentDevice,
    },
  );
}

/** 44255 `state` for the slot. */
export function deviceState(
  device: DeviceContext,
  createdAt: number,
  state: "booting" | "open" | "closed" | "failed",
  command: string,
): RelayEvent {
  return deviceRecord(
    device,
    createdAt,
    "state",
    { slot: true, command },
    {
      state,
      platform: "ios",
      model: "iPhone 17",
      osVersion: "27.0",
      drivers: ["agent", "host-owner"],
      capture: { mode: "snapshot-poll", maxIntervalMs: 3_000 },
    },
  );
}

/** A 44253 snapshot blob URL the specs route to a fixture image. */
export function snapshotUrl(sha: string): string {
  return `https://example.com/e2e/surface/${sha}.png`;
}

/** Serve each fixture PNG at its {@link snapshotUrl}. */
export async function routeSnapshotBlobs(
  page: Page,
  blobs: readonly string[],
): Promise<void> {
  const bySha = new Map(blobs.map((blob) => [sha256OfBase64(blob), blob]));
  await page.route("https://example.com/e2e/surface/**", (route) => {
    const sha = /([0-9a-f]{64})\.png$/.exec(route.request().url())?.[1];
    const blob = sha ? bySha.get(sha) : undefined;
    if (!blob) return route.fulfill({ status: 404, body: "" });
    return route.fulfill({
      body: Buffer.from(blob, "base64"),
      contentType: "image/png",
    });
  });
}

/** 44253, in WIRE-C5 § 6 order. */
export function surfaceSnapshot(input: {
  surface: "device" | "preview";
  d: string;
  blob: string;
  dim: string;
  takenAtMs: number;
  providerPubkey: string;
  requestedBy?: string;
  commit?: { sha: string; state: "clean" | "dirty" };
  page?: string;
  title?: string;
  alt?: string;
  secret: Uint8Array;
}): RelayEvent {
  const sha = sha256OfBase64(input.blob);
  const tags: string[][] = [
    ["h", CHANNEL_ID],
    ["ssn-v", "1"],
    ["ssn-type", "snapshot"],
    ["surface", input.surface],
    ["d", input.d],
    ["x", sha],
    ["url", snapshotUrl(sha)],
    ["m", "image/png"],
    ["dim", input.dim],
    ["taken-at", String(input.takenAtMs)],
    ["provider", input.providerPubkey],
  ];
  if (input.requestedBy) {
    tags.push(["p", input.requestedBy, "", "requested-by"]);
  }
  if (input.commit) tags.push(["commit", input.commit.sha, input.commit.state]);
  if (input.page !== undefined) tags.push(["page", input.page]);
  if (input.title !== undefined) tags.push(["title", input.title]);
  return signed(
    KIND_SURFACE_SNAPSHOT,
    Math.floor(input.takenAtMs / 1_000),
    tags,
    input.alt ?? "",
    input.secret,
  );
}

/** 24321, in WIRE-C5 § 4 order. `t=paused|end` carries no content. */
export function surfaceFrame(input: {
  surface: "device" | "preview";
  d: string;
  t: "frame" | "paused" | "end";
  seq: number;
  epoch: number;
  cadenceMs: number;
  dim: string;
  capturedAtMs: number;
  jpeg?: string;
  actor?: string;
  secret: Uint8Array;
}): RelayEvent {
  const tags: string[][] = [
    ["h", CHANNEL_ID],
    ["surface", input.surface],
    ["d", input.d],
    ["t", input.t],
    ["seq", String(input.seq)],
    ["epoch", String(input.epoch)],
    ["cadence-ms", String(input.cadenceMs)],
    ["dim", input.dim],
    ["captured-at", String(input.capturedAtMs)],
  ];
  if (input.actor) tags.push(["actor", input.actor]);
  return signed(
    KIND_SURFACE_FRAME,
    Math.floor(input.capturedAtMs / 1_000),
    tags,
    input.t === "frame" ? (input.jpeg ?? "") : "",
    input.secret,
  );
}

/** 30626, in WIRE-C5 § 5 order. */
export function previewAnnounce(input: {
  sessionRef: string;
  status: "open" | "closed";
  csTarget?: string;
  providerPubkey?: string;
  page: string;
  title: string;
  viewport: string;
  stream: "frames" | "snapshots";
  createdAt: number;
  secret: Uint8Array;
}): RelayEvent {
  const tags: string[][] = [
    ["h", CHANNEL_ID],
    ["d", input.sessionRef],
    ["spa-v", "1"],
    ["status", input.status],
  ];
  if (input.csTarget) tags.push(["cs-target", input.csTarget]);
  if (input.providerPubkey) tags.push(["provider", input.providerPubkey]);
  tags.push(
    ["page", input.page],
    ["title", input.title],
    ["viewport", input.viewport],
    ["stream", input.stream],
    ["input", "synthetic"],
  );
  return signed(
    KIND_SESSION_PREVIEW_ANNOUNCE,
    input.createdAt,
    tags,
    "",
    input.secret,
  );
}

// ---- Page hooks -----------------------------------------------------------

/** Seed signed events into the mock relay (stored, or live-only if ephemeral). */
export async function seedSurfaceEvents(
  page: Page,
  events: readonly RelayEvent[],
): Promise<void> {
  await page.evaluate(
    ({ channelName, events: list }) => {
      const seed = window.__BEEKEEPER_E2E_SURFACE_SEED__;
      if (!seed) throw new Error("surface seeding hook is missing");
      for (const event of list) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: [...events] },
  );
}

/** Wait until the app holds a live subscription for `kind` in the channel. */
export async function waitForLiveKind(page: Page, kind: number): Promise<void> {
  await page.waitForFunction(
    ({ channelName, kind: wanted }) =>
      window.__BEEKEEPER_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
        channelName,
        kind: wanted,
      }) === true,
    { channelName: CHANNEL_NAME, kind },
    { timeout: 15_000 },
  );
}

/** The app's own published 24320 watches (parsed), oldest first. */
export async function publishedWatches(
  page: Page,
): Promise<Array<{ pubkey: string; tags: string[][]; action: string }>> {
  return page.evaluate(() =>
    (window.__BEEKEEPER_E2E_SURFACE_PUBLISHED__ ?? [])
      .filter((event) => event.kind === 24320)
      .map((event) => {
        let action = "";
        try {
          action = String(
            (JSON.parse(event.content) as { action?: unknown }).action ?? "",
          );
        } catch {
          action = "";
        }
        return { pubkey: event.pubkey, tags: event.tags, action };
      }),
  );
}
