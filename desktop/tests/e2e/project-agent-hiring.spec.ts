import { mkdirSync } from "node:fs";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionClosureEvent } from "@/features/coding-sessions/lib/codingSessionClosure";
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
import type { ProjectInstalledRoles } from "@/features/roles/lib/projectInstalledRoles";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_MANAGED_AGENT,
  KIND_PROJECT,
  KIND_PROJECT_MEMBERS,
} from "@/shared/constants/kinds";
import { projectAgentDigest } from "@/shared/lib/projectAgentAssociation";
import type { MockManagedAgentSeed } from "../../src/testing/e2eBridge";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Project agents and hiring (`docs/PROJECT_AGENT_HIRING_IMPL.md`, Acceptance
 * A, C, F, G, H), driven through the surfaces a person reaches.
 *
 * Two projects on one computer, Tank Loop and Harbor, each with its own
 * lead, builder, runner and verifier: the role names are shared, the
 * identities are not, and Harbor's builder is named to sort before Tank
 * Loop's. Bob is a builder that belongs to no project but holds an idle seat
 * in an open Tank Loop session and has closed Tank Loop history; Harbor's
 * runner has closed Tank Loop history only. Tank Loop's verifier lives on a
 * collaborator's computer and is known only by its published association; an
 * outsider's claim naming Tank Loop must be ignored.
 *
 * Association is membership. Seats, matching role names and installed packs
 * are never evidence of it.
 */

// These land in the repo's own `test-results/`, like every other spec's
// shots. The absolute path this replaced was a review directory on one
// laptop, so the whole file errored `ENOENT`/`EACCES` for anyone else and
// the evidence it claims to produce existed on exactly one machine.
const SHOTS = "test-results/project-agent-hiring";

const FOUNDER = TEST_IDENTITIES.tyler;
const COLLABORATOR = TEST_IDENTITIES.alice.pubkey;
const OUTSIDER = TEST_IDENTITIES.outsider.pubkey;

const TANK = {
  name: "Tank Loop",
  dtag: "tankloop",
  ref: `${KIND_PROJECT}:${FOUNDER.pubkey}:tankloop`,
};
const HARBOR = {
  name: "Harbor",
  dtag: "harbor",
  ref: `${KIND_PROJECT}:${FOUNDER.pubkey}:harbor`,
};
/** `engineering` in the mock channel fixture; Tank Loop lists it. */
const TANK_CHANNEL_NAME = "engineering";
const TANK_CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";

const PACKS_REPO = `30617:${FOUNDER.pubkey}:packs`;
const TANK_SHA = "7a2c9e41b0d35f6a8c1e2d3f4a5b6c7d8e9f0a1b";
const HARBOR_SHA = "3be07d1c5a9f2e4b6d8c0a1f3e5d7b9c1a2e4f6d";

type Agent = { pubkey: string; name: string; role: string };

const LOOM: Agent = { pubkey: "c1".repeat(32), name: "Loom", role: "lead" };
const BUILDER: Agent = {
  pubkey: "c2".repeat(32),
  name: "Builder",
  role: "builder",
};
const RUNNER: Agent = {
  pubkey: "c3".repeat(32),
  name: "Runner",
  role: "runner",
};
/** Tank Loop's verifier: no record here, published by the collaborator. */
const QUILL: Agent = {
  pubkey: "c4".repeat(32),
  name: "Quill",
  role: "verifier",
};
const MOORING: Agent = {
  pubkey: "d1".repeat(32),
  name: "Mooring",
  role: "lead",
};
/** Harbor's builder, named to sort before Tank Loop's. */
const AARDVARK: Agent = {
  pubkey: "d2".repeat(32),
  name: "Aardvark",
  role: "builder",
};
const RIGGER: Agent = {
  pubkey: "d3".repeat(32),
  name: "Rigger",
  role: "runner",
};
const WARDEN: Agent = {
  pubkey: "d4".repeat(32),
  name: "Warden",
  role: "verifier",
};
/** A builder on this computer that belongs to no project. */
const BOB: Agent = { pubkey: "e1".repeat(32), name: "Bob", role: "builder" };
/** Published by an outsider naming Tank Loop; never accepted. */
const IMPOSTOR: Agent = {
  pubkey: "e2".repeat(32),
  name: "Impostor",
  role: "builder",
};

const TANK_AGENTS = [LOOM, BUILDER, RUNNER];
const HARBOR_AGENTS = [MOORING, AARDVARK, RIGGER, WARDEN];

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

const OPEN_SESSION_REF = "a7b1c2d3-1111-4222-8333-444455556666";
const CLOSED_SESSION_REF = "b8c2d3e4-5555-4666-8777-888899990000";

type HiringWindow = Window & {
  __HIRING_CALLS__?: { command: string; args: unknown }[];
  __HIRING_CHANNEL_PROJECT__?: string | null;
  __HIRING_SETUP__?: {
    projectRef: string;
    leadPubkey: string;
    roles: { role: string; agentPubkey: string }[];
  } | null;
};

function founderSecret(): Uint8Array {
  return Uint8Array.from(FOUNDER.privateKey.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function packRef(role: string, sha = TANK_SHA) {
  return { repo: PACKS_REPO, sha, role, path: `personas/roles/${role}` };
}

function projectHead(project: typeof TANK, listsChannel: boolean): RelayEvent {
  return {
    id: `hiring-${project.dtag}`.padEnd(64, "0"),
    pubkey: FOUNDER.pubkey,
    created_at: nowSeconds() - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", project.dtag],
      ["name", project.name],
      ...(listsChannel ? [["channel", TANK_CHANNEL_ID]] : []),
    ],
    content: "",
    sig: "0".repeat(128),
  };
}

/** The relay-signed roster: the collaborator may associate agents. */
function tankRoster(): RelayEvent {
  return {
    id: "hiring-roster-tankloop".padEnd(64, "0"),
    pubkey: "f0".repeat(32),
    created_at: nowSeconds() - 3_600,
    kind: KIND_PROJECT_MEMBERS,
    tags: [
      ["d", TANK.ref],
      ["p", COLLABORATOR, "", "collaborator"],
    ],
    content: "",
    sig: "0".repeat(128),
  };
}

/** An owner's kind:30177 claiming `agent` for `projectRef`. */
function publishedAssociation(
  owner: string,
  agent: Agent,
  projectRef: string,
): RelayEvent {
  return {
    id: `hiring-30177-${agent.name}`.padEnd(64, "0"),
    pubkey: owner,
    created_at: nowSeconds() - 1_800,
    kind: KIND_MANAGED_AGENT,
    tags: [["d", agent.pubkey]],
    content: JSON.stringify({
      name: agent.name,
      home_role: agent.role,
      project_digest: projectAgentDigest(projectRef),
    }),
    sig: "0".repeat(128),
  };
}

function installation(
  project: typeof TANK,
  agents: Agent[],
  sha: string,
): ProjectInstalledRoles {
  return {
    projectRef: project.ref,
    setupId: `setup-${project.dtag}`,
    publicationId: `publication-${project.dtag}`,
    teamId: `team-${project.dtag}`,
    source: { repoRef: PACKS_REPO, sha, packPath: "personas/roles" },
    leadChannelId: null,
    roles: agents.map((agent) => ({
      role: agent.role,
      agentPubkey: agent.pubkey,
      packRef: packRef(agent.role, sha),
    })),
  };
}

function seed(agent: Agent, projectRef: string | null): MockManagedAgentSeed {
  return {
    pubkey: agent.pubkey,
    name: agent.name,
    status: "stopped",
    homeRole: agent.role,
    projectRef,
    hasRolePack: true,
  };
}

const CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: true,
  context: false,
  diff: false,
  plan: true,
};

function sessionTarget(instanceId: string, sessionId: string) {
  return { driver: "claude-agent-acp", instanceId, sessionId, generation: 1 };
}

/** A provider-signed seat report. Key order is the strict decoder's own. */
function seatMetadata(input: {
  target: ReturnType<typeof sessionTarget>;
  agent: Agent;
  sessionRef: string;
  title: string;
  model: string;
  status: "idle" | "running";
  secondsAgo: number;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: nowSeconds() - input.secondsAgo,
      tags: [
        ["h", TANK_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: TANK.ref,
        repoRef: null,
        title: input.title,
        agentRef: input.agent.pubkey,
        provider: "claude-primary",
        runtime: "claude-agent-acp",
        model: input.model,
        status: input.status,
        branch: null,
        capabilities: CAPABILITIES,
        sessionRef: input.sessionRef,
        role: input.agent.role,
        packRef: packRef(input.agent.role),
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/**
 * Tank Loop's signed session history: an open session where Loom is idle,
 * Builder is working and Bob is idle; and a founder-closed session in which
 * Bob and Harbor's Rigger once held seats.
 */
function tankSessionEvents(): RelayEvent[] {
  const open = "Tank Loop reliability";
  const closedTitle = "Pump calibration";
  const founder = founderSecret();
  const commandId = "csl-hiring-closed";
  const closedLead = sessionTarget(
    "hiring-closed-bob",
    "c0c0c0c0-1111-4222-8333-444455556666",
  );
  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: TANK_CHANNEL_ID,
    sessionRef: CLOSED_SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: builtGenesis.kind,
      created_at: nowSeconds() - 9_000,
      tags: builtGenesis.tags,
      content: builtGenesis.content,
    },
    founder,
  ) as unknown as RelayEvent;
  const builtCreate = buildCodingSessionCreateEvent({
    channelId: TANK_CHANNEL_ID,
    commandId,
    projectRef: TANK.ref,
    repoRef: null,
    sessionRef: CLOSED_SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: closedTitle,
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: builtCreate.kind,
      created_at: nowSeconds() - 8_990,
      tags: builtCreate.tags,
      content: builtCreate.content,
    },
    founder,
  ) as unknown as RelayEvent;
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: nowSeconds() - 8_980,
      tags: [
        ["h", TANK_CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "created",
        session: closedLead,
        error: null,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
  const builtClosure = buildCodingSessionClosureEvent({
    action: "closed",
    channelId: TANK_CHANNEL_ID,
    genesisRef: genesis.id,
    sessionRef: CLOSED_SESSION_REF,
  });
  const closure = finalizeEvent(
    {
      kind: builtClosure.kind,
      created_at: nowSeconds() - 4_000,
      tags: builtClosure.tags,
      content: builtClosure.content,
    },
    founder,
  ) as unknown as RelayEvent;

  return [
    seatMetadata({
      target: sessionTarget(
        "hiring-open-loom",
        "a1a1a1a1-1111-4222-8333-444455556666",
      ),
      agent: LOOM,
      sessionRef: OPEN_SESSION_REF,
      title: open,
      model: "opus",
      status: "idle",
      secondsAgo: 300,
    }),
    seatMetadata({
      target: sessionTarget(
        "hiring-open-builder",
        "a2a2a2a2-1111-4222-8333-444455556666",
      ),
      agent: BUILDER,
      sessionRef: OPEN_SESSION_REF,
      title: open,
      model: "sonnet",
      status: "running",
      secondsAgo: 60,
    }),
    seatMetadata({
      target: sessionTarget(
        "hiring-open-bob",
        "a3a3a3a3-1111-4222-8333-444455556666",
      ),
      agent: BOB,
      sessionRef: OPEN_SESSION_REF,
      title: open,
      model: "sonnet",
      status: "idle",
      secondsAgo: 200,
    }),
    genesis,
    create,
    receipt,
    seatMetadata({
      target: closedLead,
      agent: BOB,
      sessionRef: CLOSED_SESSION_REF,
      title: closedTitle,
      model: "sonnet",
      status: "idle",
      secondsAgo: 5_400,
    }),
    seatMetadata({
      target: sessionTarget(
        "hiring-closed-rigger",
        "c1c1c1c1-1111-4222-8333-444455556666",
      ),
      agent: RIGGER,
      sessionRef: CLOSED_SESSION_REF,
      title: closedTitle,
      model: "haiku",
      status: "idle",
      secondsAgo: 5_000,
    }),
    closure,
  ];
}

/**
 * Record calls, make the founder a member of Tank Loop's channel and set its
 * project for the founded page,
 * and answer project setup reads from `__HIRING_SETUP__`. Registered before
 * the bridge, which assigns `invoke` at boot.
 */
function invokeWrapperInitScript() {
  type Invoke = (
    cmd: string,
    args?: Record<string, unknown>,
    options?: unknown,
  ) => Promise<unknown>;
  const w = window as HiringWindow;
  w.__HIRING_CALLS__ = [];
  const setupAnswer = (
    cmd: string,
    args: Record<string, unknown> | undefined,
  ): { value: unknown } | null => {
    const setup = w.__HIRING_SETUP__;
    if (!setup || !cmd.startsWith("project_team_setup_")) return null;
    const relayUrl = String(args?.expectedRelayUrl ?? "");
    const repoRef = `30617:${setup.projectRef.split(":")[1]}:packs`;
    const sha = "7a2c9e41b0d35f6a8c1e2d3f4a5b6c7d8e9f0a1b";
    const publication = {
      publicationId: "publication-tankloop",
      setupId: "setup-tankloop",
      status: "adopted",
      snapshotId: "a".repeat(64),
      destination: {
        repoRef,
        packPath: "personas/roles",
        baseCommit: null,
        createAnnouncement: null,
      },
      sourceExpectation: { kind: "if_unset" },
      candidateRef: "refs/heads/setup/publication-tankloop",
      candidateCommit: sha,
      sourceEventId: "d".repeat(64),
      message: null,
    };
    switch (cmd) {
      case "project_team_setup_get":
        return {
          value: {
            setupId: "setup-tankloop",
            projectRef: setup.projectRef,
            ownerPubkey: setup.projectRef.split(":")[1],
            relayUrl,
            status: "draft",
            projectDirectory: "/projects/tankloop",
            draftDirectory: "/drafts/setup-tankloop",
            rolesDirectory: "/drafts/setup-tankloop/personas/roles",
            intent: "Keep Tank Loop reliable.",
            createdAt: "2026-09-14T12:00:00Z",
            roles: setup.roles.map((entry) => entry.role),
            latestSnapshotId: "a".repeat(64),
          },
        };
      case "project_team_setup_snapshot":
        return {
          value: {
            setupId: "setup-tankloop",
            snapshotId: "a".repeat(64),
            rolesDirectory: "/snapshots/tankloop/personas/roles",
            manifestPath: "/snapshots/tankloop/manifest.json",
            roles: setup.roles.map((entry) => entry.role),
          },
        };
      case "project_team_setup_validate":
        return {
          value: {
            setupId: "setup-tankloop",
            status: "draft",
            valid: true,
            roles: setup.roles.map((entry) => entry.role),
            diagnostics: [],
          },
        };
      case "project_team_setup_get_authoring":
      case "project_team_setup_get_launch":
        return { value: null };
      case "project_team_setup_get_publication_options":
        return {
          value: {
            currentSourceEventId: "d".repeat(64),
            suggestedDestination: null,
            sourceExpectation: { kind: "if_unset" },
            publication,
          },
        };
      case "project_team_setup_get_publication":
      case "project_team_setup_peek_publication":
        return { value: publication };
      case "project_team_setup_get_activation":
        return {
          value: {
            publicationId: "publication-tankloop",
            source: { repoRef, commit: sha, packPath: "personas/roles" },
            installation: {
              status: "installed",
              installedRoles: setup.roles.map((entry) => ({
                ...entry,
                packRef: {
                  repo: repoRef,
                  sha,
                  role: entry.role,
                  path: `personas/roles/${entry.role}`,
                },
              })),
              message: "Installed from the adopted revision.",
            },
            lead: {
              status: "started",
              channelId: "tankloop-lead-channel",
              sessionRef: "tankloop-lead-session",
              leadPubkey: setup.leadPubkey,
              message: "The project lead is running.",
            },
          },
        };
      default:
        return null;
    }
  };
  let internals: Record<string, unknown> | undefined;
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    configurable: true,
    get: () => internals,
    set: (value: Record<string, unknown>) => {
      internals = value;
      let real: Invoke | undefined;
      const wrapped: Invoke = async (cmd, args, options) => {
        w.__HIRING_CALLS__?.push({ command: cmd, args });
        if (cmd === "get_coding_session_workdir_state") {
          return {
            version: 1,
            byProject: {},
            byChannel: {},
            mru: [],
            pending: {},
          };
        }
        // Nobody in this fixture was given a signed assignment; the bridge has
        // no fixture for the native declared-work projection.
        if (cmd === "pulse_declared_work") {
          return {
            schema: "buzz-pulse-declared-work/v1",
            viewerPubkey: null,
            sessions: [],
            errors: [],
          };
        }
        const setup = setupAnswer(cmd, args);
        if (setup) return setup.value;
        if (!real) throw new Error("mock invoke is not installed yet");
        const result = await real(cmd, args, options);
        // The founder is a member of Tank Loop's channel (the fixture lists
        // other members), and the founded page reads the channel's project.
        if (cmd === "get_channels") {
          const channelProject = w.__HIRING_CHANNEL_PROJECT__ ?? null;
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
                      is_member: true,
                      ...(channelProject
                        ? { project_ref: channelProject }
                        : {}),
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

async function boot(
  page: Page,
  options: {
    /** Pubkeys whose local record lacks the association. */
    unassociated?: string[];
    channelProject?: string | null;
    setup?: HiringWindow["__HIRING_SETUP__"];
  } = {},
) {
  mkdirSync(SHOTS, { recursive: true });
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true }),
    );
  }, FOUNDER);
  await page.addInitScript(
    ({ events, installed, channelProject, setup }) => {
      const w = window as HiringWindow;
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
      window.__BUZZ_E2E_PROJECT_TEAM_SETUP__ = { installedRoles: installed };
      w.__HIRING_CHANNEL_PROJECT__ = channelProject;
      w.__HIRING_SETUP__ = setup;
    },
    {
      events: [
        projectHead(TANK, true),
        projectHead(HARBOR, false),
        tankRoster(),
        publishedAssociation(COLLABORATOR, QUILL, TANK.ref),
        publishedAssociation(OUTSIDER, IMPOSTOR, TANK.ref),
      ],
      installed: [
        installation(TANK, TANK_AGENTS, TANK_SHA),
        installation(HARBOR, HARBOR_AGENTS, HARBOR_SHA),
      ],
      channelProject: options.channelProject ?? null,
      setup: options.setup ?? null,
    },
  );
  await page.addInitScript(invokeWrapperInitScript);
  const unassociated = new Set(options.unassociated ?? []);
  const projectOf = (agent: Agent, projectRef: string) =>
    unassociated.has(agent.pubkey) ? null : projectRef;
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
        capabilities: { ...CAPABILITIES, threadSteer: false },
      },
    ],
    managedAgents: [
      ...TANK_AGENTS.map((agent) => seed(agent, projectOf(agent, TANK.ref))),
      ...HARBOR_AGENTS.map((agent) =>
        seed(agent, projectOf(agent, HARBOR.ref)),
      ),
      seed(BOB, null),
    ],
  });
  await page.setViewportSize({ width: 1280, height: 1400 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

async function openProjectTab(
  page: Page,
  project: typeof TANK,
  tab: "agents" | "packs",
) {
  const group = page.getByTestId(`project-group-${project.dtag}`);
  await expect(group).toBeVisible({ timeout: 15_000 });
  await group.hover();
  await page.getByTestId(`project-open-${project.dtag}`).click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId(`project-tab-${tab}`).click();
}

async function seedTankSessions(page: Page) {
  await page.evaluate(
    ({ channelName, events }) => {
      const seedEvent = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seedEvent) throw new Error("mock signed-event seam is missing");
      for (const event of events as never[]) seedEvent({ channelName, event });
    },
    {
      channelName: TANK_CHANNEL_NAME,
      events: tankSessionEvents() as unknown as never[],
    },
  );
}

function agentRow(scope: Page | Locator, agent: Agent): Locator {
  return scope.locator(
    `[data-testid="project-agent-row"][data-agent-pubkey="${agent.pubkey}"]`,
  );
}

async function openTankAgentsWithHistory(page: Page) {
  await openProjectTab(page, TANK, "agents");
  await expect(page.getByTestId("project-agents-screen")).toBeVisible();
  await expect(page.getByTestId("project-agents-members")).toContainText(
    LOOM.name,
    { timeout: 15_000 },
  );
  await seedTankSessions(page);
  await expect(agentRow(page, BUILDER)).toHaveAttribute(
    "data-agent-state",
    "working",
    { timeout: 15_000 },
  );
  await expect(page.getByTestId("project-agents-previous")).toContainText(
    RIGGER.name,
    { timeout: 15_000 },
  );
}

async function recordedCalls(page: Page, command: string) {
  return page.evaluate(
    (name) =>
      ((window as HiringWindow).__HIRING_CALLS__ ?? []).filter(
        (call) => call.command === name,
      ),
    command,
  );
}

test("each project's Agents tab lists only its own agents, with borrowed and previous participants apart", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await boot(page);
  await openTankAgentsWithHistory(page);

  // ── Tank Loop: members are exactly its associated agents. ──
  const members = page.getByTestId("project-agents-members");
  const memberRows = members.locator('[data-testid="project-agent-row"]');
  await expect(memberRows).toHaveCount(4);
  expect(
    await memberRows.evaluateAll((rows) =>
      rows.map((row) => row.getAttribute("data-agent-pubkey")),
    ),
  ).toEqual([BUILDER.pubkey, LOOM.pubkey, QUILL.pubkey, RUNNER.pubkey]);
  const states: [Agent, string, string][] = [
    [LOOM, "idle", "Idle"],
    [BUILDER, "working", "Working"],
    [RUNNER, "available", "Available"],
    [QUILL, "elsewhere", "On another computer"],
  ];
  for (const [agent, state, text] of states) {
    const row = agentRow(members, agent);
    await expect(row).toHaveAttribute("data-agent-state", state);
    await expect(row.getByTestId("project-agent-state")).toHaveText(text);
    await expect(row.getByTestId("project-agent-role")).toHaveText(
      agent.role[0].toUpperCase() + agent.role.slice(1),
    );
    await expect(row.getByTestId("project-agent-badge")).toHaveText(
      "Project agent",
    );
  }
  await expect(
    agentRow(members, QUILL).getByTestId("project-agent-owner"),
  ).toContainText("can't run on this computer");
  // Harbor's agents share every role name; none of them is listed here.
  for (const agent of [...HARBOR_AGENTS.filter((a) => a !== RIGGER), BOB]) {
    await expect(agentRow(members, agent)).toHaveCount(0);
  }
  // An outsider's claim naming this project is not membership.
  await expect(agentRow(page, IMPOSTOR)).toHaveCount(0);

  // Bob holds an idle seat in an open session: borrowed, not a member.
  const borrowed = page.getByTestId("project-agents-borrowed");
  const bob = agentRow(borrowed, BOB);
  await expect(
    borrowed.locator('[data-testid="project-agent-row"]'),
  ).toHaveCount(1);
  await expect(bob).toHaveAttribute("data-agent-state", "idle");
  await expect(bob.getByTestId("project-agent-badge")).toHaveText("Borrowed");
  await expect(bob.getByTestId("project-agent-not-member")).toContainText(
    `Not a ${TANK.name} agent`,
  );
  // His closed history stays attributed to him, under his own row.
  await bob.getByTestId("project-agent-sessions").locator("summary").click();
  await expect(bob.getByTestId("project-agent-session")).toHaveCount(2);
  await expect(
    bob.locator(
      '[data-testid="project-agent-session"][data-session-closed="true"]',
    ),
  ).toContainText("Pump calibration");

  // Harbor's runner only ever sat in the closed session: previously here,
  // labelled as Harbor's.
  const previous = page.getByTestId("project-agents-previous");
  const rigger = agentRow(previous, RIGGER);
  await expect(rigger).toHaveAttribute("data-agent-state", "historical");
  await expect(rigger.getByTestId("project-agent-badge")).toHaveText([
    "Previously here",
    "Borrowed",
  ]);
  await expect(rigger.getByTestId("project-agent-not-member")).toContainText(
    `Not a ${TANK.name} agent — belongs to ${HARBOR.name}`,
  );

  await waitForAnimations(page);
  await page.getByTestId("project-agents").screenshot({
    path: `${SHOTS}/01-tank-loop-agents-tab.png`,
  });

  // Details: role @ sha, runtime and model, host key, pubkey.
  const builder = agentRow(members, BUILDER);
  await builder.getByTestId("project-agent-details").locator("summary").click();
  await expect(builder.getByTestId("project-agent-pubkey")).toHaveText(
    BUILDER.pubkey,
  );
  await expect(builder.getByTestId("project-agent-installation")).toHaveText(
    `Installed as Builder · instructions ${TANK_SHA.slice(0, 8)}`,
  );
  const session = builder.getByTestId("project-agent-session");
  await expect(session).toBeVisible();
  await expect(session).toContainText("claude-primary · sonnet");
  await expect(
    session.getByTestId("project-agent-session-instructions"),
  ).toHaveText(`Instructions builder @ ${TANK_SHA.slice(0, 8)}`);
  await expect(session).toContainText(
    `host ${PROVIDER_PUBKEY.slice(0, 8)}…${PROVIDER_PUBKEY.slice(-4)}`,
  );
  await waitForAnimations(page);
  await builder.screenshot({
    path: `${SHOTS}/02-tank-loop-builder-details.png`,
  });

  // ── Harbor: its own four, Aardvark first; nothing of Tank Loop's. ──
  await openProjectTab(page, HARBOR, "agents");
  const harborMembers = page.getByTestId("project-agents-members");
  await expect(harborMembers).toContainText(AARDVARK.name, {
    timeout: 15_000,
  });
  const harborRows = harborMembers.locator('[data-testid="project-agent-row"]');
  await expect(harborRows).toHaveCount(4);
  expect(
    await harborRows.evaluateAll((rows) =>
      rows.map((row) => row.getAttribute("data-agent-pubkey")),
    ),
  ).toEqual([AARDVARK.pubkey, MOORING.pubkey, RIGGER.pubkey, WARDEN.pubkey]);
  for (const agent of [...TANK_AGENTS, QUILL, BOB, IMPOSTOR]) {
    await expect(agentRow(page, agent)).toHaveCount(0);
  }
  await expect(page.getByTestId("project-agents-borrowed")).toHaveCount(0);
  await waitForAnimations(page);
  await page.getByTestId("project-agents").screenshot({
    path: `${SHOTS}/03-harbor-agents-tab.png`,
  });
});

test("associating Bob with Tank Loop asks first and moves him from borrowed to project agents", async ({
  page,
}) => {
  test.setTimeout(90_000);
  await boot(page);
  await openTankAgentsWithHistory(page);

  const borrowed = page.getByTestId("project-agents-borrowed");
  const bob = agentRow(borrowed, BOB);
  await expect(bob).toBeVisible();
  // Harbor's runner belongs to another project: no Associate control at all.
  await expect(
    agentRow(page, RIGGER).getByTestId("project-agent-associate-button"),
  ).toHaveCount(0);

  const associate = bob.getByTestId("project-agent-associate-button");
  await expect(associate).toHaveText(`Associate with ${TANK.name}`);
  await expect(associate).toBeEnabled();
  await associate.click();
  const confirm = bob.getByTestId("project-agent-associate-confirm");
  await expect(confirm).toContainText(
    `Bob becomes a permanent ${TANK.name} Builder agent. Its history stays attributed to Bob. Adds the agent to the project roster as a collaborator, so it can read and write the project (Pulse, to-dos) under its own key.`,
  );
  // Nothing is written until the person confirms.
  expect(
    await recordedCalls(page, "associate_managed_agent_with_project"),
  ).toHaveLength(0);
  await waitForAnimations(page);
  await bob.screenshot({ path: `${SHOTS}/04-associate-bob-confirm.png` });

  await bob.getByTestId("project-agent-associate-yes").click();
  const members = page.getByTestId("project-agents-members");
  const bobMember = agentRow(members, BOB);
  await expect(bobMember).toBeVisible({ timeout: 15_000 });
  await expect(bobMember).toHaveAttribute("data-agent-section", "project");
  await expect(bobMember).toHaveAttribute("data-agent-state", "idle");
  await expect(bobMember.getByTestId("project-agent-badge")).toHaveText(
    "Project agent",
  );
  await expect(page.getByTestId("project-agents-borrowed")).toHaveCount(0);
  const calls = await recordedCalls(
    page,
    "associate_managed_agent_with_project",
  );
  expect(calls).toHaveLength(1);
  expect(calls[0].args).toEqual({ pubkey: BOB.pubkey, projectRef: TANK.ref });
  // His closed history is still his.
  await bobMember
    .getByTestId("project-agent-sessions")
    .locator("summary")
    .click();
  await expect(
    bobMember.locator(
      '[data-testid="project-agent-session"][data-session-closed="true"]',
    ),
  ).toContainText("Pump calibration");
  await waitForAnimations(page);
  await members.screenshot({ path: `${SHOTS}/05-bob-now-a-project-agent.png` });
});

test("Who leads in a Tank Loop session offers only Tank Loop agents and counts the rest; Solo is unaffected", async ({
  page,
}) => {
  test.setTimeout(120_000);
  await boot(page, { channelProject: TANK.ref });
  await page.getByTestId(`channel-${TANK_CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });

  await page.getByTestId("coding-session-founded-mode-team").click();
  const leadSelect = page.getByTestId("new-coding-session-lead-select");
  await expect(leadSelect).toBeVisible();
  const group = page.getByTestId("new-coding-session-lead-group-project");
  await expect(group).toHaveAttribute("label", `${TANK.name} agents`);
  const label = (agent: Agent) =>
    `${agent.name} · ${agent.role} · ${agent.pubkey.slice(0, 8)}…${agent.pubkey.slice(-4)}`;
  await expect(group.locator("option")).toHaveText(
    [LOOM, BUILDER, RUNNER].map(label),
  );
  expect(
    await leadSelect
      .locator("optgroup")
      .evaluateAll((groups) => groups.map((entry) => entry.dataset.testid)),
  ).toEqual(["new-coding-session-lead-group-project"]);
  for (const agent of [...HARBOR_AGENTS, BOB]) {
    await expect(
      leadSelect.locator(`option[value="${agent.pubkey}"]`),
    ).toHaveCount(0);
  }
  const excluded = page.getByTestId("new-coding-session-lead-excluded");
  await expect(excluded).toContainText(
    `5 agents on this computer aren't ${TANK.name} agents, so they can't lead here.`,
  );
  // The one Tank Loop lead is the default.
  await expect(leadSelect).toHaveValue(LOOM.pubkey);
  await waitForAnimations(page);
  await page
    .getByTestId("new-coding-session-lead")
    .screenshot({ path: `${SHOTS}/06-who-leads-tank-loop.png` });

  // Solo needs no lead and is not held for one.
  await page.getByTestId("coding-session-founded-mode-solo").click();
  await expect(leadSelect).toHaveCount(0);
  await expect(page.getByTestId("new-coding-session-blocker-lead")).toHaveCount(
    0,
  );
  await expect(page.getByTestId("coding-session-founded-start")).toBeEnabled();
});

for (const blocked of [false, true]) {
  test(`setup ends with the Tank Loop roster ${blocked ? "blocked while an installed agent lacks the association" : "complete once every installed agent is associated"}`, async ({
    page,
  }) => {
    test.setTimeout(90_000);
    await boot(page, {
      unassociated: blocked ? [RUNNER.pubkey] : [],
      setup: {
        projectRef: TANK.ref,
        leadPubkey: LOOM.pubkey,
        roles: TANK_AGENTS.map((agent) => ({
          role: agent.role,
          agentPubkey: agent.pubkey,
        })),
      },
    });
    await openProjectTab(page, TANK, "packs");
    await page.getByTestId("project-team-setup-open").click();
    const dialog = page.getByTestId("project-team-setup-dialog");
    const roster = dialog.getByTestId("project-team-setup-roster");
    await expect(roster).toBeVisible({ timeout: 15_000 });
    await expect(roster).toHaveAttribute(
      "data-roster",
      blocked ? "blocked" : "ready",
      { timeout: 15_000 },
    );
    for (const agent of TANK_AGENTS)
      await expect(roster).toContainText(agent.name);
    const next = roster.getByTestId("project-team-setup-roster-next");
    if (blocked) {
      await expect(next).toHaveAttribute("data-state", "blocked");
      await expect(next).toContainText(
        "Lead started, but it can't hire Runner: it isn't associated with this project on this computer.",
      );
      await expect(dialog).not.toContainText("Setup complete");
      await expect(
        roster.getByRole("button", { name: "Retry installation" }),
      ).toBeVisible();
    } else {
      await expect(next).toHaveAttribute("data-stage", "lead_started");
      await expect(next).toHaveText(
        `Setup complete. ${LOOM.name} leads ${TANK.name}. Give ${LOOM.name} a task in its session; its first message lists these agents, and it hires only them.`,
      );
    }
    await waitForAnimations(page);
    await roster.screenshot({
      path: `${SHOTS}/${blocked ? "08-setup-roster-blocked" : "07-setup-roster-complete"}.png`,
    });
  });
}

/** Every state word and badge sits inside the viewport and is not clipped. */
async function expectUnclipped(page: Page, width: number) {
  const boxes = await page
    .locator(
      '[data-testid="project-agent-state"], [data-testid="project-agent-badge"], [data-testid="project-agent-role"]',
    )
    .evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return {
          text: element.textContent ?? "",
          left: rect.left,
          right: rect.right,
          clipped: element.scrollWidth > element.clientWidth + 1,
        };
      }),
    );
  expect(boxes.length).toBeGreaterThan(0);
  for (const box of boxes) {
    expect(box.left, box.text).toBeGreaterThanOrEqual(0);
    expect(box.right, box.text).toBeLessThanOrEqual(width);
    expect(box.clipped, box.text).toBe(false);
  }
}

for (const layout of [
  {
    name: "narrow window",
    shot: "09-agents-tab-narrow",
    width: 480,
    fontSize: "",
  },
  {
    name: "text at 125%",
    shot: "10-agents-tab-text-125",
    width: 1280,
    fontSize: "125%",
  },
  {
    name: "text at 150%",
    shot: "11-agents-tab-text-150",
    width: 1280,
    fontSize: "150%",
  },
]) {
  test(`the Tank Loop Agents tab wraps without clipping in a ${layout.name}`, async ({
    page,
  }) => {
    test.setTimeout(90_000);
    await boot(page);
    await openTankAgentsWithHistory(page);
    await page.setViewportSize({ width: layout.width, height: 900 });
    if (layout.fontSize) {
      await page.evaluate((fontSize) => {
        document.documentElement.style.fontSize = fontSize;
      }, layout.fontSize);
    }
    for (const agent of [LOOM, BUILDER, RUNNER, QUILL, BOB]) {
      await expect(
        agentRow(page, agent).getByTestId("project-agent-state"),
      ).toBeVisible();
    }
    await waitForAnimations(page);
    await expectUnclipped(page, layout.width);
    await page.getByTestId("project-agents-members").screenshot({
      path: `${SHOTS}/${layout.shot}.png`,
    });
  });
}
