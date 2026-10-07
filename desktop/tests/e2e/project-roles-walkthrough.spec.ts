import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

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
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import type { ProjectInstalledRoles } from "@/features/roles/lib/projectInstalledRoles";
import {
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_PROJECT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { openDashboardTab } from "../helpers/dashboard";

/**
 * The Tank Loop setup walkthrough (docs/TANK_LOOP_WALKTHROUGH_IMPL.md,
 * Acceptance), driven through the surfaces a person reaches:
 *
 * 1. An agent associated with a project (`ManagedAgent.projectRef`, which
 *    setup records on install) — stopped, never seated — is listed under that
 *    project's filter with its project and primary role, and renames in place
 *    without its pubkey or association changing.
 * 2. "Who leads" on a founded page in that project offers only that project's
 *    agents, counts the ones left out, preselects the one project lead, and
 *    keeps an explicit choice; Solo needs no lead
 *    (`docs/PROJECT_AGENT_HIRING_IMPL.md` § UI rules).
 * 3. A session in a project transport channel the viewer is not a member of
 *    stays steerable for a project owner, and read-only for a viewer.
 *
 * Installed roles come from the setup journals, seeded through the bridge's
 * `__BEEKEEPER_E2E_PROJECT_TEAM_SETUP__`. Everything else the app reads — managed
 * agents, projects, channels, signed session facts — goes through the normal
 * mock paths, with a thin invoke wrapper only where the bridge has no fixture
 * (a channel's `project_ref`, the workdir state) or where the spec must read
 * the exact arguments a command received.
 */

const SHOTS = "test-results/project-roles-walkthrough";

const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};

const PROJECT_NAME = "Tank Loop";
const PROJECT_DTAG = "tankloop";
const PROJECT_REF = `30621:${FOUNDER_IDENTITY.pubkey}:${PROJECT_DTAG}`;
const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

const LOOM = {
  pubkey: "1c47d440".padEnd(64, "a"),
  name: "Loom",
};
const KEYSTONE = {
  pubkey: "b2".repeat(32),
  name: "Keystone",
};

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

type WalkthroughWindow = Window & {
  __WALKTHROUGH_CALLS__?: { command: string; args: unknown }[];
  __WALKTHROUGH_CHANNEL__?: {
    channelType: "stream" | "transport";
    isMember: boolean;
    projectRef: string | null;
  } | null;
};

function projectHead(input: {
  owner: string;
  dtag: string;
  name: string;
  members?: [pubkey: string, role: string][];
  /** List `engineering` under this project in the sidebar. */
  listsChannel?: boolean;
}): RelayEvent {
  return {
    id: `walkthrough-${input.dtag}`.padEnd(64, "0"),
    pubkey: input.owner,
    created_at: Math.floor(Date.now() / 1000) - 3_600,
    kind: KIND_PROJECT,
    tags: [
      ["d", input.dtag],
      ["name", input.name],
      ...(input.listsChannel ? [["channel", CHANNEL_ID]] : []),
      ...(input.members ?? []).map(([pubkey, role]) => ["p", pubkey, "", role]),
    ],
    content: "",
    sig: "0".repeat(128),
  };
}

function installation(): ProjectInstalledRoles {
  return {
    projectRef: PROJECT_REF,
    setupId: "setup-tankloop",
    publicationId: "publication-1",
    teamId: "team-tankloop",
    source: {
      repoRef: `30617:${FOUNDER_IDENTITY.pubkey}:tankloop-packs`,
      sha: "c".repeat(40),
      packPath: "personas/roles",
    },
    leadChannelId: null,
    roles: [
      {
        role: "lead",
        agentPubkey: LOOM.pubkey,
        packRef: {
          repo: `30617:${FOUNDER_IDENTITY.pubkey}:tankloop-packs`,
          sha: "c".repeat(40),
          role: "lead",
          path: "personas/roles/lead",
        },
      },
    ],
  };
}

/**
 * Wrap the mock invoke: record calls, rewrite the one channel's membership
 * and project from `__WALKTHROUGH_CHANNEL__`, and answer the workdir read.
 * Registered before the bridge; the bridge assigns `invoke` at boot.
 */
function invokeWrapperInitScript() {
  type Invoke = (
    cmd: string,
    args?: Record<string, unknown>,
    options?: unknown,
  ) => Promise<unknown>;
  const w = window as WalkthroughWindow;
  w.__WALKTHROUGH_CALLS__ = [];
  let internals: Record<string, unknown> | undefined;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    get: () => internals,
    set: (value: Record<string, unknown>) => {
      internals = value;
      let real: Invoke | undefined;
      const wrapped: Invoke = async (cmd, args, options) => {
        w.__WALKTHROUGH_CALLS__?.push({ command: cmd, args });
        if (cmd === "get_coding_session_workdir_state") {
          return {
            version: 1,
            byProject: {},
            byChannel: {},
            mru: [],
            pending: {},
          };
        }
        if (!real) throw new Error("mock invoke is not installed yet");
        const result = await real(cmd, args, options);
        const override = w.__WALKTHROUGH_CHANNEL__;
        if (cmd === "get_channels" && override) {
          const payload = result as {
            channels: Record<string, unknown>[] | null;
          };
          return {
            ...payload,
            channels:
              payload.channels?.map((channel) =>
                channel.name === "engineering"
                  ? {
                      ...channel,
                      channel_type: override.channelType,
                      is_member: override.isMember,
                      project_ref: override.projectRef,
                    }
                  : channel,
              ) ?? null,
          };
        }
        return result;
      };
      Object.defineProperty(value, "invoke", {
        configurable: true,
        get: () => (real ? wrapped : undefined),
        set: (fn: Invoke) => {
          real = fn;
        },
      });
    },
  });
}

async function bootWalkthrough(
  page: Page,
  options: { channelInProject: boolean; extraProjects?: RelayEvent[] },
) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, FOUNDER_IDENTITY);
  await page.addInitScript(
    ({ events, installed, channel }) => {
      window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = events;
      window.__BEEKEEPER_E2E_PROJECT_TEAM_SETUP__ = {
        installedRoles: installed,
      };
      (window as WalkthroughWindow).__WALKTHROUGH_CHANNEL__ = channel;
    },
    {
      events: [
        projectHead({
          owner: FOUNDER_IDENTITY.pubkey,
          dtag: PROJECT_DTAG,
          name: PROJECT_NAME,
          listsChannel: true,
        }),
        ...(options.extraProjects ?? []),
      ],
      installed: [installation()],
      channel: options.channelInProject
        ? {
            channelType: "stream" as const,
            isMember: true,
            projectRef: PROJECT_REF,
          }
        : null,
    },
  );
  await page.addInitScript(invokeWrapperInitScript);
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
      instanceId: "0123456789abcdef",
    },
    codingSessionProviderRuntimes: [
      {
        instanceRef: "claude-primary",
        runtime: "claude",
        driver: "claude-agent-acp",
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
      // Installed and associated by Tank Loop's setup a minute ago: stopped,
      // no seat ever.
      {
        pubkey: LOOM.pubkey,
        name: LOOM.name,
        status: "stopped",
        homeRole: "lead",
        projectRef: PROJECT_REF,
        hasRolePack: true,
      },
      // An agent on this computer that belongs to no project.
      {
        pubkey: KEYSTONE.pubkey,
        name: KEYSTONE.name,
        status: "stopped",
        homeRole: "builder",
        hasRolePack: true,
      },
    ],
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

async function recordedCalls(page: Page, command: string) {
  return page.evaluate(
    (name) =>
      ((window as WalkthroughWindow).__WALKTHROUGH_CALLS__ ?? []).filter(
        (call) => call.command === name,
      ),
    command,
  );
}

test("an associated, never-seated agent is listed under its project and renames in place", async ({
  page,
}) => {
  await bootWalkthrough(page, { channelInProject: false });
  await openDashboardTab(page, "agents");

  const projectFilter = page.getByTestId("agent-filter-project");
  await expect(projectFilter).toBeVisible({ timeout: 15_000 });
  await projectFilter.selectOption({ label: PROJECT_NAME });

  const loomRow = page
    .getByTestId("agent-row")
    .and(page.locator(`[data-pubkey="${LOOM.pubkey}"]`));
  await expect(loomRow).toBeVisible();
  await expect(loomRow.getByTestId("agent-row-name")).toHaveText(LOOM.name);
  const loomProject = loomRow.getByTestId("agent-row-project");
  await expect(loomProject).toHaveText(`${PROJECT_NAME} · lead`);
  await expect(loomProject).toHaveAttribute("data-project-ref", PROJECT_REF);
  // Associated, so no missing-association warning.
  await expect(loomRow.getByTestId("agent-row-unassociated")).toHaveCount(0);
  // The project filter admits it by association alone: the agent that
  // belongs to no project and holds no seat is filtered out.
  await expect(
    page
      .getByTestId("agent-row")
      .and(page.locator(`[data-pubkey="${KEYSTONE.pubkey}"]`)),
  ).toHaveCount(0);

  // One selected project drives the role-pack selector too.
  const rolePacks = page.getByTestId("agents-project-roles");
  await expect(rolePacks).toContainText(PROJECT_NAME);
  await expect(
    rolePacks.getByTestId("role-packs-project-fallback"),
  ).toHaveCount(0);

  const rowItem = page.locator("div.relative", { has: loomRow });
  await rowItem.getByTestId("agent-row-rename").click();
  const input = rowItem.getByTestId("agent-row-rename-input");
  await expect(input).toHaveValue(LOOM.name);
  await input.fill("  Tank Lead  ");
  await rowItem.getByTestId("agent-row-rename-save").click();
  await expect(rowItem.getByTestId("agent-row-rename-form")).toHaveCount(0);

  const renamedRow = page
    .getByTestId("agent-row")
    .and(page.locator(`[data-pubkey="${LOOM.pubkey}"]`));
  await expect(renamedRow.getByTestId("agent-row-name")).toHaveText(
    "Tank Lead",
  );
  // A rename changes neither the project nor the primary role.
  await expect(renamedRow.getByTestId("agent-row-project")).toHaveText(
    `${PROJECT_NAME} · lead`,
  );
  const updates = await recordedCalls(page, "update_managed_agent");
  expect(updates).toHaveLength(1);
  expect(updates[0].args).toEqual({
    input: { pubkey: LOOM.pubkey, name: "Tank Lead" },
  });
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/01-renamed-installed-agent.png` });
});

const FOUNDED_URL =
  /#\/coding-sessions\/([0-9a-f-]{36})\/founded\/([0-9a-f-]{36})/;

async function foundSession(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page).toHaveURL(FOUNDED_URL);
  // The goal reader waits on the startup send budget (see
  // coding-session-founded-setup.spec.ts); drive the card after it settles.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
}

test("Who leads offers only the project's agents, counts the rest and preselects the lead; Solo needs no lead", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await bootWalkthrough(page, { channelInProject: true });
  await foundSession(page);

  await page.getByTestId("coding-session-founded-mode-team").click();
  const leadSelect = page.getByTestId("new-coding-session-lead-select");
  await expect(leadSelect).toBeVisible();

  // One group, the project's own agents. Keystone is on this computer with a
  // role, but belongs to no project, so it is not an option at all.
  const projectGroup = page.getByTestId(
    "new-coding-session-lead-group-project",
  );
  await expect(projectGroup).toHaveAttribute("label", `${PROJECT_NAME} agents`);
  await expect(projectGroup.locator("option")).toHaveText([
    `${LOOM.name} · lead · ${LOOM.pubkey.slice(0, 8)}…${LOOM.pubkey.slice(-4)}`,
  ]);
  expect(
    await leadSelect
      .locator("optgroup")
      .evaluateAll((groups) => groups.map((group) => group.dataset.testid)),
  ).toEqual(["new-coding-session-lead-group-project"]);
  await expect(
    leadSelect.locator(`option[value="${KEYSTONE.pubkey}"]`),
  ).toHaveCount(0);
  // …and the one left out is counted, never silently dropped.
  await expect(
    page.getByTestId("new-coding-session-lead-excluded"),
  ).toContainText(
    `1 agent on this computer isn't a ${PROJECT_NAME} agent, so it can't lead here.`,
  );

  // The single project lead is the default, named with its short pubkey.
  await expect(leadSelect).toHaveValue(LOOM.pubkey);
  await expect(
    page.getByTestId("new-coding-session-lead-identity"),
  ).toContainText(LOOM.name);

  // An explicit choice — here, nobody — sticks over the default.
  await leadSelect.selectOption("");
  await expect(leadSelect).toHaveValue("");
  await page
    .getByTestId("coding-session-founded-prompt")
    .fill("Keep Tank Loop reliable.");
  await page.getByTestId("coding-session-founded-prompt").blur();
  await expect(leadSelect).toHaveValue("");
  await leadSelect.selectOption(LOOM.pubkey);
  await expect(leadSelect).toHaveValue(LOOM.pubkey);
  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/02-who-leads.png`,
    fullPage: true,
  });

  // Solo: nobody to pick, and Start is not held for a lead.
  await page.getByTestId("coding-session-founded-mode-solo").click();
  await expect(leadSelect).toHaveCount(0);
  await expect(page.getByTestId("coding-session-founded-start")).toBeEnabled();
  await page.getByTestId("coding-session-founded-start").click();
  await expect(page.getByTestId("new-coding-session-blocker-lead")).toHaveCount(
    0,
  );
  await expect
    .poll(
      async () =>
        (
          await page.evaluate(
            () => window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [],
          )
        )
          .filter((event) => event.kind === 44221)
          .map((event) => JSON.parse(event.content))
          .filter((payload) => payload.action?.type === "session.create"),
      { timeout: 25_000 },
    )
    .toHaveLength(1);
  const create = (
    await page.evaluate(() => window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [])
  )
    .filter((event) => event.kind === 44221)
    .map((event) => JSON.parse(event.content))
    .find((payload) => payload.action?.type === "session.create");
  expect(create.action.actor).toBeUndefined();
  expect(create.action.role).toBeUndefined();
});

// ── 3. Composer access in a project transport ──────────────────────────────

const OTHER_OWNER = getPublicKey(generateSecretKey());
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
const CREATE_COMMAND_ID = "csl-walkthrough-session";
/** A project someone else created, where the viewer is a roster owner. */
const OWNED_PROJECT_REF = `30621:${OTHER_OWNER}:owned-transport`;
/** A project someone else created, where the viewer is a roster viewer. */
const VIEWER_PROJECT_REF = `30621:${OTHER_OWNER}:viewer-transport`;

function founderSecret(): Uint8Array {
  return Uint8Array.from(
    FOUNDER_IDENTITY.privateKey.match(/../g) ?? [],
    (byte) => Number.parseInt(byte, 16),
  );
}

function sessionEvents(): RelayEvent[] {
  const secret = founderSecret();
  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: builtGenesis.kind,
      created_at: BASE_CREATED_AT - 2,
      tags: builtGenesis.tags,
      content: builtGenesis.content,
    },
    secret,
  ) as unknown as RelayEvent;
  const builtCreate = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: CREATE_COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Project transport session",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: builtCreate.kind,
      created_at: BASE_CREATED_AT - 1,
      tags: builtCreate.tags,
      content: builtCreate.content,
    },
    secret,
  ) as unknown as RelayEvent;
  const provider = (
    kind: number,
    createdAt: number,
    tags: string[][],
    content: unknown,
  ) =>
    finalizeEvent(
      { kind, created_at: createdAt, tags, content: JSON.stringify(content) },
      PROVIDER_SECRET,
    ) as unknown as RelayEvent;
  return [
    genesis,
    create,
    provider(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", CREATE_COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(CREATE_COMMAND_ID)],
      ],
      {
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: CREATE_COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      },
    ),
    provider(
      KIND_CODING_SESSION_METADATA,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      {
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: "Project transport session",
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
      },
    ),
    provider(
      KIND_CODING_SESSION_LEASE,
      Math.floor(Date.now() / 1_000) - 5,
      [
        ["h", CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", TARGET_KEY],
        ["csl-command", CREATE_COMMAND_ID],
        ["cslease-seq", "1"],
      ],
      {
        schema: "buzz-coding-session-lease/v1",
        target: TARGET,
        state: "live",
        leaseSequence: 1,
      },
    ),
    provider(
      KIND_CODING_SESSION_TRANSCRIPT,
      BASE_CREATED_AT + 1,
      [
        ["h", CHANNEL_ID],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", TARGET_KEY],
        ["cst-seq", "1"],
        ["cst-key", codingSessionTranscriptSemanticKey(TARGET, 1)],
      ],
      {
        schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
        session: TARGET,
        eventSeq: 1,
        timestamp: BASE_TIMESTAMP_MS + 1_000,
        turnId: "turn-1",
        item: {
          kind: "user_prompt",
          content: "Check the transport access",
          commandId: CREATE_COMMAND_ID,
        },
      },
    ),
  ];
}

async function setChannel(
  page: Page,
  channel: NonNullable<WalkthroughWindow["__WALKTHROUGH_CHANNEL__"]>,
) {
  await page.evaluate(async (next) => {
    (window as WalkthroughWindow).__WALKTHROUGH_CHANNEL__ = next;
    await window.__BEEKEEPER_E2E_QUERY_CLIENT__?.invalidateQueries({
      queryKey: ["channels"],
    });
  }, channel);
}

test("a project owner steers a transport session without channel membership; a project viewer reads only", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await bootWalkthrough(page, {
    channelInProject: false,
    extraProjects: [
      projectHead({
        owner: OTHER_OWNER,
        dtag: "owned-transport",
        name: "Owned Transport",
        members: [[FOUNDER_IDENTITY.pubkey, "owner"]],
      }),
      projectHead({
        owner: OTHER_OWNER,
        dtag: "viewer-transport",
        name: "Viewer Transport",
        members: [[FOUNDER_IDENTITY.pubkey, "viewer"]],
      }),
    ],
  });
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toBeVisible();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: sessionEvents() },
  );
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-composer")).toBeVisible({
    timeout: 15_000,
  });

  const editor = page.getByLabel("Coding-session instruction");
  const authority = page.getByTestId("coding-session-control-authority");
  // Baseline: the founder is a member and can control.
  await expect(editor).toBeEnabled();
  await expect(authority).toHaveText("Can control");

  // Control: an ordinary channel without membership is the strict denial,
  // which proves the rewrite below reaches the composer.
  await setChannel(page, {
    channelType: "stream",
    isMember: false,
    projectRef: null,
  });
  await expect(editor).toHaveAttribute(
    "placeholder",
    "Join this channel to send a message.",
  );
  await expect(authority).toHaveText("View only");

  // A project transport the viewer is not a member of, where their roster
  // role is owner: the relay admits the write, so the composer does too.
  await setChannel(page, {
    channelType: "transport",
    isMember: false,
    projectRef: OWNED_PROJECT_REF,
  });
  await expect(authority).toHaveText("Can control", { timeout: 15_000 });
  await expect(editor).toBeEnabled();
  await expect(editor).not.toHaveAttribute(
    "placeholder",
    /Join this channel|View only|Read only|Checking your access/,
  );
  await expect(
    page.getByText("Join this channel to send a message."),
  ).toHaveCount(0);
  await waitForAnimations(page);
  await page.getByTestId("coding-session-composer").screenshot({
    path: `${SHOTS}/03-transport-owner-composer.png`,
  });

  // The same transport under a project where the roster role is viewer.
  await setChannel(page, {
    channelType: "transport",
    isMember: false,
    projectRef: VIEWER_PROJECT_REF,
  });
  await expect(authority).toHaveText("Read only", { timeout: 15_000 });
  await expect(editor).toHaveAttribute(
    "placeholder",
    "You can read this project session, but your project role (Viewer) does not allow steering.",
  );
  await expect(editor).toBeDisabled();
  await waitForAnimations(page);
  await page.getByTestId("coding-session-composer").screenshot({
    path: `${SHOTS}/03-transport-viewer-composer.png`,
  });
});
