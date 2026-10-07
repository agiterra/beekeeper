import { expect, test } from "@playwright/test";
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
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";

/**
 * SV-116: a session longer than one relay page (1000 events of a kind) opens
 * whole. Before paging, the desktop read each kind once with a 1000-event cap,
 * so the oldest part of a long transcript was missing and nothing said so
 * (tank-loop: 1000 of 4375 transcript events, ledger 347).
 *
 * The events are seeded before the session's channel is opened, so they reach
 * the session's ingress only through its history read — the path that pages —
 * and not through a live subscription that would have streamed every one.
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
const SESSION_REF = "6d1f0a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b";
const CREATE_COMMAND_ID = "csl-paging-session";
/** More than one relay page of transcript: 650 turns of prompt + reply. */
const TURNS = 650;
const FIRST_PROMPT = "Paging check: the very first prompt of this session";
const LAST_PROMPT = "Paging check: the newest prompt of this session";

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function sign(
  secret: Uint8Array,
  event: {
    kind: number;
    created_at: number;
    tags: string[][];
    content: string;
  },
): RelayEvent {
  return finalizeEvent(event, secret) as unknown as RelayEvent;
}

function sessionEvents(): RelayEvent[] {
  const genesisBuilt = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = sign(FOUNDER_SECRET, {
    kind: genesisBuilt.kind,
    created_at: BASE_CREATED_AT - 3,
    tags: genesisBuilt.tags,
    content: genesisBuilt.content,
  });
  const createBuilt = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: CREATE_COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "A long session",
    initialTurn: null,
  });
  const create = sign(FOUNDER_SECRET, {
    kind: createBuilt.kind,
    created_at: BASE_CREATED_AT - 2,
    tags: createBuilt.tags,
    content: createBuilt.content,
  });
  const receipt = sign(PROVIDER_SECRET, {
    kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    created_at: BASE_CREATED_AT - 1,
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
  });
  const metadata = sign(PROVIDER_SECRET, {
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
      title: "A long session",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude",
      model: "sonnet",
      status: "idle",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: false,
        context: false,
        diff: false,
        plan: true,
      },
      sessionRef: SESSION_REF,
    }),
  });

  const transcript: RelayEvent[] = [];
  for (let turn = 0; turn < TURNS; turn += 1) {
    const turnId = `turn-${turn + 1}`;
    const prompt =
      turn === 0
        ? FIRST_PROMPT
        : turn === TURNS - 1
          ? LAST_PROMPT
          : `Paging check prompt ${turn + 1}`;
    const items = [
      { kind: "user_prompt", content: prompt, commandId: `cmd-${turn + 1}` },
      { kind: "assistant_text", text: `Reply ${turn + 1}.` },
    ];
    for (const item of items) {
      const eventSeq = transcript.length + 1;
      transcript.push(
        sign(PROVIDER_SECRET, {
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
            timestamp: (BASE_CREATED_AT + eventSeq) * 1_000,
            turnId,
            item,
          }),
        }),
      );
    }
  }
  return [genesis, create, receipt, metadata, ...transcript];
}

test("a transcript longer than one relay page opens whole and says nothing is missing", async ({
  page,
}) => {
  test.setTimeout(120_000);
  const events = sessionEvents();
  expect(
    events.filter((event) => event.kind === KIND_CODING_SESSION_TRANSCRIPT)
      .length,
  ).toBeGreaterThan(1_000);

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
  // Seed before the channel is opened: the session's own ingress must find
  // these through history, page by page.
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events },
  );
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 30_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();

  const transcript = page.getByTestId("coding-session-transcript");
  await expect(transcript).toContainText(LAST_PROMPT, { timeout: 30_000 });
  // Whole history, so no disclosure remains once paging finishes.
  await expect(
    page.getByTestId("coding-session-history-disclosure"),
  ).toHaveCount(0, { timeout: 30_000 });
  // The oldest prompt is on the second page back. Without paging it never
  // arrives; with paging it arrives behind the newest page. The transcript is
  // virtualized, so only rows near the viewport are in the DOM: scroll to the
  // top (repeatedly, as the virtualizer measures rows in) before looking. A
  // bare scrollTop write is not reader input and a bottom-pinned transcript
  // holds, so each jump starts with an upward wheel tick.
  const scroller = page.getByTestId("coding-session-transcript-scroll");
  await expect
    .poll(
      async () => {
        await scroller.evaluate((element) => {
          element.dispatchEvent(new WheelEvent("wheel", { deltaY: -1 }));
          element.scrollTop = 0;
        });
        return (await transcript.textContent()) ?? "";
      },
      { timeout: 30_000 },
    )
    .toContain(FIRST_PROMPT);
});
