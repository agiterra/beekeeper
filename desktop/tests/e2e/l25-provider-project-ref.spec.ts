import { expect, test, type Page } from "@playwright/test";
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
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_PROJECT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * LANE-L25 — the join dialog ("Add a provider to this session") now resolves
 * the project its channel belongs to and threads it into the seat field's
 * pack preview (LANE-L23's `NewCodingSessionAgentSeatField`), where before it
 * always passed `null`. `AddCodingSessionProviderDialog.tsx:404` was the one
 * caller L23C's finalizer note left unwired.
 *
 * This drives the real dialog against a real signed project event (kind
 * 30621, filing the channel via a `channel` tag) and a real signed session,
 * seats a real managed agent, and reads the preview line's text off the
 * actual rendered app — not a harness mount.
 */

const SHOTS = "test-results/l25-provider-project-ref";

const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const FOUNDER_SECRET = Uint8Array.from(
  (FOUNDER_IDENTITY.privateKey.match(/.{2}/g) ?? []).map((byte) =>
    Number.parseInt(byte, 16),
  ),
);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const AGENT_SECRET = generateSecretKey();
const AGENT_PUBKEY = getPublicKey(AGENT_SECRET);

/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const PROJECT_DTAG = "l25-project-ref";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const BASE_CREATED_AT = 1_800_000_000;
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-founded-session";
const SEAT_ROLE = "builder";

/** The project this test files `engineering` under (kind 30621). */
function projectEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_PROJECT,
      created_at: BASE_CREATED_AT - 10,
      tags: [
        ["d", PROJECT_DTAG],
        ["name", "L25 Project"],
        ["channel", CHANNEL_ID],
      ],
      content: "",
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
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
    commandId: COMMAND_ID,
    // Deliberately null: the project must resolve from the channel's own
    // membership (the `projectIdByChannel` bucket), exactly like a session
    // founded before LANE-L20 gave `session.create` a `repoRef`/`projectRef`
    // of its own. If this test only passed with a create-carried
    // `projectRef`, it would not be proving what LANE-L25 actually fixed.
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Prove the join dialog knows its project",
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
  ) as unknown as RelayEvent;
  return [create, receipt];
}

function metadataEvent(): RelayEvent {
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
        title: "Prove the join dialog knows its project",
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
  ) as unknown as RelayEvent;
}

async function openApp(page: Page) {
  // Before `installMockBridge`: the bridge reads this at boot and both signs
  // and answers `get_identity` as this key, so the seeded genesis below is
  // founded by the person driving the app.
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, FOUNDER_IDENTITY);
  // Project events (kind 30621) are served from a dedicated mock project
  // store, not the generic per-channel message store
  // `__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__` writes into — this is the seam
  // `getMockProjectEventStore` documents for exactly this case: standalone
  // project-scoped events a test needs before the app boots.
  await page.addInitScript((event) => {
    (
      window as unknown as {
        __BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__?: unknown[];
      }
    ).__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = [event];
  }, projectEvent());
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
      instanceId: "l25instanceid00",
    },
    codingSessionProviderRuntimes: [
      {
        instanceRef: "claude-primary",
        runtime: "claude",
        driver: "claude-acp",
        label: "Claude Code",
        authState: "ready",
        defaultModel: "sonnet",
        allowedModels: ["default", "sonnet", "haiku"],
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: false,
          context: false,
          diff: false,
          plan: true,
        },
      },
    ],
    managedAgents: [
      {
        pubkey: AGENT_PUBKEY,
        name: "Ada",
        personaId: "persona-ada",
        status: "running",
      },
    ],
    // LANE-L23's binding for `preview_coding_session_seat_pack`: the
    // "shipped defaults" rung, so the assertion below is about *whether the
    // probe fires at all* (which is what a poisoned/absent `projectRef`
    // prevented) rather than which rung answered.
    codingSessionPackStatusByRole: {
      [SEAT_ROLE]: {
        packStaged: true,
        origin: "shipped",
        role: SEAT_ROLE,
        packDir: "/fake/packs/builder",
        personaId: "persona-ada",
        packRef: null,
        refusal: null,
        reason: null,
      },
    },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

async function openJoinDialogOnSeededSession(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toBeVisible();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: (() => {
        const genesis = genesisEvent();
        return [
          genesis,
          ...createAndReceiptEvents(genesis.id),
          metadataEvent(),
        ];
      })(),
    },
  );
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toHaveAttribute("aria-label", "Coding sessions (1)", { timeout: 15_000 });
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-header")).toBeVisible({
    timeout: 15_000,
  });
  // Add provider lives in the header's `⋯` session-actions menu.
  await page.getByTestId("coding-session-overflow").click();
  await page.getByTestId("coding-session-overflow-add-provider").click();
  await expect(
    page.getByTestId("add-coding-session-provider-dialog"),
  ).toBeVisible();
}

test("LANE-L25: the join dialog resolves its session's project and the seat field's pack preview renders", async ({
  page,
}) => {
  await openApp(page);
  await openJoinDialogOnSeededSession(page);

  const seatField = page.getByTestId("new-coding-session-seat");
  await expect(seatField).toBeVisible();

  // Before this lane, `AddCodingSessionProviderDialog` always passed `null`
  // for `projectRef` (`AddCodingSessionProviderDialog.tsx:404`), so
  // `useCodingSessionPackStatusPreview`'s effect short-circuited before ever
  // calling the probe and this line never appeared, seat or no seat.
  await page.getByTestId("new-coding-session-seat-agent").click();
  await page
    .getByTestId(`new-coding-session-seat-agent-${AGENT_PUBKEY}`)
    .click();
  await page.getByTestId("new-coding-session-seat-role").fill(SEAT_ROLE);

  const preview = page.getByTestId("new-coding-session-pack-preview");
  await expect(preview).toBeVisible({ timeout: 10_000 });
  await expect(preview).toHaveText(
    "Stages the builder pack this build of Beekeeper ships. Give the project a packs repository to version it.",
  );
  await waitForAnimations(page);
  await seatField.screenshot({ path: `${SHOTS}/01-join-pack-preview.png` });
});
