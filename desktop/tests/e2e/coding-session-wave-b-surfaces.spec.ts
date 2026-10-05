import { createHash } from "node:crypto";

import {
  type Browser,
  expect,
  type Locator,
  type Page,
  test,
} from "@playwright/test";
import { bytesToHex } from "@noble/hashes/utils.js";
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
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_OBSERVATION,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { WaveBSurfacesMock } from "@/testing/e2eBridgeWaveBSurfaces";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { E2E_IDENTITY_OVERRIDE_STORAGE_KEY } from "../helpers/onboarding";

// Session-view parity Wave B, lane B2: the contents of every surface (SV-24),
// the dimmed reasons (SV-23) and Landing's running-gate line (SV-41 UI).
// Every shot is scoped to its panel and the set is gated on distinct hashes.
//
// One governed session, run by a provider whose key the mock either is
// (`isLocalProvider: true` — the working tree is here) or is not (the tree is
// on the provider's computer). The repository and project are the mock
// relay's own seeded `buzz` repository and project, so the announcement's
// clone URL is a real read.

const SHOTS = "test-results/session-parity-b";
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "b2b2b2b2-0000-4000-8000-000000000024";
const COMMAND_ID = "b2b2b2b2-1111-4000-8000-000000000024";
const TITLE = "Surface contents";
/** The bridge's mock identity owns the seeded `buzz` repository and project. */
const MOCK_OWNER = "deadbeef".repeat(8);
const REPO_REF = `30617:${MOCK_OWNER}:buzz`;
const PROJECT_REF = `30621:${MOCK_OWNER}:buzz`;
const HEAD = "07c470be007c470be007c470be007c470be007c4";
const OBSERVATION_SCHEMA = "buzz-coding-session-observation/v1";
const FOLD_SCHEMA = "buzz-coding-session-observation-fold-adapter/v2";
const DISCLOSURE =
  "an observation is something its author saw, not a decision: it settles nothing, authorizes nothing and excludes nothing, and every duration in it is the author's own measurement";

const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const RELAY_PUBKEY = getPublicKey(generateSecretKey());
const ELSEWHERE_PROVIDER = getPublicKey(generateSecretKey());
const INVITEE_PUBKEY = getPublicKey(generateSecretKey());
const INVITEE_NAME = "Beekeeper Zoe Winters";

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "b2b2b2b2b2b2b2b2",
  sessionId: "b2000000-0000-4000-8000-000000000024",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const BASE = Math.floor(Date.now() / 1_000) - 3_600;

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

function sessionEvents(genesis: RelayEvent): RelayEvent[] {
  const create = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: PROJECT_REF,
    repoRef: REPO_REF,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: TITLE,
    initialTurn: null,
  });
  const transcript = (seq: number, item: unknown) =>
    signed(
      KIND_CODING_SESSION_TRANSCRIPT,
      BASE + 10 + seq,
      [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["cst-seq", String(seq)],
        ["cst-key", codingSessionTranscriptSemanticKey(TARGET, seq)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: TARGET,
        eventSeq: seq,
        timestamp: (BASE + 10 + seq) * 1_000,
        turnId: "surfaces-turn",
        item,
      }),
      PROVIDER_SECRET,
    );
  return [
    genesis,
    signed(create.kind, BASE - 1, create.tags, create.content, FOUNDER_SECRET),
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE,
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
      PROVIDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_METADATA,
      BASE,
      [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: PROJECT_REF,
        repoRef: REPO_REF,
        title: TITLE,
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
      PROVIDER_SECRET,
    ),
    transcript(1, {
      kind: "user_prompt",
      content: "Bound the reconnect retries and show me the plan.",
    }),
    transcript(2, {
      kind: "plan",
      entries: [
        { content: "Map the reconnect call sites", status: "completed" },
        { content: "Reset the retry counter on close", status: "in_progress" },
        { content: "Run the gates", status: "pending" },
      ],
    }),
    transcript(3, {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolKind: "edit",
        toolId: "edit-1",
        input: {
          file_path: "src/useReconnect.ts",
          old_string: "attempts += 1;",
          new_string: "attempts = 0;",
        },
      },
    }),
    transcript(4, {
      kind: "tool_result",
      toolId: "edit-1",
      toolName: "Edit",
      content: "Edited src/useReconnect.ts",
      isError: false,
    }),
    transcript(5, {
      kind: "assistant_text",
      text: "The retry counter now resets when the socket closes.",
    }),
  ];
}

type GateFixture = {
  gate: string;
  outcome: "passed" | "failed" | "not-run";
};

/** Signed 44246 rows and the fold the native adapter would return for them. */
function observations(
  genesisRef: string,
  input: {
    gates: GateFixture[];
    runningGate?: string;
    runningGateStale?: boolean;
  },
) {
  const gateEvents = input.gates.map((row, index) =>
    signed(
      KIND_CODING_SESSION_OBSERVATION,
      BASE + 100 + index,
      [
        ["h", CHANNEL_ID],
        ["d", SESSION_REF],
        ["csob-v", OBSERVATION_SCHEMA],
        ["csob-genesis", genesisRef],
        ["csob-type", "gate"],
      ],
      JSON.stringify({
        schema: OBSERVATION_SCHEMA,
        sessionRef: SESSION_REF,
        genesisRef,
        type: "gate",
        source: "observed",
        assignmentRef: null,
        body: {
          rows: [
            {
              gate: row.gate,
              outcome: row.outcome,
              command: `${row.gate} --workspace`,
              summary: null,
              durationMs: 30_000,
              headSha: HEAD,
              dirty: false,
            },
          ],
        },
      }),
      PROVIDER_SECRET,
    ),
  );
  // A stale start is older than the fold's `gateStartStaleAfterMs` (30 min)
  // with no close: it reads "no result observed", never "running" (SV-41).
  const startedAtMs =
    Date.now() - (input.runningGateStale ? 3_600_000 : 90_000);
  const startEvent = input.runningGate
    ? signed(
        KIND_CODING_SESSION_OBSERVATION,
        BASE + 200,
        [
          ["h", CHANNEL_ID],
          ["d", SESSION_REF],
          ["csob-v", OBSERVATION_SCHEMA],
          ["csob-genesis", genesisRef],
          ["csob-type", "phase"],
        ],
        JSON.stringify({
          schema: OBSERVATION_SCHEMA,
          sessionRef: SESSION_REF,
          genesisRef,
          type: "phase",
          source: "observed",
          assignmentRef: null,
          body: {
            phase: `gate:${input.runningGate}`,
            startedAtMs,
            endedAtMs: null,
            durationMs: null,
          },
        }),
        PROVIDER_SECRET,
      )
    : null;
  const events = startEvent ? [...gateEvents, startEvent] : gateEvents;
  const fold = {
    schema: FOLD_SCHEMA,
    implementation: "buzz-core",
    inputEventIds: events.map((event) => event.id),
    sessionRef: SESSION_REF,
    genesisRef,
    checkpoints: [],
    gates: input.gates.map((row, index) => ({
      authorPubkey: PROVIDER_PUBKEY,
      source: "observed",
      eventIds: [gateEvents[index].id],
      droppedEventIds: 0,
      assignmentRef: null,
      gate: row.gate,
      outcome: row.outcome,
      command: `${row.gate} --workspace`,
      summary: null,
      durationMs: 30_000,
      headSha: HEAD,
      dirty: false,
    })),
    findings: [],
    phases: [],
    gateStarts:
      startEvent && input.runningGate
        ? [
            {
              eventId: startEvent.id,
              authorPubkey: PROVIDER_PUBKEY,
              gate: input.runningGate,
              startedAtMs,
              assignmentRef: null,
              closeEventId: null,
              endedAtMs: null,
              durationMs: null,
            },
          ]
        : [],
    gateStartStaleAfterMs: 1_800_000,
    unresolved: [],
    ignored: [],
    misclaimedObserved: [],
    provenanceChecked: true,
    truncated: {
      checkpoints: 0,
      gates: 0,
      findings: 0,
      phases: 0,
      unresolved: 0,
      ignored: 0,
      misclaimedObserved: 0,
      entryEventIds: 0,
      displacedGates: 0,
      displacedFindings: 0,
      gateStarts: 0,
      gateStartClosesUnmatched: 0,
    },
    disclosure: DISCLOSURE,
  };
  return { events, fold };
}

function landAnswer(state: "ready" | "refused") {
  const base = {
    schema: "buzz-coding-session-land-adapter/v1",
    implementation: "buzz-core",
    repositoryKnown: true,
    ruleGoverns: true,
    founders: [FOUNDER_PUBKEY],
    foundersNote: `founders of this repository are ${FOUNDER_PUBKEY} (1).`,
    viewerIsFounder: true,
    rulesSigner: FOUNDER_PUBKEY,
    rosterRead: true,
    seatsRead: true,
    gateRowsRead: true,
    boundRepositoriesRead: true,
  };
  if (state === "ready") {
    return {
      ...base,
      admitted: true,
      evidence: {
        arm: "observed-gates",
        sessionRef: SESSION_REF,
        dispositionEventId: "",
        dispositionAuthorPubkey: "",
        refutationEventId: "",
        verifierPubkey: "",
        observedGates: ["cargo test", "clippy"],
        reportEventId: "",
        headSha: HEAD,
        policyResolution: "absent",
        policyEventId: null,
        policyNotEvaluated: null,
      },
      refusalReason: null,
      newestVerdict: {
        eventId: "ab".repeat(32),
        authorPubkey: FOUNDER_PUBKEY,
        decision: "approve",
        reportEventId: "cd".repeat(32),
        headSha: HEAD,
      },
      command: `git push origin ${HEAD}:refs/heads/main`,
    };
  }
  return {
    ...base,
    admitted: false,
    evidence: null,
    refusalReason: `gate cargo test has no observed green row on ${HEAD.slice(0, 8)}.`,
    newestVerdict: {
      eventId: "ef".repeat(32),
      authorPubkey: FOUNDER_PUBKEY,
      decision: "changes-requested",
      reportEventId: "cd".repeat(32),
      headSha: HEAD,
    },
    command: null,
  };
}

type Variant = {
  local: boolean;
  gates: GateFixture[];
  runningGate?: string;
  runningGateStale?: boolean;
  land: "ready" | "refused";
  surfaces: WaveBSurfacesMock;
  /**
   * The project names an agents repository (kind:30624 at the root). Files
   * opens on a computer without the tree only when there is one (DB3: "an
   * agents repo, or a local tree"); without it the row is truthfully dimmed.
   */
  agentsRepo?: boolean;
};

/** A fresh page on one variant of the session, opened on its workspace. */
async function openVariant(browser: Browser, variant: Variant): Promise<Page> {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 900 },
  });
  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = signed(
    builtGenesis.kind,
    BASE - 2,
    builtGenesis.tags,
    builtGenesis.content,
    FOUNDER_SECRET,
  );
  const observed = observations(genesis.id, variant);
  await page.addInitScript(
    ({ identity, storageKey, surfaces, owner, channelId, agentsRepo }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(identity));
      (
        window as Window & { __BUZZ_E2E_WAVE_B_SURFACES__?: unknown }
      ).__BUZZ_E2E_WAVE_B_SURFACES__ = surfaces;
      // Pulse reads session facts only from the project's channels, and the
      // seeded `buzz` head names none, so this session's channel would be
      // outside it and Pulse would truthfully read empty. A newer head for
      // the same project (NIP-33: newest `(owner, d)` wins) that names this
      // channel puts the session where Pulse looks.
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "wave-b-surfaces-project".padEnd(64, "0"),
          pubkey: owner,
          created_at: Math.floor(Date.now() / 1000) + 60,
          kind: 30621,
          tags: [
            ["d", "buzz"],
            ["name", "buzz"],
            ["description", "The complete Buzz community platform."],
            ["a", `30617:${owner}:buzz`],
            ["channel", channelId],
          ],
          content: "",
          sig: "0".repeat(128),
        },
      ];
      if (agentsRepo) {
        window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__.push({
          id: "wave-b-surfaces-agents-repo".padEnd(64, "0"),
          pubkey: owner,
          created_at: Math.floor(Date.now() / 1000) - 3_600,
          kind: 30624,
          tags: [
            ["d", `30621:${owner}:buzz`],
            ["repo", `30617:${owner}:buzz-agents`],
            ["ref", "refs/heads/main"],
            ["path", "."],
          ],
          content: "",
          sig: "0".repeat(128),
        });
        window.__BUZZ_E2E_AGENTS_REPO__ = {
          listing: {
            repo: `30617:${owner}:buzz-agents`,
            branch: "main",
            commit: "5".repeat(40),
            syncedAt: null,
            entries: [
              {
                path: "team.yml",
                blob: "7b9a85bbe3dbcc64eadd04d4759783cd555a2b8d",
                size: 40,
                kind: "manifest",
              },
            ],
          },
          files: {},
          commitResults: [],
        };
      }
    },
    {
      agentsRepo: variant.agentsRepo === true,
      owner: MOCK_OWNER,
      channelId: CHANNEL_ID,
      storageKey: E2E_IDENTITY_OVERRIDE_STORAGE_KEY,
      identity: {
        privateKey: bytesToHex(FOUNDER_SECRET),
        pubkey: FOUNDER_PUBKEY,
        username: "tyler",
      },
      surfaces: { ...variant.surfaces, land: landAnswer(variant.land) },
    },
  );
  await installMockBridge(page, {
    relaySelf: RELAY_PUBKEY,
    codingSessionObservationFoldResponse: observed.fold,
    // `isLocalProvider`, mocked both ways: this computer's provider either is
    // the key that runs the session, or another one.
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: variant.local ? PROVIDER_PUBKEY : ELSEWHERE_PROVIDER,
      instanceId: "0123456789abcdef",
    },
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "Brian's provider" },
      ],
    },
    searchProfiles: [
      { pubkey: FOUNDER_PUBKEY, displayName: "Tyler" },
      { pubkey: PROVIDER_PUBKEY, displayName: "Brian" },
      { pubkey: INVITEE_PUBKEY, displayName: INVITEE_NAME },
    ],
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  // The catalog is a bounded history read plus live watches, and the mock
  // relay's live REQ replays nothing: events seeded after the history read but
  // before a watch is registered are never seen ("Coding sessions (0)"). Seed
  // only once both channel watches are live — trusted ingress (44223/44225)
  // and the create-observation read (44226).
  for (const kind of [
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_TRANSCRIPT,
    KIND_CODING_SESSION_GENESIS,
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
      events: [...sessionEvents(genesis), ...observed.events],
    },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(page.getByTestId("coding-session-workspace")).toContainText(
    TITLE,
    { timeout: 15_000 },
  );
  // ⌘⌥B opens the right panel on the launcher.
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  return page;
}

/** Open a surface from the launcher by its letter, then return its panel. */
async function openByLetter(
  page: Page,
  id: string,
  letter: string,
): Promise<Locator> {
  // Back to the launcher: "+" lists the same rows and takes the same letters.
  if (
    (await page.getByTestId("coding-session-surface-launcher").count()) === 0
  ) {
    await page.getByTestId("coding-session-surface-add").click();
    await expect(
      page.getByTestId("coding-session-surface-add-menu"),
    ).toBeVisible();
  }
  await page.keyboard.press(letter.toLowerCase());
  await expect(
    page.getByTestId(`coding-session-surface-tab-${id}`),
  ).toHaveAttribute("aria-selected", "true");
  const panel = page.getByTestId(`coding-session-surface-panel-${id}`);
  await expect(panel).toBeVisible();
  return panel;
}

const LOCAL_TREE: WaveBSurfacesMock = {
  treeResolution: {
    available: true,
    source: "session",
    label: "this session's worktree",
    reason: null,
    refusal: null,
  },
  treeListings: {
    "": {
      entries: [
        { name: "src", relPath: "src", kind: "directory" },
        { name: "docs", relPath: "docs", kind: "directory" },
        { name: "Cargo.toml", relPath: "Cargo.toml", kind: "file" },
        { name: "README.md", relPath: "README.md", kind: "file" },
      ],
      truncated: false,
    },
    src: {
      entries: [
        {
          name: "useReconnect.ts",
          relPath: "src/useReconnect.ts",
          kind: "file",
        },
        { name: "relay.ts", relPath: "src/relay.ts", kind: "file" },
      ],
      truncated: false,
    },
  },
  mainCommits: [HEAD, "1".repeat(40), "2".repeat(40)],
};

test("SV-24, SV-23 and SV-41: every surface's contents, both localities, Landing's states", async ({
  browser,
}) => {
  test.setTimeout(240_000);
  const hashes = new Map<string, string>();
  const shoot = async (page: Page, name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await page.mouse.move(2, 2);
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };

  // ---- This computer runs the session: the tree is here. ----------------
  const page = await openVariant(browser, {
    local: true,
    gates: [
      { gate: "cargo test", outcome: "passed" },
      { gate: "clippy", outcome: "passed" },
    ],
    land: "ready",
    surfaces: LOCAL_TREE,
  });

  // Agents: the seat with the machine that runs it and its live/idle word.
  const agents = await openByLetter(page, "agents", "A");
  await expect(
    agents.getByTestId("coding-session-execution-card").first(),
  ).toBeVisible();
  await expect(
    agents.getByTestId("coding-session-execution-machine").first(),
  ).toHaveText("on this computer");
  await expect(
    agents.getByTestId("coding-session-execution-status").first(),
  ).not.toBeEmpty();
  // Subagents are listed once, as the orchestration's Direct spawns: the
  // rail's own subagent list never renders beside it, and no spawn title
  // repeats (the unit render in codingSessionSurfaceReasons.test.mjs seeds
  // a spawn; this fixture has none).
  await expect(agents.getByTestId("coding-session-subagent-row")).toHaveCount(
    0,
  );
  const spawnTitles = await agents
    .getByTestId("coding-session-agents-spawn-row")
    .allTextContents();
  expect(new Set(spawnTitles).size).toBe(spawnTitles.length);
  await shoot(page, "SV24-agents", agents);

  // Diff: relabelled, still saying it is observed edits.
  const diff = await openByLetter(page, "diff", "D");
  await expect(diff).toContainText("Diff");
  await expect(
    diff.getByTestId("coding-session-diff-provenance"),
  ).toContainText("not a git diff");
  await expect(diff).toContainText("useReconnect.ts");
  await expect(diff).toContainText("Observed in transcript activity");
  await shoot(page, "SV24-diff", diff);

  // Files: the working tree (read-only, relative, lazy) and the agents repo.
  const files = await openByLetter(page, "files", "F");
  const rows = files.getByTestId("coding-session-files-tree-row");
  await expect(rows.first()).toBeVisible();
  await expect(files).toContainText("Cargo.toml");
  await expect(files).not.toContainText(".git");
  await files.getByRole("button", { name: /^src$/ }).click();
  await expect(files).toContainText("useReconnect.ts");
  await expect(
    files.getByTestId("coding-session-files-agents-repo"),
  ).toBeVisible();
  await shoot(page, "SV24-files", files);

  // Plan: the task rail's surface variant; the dock above the composer stays.
  const plan = await openByLetter(page, "plan", "P");
  await expect(plan).toContainText("Reset the retry counter on close");
  await expect(plan.getByTestId("coding-session-task-rail")).toHaveAttribute(
    "data-variant",
    "surface",
  );
  await shoot(page, "SV24-plan", plan);

  // Landing: four rows, "checked at" on Landed.
  const landing = await openByLetter(page, "landing", "L");
  for (const row of ["gate", "verdict", "land", "landed"]) {
    await expect(
      landing.getByTestId(`coding-session-landing-${row}`),
    ).toBeVisible();
  }
  await expect(
    landing.getByTestId("coding-session-landing-gate-word"),
  ).toHaveText("passed", { timeout: 15_000 });
  await expect(
    landing.getByTestId("coding-session-landing-landed-meta"),
  ).toContainText("checked at", { timeout: 15_000 });
  await expect(
    landing.getByTestId("coding-session-landing-landed-word"),
  ).toHaveText("on main");
  await shoot(page, "SV24-landing", landing);

  // People: the roster and invite body, out of the dialog; invite works here.
  const people = await openByLetter(page, "people", "E");
  await expect(
    people.getByTestId("coding-session-people-roster"),
  ).toBeVisible();
  await shoot(page, "SV24-people", people);
  const search = page.getByTestId(
    "coding-session-people-invite-recipient-search",
  );
  await search.click();
  await search.fill(INVITEE_NAME);
  await page
    .getByTestId(
      `coding-session-people-invite-recipient-option-${INVITEE_PUBKEY}`,
    )
    .click();
  await people.getByTestId("coding-session-people-invite").click();
  await expect
    .poll(
      () =>
        page.evaluate(
          ({ kind, grantee }) =>
            (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).filter(
              (event) =>
                event.kind === kind &&
                (JSON.parse(event.content) as { granteePubkey?: string })
                  .granteePubkey === grantee,
            ).length,
          {
            kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
            grantee: INVITEE_PUBKEY,
          },
        ),
      { timeout: 15_000 },
    )
    .toBe(1);

  // Pulse: the project's Pulse, this session first, and the way to the rest.
  const pulse = await openByLetter(page, "pulse", "U");
  await expect(pulse).toContainText("Pulse");
  await expect(
    pulse.getByTestId("coding-session-pulse-open-full"),
  ).toBeVisible();
  const leadSession = pulse.getByTestId("pulse-lead-session");
  await expect(leadSession).toBeVisible({ timeout: 15_000 });
  await expect(leadSession).toContainText(TITLE);
  // The lead section holds the first session card the panel draws.
  const firstCard = pulse.getByTestId("pulse-session-card").first();
  await expect(firstCard).toContainText(TITLE);
  await expect(
    leadSession.getByTestId("pulse-session-card").first(),
  ).toContainText(TITLE);
  expect(
    await firstCard.evaluate(
      (card) => card.closest('[data-testid="pulse-lead-session"]') !== null,
    ),
  ).toBe(true);
  await shoot(page, "SV24-pulse", pulse);

  // Browser and Device: dimmed; their letters open nothing. Their panels,
  // if a tab is left open, say the same reason — reached through the stored
  // panel state, which is how a stale tab would survive.
  await page.getByTestId("coding-session-surface-add").click();
  await page.keyboard.press("b");
  await expect(
    page.getByTestId("coding-session-surface-tab-browser"),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.evaluate(() => {
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const key = window.localStorage.key(index);
      if (!key?.startsWith("beekeeper:session-panels:v1:")) continue;
      const state = JSON.parse(window.localStorage.getItem(key) ?? "{}");
      window.localStorage.setItem(
        key,
        JSON.stringify({
          ...state,
          rightOpen: true,
          tabs: ["browser", "device"],
          active: "browser",
        }),
      );
    }
  });
  await page.reload();
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-session-open").first().click();
  const browserPanel = page.getByTestId("coding-session-surface-panel-browser");
  await expect(browserPanel).toContainText("Arrives with live preview.");
  await shoot(page, "SV24-browser", browserPanel);
  await page.getByTestId("coding-session-surface-tab-device").click();
  const devicePanel = page.getByTestId("coding-session-surface-panel-device");
  await expect(devicePanel).toContainText("Arrives with device support.");
  await shoot(page, "SV24-device", devicePanel);
  await page.close();

  // ---- Another computer runs it: one line, and a refused landing. ------
  const remote = await openVariant(browser, {
    local: false,
    agentsRepo: true,
    gates: [{ gate: "cargo test", outcome: "failed" }],
    land: "refused",
    surfaces: {
      treeResolution: {
        available: false,
        source: null,
        label: "no working tree",
        reason: "The working tree is on another computer.",
        refusal: "notLocal",
      },
      mainCommits: ["3".repeat(40), "4".repeat(40)],
    },
  });
  const remoteFiles = await openByLetter(remote, "files", "F");
  await expect(
    remoteFiles.getByTestId("coding-session-files-tree-line"),
  ).toHaveText(
    /^The working tree is on another computer( \(.+'s provider runs this session\))?\.$/,
  );
  await expect(
    remoteFiles.getByTestId("coding-session-files-tree-row"),
  ).toHaveCount(0);
  await shoot(remote, "SV24-files-remote", remoteFiles);
  const refused = await openByLetter(remote, "landing", "L");
  await expect(
    refused.getByTestId("coding-session-landing-gate-word"),
  ).toHaveText("failed", { timeout: 15_000 });
  await expect(
    refused.getByTestId("coding-session-landing-verdict-word"),
  ).toHaveText("changes-requested", { timeout: 15_000 });
  await expect(
    refused.getByTestId("coding-session-landing-land-word"),
  ).toHaveText("refused");
  // A head missing from main's bounded history is "not seen", never "not landed".
  await expect(
    refused.getByTestId("coding-session-landing-landed-sentence"),
  ).toContainText("not seen in main's last 2 commits");
  await expect(refused).not.toContainText("not landed");
  await shoot(remote, "SV24-landing-refused", refused);
  await remote.close();

  // ---- A gate the provider signed as started (SV-41). ------------------
  const running = await openVariant(browser, {
    local: true,
    gates: [{ gate: "clippy", outcome: "passed" }],
    runningGate: "cargo test",
    land: "ready",
    surfaces: LOCAL_TREE,
  });
  const gateRunning = await openByLetter(running, "landing", "L");
  const runningLine = gateRunning.getByTestId("coding-session-gate-running");
  await expect(runningLine).toContainText("cargo test · running since", {
    timeout: 15_000,
  });
  // Who watched it, and whose clock dated it (SV-41 attribution).
  await expect(runningLine).toContainText("· watched by");
  await expect(runningLine).toHaveAttribute(
    "title",
    /^Started .+ by the provider's clock\./,
  );
  await expect(
    runningLine.getByTestId("coding-session-gate-start-event"),
  ).toContainText("start ");
  await expect(
    gateRunning.getByTestId("coding-session-landing-gate-word"),
  ).toHaveText("running");
  await shoot(running, "SV24-landing-gate-running", gateRunning);
  await running.close();

  // ---- A start with no result past the stale window (SV-41). -----------
  const stale = await openVariant(browser, {
    local: true,
    gates: [{ gate: "clippy", outcome: "passed" }],
    runningGate: "cargo test",
    runningGateStale: true,
    land: "ready",
    surfaces: LOCAL_TREE,
  });
  const gateStale = await openByLetter(stale, "landing", "L");
  const staleLine = gateStale.getByTestId("coding-session-gate-start-stale");
  await expect(staleLine).toContainText("cargo test · started", {
    timeout: 15_000,
  });
  await expect(staleLine).toContainText("no result observed");
  await expect(
    gateStale.getByTestId("coding-session-gate-running"),
  ).toHaveCount(0);
  await expect(
    gateStale.getByTestId("coding-session-landing-gate-word"),
  ).not.toHaveText("running");
  // A stale start is not activity: Landing's badge draws nothing.
  await expect(
    stale.getByTestId("coding-session-surface-badge-landing"),
  ).toHaveCount(0);
  await shoot(stale, "SV41-landing-stale", gateStale);
  await stale.close();

  // Every shot proves a different state.
  const unique = new Set(hashes.values());
  expect(
    unique.size,
    `screenshot hashes must be distinct: ${JSON.stringify([...hashes])}`,
  ).toBe(hashes.size);
  expect([...hashes.keys()].sort()).toEqual([
    "SV24-agents",
    "SV24-browser",
    "SV24-device",
    "SV24-diff",
    "SV24-files",
    "SV24-files-remote",
    "SV24-landing",
    "SV24-landing-gate-running",
    "SV24-landing-refused",
    "SV24-people",
    "SV24-plan",
    "SV24-pulse",
    "SV41-landing-stale",
  ]);
});
