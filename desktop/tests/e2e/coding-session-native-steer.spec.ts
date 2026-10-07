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
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
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

/**
 * Native steering, as the browser sees it (docs/NATIVE_STEERING_IMPL.md §3.5).
 *
 * The execution is seeded *working* with `capabilities.threadSteer: true`, so
 * the composer's primary act is Steer and the 44220 it signs asks for
 * `deliver: "steer"`. Everything after that is the provider's own signed
 * word, seeded through the mock relay exactly as the relay would carry it:
 *
 * 1. `turn_injected` relabels the pending row "Injected into the running
 *    turn"; the `user_prompt{steered: true}` echo naming the command retires
 *    the row and renders with a visible "steered" marker beside its author.
 * 2. `turn_delivery_unknown` relabels the row "Delivery unknown — …", keeps
 *    the words on screen (never restores them to the editor), and offers
 *    Dismiss — the person settles it, not a clock.
 *
 * The viewer *is* the founder (identity override), so the composer is not
 * authority-gated; the provider is a separate key the mocked global config
 * names as an ingress authority, which is what makes the seeded receipts
 * trusted rather than rejected.
 */

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
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
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

function transcriptEvent(eventSeq: number, item: unknown): RelayEvent {
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
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: TARGET,
        eventSeq,
        timestamp: BASE_TIMESTAMP_MS + eventSeq * 1_000,
        turnId: RUNNING_TURN_ID,
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
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events },
  );
}

/** Send one instruction while the execution works, and return the command it signed. */
async function steerFromComposer(page: Page, text: string) {
  const before = await page.evaluate(
    () => (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? []).length,
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
  await steer.click();

  const signed = await page.waitForFunction((count) => {
    const events = window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [];
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

test("a steer the runtime injected is said so on the row, then settles on its steered echo", async ({
  page,
}) => {
  const commandId = await steerFromComposer(
    page,
    "Also run the integration suite",
  );
  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Also run the integration suite");

  await seed(page, [
    turnReceiptEvent({
      commandId,
      status: "turn_injected",
      createdAt: BASE_CREATED_AT + 10,
    }),
  ]);
  await expect(row).toHaveAttribute("data-pending-state", "injected");
  await expect(
    row.getByTestId("coding-session-pending-turn-status"),
  ).toHaveText("Injected into the running turn");
  await waitForAnimations(page);
  await row.screenshot({
    path: "test-results/screenshots/native-steer-injected-row.png",
  });

  // The provider's own echo of the steered words, on the running turn and
  // naming the command, is what retires the row — the receipt never does.
  await seed(page, [
    transcriptEvent(3, {
      kind: "user_prompt",
      content: "Also run the integration suite",
      steered: true,
      commandId,
      operatorPubkey: FOUNDER_PUBKEY,
    }),
  ]);
  await expect(page.getByTestId("coding-session-pending-turn")).toHaveCount(0);
  const steered = page
    .getByTestId("coding-session-user-message")
    .filter({ hasText: "Also run the integration suite" });
  await expect(steered).toHaveCount(1);
  await expect(
    steered.getByTestId("coding-session-user-message-steered"),
  ).toHaveText("steered");
  // The prompt that opened the turn carries no such marker.
  await expect(
    page
      .getByTestId("coding-session-user-message")
      .filter({ hasText: "Fix the reconnect bug" })
      .getByTestId("coding-session-user-message-steered"),
  ).toHaveCount(0);
  // Nothing came back to the editor: the words went in.
  await expect(page.getByLabel("Coding-session instruction")).toHaveValue("");
  await waitForAnimations(page);
  await steered.screenshot({
    path: "test-results/screenshots/native-steer-steered-echo.png",
  });
});

test("a steer whose delivery is unknown keeps its words on the row, says why, and is dismissed by the person", async ({
  page,
}) => {
  const commandId = await steerFromComposer(
    page,
    "Stop after the first failure",
  );
  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveCount(1);

  await seed(page, [
    turnReceiptEvent({
      commandId,
      status: "turn_delivery_unknown",
      createdAt: BASE_CREATED_AT + 10,
    }),
  ]);
  await expect(row).toHaveAttribute("data-pending-state", "unknown");
  const status = row.getByTestId("coding-session-pending-turn-status");
  await expect(status).toContainText(
    "Delivery unknown — the prompt ended before the acknowledgement arrived",
  );
  // The words stay where the person can read them, and are not put back in
  // the editor as if refused — they may already be inside the running turn.
  await expect(row).toContainText("Stop after the first failure");
  await expect(page.getByLabel("Coding-session instruction")).toHaveValue("");
  await expect(page.getByTestId("coding-session-composer-error")).toHaveCount(
    0,
  );
  await waitForAnimations(page);
  await row.screenshot({
    path: "test-results/screenshots/native-steer-delivery-unknown-row.png",
  });

  // The one exit: the person's own dismissal. The control lives inside the
  // bubble, which stays clear of the composer dock; the caption line under
  // it does not at full scroll (the plain workspace's `pb-44` reserve is a
  // constant, one caption short of the dock at this viewport).
  await row.getByTestId("coding-session-pending-turn-dismiss").click();
  await expect(page.getByTestId("coding-session-pending-turn")).toHaveCount(0);
  await expect(page.getByLabel("Coding-session instruction")).toHaveValue("");
});

/** Press the named composer control and return the 44220 it signed. */
async function sendFromComposer(
  page: Page,
  text: string,
  control:
    | "coding-session-composer-steer"
    | "coding-session-composer-queue-next",
) {
  const before = await page.evaluate(
    () => (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? []).length,
  );
  const editor = page.getByLabel("Coding-session instruction");
  await expect(editor).toBeEnabled();
  await editor.fill(text);
  const button = page.getByTestId(control);
  await expect(button).toBeVisible();
  await button.click();
  const signed = await page.waitForFunction((count) => {
    const events = window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [];
    return events.slice(count).find((event) => event.kind === 44220) ?? null;
  }, before);
  const value = (await signed.jsonValue()) as { content: string };
  return JSON.parse(value.content) as {
    commandId: string;
    action: { type: string; text: string; deliver?: string };
  };
}

// The choice the composer used to make for the person. A working execution
// that can steer now offers both classes, and asking for the boundary signs a
// boundary command — not a steer the provider would have to downgrade.
test("a working execution can be asked for the next boundary instead of the running turn", async ({
  page,
}) => {
  // Both controls are present, and the sentence naming the difference is on
  // screen rather than in a tooltip.
  await expect(page.getByTestId("coding-session-composer-steer")).toBeVisible();
  await expect(
    page.getByTestId("coding-session-composer-queue-next"),
  ).toBeVisible();
  await expect(
    page.getByTestId("coding-session-composer-delivery-hint"),
  ).toHaveText(
    "Steer joins the turn that is running. Queue next runs after it.",
  );

  const command = await sendFromComposer(
    page,
    "When this finishes, run the gate",
    "coding-session-composer-queue-next",
  );
  expect(command.action.type).toBe("thread.turn.start");
  expect(command.action.text).toBe("When this finishes, run the gate");
  // The whole point: the class the person chose, not the one the composer
  // would have chosen for them. `boundary` is the wire default and this fork
  // omits the key at its default (NIP-CSC), so the absence of `deliver` *is*
  // the boundary class — what must not appear is a steer.
  expect(command.action.deliver ?? "boundary").toBe("boundary");

  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("When this finishes, run the gate");
  await expect(page.getByLabel("Coding-session instruction")).toHaveValue("");
  await waitForAnimations(page);
  await row.screenshot({
    path: "test-results/screenshots/native-steer-queue-next-row.png",
  });
});

// The degrade reason used to be discarded on the way to the row, which then
// rendered one fixed sentence: "Delivered at the next turn boundary — this
// provider cannot steer". Both halves could be false. This is the half that
// was false most often: a turn that ended before the input reached it says
// nothing about whether the runtime can steer.
test("a downgraded steer names the provider's own reason and never claims delivery", async ({
  page,
}) => {
  const commandId = await steerFromComposer(page, "Check the other file too");
  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveCount(1);

  await seed(page, [
    turnReceiptEvent({
      commandId,
      status: "turn_degraded",
      createdAt: BASE_CREATED_AT + 10,
    }),
  ]);
  await expect(row).toHaveAttribute("data-pending-state", "degraded");
  const status = row.getByTestId("coding-session-pending-turn-status");
  await expect(status).toContainText(
    "Not steered — the turn ended before this reached it",
  );
  await expect(status).toContainText("queued for the next turn boundary");
  // The two claims that were wrong before.
  await expect(status).not.toContainText("Delivered at");
  await expect(status).not.toContainText("does not offer mid-turn steering");
  // The words stay sent: a downgrade is not a refusal.
  await expect(page.getByLabel("Coding-session instruction")).toHaveValue("");
  await waitForAnimations(page);
  await row.screenshot({
    path: "test-results/screenshots/native-steer-degraded-reason-row.png",
  });
});

// Recovery for an input nobody can account for: the words come back beside
// whatever is being written now, the original row is untouched, and nothing
// is published. The narrow viewport and 250% text are where a control that
// only just fits stops being reachable.
test("copy to draft recovers the words at 250% text without sending or settling anything", async ({
  page,
}) => {
  const commandId = await steerFromComposer(
    page,
    "Stop after the first failure",
  );
  const row = page.getByTestId("coding-session-pending-turn");
  await expect(row).toHaveCount(1);
  await seed(page, [
    turnReceiptEvent({
      commandId,
      status: "turn_delivery_unknown",
      createdAt: BASE_CREATED_AT + 10,
    }),
  ]);
  await expect(row).toHaveAttribute("data-pending-state", "unknown");

  // Now the conditions the recovery has to survive: a narrow window and text
  // at 250%, where every label in this row wraps and the bubble grows.
  //
  // The *width* is the narrow case. The height is deliberately generous, and
  // that is a disclosure rather than a convenience. At 250% this panel's own
  // chrome is about 1,000px tall — a 140px header, the founder banner, and a
  // composer dock that measures 635px once its controls and hint wrap — so in
  // a 720px window the conversation has roughly 257px and the chrome covers
  // all of it: nothing in the transcript can be clicked, including this
  // control. That is a pre-existing responsive limit of the panel, not of
  // this slice — measured with the delivery hint removed entirely, the header
  // and the send control still occupied identical coordinates — and it is
  // reported in this branch's history note rather than asserted here.
  await page.setViewportSize({ width: 900, height: 1600 });
  await page.addStyleTag({ content: "html { font-size: 250% }" });

  // What the row promises before it is pressed: it may already have arrived.
  await expect(row).toContainText("may already have reached the running turn");

  // Something else is already half-written.
  const editor = page.getByLabel("Coding-session instruction");
  await editor.fill("meanwhile, check CI");

  const signedBefore = await page.evaluate(
    () => (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? []).length,
  );
  // Where a person is after sending, and after a resize: looking at the
  // latest. The conversation reserves the composer dock's *measured* height,
  // so at the end of the scroll this row clears the dock even at 250%, where
  // the dock is three times its usual height. Before that reserve was
  // measured it was the constant `pb-44`, and this click landed on the
  // composer instead.
  await page.evaluate(() => {
    const row = document.querySelector(
      '[data-testid="coding-session-pending-turn"]',
    );
    const scroller = row?.closest(".overflow-y-auto") as HTMLElement | null;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  });
  await waitForAnimations(page);
  const copy = row.getByTestId("coding-session-pending-turn-copy-draft");
  await expect(copy).toBeVisible();
  await copy.click();

  // Recovered beside the existing draft, not instead of it.
  await expect(editor).toHaveValue(
    "Stop after the first failure\n\nmeanwhile, check CI",
  );
  // Nothing signed: copying is not sending.
  expect(
    await page.evaluate(
      (from) =>
        (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [])
          .slice(from)
          .map((event) => ({ kind: event.kind, tags: event.tags })),
      signedBefore,
    ),
  ).toEqual([]);
  // And the original still says what it said: the provider could not
  // establish delivery, and copying did not change that.
  await expect(row).toHaveAttribute("data-pending-state", "unknown");
  await expect(row).toContainText("Stop after the first failure");
  await waitForAnimations(page);
  await row.screenshot({
    path: "test-results/screenshots/native-steer-copy-to-draft-row.png",
  });

  // Dismiss remains local and separate: it retires this client's row and
  // leaves the recovered words in the editor.
  await row.getByTestId("coding-session-pending-turn-dismiss").click();
  await expect(page.getByTestId("coding-session-pending-turn")).toHaveCount(0);
  await expect(editor).toHaveValue(
    "Stop after the first failure\n\nmeanwhile, check CI",
  );
});
