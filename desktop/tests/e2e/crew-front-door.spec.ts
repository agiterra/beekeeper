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
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The crew front door, driven rather than described.
 *
 * Four surfaces landed in one batch and none of them existed before: the
 * crew-role installer, the Crew tab reading a minted crew, the join dialog's
 * seat field, and the pending screen's seat line. Each of them makes a claim
 * about something the operator cannot see — which pack was installed, which
 * seats a crew holds, what role an agent *is*, and whether custody actually
 * staged a role pack — so each of them is a place the app can lie without
 * anything failing.
 *
 * The mock bridge carries the shapes the backend commands answer with. The
 * two crew commands (`install_crew_role_packs`,
 * `pick_crew_role_packs_directory`) and the seat-staging command are not in
 * the shared bridge, so they are answered here, in front of it, with exactly
 * the payloads the Rust structs serialise
 * (`desktop/src-tauri/src/managed_agents/crew_roles.rs`,
 * `.../actor_seats.rs`). Everything else is the app's own code path.
 */

const SHOTS = "test-results/fd2-poke";

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
/**
 * The founder is the *operator*, not a stranger: joining a session and adding
 * a provider is founder-only authority (`CodingSessionWorkspace.tsx:239`), so
 * a spec that seeds a session founded by anybody else can never reach the
 * dialog under test. `tyler` is the bridge's own known identity, seeded into
 * the identity override below so the mock signs with a real key.
 */
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
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-founded-session";
const PACK_ROOT = "/Users/brian/Projects/beekeeper/beekeeper/personas/roles";

/** One synthetic managed agent per installed role pack. */
type CrewRoleFixture = {
  role: string;
  name: string;
  pubkey: string;
  personaId: string;
  hasRolePack: boolean;
  seated: boolean;
  refreshed: boolean;
};

const CREW_ROLES: CrewRoleFixture[] = [
  {
    role: "lead",
    name: "Lead",
    pubkey: "a1".repeat(32),
    personaId: "persona-lead",
    hasRolePack: true,
    seated: true,
    refreshed: false,
  },
  {
    role: "architect",
    name: "Architect",
    pubkey: "a2".repeat(32),
    personaId: "persona-architect",
    hasRolePack: true,
    seated: true,
    refreshed: false,
  },
  {
    role: "builder",
    name: "Builder",
    pubkey: "a3".repeat(32),
    personaId: "persona-builder",
    hasRolePack: true,
    seated: true,
    refreshed: false,
  },
  {
    // The disclosure case: a home role this computer holds no pack behind.
    role: "verifier",
    name: "Verifier",
    pubkey: "a4".repeat(32),
    personaId: "persona-verifier",
    hasRolePack: false,
    seated: true,
    refreshed: false,
  },
  {
    role: "runner",
    name: "Runner",
    pubkey: "a5".repeat(32),
    personaId: "persona-runner",
    hasRolePack: true,
    seated: true,
    refreshed: true,
  },
  {
    role: "poker",
    name: "Poker",
    pubkey: "a6".repeat(32),
    personaId: "persona-poker",
    hasRolePack: true,
    seated: false,
    refreshed: false,
  },
  {
    role: "designer",
    name: "Designer",
    pubkey: "a7".repeat(32),
    personaId: "persona-designer",
    hasRolePack: true,
    seated: false,
    refreshed: false,
  },
];

const CREW_TEAM_ID = "team-crew-roles";

/** The roster, in seat order — `CREW_SEAT_ROSTER` in `crew_roles.rs`. */
const SEAT_ROSTER = ["lead", "architect", "builder", "verifier", "runner"];

/** `InstallCrewRolePacksResponse`, exactly as `crew_roles.rs` serialises it. */
const INSTALL_RESPONSE = {
  teamId: CREW_TEAM_ID,
  teamName: "Crew roles",
  seated: SEAT_ROSTER,
  dropped: [] as string[],
  installed: CREW_ROLES.map((role) => ({
    personaId: role.personaId,
    personaName: role.role,
    role: role.role,
    agentPubkey: role.pubkey,
    agentName: role.name,
    packDir: `${PACK_ROOT}/${role.role}`,
    refreshed: role.refreshed,
    seated: role.seated,
  })),
  skipped: [
    {
      path: `${PACK_ROOT}/notes`,
      reason:
        "no persona in this pack declares a role, so it was skipped." as const,
    },
  ],
};

function crewInvokeInitScript(input: {
  agents: CrewRoleFixture[];
  installResponse: unknown;
  packRoot: string;
  crewTeamId: string;
}) {
  return (config: typeof input) => {
    type Invoke = (
      cmd: string,
      args?: Record<string, unknown>,
      options?: unknown,
    ) => Promise<unknown>;

    const state = {
      // Flipped by the spec before a seated create, so the pending screen's
      // three-valued `packStaged` can be driven to each of its states.
      packStaged: false,
      installFailure: null as { failure: string; detail: string } | null,
      /** Role whose pack this folder does not hold, for the dropped-seat case. */
      dropRole: null as string | null,
    };
    (window as unknown as { __FD1__: typeof state }).__FD1__ = state;

    const rawAgent = (agent: (typeof config.agents)[number]) => ({
      pubkey: agent.pubkey,
      name: agent.name,
      persona_id: agent.personaId,
      runtime: null,
      team_id: config.crewTeamId,
      home_role: agent.role,
      has_role_pack: agent.hasRolePack,
      relay_url: "ws://localhost:3000",
      acp_command: "sprig",
      agent_command: "buzz-agent",
      agent_args: [],
      mcp_command: "buzz-dev-mcp",
      turn_timeout_seconds: 300,
      idle_timeout_seconds: null,
      max_turn_duration_seconds: null,
      parallelism: 1,
      system_prompt: null,
      model: "sonnet",
      provider: null,
      persona_out_of_date: false,
      persona_orphaned: false,
      needs_restart: false,
      status: "stopped",
      pid: null,
      created_at: "2026-08-27T00:00:00Z",
      updated_at: "2026-08-27T00:00:00Z",
      last_started_at: null,
      last_stopped_at: null,
      last_exit_code: null,
      last_error: null,
      last_error_code: null,
      log_path: "/tmp/agent.log",
      start_on_app_launch: false,
      backend: "managed",
      backend_agent_id: null,
    });

    const crewTeam = {
      id: config.crewTeamId,
      name: "Crew roles",
      description: null,
      persona_ids: config.agents.map((agent) => agent.personaId),
      is_builtin: false,
      source_dir: null,
      is_symlink: false,
      symlink_target: null,
      version: null,
      created_at: "2026-08-27T00:00:00Z",
      updated_at: "2026-08-27T00:00:00Z",
      crew: {
        primary: config.agents[0].personaId,
        seats: config.agents
          .filter((agent) => agent.seated)
          .map((agent) => ({ personaId: agent.personaId, role: agent.role })),
      },
    };

    let internals: Record<string, unknown> | undefined;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value: Record<string, unknown>) => {
        internals = value;
        let real: Invoke | undefined;
        const wrapped: Invoke = async (cmd, args, options) => {
          switch (cmd) {
            case "pick_crew_role_packs_directory":
              return config.packRoot;
            case "install_crew_role_packs": {
              // The backend rejects with `CrewRoleInstallError`, a serialised
              // struct — not a string. Throwing the struct is what the real
              // bridge does, and it is what the dialog now reads the stage off.
              if (state.installFailure) throw state.installFailure;
              const response = config.installResponse as {
                installed: { role: string }[];
                seated: string[];
                dropped: string[];
              };
              if (!state.dropRole) return response;
              const dropped = state.dropRole;
              return {
                ...response,
                installed: response.installed.filter(
                  (row) => row.role !== dropped,
                ),
                // A roster role whose pack is missing is dropped from the
                // seats the install writes, exactly as `build_crew` does.
                seated: response.seated.filter((role) => role !== dropped),
                dropped: [...response.dropped, dropped],
              };
            }
            case "stage_coding_session_actor_seat":
              return { packStaged: state.packStaged };
            case "clear_coding_session_actor_seat":
              return null;
            default:
              break;
          }
          if (!real) throw new Error("mock invoke is not installed yet");
          const result = await real(cmd, args, options);
          if (cmd === "list_managed_agents") {
            // The last entry is an ordinary agent as a backend that predates
            // the crew installer answers: no `home_role`, no `has_role_pack`.
            const plain = rawAgent({
              ...config.agents[0],
              name: "Scribe",
              pubkey: "b0".repeat(32),
              personaId: "persona-scribe",
            }) as Record<string, unknown>;
            delete plain.home_role;
            delete plain.has_role_pack;
            delete plain.team_id;
            return [
              ...config.agents.map(rawAgent),
              plain,
              ...(result as unknown[]),
            ];
          }
          if (cmd === "list_teams") {
            return [crewTeam, ...(result as unknown[])];
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
  };
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
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Fix the reconnect bug",
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
        title: "Fix the reconnect bug",
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

function runtimeFixture(input: {
  instanceRef: string;
  runtime: string;
  label: string;
  allowedModels: string[];
}) {
  return {
    instanceRef: input.instanceRef,
    runtime: input.runtime,
    driver: `${input.runtime}-acp`,
    label: input.label,
    authState: "ready" as const,
    defaultModel: input.allowedModels[0],
    allowedModels: input.allowedModels,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: false,
      context: false,
      diff: false,
      plan: true,
    },
  };
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
      runtimeFixture({
        instanceRef: "claude-primary",
        runtime: "claude",
        label: "Claude Code",
        allowedModels: ["default", "sonnet", "haiku"],
      }),
    ],
  });
  await page.addInitScript(
    crewInvokeInitScript({
      agents: CREW_ROLES,
      installResponse: INSTALL_RESPONSE,
      packRoot: PACK_ROOT,
      crewTeamId: CREW_TEAM_ID,
    }),
    {
      agents: CREW_ROLES,
      installResponse: INSTALL_RESPONSE,
      packRoot: PACK_ROOT,
      crewTeamId: CREW_TEAM_ID,
    },
  );
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

async function openNewCodingSessionDialog(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
}

test.describe("crew front door", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("01 — the installer surface, from the menu entry to its result list", async ({
    page,
  }) => {
    await openApp(page);
    await page.getByTestId("open-agents-view").click();
    await expect(page.getByTestId("agents-library-teams")).toBeVisible({
      timeout: 15_000,
    });

    // The entry point: the "New team" card's dropdown.
    await page.getByTestId("new-team-card").click();
    const menuItem = page.getByTestId("install-crew-roles");
    await expect(menuItem).toBeVisible();
    await waitForAnimations(page);
    await page
      .locator("[role='menu']")
      .screenshot({ path: `${SHOTS}/01-install-menu.png` });

    await menuItem.click();
    const dialog = page.getByTestId("install-crew-roles-dialog");
    await expect(dialog).toBeVisible();
    await expect(page.getByTestId("install-crew-roles-submit")).toBeDisabled();
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/02-install-idle.png` });

    await page.getByTestId("install-crew-roles-choose").click();
    await expect(page.getByTestId("install-crew-roles-path")).toHaveValue(
      PACK_ROOT,
    );
    await expect(page.getByTestId("install-crew-roles-submit")).toBeEnabled();

    await page.getByTestId("install-crew-roles-submit").click();
    const result = page.getByTestId("install-crew-roles-result");
    await expect(result).toBeVisible();
    await expect(result).toContainText("Lead — Lead");
    await expect(result).toContainText(
      "already installed from this pack — role and pack link refreshed",
    );
    await expect(result).toContainText(
      "no persona in this pack declares a role, so it was skipped.",
    );
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/03-install-result.png` });

    await page.getByTestId("install-crew-roles-close").click();
    await expect(dialog).toHaveCount(0);

    // The minted team, as the library shows it. (The card's "no longer in your
    // agents" warning is this fixture's doing — the synthetic definitions have
    // no persona rows behind them — not a product finding.)
    const teamCard = page.getByTestId(`team-card-${CREW_TEAM_ID}`);
    await teamCard.scrollIntoViewIfNeeded();
    await expect(teamCard.getByTestId("team-crew-badge")).toHaveText(
      "Crew · 5 seats",
    );
    await waitForAnimations(page);
    await teamCard.screenshot({ path: `${SHOTS}/14-team-crew-badge.png` });
  });

  test("07 — a roster role with no pack is dropped, and the result says so", async ({
    page,
  }) => {
    // Lane A's rule: "a roster role with no installed pack is dropped from
    // `seats` and reported". The response now carries both lists, so the
    // dialog names the seats that exist instead of reciting the roster.
    // Install a folder missing the verifier pack and read what the screen says.
    await openApp(page);
    await page.getByTestId("open-agents-view").click();
    await expect(page.getByTestId("agents-library-teams")).toBeVisible({
      timeout: 15_000,
    });
    await page.evaluate((verifierRole) => {
      const w = window as unknown as {
        __FD1__: { dropRole: string | null };
      };
      w.__FD1__.dropRole = verifierRole;
    }, "verifier");

    await page.getByTestId("new-team-card").click();
    await page.getByTestId("install-crew-roles").click();
    await page.getByTestId("install-crew-roles-choose").click();
    await page.getByTestId("install-crew-roles-submit").click();

    const result = page.getByTestId("install-crew-roles-result");
    await expect(result).toBeVisible();
    // The verifier is genuinely absent from what was installed…
    await expect(result).not.toContainText("Verifier — Verifier");
    const dialog = page.getByTestId("install-crew-roles-dialog");
    // …and the seat report names the four seats that exist, not the five the
    // roster names.
    const seats = page.getByTestId("install-crew-roles-seats");
    await expect(seats).toContainText(
      "Seated: lead, architect, builder, runner.",
    );
    await expect(seats).toContainText(
      "verifier: no pack installed, so it holds no seat.",
    );
    await expect(dialog).not.toContainText("Seated by default");
    // The two roles installed on purpose and seated on purpose never say so
    // on their own rows, rather than reading like the seated ones.
    await expect(result).toContainText(
      "Poker — Poker — installed, but not seated in the crew",
    );
    await expect(result).toContainText(
      "Designer — Designer — installed, but not seated in the crew",
    );
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/15-install-dropped-role.png` });
  });

  test("08 — a failure that is not the folder's is not blamed on the folder", async ({
    page,
  }) => {
    // `install_crew_role_packs` now answers with a `CrewRoleInstallError`
    // naming the stage that failed — folder, keys, or store. A locked keychain
    // is a `keys` failure, and the dialog owes it its own sentence: the catch
    // used to wrap every failure in "That folder could not be read:" and send
    // the operator to look at a folder that was read fine.
    await openApp(page);
    await page.getByTestId("open-agents-view").click();
    await expect(page.getByTestId("agents-library-teams")).toBeVisible({
      timeout: 15_000,
    });
    await page.evaluate(() => {
      const w = window as unknown as {
        __FD1__: {
          installFailure: { failure: string; detail: string } | null;
        };
      };
      w.__FD1__.installFailure = {
        failure: "keys",
        detail:
          "the keychain is locked, so a new agent key could not be minted",
      };
    });

    await page.getByTestId("new-team-card").click();
    await page.getByTestId("install-crew-roles").click();
    await page.getByTestId("install-crew-roles-choose").click();
    await page.getByTestId("install-crew-roles-submit").click();

    const error = page.getByTestId("install-crew-roles-error");
    await expect(error).toBeVisible();
    await expect(error).toHaveText(
      "No agent key could be minted, so nothing was installed: the keychain " +
        "is locked, so a new agent key could not be minted",
    );
    await expect(error).not.toContainText("folder");
    // And nothing of the thrown value's own plumbing reaches the operator.
    await expect(error).not.toContainText("Error:");
    await waitForAnimations(page);
    await page
      .getByTestId("install-crew-roles-dialog")
      .screenshot({ path: `${SHOTS}/16-install-error-names-the-keychain.png` });
  });

  test("02 — the home-role badges reach both screens the Agents view renders", async ({
    page,
  }) => {
    // `Home role: {Role}` and the no-pack warning are the disclosure
    // `hasRolePack` exists for, and they used to render only from
    // `ManagedAgentRow` — a row whose only caller, `AgentGroupRows`, nothing
    // imported. Both are deleted; the badges are on the card grid and the
    // profile panel. Driven here rather than read: seven agents each with a
    // `home_role`, one of them with `has_role_pack: false`, plus one plain
    // agent the backend answered nothing about.
    await openApp(page);
    await page.getByTestId("open-agents-view").click();
    await expect(page.getByTestId("unified-agents-groups")).toBeVisible({
      timeout: 15_000,
    });
    const verifierCard = page.getByTestId(
      `managed-agent-${CREW_ROLES[3].pubkey}`,
    );
    await expect(verifierCard).toBeVisible({ timeout: 15_000 });
    // Seven role-carrying agents, each showing the role it is…
    await expect(page.getByTestId("agent-home-role")).toHaveCount(
      CREW_ROLES.length,
    );
    // …and exactly one of them disclosing that its pack is not installed here.
    await expect(page.getByTestId("agent-no-role-pack")).toHaveCount(1);
    await expect(verifierCard.getByTestId("agent-home-role")).toHaveText(
      "Home role: Verifier",
    );
    // The card states the fact; the remedy needs room the card does not have.
    await expect(verifierCard.getByTestId("agent-no-role-pack")).toHaveText(
      "Role pack not installed here",
    );
    // The plain agent the backend never answered about claims neither.
    const plainCard = page.getByTestId(`managed-agent-${"b0".repeat(32)}`);
    await expect(plainCard).toBeVisible();
    await expect(plainCard.getByTestId("agent-home-role")).toHaveCount(0);
    await expect(plainCard.getByTestId("agent-no-role-pack")).toHaveCount(0);
    await waitForAnimations(page);
    await page
      .getByTestId("unified-agents-groups")
      .screenshot({ path: `${SHOTS}/04-agents-home-role-badges.png` });

    // The card's own detail surface is the other place an operator would look.
    await verifierCard.click();
    const panel = page.getByTestId("user-profile-summary-scroll-layout");
    await expect(panel).toBeVisible({ timeout: 15_000 });
    await expect(panel.getByTestId("agent-home-role")).toHaveText(
      "Home role: Verifier",
    );
    // The panel has room for the remedy, so it carries the whole sentence.
    await expect(panel.getByTestId("agent-no-role-pack")).toHaveText(
      "Role pack not installed here — install crew roles from the project's " +
        "personas/roles",
    );
    await waitForAnimations(page);
    await page.screenshot({
      path: `${SHOTS}/05-agent-detail-home-role.png`,
      clip: { x: 640, y: 0, width: 640, height: 900 },
    });
  });

  test("03 — the Crew tab lists the minted crew's seats", async ({ page }) => {
    await openApp(page);
    await openNewCodingSessionDialog(page);
    await page.getByTestId("new-coding-session-tab-crew").click();

    const crewTab = page.getByTestId("new-coding-session-crew");
    await expect(crewTab).toBeVisible();
    await expect(page.getByTestId("new-coding-session-crew-team")).toHaveValue(
      CREW_TEAM_ID,
    );
    const roster = page.getByTestId("new-coding-session-crew-roster");
    await expect(roster).toBeVisible({ timeout: 15_000 });
    await expect(roster.locator("li")).toHaveCount(5);
    await expect(roster).toContainText("lead");
    await expect(roster).toContainText("verifier");
    await waitForAnimations(page);
    await crewTab.screenshot({ path: `${SHOTS}/06-crew-tab.png` });

    // The honesty question this tab has to answer before it signs anything:
    // one of these five seats has no role pack on this computer, and it says
    // so on that seat's own line — before the launch, not after staging
    // reports it.
    await expect(
      roster.getByTestId(`crew-seat-no-role-pack-${CREW_ROLES[3].personaId}`),
    ).toContainText(
      "carries no role skills: this computer has no role pack behind it.",
    );
    await expect(
      roster.locator("[data-testid^='crew-seat-no-role-pack-']"),
    ).toHaveCount(1);
    await roster.screenshot({ path: `${SHOTS}/07-crew-roster.png` });
  });

  test("04 — the join dialog's seat field: default role, mismatch, no pack", async ({
    page,
  }) => {
    await openApp(page);
    await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
    await expect(
      page.getByTestId("channel-coding-sessions-trigger"),
    ).toBeVisible();
    await page.evaluate(
      ({ channelName, events }) => {
        const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
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
    ).toHaveAttribute("aria-label", "Coding sessions (1)", {
      timeout: 15_000,
    });
    await page.getByTestId("channel-coding-sessions-trigger").click();
    await page.getByTestId("channel-coding-session-open").click();
    await expect(page.getByTestId("coding-session-header")).toBeVisible({
      timeout: 15_000,
    });

    await page.getByTestId("coding-session-add-provider").click();
    const dialog = page.getByTestId("add-coding-session-provider-dialog");
    await expect(dialog).toBeVisible();

    const seatField = page.getByTestId("new-coding-session-seat");
    await expect(seatField).toBeVisible();

    // Default: the role box fills from the agent's own home role.
    await page.getByTestId("new-coding-session-seat-agent").click();
    await page
      .getByTestId(`new-coding-session-seat-agent-${CREW_ROLES[2].pubkey}`)
      .click();
    await expect(page.getByTestId("new-coding-session-seat-role")).toHaveValue(
      "builder",
    );
    await expect(
      page.getByTestId("new-coding-session-seat-role-notice"),
    ).toHaveText("Its home role.");
    await waitForAnimations(page);
    await seatField.screenshot({ path: `${SHOTS}/08-seat-home-role.png` });

    // Mismatch: seating a builder as a lead must say which pack it carries.
    await page.getByTestId("new-coding-session-seat-role").fill("lead");
    await expect(
      page.getByTestId("new-coding-session-seat-role-notice"),
    ).toHaveText(
      "Builder is a builder — seating it as lead; it will carry the builder pack.",
    );
    await waitForAnimations(page);
    await seatField.screenshot({ path: `${SHOTS}/09-seat-mismatch.png` });

    // No pack: the agent whose home role has no pack on this computer.
    await page.getByTestId("new-coding-session-seat-agent").click();
    await page
      .getByTestId(`new-coding-session-seat-agent-${CREW_ROLES[3].pubkey}`)
      .click();
    await expect(page.getByTestId("new-coding-session-seat-pack")).toHaveText(
      "Verifier has no role pack on this computer, so this seat carries no role skills and runs on its persona prompt alone.",
    );
    await waitForAnimations(page);
    await seatField.screenshot({ path: `${SHOTS}/10-seat-no-pack.png` });
  });

  test("05 — a plain agent nobody asked about a pack is accused of nothing", async ({
    page,
  }) => {
    // The seat field's own contract says an unanswered field renders nothing
    // ("absence is not a claim", codingSessionActorSeat.ts:180-190).
    // `fromRawManagedAgent` used to map a missing `has_role_pack` to `false`,
    // so `undefined` never reached the component and every ordinary managed
    // agent read as one whose role pack is missing. It now maps to
    // `undefined`, and this drives the whole path to prove it.
    await openApp(page);
    await openNewCodingSessionDialog(page);

    const seatField = page.getByTestId("new-coding-session-seat");
    await page.getByTestId("new-coding-session-seat-agent").click();
    const crewPubkeys = CREW_ROLES.map((role) => role.pubkey);
    const plainPubkey = await page.evaluate((seeded) => {
      const prefix = "new-coding-session-seat-agent-";
      for (const node of document.querySelectorAll(
        `[data-testid^="${prefix}"]`,
      )) {
        const id = node.getAttribute("data-testid")?.slice(prefix.length);
        if (id && id !== "none" && !seeded.includes(id)) return id;
      }
      return null;
    }, crewPubkeys);
    expect(plainPubkey).not.toBeNull();

    await page
      .getByTestId(`new-coding-session-seat-agent-${plainPubkey}`)
      .click();
    await page.getByTestId("new-coding-session-seat-role").fill("builder");
    // No home role is known for this agent, so no role notice is claimed…
    await expect(
      page.getByTestId("new-coding-session-seat-role-notice"),
    ).toHaveCount(0);
    // …and no pack claim either: nothing was ever asked about this one.
    await expect(page.getByTestId("new-coding-session-seat-pack")).toHaveCount(
      0,
    );
    await waitForAnimations(page);
    await seatField.screenshot({
      path: `${SHOTS}/11-seat-plain-agent-says-nothing.png`,
    });
  });

  test("06 — the pending screen names the seat, and what custody staged", async ({
    page,
  }) => {
    await openApp(page);
    await openNewCodingSessionDialog(page);

    await page.getByTestId("new-coding-session-seat-agent").click();
    await page
      .getByTestId(`new-coding-session-seat-agent-${CREW_ROLES[3].pubkey}`)
      .click();
    await expect(page.getByTestId("new-coding-session-seat-role")).toHaveValue(
      "verifier",
    );
    await page
      .getByTestId("new-coding-session-initial-turn")
      .fill("Pin the seat line.");

    await page.getByTestId("new-coding-session-submit").click();

    const pending = page.getByTestId("new-coding-session-pending");
    await expect(pending).toBeVisible({ timeout: 20_000 });
    const seatLine = page.getByTestId("pending-coding-session-seat");
    await expect(seatLine).toBeVisible();
    await expect(seatLine).toContainText("Seated:");
    await waitForAnimations(page);
    await pending.screenshot({ path: `${SHOTS}/12-pending-seated.png` });
    await seatLine.screenshot({ path: `${SHOTS}/13-pending-seat-line.png` });
  });
});
