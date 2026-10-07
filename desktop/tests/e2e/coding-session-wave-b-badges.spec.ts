import { createHash } from "node:crypto";

import { bytesToHex } from "@noble/hashes/utils.js";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { CODING_SESSION_TEAM_TRANSACTION_SCHEMA } from "@/features/coding-sessions/lib/codingSessionTeamTransactionWire";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_OBSERVATION,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { E2E_IDENTITY_OVERRIDE_STORAGE_KEY } from "../helpers/onboarding";

// Session-view parity Wave B, lane B3: surface badges count what is
// happening now (SV-22) — running subagents, files changed since this device
// last opened Diff, a ruling a person owes, and a gate the provider signed
// as started (SV-41). Every state is driven by signed events seeded live
// into the open session, and every shot is gated on a distinct hash.

const SHOTS = "test-results/session-parity-b";
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const hashes = new Map<string, string>();

test.describe.configure({ mode: "serial" });

function hexToBytes(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

// Pinned keys, so seat accents and screenshots are stable between runs.
const PROVIDER_SECRET = hexToBytes("b3".repeat(32));
const PROVIDER = getPublicKey(PROVIDER_SECRET);
const SECOND_PROVIDER_SECRET = hexToBytes("b4".repeat(32));
const SECOND_PROVIDER = getPublicKey(SECOND_PROVIDER_SECRET);
const BUILDER_ACTOR_SECRET = hexToBytes("b5".repeat(32));
const BUILDER_ACTOR = getPublicKey(BUILDER_ACTOR_SECRET);
// The relay's NIP-11 self key. Mission evidence — the team fold a ruling
// owed is read from — refuses to project without one (SV-61: with none the
// decision.request below never reached a badge).
const RELAY_PUBKEY = getPublicKey(hexToBytes("b7".repeat(32)));
// The founder is the bridge's real-key identity ("tyler"), which the team
// test signs in as through the identity override, so a ruling held on the
// founder reads as waiting on "you" (`codingSessionSurfaceRulingHolderName`).
// The bridge's default viewer is a different, mock key (SV-61).
const FOUNDER_SECRET = hexToBytes(
  "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
);
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

function signed(
  secret: Uint8Array,
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

async function seed(page: Page, events: RelayEvent[]): Promise<void> {
  await page.evaluate(
    ({ channelName, signedEvents }) => {
      const hook = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!hook) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) hook({ channelName, event });
    },
    { channelName: CHANNEL_NAME, signedEvents: events },
  );
}

async function shoot(page: Page, name: string, locator: Locator) {
  await expect(locator).toBeVisible();
  // Park the pointer off the panel so no hover state leaks into a shot.
  await page.mouse.move(2, 2);
  await page.mouse.move(3, 3);
  await waitForAnimations(page);
  const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
  hashes.set(name, createHash("sha256").update(png).digest("hex"));
}

function metadata(input: {
  secret: Uint8Array;
  target: {
    driver: string;
    instanceId: string;
    sessionId: string;
    generation: number;
  };
  title: string;
  status: string;
  sessionRef?: string;
}): RelayEvent {
  return signed(
    input.secret,
    KIND_CODING_SESSION_METADATA,
    nowSeconds() - 900,
    [
      ["h", CHANNEL_ID],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(input.target)],
      ["csm-key", codingSessionMetadataSemanticKey(input.target)],
    ],
    JSON.stringify({
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session: input.target,
      projectRef: null,
      repoRef: null,
      title: input.title,
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: input.status,
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
      ...(input.sessionRef ? { sessionRef: input.sessionRef } : {}),
    }),
  );
}

async function openSession(
  page: Page,
  title: string,
  workspaceTestId:
    | "coding-session-workspace"
    | "coding-session-umbrella-workspace" = "coding-session-workspace",
): Promise<void> {
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  // A session with more than one execution renders the umbrella workspace,
  // not the single-execution one (SV-61: the team test waited on the wrong
  // workspace and never saw its title).
  await expect(page.getByTestId(workspaceTestId)).toContainText(title, {
    timeout: 15_000,
  });
}

async function openLauncher(page: Page): Promise<Locator> {
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  const launcher = page.getByTestId("coding-session-surface-launcher");
  await expect(launcher).toBeVisible();
  return launcher;
}

function badge(scope: Locator | Page, surfaceId: string): Locator {
  return scope.getByTestId(`coding-session-surface-badge-${surfaceId}`);
}

// ---------------------------------------------------------------------------
// One execution: subagents and Diff
// ---------------------------------------------------------------------------

const SOLO_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "b3b3b3b3b3b3b3b3",
  sessionId: "b3000000-0000-4000-8000-000000000022",
  generation: 1,
};
const SOLO_TITLE = "Badges count what is happening now";

/** One transcript item; `atMs` is the producer's clock for it. */
function transcript(seq: number, atMs: number, item: unknown): RelayEvent {
  return signed(
    PROVIDER_SECRET,
    KIND_CODING_SESSION_TRANSCRIPT,
    Math.floor(atMs / 1_000),
    [
      ["h", CHANNEL_ID],
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(SOLO_TARGET)],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(SOLO_TARGET, seq)],
    ],
    JSON.stringify({
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: SOLO_TARGET,
      eventSeq: seq,
      timestamp: atMs,
      turnId: "badges-turn",
      item,
    }),
  );
}

function edit(seq: number, atMs: number, id: string, path: string) {
  return [
    transcript(seq, atMs, {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolKind: "edit",
        toolId: id,
        input: { file_path: path, old_string: "a", new_string: "b" },
      },
    }),
    transcript(seq + 1, atMs, {
      kind: "tool_result",
      toolId: id,
      toolName: "Edit",
      content: `Edited ${path}`,
      isError: false,
    }),
  ];
}

function taskCall(seq: number, atMs: number, id: string, description: string) {
  return transcript(seq, atMs, {
    kind: "tool_call",
    tool: {
      toolName: "Task",
      toolKind: "think",
      toolId: id,
      input: { description, prompt: "Look around", subagent_type: "Explore" },
    },
  });
}

function taskResult(seq: number, atMs: number, id: string) {
  return transcript(seq, atMs, {
    kind: "tool_result",
    toolId: id,
    toolName: "Task",
    content: "Done.",
    isError: false,
  });
}

test("SV-22: Agents counts running subagents and clears; Diff counts what this device has not shown", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER, label: "Badges provider" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const earlier = Date.now() - 600_000;
  await seed(page, [
    metadata({
      secret: PROVIDER_SECRET,
      target: SOLO_TARGET,
      title: SOLO_TITLE,
      status: "running",
    }),
    transcript(1, earlier, {
      kind: "user_prompt",
      content: "Fix the reconnect stall and map the call sites.",
    }),
    // An edit from before this device ever showed the session: history,
    // not "since you last looked" (T3 badges no old completion on load).
    ...edit(2, earlier, "edit-old", "src/useReconnect.ts"),
  ]);
  await openSession(page, SOLO_TITLE);
  const launcher = await openLauncher(page);
  const host = page.getByTestId("coding-session-surface-host");
  await expect(badge(launcher, "agents")).toHaveCount(0);
  await expect(badge(launcher, "diff")).toHaveCount(0);
  // People, Pulse, Files, Browser and Device carry no badge.
  for (const id of ["people", "pulse", "files", "browser", "device"]) {
    await expect(
      page.getByTestId(`coding-session-surface-badge-slot-${id}`),
    ).toHaveCount(0);
  }

  // Two subagents start: the Agents pill reads 2 and says so.
  let seq = 10;
  const started = Date.now();
  await seed(page, [
    taskCall(seq++, started, "task-a", "Map the reconnect call sites"),
    taskCall(seq++, started, "task-b", "Read the socket tests"),
  ]);
  const agents = badge(launcher, "agents");
  await expect(agents).toHaveText("2");
  await expect(agents).toHaveAttribute("aria-label", "2 subagents running");
  await expect(agents).toHaveAttribute("data-tone", "activity");
  await shoot(page, "SV22-running", host);

  // Both finish: the badge clears. Two finished subagents are not a count.
  await seed(page, [
    taskResult(seq++, Date.now(), "task-a"),
    taskResult(seq++, Date.now(), "task-b"),
  ]);
  await expect(agents).toHaveCount(0);
  await shoot(page, "SV22-cleared", host);

  // Two files are edited after this device began tracking the session.
  const edited = Date.now() + 1_000;
  await seed(page, [
    ...edit(seq, edited, "edit-new-1", "src/useReconnect.ts"),
    ...edit(seq + 2, edited, "edit-new-2", "src/socket.ts"),
  ]);
  seq += 4;
  const diff = badge(launcher, "diff");
  await expect(diff).toHaveText("2");
  await expect(diff).toHaveAttribute("data-tone", "neutral");
  await expect(diff).toHaveAttribute("title", /on this device/);
  await shoot(page, "SV22-diff-unseen", host);

  // Opening Diff clears it: the tab draws no count while Diff is on screen.
  await page.keyboard.press("d");
  const diffTab = page.getByTestId("coding-session-surface-tab-diff");
  await expect(diffTab).toHaveAttribute("aria-selected", "true");
  await expect(badge(diffTab, "diff")).toHaveCount(0);
  await page.getByTestId("coding-session-surface-tab-close-diff").click();
  const reopened = await openLauncher(page);
  await expect(badge(reopened, "diff")).toHaveCount(0);

  // A new edit after that opening counts again, against the opening.
  await seed(
    page,
    edit(seq, Date.now() + 2_000, "edit-new-3", "src/socket.ts"),
  );
  await expect(badge(reopened, "diff")).toHaveText("1");
  await expect(badge(reopened, "diff")).toHaveAttribute(
    "aria-label",
    "1 file changed since you last opened Diff on this device",
  );
});

// ---------------------------------------------------------------------------
// A team session: a gate the provider signed as started, then a ruling owed
// ---------------------------------------------------------------------------

const SESSION_REF = "b3b3b3b3-0000-4000-8000-000000000041";
const TEAM_TITLE = "Badges for a team";
const TEAM_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "b3b3b3b3b3b3b3b4",
  sessionId: "b3000000-0000-4000-8000-000000000141",
  generation: 1,
};
const SECOND_TEAM_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "b3b3b3b3b3b3b3b5",
  sessionId: "b3000000-0000-4000-8000-000000000142",
  generation: 1,
};

function teamLifecycle(): { genesis: RelayEvent; events: RelayEvent[] } {
  const base = nowSeconds() - 1_200;
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = signed(
    FOUNDER_SECRET,
    built.kind,
    base,
    built.tags,
    built.content,
  );
  const events: RelayEvent[] = [genesis];
  for (const [index, seat] of [
    {
      commandId: "b3c0ffee-0000-4000-8000-000000000001",
      secret: PROVIDER_SECRET,
      provider: PROVIDER,
      target: TEAM_TARGET,
    },
    {
      commandId: "b3c0ffee-0000-4000-8000-000000000002",
      secret: SECOND_PROVIDER_SECRET,
      provider: SECOND_PROVIDER,
      target: SECOND_TEAM_TARGET,
    },
  ].entries()) {
    const create = buildCodingSessionCreateEvent({
      channelId: CHANNEL_ID,
      commandId: seat.commandId,
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      providerInstanceRef: "claude-primary",
      providerAuthorityPubkey: seat.provider,
      model: "sonnet",
      title: TEAM_TITLE,
      initialTurn: null,
    });
    events.push(
      signed(
        FOUNDER_SECRET,
        create.kind,
        base + 1 + index,
        create.tags,
        create.content,
      ),
      signed(
        seat.secret,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        base + 3 + index,
        [
          ["h", CHANNEL_ID],
          ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
          ["csl-command", seat.commandId],
          ["csl-key", lifecycleReceiptSemanticKey(seat.commandId)],
        ],
        JSON.stringify({
          schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
          commandId: seat.commandId,
          status: "created",
          session: seat.target,
          error: null,
        }),
      ),
      metadata({
        secret: seat.secret,
        target: seat.target,
        title: TEAM_TITLE,
        status: "completed",
        sessionRef: SESSION_REF,
      }),
    );
  }
  return { genesis, events };
}

function transaction(
  genesisRef: string,
  type: "assignment" | "report" | "decision.request",
  body: Record<string, unknown>,
  secret: Uint8Array,
): RelayEvent {
  return signed(
    secret,
    KIND_CODING_SESSION_TEAM_TRANSACTION,
    nowSeconds(),
    [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA],
      ["cstx-genesis", genesisRef],
      ["cstx-type", type],
    ],
    JSON.stringify({
      schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
      sessionRef: SESSION_REF,
      genesisRef,
      type,
      supersedes: null,
      deliveryCommandId: null,
      body,
    }),
  );
}

/** A gate start (endedAtMs null) or its close, as the provider signs them. */
function gatePhase(
  genesisRef: string,
  startedAtMs: number,
  endedAtMs: number | null,
): RelayEvent {
  return signed(
    PROVIDER_SECRET,
    KIND_CODING_SESSION_OBSERVATION,
    nowSeconds(),
    [
      ["h", CHANNEL_ID],
      ["d", SESSION_REF],
      ["csob-v", "buzz-coding-session-observation/v1"],
      ["csob-genesis", genesisRef],
      ["csob-type", "phase"],
    ],
    JSON.stringify({
      schema: "buzz-coding-session-observation/v1",
      sessionRef: SESSION_REF,
      genesisRef,
      type: "phase",
      source: "observed",
      assignmentRef: null,
      body: {
        phase: "gate:just ci",
        startedAtMs,
        endedAtMs,
        durationMs: endedAtMs === null ? null : endedAtMs - startedAtMs,
      },
    }),
  );
}

test("SV-22, SV-41: Landing shows a gate running; a decision.request turns Agents and Landing amber", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  // Both native folds answer from the events they are handed (lane B3's
  // bridge module), so live seeds drive the badges.
  await page.addInitScript(
    ({ storageKey, identity }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(identity));
      (
        window as Window & { __BEEKEEPER_E2E_WAVE_B_BADGES_FOLDS__?: boolean }
      ).__BEEKEEPER_E2E_WAVE_B_BADGES_FOLDS__ = true;
    },
    {
      storageKey: E2E_IDENTITY_OVERRIDE_STORAGE_KEY,
      identity: {
        privateKey: bytesToHex(FOUNDER_SECRET),
        pubkey: FOUNDER_PUBKEY,
        username: "tyler",
      },
    },
  );
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [{ pubkey: BUILDER_ACTOR, displayName: "Bob" }],
    relaySelf: RELAY_PUBKEY,
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const { genesis, events } = teamLifecycle();
  const assignment = transaction(
    genesis.id,
    "assignment",
    {
      assigneeActor: BUILDER_ACTOR,
      assigneeRole: "builder",
      objective: "Count only what is happening now.",
      brief: "Badges clear when the work ends.",
      branch: "surface-badges",
      baseSha: "1".repeat(40),
      fileOwnership: ["desktop/src/features/coding-sessions"],
      acceptanceSteps: ["Run the badges spec"],
    },
    FOUNDER_SECRET,
  );
  await seed(page, [...events, assignment]);
  await openSession(page, TEAM_TITLE, "coding-session-umbrella-workspace");
  let launcher = await openLauncher(page);
  const host = page.getByTestId("coding-session-surface-host");
  // An open assignment is a seat working, not a seat waiting on a person.
  await expect(badge(launcher, "agents")).toHaveCount(0);
  await expect(badge(launcher, "landing")).toHaveCount(0);

  // SV-41: the provider signs a gate start; Landing reads "gate running".
  // The Landing read is woken by its live kind-44246 subscription. A relay
  // replays to a subscription opened after an event (`since`); the mock
  // relay does not, and the REQ can queue behind the session's opening reads
  // (the client's send budget), so the start is signed once it is open
  // (SV-61).
  await expect
    .poll(
      () =>
        page.evaluate(
          ({ channelName, kind }) =>
            window.__BEEKEEPER_E2E_HAS_MOCK_LIVE_SUBSCRIPTION__?.({
              channelName,
              kind,
            }) ?? false,
          { channelName: CHANNEL_NAME, kind: KIND_CODING_SESSION_OBSERVATION },
        ),
      { timeout: 30_000 },
    )
    .toBe(true);
  const startedAtMs = Date.now();
  await seed(page, [gatePhase(genesis.id, startedAtMs, null)]);
  const landing = badge(launcher, "landing");
  await expect(landing).toHaveText("1", { timeout: 15_000 });
  // Attributed to the provider instance that watched it, never the seat.
  await expect(landing).toHaveAttribute(
    "aria-label",
    /^Gate running: just ci, watched by .+$/,
  );
  await expect(landing).toHaveAttribute("data-tone", "activity");
  await expect(landing).toHaveAttribute(
    "title",
    /just ci: started \d{1,2}:\d{2}.* by the provider's clock/,
  );
  await expect(landing).toHaveAttribute("title", /provider restart/);
  await shoot(page, "SV22-gate-running", host);

  // Its close clears it.
  await seed(page, [gatePhase(genesis.id, startedAtMs, startedAtMs + 41_000)]);
  await expect(landing).toHaveCount(0, { timeout: 15_000 });

  // SV-41: a start the provider never closed, older than the fold's stale
  // bound (30 minutes in the mock fold), reads "no result observed" on
  // Landing and draws no badge.
  await seed(page, [gatePhase(genesis.id, Date.now() - 31 * 60_000, null)]);
  await page.keyboard.press("l");
  const landingTab = page.getByTestId("coding-session-surface-tab-landing");
  await expect(landingTab).toHaveAttribute("aria-selected", "true");
  const landingPanel = page.getByTestId("coding-session-surface-panel-landing");
  await expect(
    landingPanel.getByTestId("coding-session-gate-start-stale"),
  ).toHaveAttribute("data-gate-run-state", "no-result", { timeout: 15_000 });
  await expect(badge(landingTab, "landing")).toHaveCount(0);
  // B2's surfaces spec owns SV41-landing-stale.png (the panel line); this
  // shot is the tab strip and panel together, proving the badge is absent.
  await shoot(page, "SV22-gate-stale-no-badge", host);
  await page.getByTestId("coding-session-surface-tab-close-landing").click();
  launcher = await openLauncher(page);
  await expect(badge(launcher, "landing")).toHaveCount(0);

  // The builder reports. A verdict on a report may be owed by a lead or a
  // verifier, so an open report alone is no person's ruling: no amber.
  await seed(page, [
    transaction(
      genesis.id,
      "report",
      {
        assignmentRef: assignment.id,
        summary: "Badges count only live work.",
        branch: "surface-badges",
        baseSha: "1".repeat(40),
        headSha: "2".repeat(40),
        files: ["desktop/src/features/coding-sessions"],
        tests: [
          {
            name: "Badges spec",
            command:
              "pnpm playwright test coding-session-wave-b-badges.spec.ts",
            outcome: "passed",
            evidence: "Five distinct shots.",
          },
        ],
        redBeforeGreen: null,
        deviations: [],
        residuals: [],
        anomalies: [],
      },
      BUILDER_ACTOR_SECRET,
    ),
  ]);
  const agents = badge(launcher, "agents");
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  await expect(agents).toHaveCount(0);
  await expect(badge(launcher, "landing")).toHaveCount(0);

  // The builder asks the founder — this viewer — for a ruling (DB8).
  await seed(page, [
    transaction(
      genesis.id,
      "decision.request",
      {
        question: "Should a stale gate start clear the Landing badge?",
        options: ["Yes", "No"],
        heldOn: "founder",
        blocks: [assignment.id],
        recommendation: "Yes",
      },
      BUILDER_ACTOR_SECRET,
    ),
  ]);
  await expect(agents).toHaveAttribute("data-tone", "waiting", {
    timeout: 15_000,
  });
  await expect(agents).toHaveAttribute(
    "aria-label",
    "Waiting on a ruling from you",
  );
  await expect(badge(launcher, "landing")).toHaveAttribute(
    "data-tone",
    "waiting",
  );
  await shoot(page, "SV22-waiting", host);
});

test("SV-22: every badge shot is distinct", () => {
  const names = [
    "SV22-running",
    "SV22-cleared",
    "SV22-diff-unseen",
    "SV22-gate-running",
    "SV22-gate-stale-no-badge",
    "SV22-waiting",
  ];
  for (const name of names) {
    expect(hashes.get(name), `${name} was not captured`).toBeTruthy();
  }
  const values = names.map((name) => hashes.get(name));
  expect(new Set(values).size).toBe(names.length);
});
