import { expect, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  buildCodingSessionHandoverContent,
  buildCodingSessionHandoverTags,
  KIND_CODING_SESSION_HANDOVER,
} from "@/features/coding-sessions/lib/codingSessionHandoverWire";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge, TEST_IDENTITIES } from "../../helpers/bridge";
import { seedActiveIdentity } from "../../helpers/onboarding";

/**
 * The absent-participant handover, end to end on the mock bridge.
 *
 * A founds a session on their own provider and goes quiet; B (the signed-in
 * viewer) holds a live operator grant. Every event below is a **real signed
 * event** — the panel verifies signatures before it reads anything, so a
 * fabricated one would prove nothing.
 *
 * The one thing stubbed is the native `handover_prepare_checkout` command:
 * Playwright has no Rust side, and the command's own behaviour (fetch, verify
 * the sha, apply or refuse the patch) is covered by `handover_tests.rs`
 * against real git repositories. The stub records the exact request the app
 * sent, so this spec still proves the app asked for the right ref and sha.
 */

const FOUNDER_SECRET = generateSecretKey();
export const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const PROVIDER_A_SECRET = generateSecretKey();
export const PROVIDER_A_PUBKEY = getPublicKey(PROVIDER_A_SECRET);
const CAROL_SECRET = generateSecretKey();
export const CAROL_PUBKEY = getPublicKey(CAROL_SECRET);
const CAROL_BODY_SECRET = generateSecretKey();
export const CAROL_BODY_PUBKEY = getPublicKey(CAROL_BODY_SECRET);
const RELAY_SECRET = generateSecretKey();
export const RELAY_PUBKEY = getPublicKey(RELAY_SECRET);
const LOCAL_BODY_SECRET = generateSecretKey();
/** This computer's provider authority — the body B claims onto. */
export const LOCAL_BODY_PUBKEY = getPublicKey(LOCAL_BODY_SECRET);
/**
 * The signed-in viewer: the operator who continues A's work.
 *
 * A **real** key, seeded as the active identity, because everything this panel
 * publishes is verified before it is read: the mock bridge's default identity
 * signs `mocksig…`, and a takeover signed with that would be refused by the
 * app's own authority projection — correctly, and uselessly for this spec.
 */
export const VIEWER_PUBKEY = TEST_IDENTITIES.tyler.pubkey;

export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
export const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "9f2f0e12-3d05-4b0a-9f4e-8c2b1d6a7e30";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const BASE_CREATED_AT = 1_800_000_000;
export const WIP_REF = "refs/heads/wip/builder/abc12345";
export const WIP_SHA = "2b".repeat(20);
export const BASE_SHA = "1a".repeat(20);
export const CHECKOUT_PATH = "/tmp/handover-checkout";
export const REPO_REF = "30617:aa/beekeeper";
/** A's uncommitted bytes, as the NIP-34 patch event carries them. */
export const PATCH_TEXT = [
  "diff --git a/keep.txt b/keep.txt",
  "--- a/keep.txt",
  "+++ b/keep.txt",
  "@@ -1,2 +1,3 @@",
  " one",
  " two",
  "+three",
  "",
].join("\n");

/** What the stubbed native command answers, and what the app recorded. */
export const CHECKOUT_REPORT = {
  branch: "handover/5b7e1c2a",
  checkedOutSha: WIP_SHA,
  recovered: [`wip-ref ${WIP_REF} at ${WIP_SHA}`],
  missing: ["the 812-byte patch did not apply to keep.txt"],
};

function signed(
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

function genesisEvent(): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    founderPubkey: FOUNDER_PUBKEY,
  });
  return signed(
    built.kind,
    BASE_CREATED_AT - 2,
    built.tags,
    built.content,
    FOUNDER_SECRET,
  );
}

function executionEvents(genesisRef: string): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "a-primary",
    providerAuthorityPubkey: PROVIDER_A_PUBKEY,
    model: "sonnet",
    title: "Absent participant handover",
    initialTurn: null,
  });
  return [
    signed(
      built.kind,
      BASE_CREATED_AT - 1,
      built.tags,
      built.content,
      FOUNDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
      PROVIDER_A_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_METADATA,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: "Absent participant handover",
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        // A's machine went quiet: the last thing it published, and no lease
        // behind it. The panel must never read that as "fenced".
        status: "disconnected",
        branch: "work/handover",
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
      PROVIDER_A_SECRET,
    ),
  ];
}

/** One accepted authority link and the relay receipt that accepted it. */
function link(input: {
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: string;
  granteePubkey: string;
  bodyPubkey?: string;
  secret: Uint8Array;
  createdAt: number;
}): { transition: RelayEvent; receipt: RelayEvent } {
  const payload = {
    genesisRef: input.genesisRef,
    prevAccepted: input.prevAccepted,
    seq: input.seq,
    type: input.type,
    granteePubkey: input.granteePubkey,
    ...(input.bodyPubkey ? { bodyPubkey: input.bodyPubkey } : {}),
  };
  const transition = signed(
    KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    input.createdAt,
    [
      ["h", CHANNEL_ID],
      ["csat-v", "csat1-1"],
      ["csat-genesis", input.genesisRef],
    ],
    JSON.stringify(payload),
    input.secret,
  );
  const receipt = signed(
    40099,
    input.createdAt + 1,
    [["h", CHANNEL_ID]],
    JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: input.genesisRef,
      acceptedEventId: transition.id,
      seq: input.seq,
      transitionType: input.type,
      granteePubkey: input.granteePubkey,
      ...(input.bodyPubkey ? { bodyPubkey: input.bodyPubkey } : {}),
    }),
    RELAY_SECRET,
  );
  return { transition, receipt };
}

/** A's durable checkpoint: what the work is, and what was not preserved. */
function checkpointEvent(genesisRef: string, patchEventId: string): RelayEvent {
  const body = {
    prevCheckpointRef: null,
    task: "Fold handover records for one umbrella",
    assignmentRefs: [],
    decisions: [
      {
        eventId: "cd".repeat(32),
        summary: "Reconstruct rather than resume: A's machine is gone",
      },
    ],
    revision: {
      repoRef: REPO_REF,
      baseSha: BASE_SHA,
      headSha: WIP_SHA,
      branch: "work/handover",
      dirty: true,
      // Not everything was preserved, and the panel must say so even though
      // the wip ref below did land.
      preserved: "partial",
    },
    artifacts: [
      {
        kind: "wip-ref",
        repoRef: REPO_REF,
        ref: WIP_REF,
        sha: WIP_SHA,
      },
      // A's uncommitted bytes: a pointer to the NIP-34 patch event below, not
      // the bytes themselves. The desktop fetches it, verifies it was signed
      // by this checkpoint's author, and only then applies it.
      {
        kind: "patch",
        repoRef: REPO_REF,
        eventId: patchEventId,
        baseSha: BASE_SHA,
        bytes: PATCH_TEXT.length,
      },
    ],
    tests: [{ name: "desktop unit", command: "pnpm test", outcome: "not-run" }],
    unresolved: ["Whether the fence covers sibling executions"],
    nextAction: "Mount the handover panel in the session workspace",
    missing: ["uncommitted notes in target/, above the patch bound"],
  };
  return signed(
    KIND_CODING_SESSION_HANDOVER,
    BASE_CREATED_AT + 10,
    buildCodingSessionHandoverTags({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef,
      type: "checkpoint",
    }),
    buildCodingSessionHandoverContent({
      sessionRef: SESSION_REF,
      genesisRef,
      type: "checkpoint",
      body,
    }),
    FOUNDER_SECRET,
  );
}

/** The NIP-34 patch event the checkpoint's `patch` artifact points at. */
function patchEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: 1617,
      created_at: BASE_CREATED_AT + 9,
      tags: [
        ["a", `30617:${FOUNDER_PUBKEY}:beekeeper`],
        ["parent-commit", BASE_SHA],
      ],
      content: PATCH_TEXT,
    },
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
}

export type HandoverVariant = "claimable" | "fenced" | "retired";

/** Seed one governed session in the state this scenario is about, and open it. */
export async function openHandoverSession(
  page: Page,
  variant: HandoverVariant,
  viewport: { width: number; height: number } = { width: 1280, height: 900 },
): Promise<{ genesisRef: string; headEventId: string }> {
  const genesis = genesisEvent();
  // Built first: its **real** event id is what the checkpoint's `patch`
  // artifact points at, so the app's id-keyed fetch finds this exact event.
  const patch = patchEvent();
  const grant = link({
    genesisRef: genesis.id,
    prevAccepted: null,
    seq: 1,
    type: "grant-operator",
    granteePubkey: VIEWER_PUBKEY,
    secret: FOUNDER_SECRET,
    createdAt: BASE_CREATED_AT + 1,
  });
  const events: RelayEvent[] = [
    genesis,
    ...executionEvents(genesis.id),
    grant.transition,
    grant.receipt,
    // The fenced view is deliberately checkpoint-less: what a fenced viewer is
    // offered there is the way back, not a reconstruction of work nobody
    // wrote down.
    ...(variant === "fenced" ? [] : [checkpointEvent(genesis.id, patch.id)]),
  ];

  if (variant === "fenced") {
    // Carol is granted, then claims the whole session onto her own body. The
    // execution this viewer is looking at is A's, so it is fenced.
    const carolGrant = link({
      genesisRef: genesis.id,
      prevAccepted: grant.transition.id,
      seq: 2,
      type: "grant-operator",
      granteePubkey: CAROL_PUBKEY,
      secret: FOUNDER_SECRET,
      createdAt: BASE_CREATED_AT + 3,
    });
    const takeover = link({
      genesisRef: genesis.id,
      prevAccepted: carolGrant.transition.id,
      seq: 3,
      type: "takeover",
      granteePubkey: CAROL_PUBKEY,
      bodyPubkey: CAROL_BODY_PUBKEY,
      secret: CAROL_SECRET,
      createdAt: BASE_CREATED_AT + 5,
    });
    events.push(
      carolGrant.transition,
      carolGrant.receipt,
      takeover.transition,
      takeover.receipt,
    );
  }

  if (variant === "retired") {
    // The relay's own statement that it applied a whole-session deletion.
    events.push(
      signed(
        40099,
        BASE_CREATED_AT + 20,
        [["h", CHANNEL_ID]],
        JSON.stringify({
          type: "coding_session_deletion_accepted",
          genesisRef: genesis.id,
          sessionRef: SESSION_REF,
          deletionEventId: "ee".repeat(32),
          channelId: CHANNEL_ID,
        }),
        RELAY_SECRET,
      ),
    );
  }

  // Before the bridge, deliberately: the app reads the stored identity on
  // mount, and the bridge triggers that mount.
  await seedActiveIdentity(page, TEST_IDENTITIES.tyler);
  // The patch event lives in the mock relay's project store, which is where a
  // `kinds:[1617] ids:[…]` read is answered — the same read the app makes.
  await page.addInitScript((event) => {
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
      ...(window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ ?? []),
      event,
    ];
  }, patch);
  await installMockBridge(page, {
    relaySelf: RELAY_PUBKEY,
    // This computer's own provider: the body a claim from here names.
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: LOCAL_BODY_PUBKEY,
      instanceId: "b0b0b0b0b0b0b0b0",
    },
    codingSessionProviderRuntimes: [
      {
        instanceRef: "claude-primary",
        runtime: "claude",
        driver: "claude-agent-acp",
        label: "Claude Code",
        authState: "ready",
        defaultModel: "sonnet",
        allowedModels: ["sonnet"],
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: false,
          plan: true,
          promptImage: false,
        },
        installed: true,
      },
    ],
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_A_PUBKEY, label: "A's computer" },
      ],
    },
    searchProfiles: [
      { pubkey: FOUNDER_PUBKEY, displayName: "Ada" },
      { pubkey: CAROL_PUBKEY, displayName: "Carol" },
      { pubkey: VIEWER_PUBKEY, displayName: "Tyler" },
    ],
  });
  // Open at a width where the channel rail is on screen, then narrow: a 640px
  // window hides the rail, and this helper is about the panel, not the rail.
  await page.setViewportSize({ width: 1280, height: viewport.height });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of seeds) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, seeds: events },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  if (viewport.width !== 1280) await page.setViewportSize(viewport);
  return { genesisRef: genesis.id, headEventId: grant.transition.id };
}

/**
 * Land somebody else's takeover **while the session is open**, as the relay
 * would: two accepted links, fanned out live.
 *
 * Nothing is clicked afterwards. If the panel changes, it changed because the
 * events arrived — which is the whole question.
 */
export async function landForeignTakeoverLive(
  page: Page,
  chain: { genesisRef: string; headEventId: string },
): Promise<void> {
  const carolGrant = link({
    genesisRef: chain.genesisRef,
    prevAccepted: chain.headEventId,
    seq: 2,
    type: "grant-operator",
    granteePubkey: CAROL_PUBKEY,
    secret: FOUNDER_SECRET,
    createdAt: BASE_CREATED_AT + 50,
  });
  const takeover = link({
    genesisRef: chain.genesisRef,
    prevAccepted: carolGrant.transition.id,
    seq: 3,
    type: "takeover",
    granteePubkey: CAROL_PUBKEY,
    bodyPubkey: CAROL_BODY_PUBKEY,
    secret: CAROL_SECRET,
    createdAt: BASE_CREATED_AT + 52,
  });
  for (const event of [
    carolGrant.transition,
    carolGrant.receipt,
    takeover.transition,
    takeover.receipt,
  ]) {
    await seedEvent(page, event);
  }
}

/**
 * Stub the native checkout command and record what the app asked it for.
 *
 * Installed after the bridge is up, because it wraps the bridge's own mocked
 * `invoke`: everything except this one command still goes to the bridge.
 */
export async function stubHandoverCheckout(page: Page): Promise<void> {
  await page.evaluate((report) => {
    const internals = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    const original = internals.invoke.bind(internals);
    (
      window as unknown as { __HANDOVER_CHECKOUTS__: unknown[] }
    ).__HANDOVER_CHECKOUTS__ = [];
    internals.invoke = async (command: string, args?: unknown) => {
      if (command === "handover_prepare_checkout") {
        (
          window as unknown as { __HANDOVER_CHECKOUTS__: unknown[] }
        ).__HANDOVER_CHECKOUTS__.push(args);
        return report;
      }
      return original(command, args);
    };
  }, CHECKOUT_REPORT);
}

/** Every `handover_prepare_checkout` request the app has made so far. */
export async function recordedCheckouts(
  page: Page,
): Promise<Array<{ request: Record<string, unknown> }>> {
  return page.evaluate(
    () =>
      (window as unknown as { __HANDOVER_CHECKOUTS__?: unknown[] })
        .__HANDOVER_CHECKOUTS__ as Array<{ request: Record<string, unknown> }>,
  );
}

/** Read events back out of the mock relay, exactly as a client would. */
export async function queryMockEvents(
  page: Page,
  filter: Record<string, unknown>,
): Promise<RelayEvent[]> {
  return page.evaluate(async (one) => {
    const invoke = window.__BUZZ_E2E_INVOKE_MOCK_COMMAND__;
    if (!invoke) throw new Error("mock command hook is missing");
    const answer = (await invoke("query_relay_filters", {
      filters: [one],
    })) as RelayEvent[] | { events?: RelayEvent[] };
    return Array.isArray(answer) ? answer : (answer.events ?? []);
  }, filter);
}

/**
 * Answer the app's takeover the way the relay would: one acceptance receipt.
 *
 * Polls the mock relay for the signed 44228 the app just published, then seeds
 * the matching 40099. Nothing is invented — the receipt names the event the
 * app actually signed.
 */
export async function acceptPublishedTakeover(page: Page): Promise<RelayEvent> {
  const takeover = await expectEventually(page, async () => {
    const events = await queryMockEvents(page, {
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      "#h": [CHANNEL_ID],
      limit: 50,
    });
    return (
      events.find((event) => {
        try {
          const content = JSON.parse(event.content) as { type?: string };
          return content.type === "takeover";
        } catch {
          return false;
        }
      }) ?? null
    );
  });
  const payload = JSON.parse(takeover.content) as {
    genesisRef: string;
    seq: number;
    granteePubkey: string;
    bodyPubkey: string;
  };
  const receipt = signed(
    40099,
    BASE_CREATED_AT + 30,
    [["h", CHANNEL_ID]],
    JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: payload.genesisRef,
      acceptedEventId: takeover.id,
      seq: payload.seq,
      transitionType: "takeover",
      granteePubkey: payload.granteePubkey,
      bodyPubkey: payload.bodyPubkey,
    }),
    RELAY_SECRET,
  );
  await seedEvent(page, receipt);
  return takeover;
}

/** Answer the app's `session.create` the way a provider would. */
export async function answerPublishedCreate(page: Page): Promise<RelayEvent> {
  const create = await expectEventually(page, async () => {
    const events = await queryMockEvents(page, {
      kinds: [44221],
      "#h": [CHANNEL_ID],
      limit: 50,
    });
    return (
      events.find((event) => {
        try {
          const content = JSON.parse(event.content) as {
            commandId?: string;
            action?: { type?: string; sessionRef?: string };
          };
          return (
            content.action?.type === "session.create" &&
            content.action.sessionRef === SESSION_REF &&
            content.commandId !== undefined &&
            event.pubkey === VIEWER_PUBKEY
          );
        } catch {
          return false;
        }
      }) ?? null
    );
  });
  const commandId = (JSON.parse(create.content) as { commandId: string })
    .commandId;
  const target = {
    driver: "claude-agent-acp",
    instanceId: "b-instance",
    sessionId: "99999999-8888-7777-6666-555555555555",
    generation: 1,
  };
  await seedEvent(
    page,
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT + 40,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "created",
        session: target,
        error: null,
      }),
      PROVIDER_A_SECRET,
    ),
  );
  return create;
}

async function seedEvent(page: Page, event: RelayEvent): Promise<void> {
  await page.evaluate(
    ({ channelName, one }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      seed({ channelName, event: one });
    },
    { channelName: CHANNEL_NAME, one: event },
  );
}

/** Poll a page-side read until it answers, or fail saying what was awaited. */
async function expectEventually(
  page: Page,
  read: () => Promise<RelayEvent | null>,
  timeoutMs = 15_000,
): Promise<RelayEvent> {
  const deadline = Date.now() + timeoutMs;
  let last: RelayEvent | null = null;
  while (Date.now() < deadline) {
    last = await read();
    if (last) return last;
    await page.waitForTimeout(150);
  }
  throw new Error("the awaited event never reached the mock relay");
}
