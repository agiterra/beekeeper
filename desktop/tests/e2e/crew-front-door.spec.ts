import {
  openAgentDefinitions,
  openDirectoryAgentProfile,
  openSavedAgentGroups,
} from "../helpers/agentDirectory";
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
import { openDashboardTab } from "../helpers/dashboard";

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

const SHOTS = "test-results/swat1-poke";

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
    // The disclosure case: a home role this computer holds no pack behind.
    role: "architect",
    name: "Architect",
    pubkey: "a2".repeat(32),
    personaId: "persona-architect",
    hasRolePack: false,
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
    // Installed on purpose, seated never: every seat of one launch runs on the
    // one selected provider, so a seated verifier could only ever share its
    // builders' vendor and D8 would refuse the roster the installer just
    // wrote (SESSION_STATE item 77, F7).
    role: "verifier",
    name: "Verifier",
    pubkey: "a4".repeat(32),
    personaId: "persona-verifier",
    hasRolePack: true,
    seated: false,
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
const SEAT_ROSTER = ["lead", "architect", "builder", "runner"];

/**
 * What every seat the installer writes declares — `build_crew` in
 * `crew_roles.rs`. Without these the Team tab read "vendor not declared ·
 * sonnet" on every row and the D8 family rule refused the launch, so the front
 * door opened onto a wall (SESSION_STATE item 77, F7).
 */
const SEAT_RUNTIME = { driver: "claude-agent-acp", vendor: "anthropic" };

/** `InstallCrewRolePacksResponse`, exactly as `crew_roles.rs` serialises it. */
const INSTALL_RESPONSE = {
  teamId: CREW_TEAM_ID,
  teamName: "Team roles",
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
    // Set by the mock from the names the dialog submitted, exactly as the
    // backend sets it: only an identity that was already installed can be
    // renamed.
    renamed: false,
    seated: role.seated,
  })),
  skipped: [
    {
      path: `${PACK_ROOT}/notes`,
      reason:
        "no persona in this pack declares a role, so it was skipped." as const,
    },
  ],
  /** Every kind:0 republish landed on this run. */
  profileSyncError: null as string | null,
};

function crewInvokeInitScript(input: {
  agents: CrewRoleFixture[];
  installResponse: unknown;
  packRoot: string;
  crewTeamId: string;
  seatRuntime: { driver: string; vendor: string };
}) {
  return (config: typeof input) => {
    type Invoke = (
      cmd: string,
      args?: Record<string, unknown>,
      options?: unknown,
    ) => Promise<unknown>;

    /** The parts of `InstallCrewRolePacksResponse` this mock rewrites. */
    type InstallResponseShape = {
      installed: {
        role: string;
        personaName: string;
        packDir: string;
        agentName: string;
        refreshed: boolean;
        renamed: boolean;
      }[];
      skipped: { path: string; reason: string }[];
      seated: string[];
      dropped: string[];
    };

    const state = {
      // Flipped by the spec before a seated create, so the pending screen's
      // three-valued `packStaged` can be driven to each of its states.
      packStaged: false,
      installFailure: null as { failure: string; detail: string } | null,
      /** Role whose pack this folder does not hold, for the dropped-seat case. */
      dropRole: null as string | null,
      /** Arguments of the last `install_crew_role_packs` call, verbatim. */
      installArgs: null as Record<string, unknown> | null,
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
      name: "Team roles",
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
          .map((agent) => ({
            personaId: agent.personaId,
            role: agent.role,
            driver: config.seatRuntime.driver,
            vendor: config.seatRuntime.vendor,
          })),
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
            case "pick_crew_role_packs_directory": {
              // `PickedCrewRolePacks`: the pick *and* the read-only scan the
              // dialog renders one name field per (ledger 84). Answering with
              // the bare directory string this used to return is the shape the
              // command carried before that landed, and the dialog reading
              // `packs` off it took the whole app to the error boundary.
              const scanned = (config.installResponse as InstallResponseShape)
                .installed;
              return {
                directory: config.packRoot,
                packs: scanned
                  .filter((row) => row.role !== state.dropRole)
                  .map((row) => ({
                    role: row.role,
                    personaName: row.personaName,
                    packDir: row.packDir,
                    // The field starts on the name that identity already
                    // carries here; a pack nothing is installed from yet
                    // offers the pack's own name.
                    defaultName: row.agentName,
                    installed: row.refreshed,
                  })),
                skipped: (config.installResponse as InstallResponseShape)
                  .skipped,
              };
            }
            case "install_crew_role_packs": {
              state.installArgs = (args ?? {}) as Record<string, unknown>;
              // The backend rejects with `CrewRoleInstallError`, a serialised
              // struct — not a string. Throwing the struct is what the real
              // bridge does, and it is what the dialog now reads the stage off.
              if (state.installFailure) throw state.installFailure;
              const submitted = (
                (args ?? {}) as { names?: Record<string, string> | null }
              ).names;
              const base = config.installResponse as InstallResponseShape;
              // The rows come back under the names the install was given, and
              // a name typed over an identity that was already installed is a
              // rename — which is what the backend reports and what the result
              // list has to be able to say.
              const response = {
                ...base,
                installed: base.installed.map((row) => {
                  const named = submitted?.[row.role];
                  if (!named || named === row.agentName) return row;
                  return { ...row, agentName: named, renamed: row.refreshed };
                }),
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
            case "get_coding_session_workdir_state":
              // A real command the shared bridge does not answer. The Agents
              // tab now resolves a project and asks which checkout directory
              // belongs to it; leaving it unanswered made the installer report
              // "That folder could not be read" over a folder it never opened.
              // No checkout is recorded here, which is the state a machine that
              // has never opened a coding session is actually in.
              return {
                version: 1,
                byProject: {},
                byChannel: {},
                mru: [],
                pending: {},
              };
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
  const crewConfig = {
    agents: CREW_ROLES,
    installResponse: INSTALL_RESPONSE,
    packRoot: PACK_ROOT,
    crewTeamId: CREW_TEAM_ID,
    seatRuntime: SEAT_RUNTIME,
  };
  await page.addInitScript(crewInvokeInitScript(crewConfig), crewConfig);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

/**
 * "New coding session" founds the topic on the click and lands on the founded
 * page, where the session is set up (Solo / Team) and started. There is no
 * dialog since 2026-09-10; the page's card is the whole form.
 */
async function foundNewCodingSession(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page).toHaveURL(
    /#\/coding-sessions\/[0-9a-f-]{36}\/founded\/[0-9a-f-]{36}/,
  );
  // The page's goal reader is held behind the client's send budget
  // (`relaySendBudget.ts`, 25 sends per 5 s) on a fresh launch and settles
  // ~10 s in; until it does, Start is blocked and a field left unsettled
  // publishes nothing — by design. Wait for it before driving the card.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
}

/** Every event this app has signed, in order. */
async function signedEvents(page: Page) {
  return page.evaluate(() => window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []);
}

/**
 * Seed a founded session in the channel and open its "add provider" dialog.
 *
 * The join dialog is where a seat is still chosen field by field — the launch
 * form asks the *lead* question instead, with the role read from the
 * identity's own pack.
 */
async function openJoinDialogOnSeededSession(page: Page) {
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

test.describe("crew front door", () => {
  test.use({ viewport: { width: 1280, height: 900 } });

  test("01 — the installer surface, from the menu entry to its result list", async ({
    page,
  }) => {
    await openApp(page);
    await openDashboardTab(page, "agents");
    await openAgentDefinitions(page);
    await openSavedAgentGroups(page);

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
    // Ledger 85's reachability follow-up: this route names no project, so the
    // installer resolves one — and then says which one, before anything is
    // installed. A resolved project the operator cannot see is the guess this
    // change exists to remove. This fixture has exactly one project, so there
    // is nothing to choose between and no selector is offered; the name is
    // still on the label.
    await expect(
      page.getByTestId("install-crew-roles-folder-label"),
    ).toHaveText(/^The project's role packs — .+/);
    await expect(
      page.getByTestId("install-crew-roles-project-note"),
    ).toHaveText(
      "This project has no checkout directory yet — set one in Project settings, or choose a folder",
    );
    await expect(page.getByTestId("role-packs-project-selector")).toHaveCount(
      0,
    );
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/02-install-idle.png` });

    await page.getByTestId("install-crew-roles-choose").click();
    await expect(page.getByTestId("install-crew-roles-path")).toHaveValue(
      PACK_ROOT,
    );
    await expect(page.getByTestId("install-crew-roles-submit")).toBeEnabled();
    // A folder the operator picked is theirs, so the label naming the project
    // retires with the folder it described.
    await expect(
      page.getByTestId("install-crew-roles-folder-label"),
    ).toHaveCount(0);

    // D11, ledger 84: every pack in the folder is an identity a person names,
    // not one role label the installer asks about. One field per scanned pack,
    // each starting on the name that identity already carries here, so an
    // operator who touches nothing renames nobody.
    const leadNameField = page.getByTestId("install-crew-roles-name-lead");
    await expect(leadNameField).toHaveValue("Lead");
    await expect(
      page.getByTestId("install-crew-roles-name-designer"),
    ).toHaveValue("Designer");
    await leadNameField.fill("Keystone");

    await page.getByTestId("install-crew-roles-submit").click();
    const result = page.getByTestId("install-crew-roles-result");
    await expect(result).toBeVisible();
    // The row reads the name that was installed, not the role it came from.
    await expect(result).toContainText("Lead — Keystone");
    await expect(result).toContainText("Designer — Designer");
    await expect(result).toContainText(
      "already installed from this pack — role and pack link refreshed",
    );
    await expect(result).toContainText(
      "no persona in this pack declares a role, so it was skipped.",
    );
    await waitForAnimations(page);
    await dialog.screenshot({ path: `${SHOTS}/03-install-result.png` });

    // …and the names reached the installer, which is what mints the records.
    // Every scanned role is in the map — an absent key would leave the backend
    // guessing what the operator meant — and the untouched ones carry the name
    // they already had.
    const installArgs = await page.evaluate(() => {
      const w = window as unknown as {
        __FD1__: { installArgs: Record<string, unknown> | null };
      };
      return w.__FD1__.installArgs;
    });
    const submittedNames = installArgs?.names as
      | Record<string, string>
      | undefined;
    expect(submittedNames?.lead).toBe("Keystone");
    expect(submittedNames?.designer).toBe("Designer");
    expect(Object.keys(submittedNames ?? {}).sort()).toEqual(
      CREW_ROLES.map((role) => role.role).sort(),
    );

    await page.getByTestId("install-crew-roles-close").click();
    await expect(dialog).toHaveCount(0);

    // The minted team, as the library shows it. (The card's "no longer in your
    // agents" warning is this fixture's doing — the synthetic definitions have
    // no persona rows behind them — not a product finding.)
    const teamCard = page.getByTestId(`team-card-${CREW_TEAM_ID}`);
    await teamCard.scrollIntoViewIfNeeded();
    await expect(teamCard.getByTestId("team-crew-badge")).toHaveText(
      "Team · 4 seats",
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
    await openDashboardTab(page, "agents");
    await openAgentDefinitions(page);
    await openSavedAgentGroups(page);
    await page.evaluate((droppedRole) => {
      const w = window as unknown as {
        __FD1__: { dropRole: string | null };
      };
      w.__FD1__.dropRole = droppedRole;
    }, "architect");

    await page.getByTestId("new-team-card").click();
    await page.getByTestId("install-crew-roles").click();
    await page.getByTestId("install-crew-roles-choose").click();
    await page.getByTestId("install-crew-roles-submit").click();

    const result = page.getByTestId("install-crew-roles-result");
    await expect(result).toBeVisible();
    // The architect is genuinely absent from what was installed…
    await expect(result).not.toContainText("Architect — Architect");
    const dialog = page.getByTestId("install-crew-roles-dialog");
    // …and the seat report names the four seats that exist, not the five the
    // roster names.
    const seats = page.getByTestId("install-crew-roles-seats");
    await expect(seats).toContainText("Seated: lead, builder, runner.");
    await expect(seats).toContainText(
      "architect: no pack installed, so it holds no seat.",
    );
    await expect(dialog).not.toContainText("Seated by default");
    // The two roles installed on purpose and seated on purpose never say so
    // on their own rows, rather than reading like the seated ones. The note is
    // parenthesised, as `crewRoleResultLine` renders it.
    await expect(result).toContainText(
      "Poker — Poker (installed, but not seated in the team)",
    );
    await expect(result).toContainText(
      "Designer — Designer (installed, but not seated in the team)",
    );
    // And the verifier, unseated for a reason the plan gives rather than for
    // want of a pack: it is installed, and it holds no seat.
    await expect(result).toContainText(
      "Verifier — Verifier (installed, but not seated in the team)",
    );
    await expect(seats).not.toContainText("verifier: no pack installed");
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
    await openDashboardTab(page, "agents");
    await openAgentDefinitions(page);
    await openSavedAgentGroups(page);
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
    await openApp(page);
    await openDashboardTab(page, "agents");
    const rows = page.getByTestId("agent-row");
    const packlessCard = rows.filter({
      has: page.getByText(CREW_ROLES[1].name, { exact: true }),
    });
    await expect(packlessCard).toBeVisible({ timeout: 15_000 });
    for (const role of CREW_ROLES) {
      const row = rows.filter({
        has: page.getByText(role.name, { exact: true }),
      });
      await expect(row.getByTestId("agent-row-launches-as")).toContainText(
        role.role,
        { ignoreCase: true },
      );
    }
    await expect(rows.getByTestId("agent-row-pack")).toHaveCount(1);
    await expect(packlessCard.getByTestId("agent-row-pack")).toHaveText(
      "Role pack not installed here",
    );
    const plainRow = page.locator(
      `[data-testid="agent-row"][data-pubkey="${"b0".repeat(32)}"]`,
    );
    await expect(plainRow).toBeVisible();
    await expect(plainRow.getByTestId("agent-row-pack")).toHaveCount(0);
    await expect(plainRow.getByTestId("agent-row-launches-as")).toHaveText(
      "launches as — not set",
    );
    await waitForAnimations(page);
    await page
      .getByTestId("agents-page-content")
      .screenshot({ path: `${SHOTS}/04-agents-home-role-badges.png` });
    await openDirectoryAgentProfile(page, CREW_ROLES[1].name);
    const panel = page.getByTestId("user-profile-summary-scroll-layout");
    await expect(panel).toBeVisible({ timeout: 15_000 });
    await expect(panel.getByTestId("agent-home-role")).toHaveText(
      "Home role: Architect",
    );
    // The panel has room for the remedy, so it carries the whole sentence.
    await expect(panel.getByTestId("agent-no-role-pack")).toHaveText(
      "Role pack not installed here — install team roles from the project's " +
        "personas/roles",
    );
    await waitForAnimations(page);
    await page.screenshot({
      path: `${SHOTS}/05-agent-detail-home-role.png`,
      clip: { x: 640, y: 0, width: 640, height: 900 },
    });
  });

  test("03 — the founded page seats the lead, and says so before it is pressed", async ({
    page,
  }) => {
    // This test drove the *Team* tab until 2026-09-01, then the one form's
    // lead field until 2026-09-10. There is no form now: the click founds the
    // topic and the founded page is where Solo or Team, the lead, the bench
    // and the policy are picked (Andy, 2026-09-10). What is checked here is
    // what this spec uniquely covered: that the front door still says what
    // pressing it does, still refuses in words rather than in silence, and
    // still cuts the lead its own worktree.
    await openApp(page);
    await foundNewCodingSession(page);

    const card = page.getByTestId("coding-session-founded-setup-card");
    await expect(card).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("new-coding-session-form")).toHaveCount(0);

    // A blank prompt does not disable the button; pressing it is what shows
    // the reason, under the prompt field (Andy, 2026-09-10).
    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeEnabled();
    await page.getByTestId("coding-session-founded-start").click();
    await expect(
      page.getByTestId("new-coding-session-blocker-goal"),
    ).toContainText("Please specify the initial prompt to start the session.");

    const prompt = page.getByTestId("coding-session-founded-prompt");
    await prompt.fill("Close ledger item 77.");
    await prompt.blur();

    // Team means an agent leads; the lead field offers agents only.
    await page.getByTestId("coding-session-founded-mode-team").click();
    const leadSelect = page.getByTestId("new-coding-session-lead-select");
    await expect(leadSelect).toBeVisible();
    // Nothing is said about the missing lead until Start is pressed.
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveCount(0);
    await page.getByTestId("coding-session-founded-start").click();
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveText("Pick an agent to lead this session, or switch to Solo.");
    await leadSelect.selectOption(CREW_ROLES[0].pubkey);
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveCount(0);

    // D14's sentence, in the page's own voice: the rest is a bench the lead
    // may hire from. (The plan line naming the one seat shows only after a
    // refused press; the signed create below is the proof.)
    await expect(page.getByTestId("new-coding-session-bench")).toContainText(
      "bee sessions hire",
    );

    // Item 87(d): the lead gets a worktree of its own, on by default, rather
    // than running in the operator's checkout while every seat it hires gets
    // one.
    const worktreeToggle = page.getByTestId("coding-session-worktree-toggle");
    await expect(worktreeToggle).toBeVisible();
    await expect(worktreeToggle).toHaveAttribute("data-state", "checked");

    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeEnabled();
    await waitForAnimations(page);
    await card.screenshot({ path: `${SHOTS}/06-crew-tab.png` });
  });

  test("04 — the join dialog's seat field: default role, mismatch, no pack", async ({
    page,
  }) => {
    await openApp(page);
    await openJoinDialogOnSeededSession(page);

    const seatField = page.getByTestId("new-coding-session-seat");
    await expect(seatField).toBeVisible();

    // Default: the role box fills from the agent's own home role — and stays
    // there. A new seat never relabels an agent, so the box is read-only and
    // says why; the host refuses any other role outright
    // (`SEAT_ROLE_NOT_PRIMARY`, `useCodingSessionSeatDraft.ts`).
    //
    // There used to be a mismatch case here — seating a builder as a lead and
    // reading back which pack it would carry. It is not a stale expectation
    // but a removed capability: nothing in this dialog can produce that state
    // any more, so the assertion is the lock instead. An agent with no home
    // role is the one whose role is still typed, and test 05 drives it.
    await page.getByTestId("new-coding-session-seat-agent").click();
    await page
      .getByTestId(`new-coding-session-seat-agent-${CREW_ROLES[2].pubkey}`)
      .click();
    const roleBox = page.getByTestId("new-coding-session-seat-role");
    await expect(roleBox).toHaveValue("builder");
    await expect(roleBox).toHaveAttribute("readonly", "");
    await expect(
      page.getByTestId("new-coding-session-seat-role-locked"),
    ).toHaveText(
      "A new seat takes the agent's primary role, so this agent is seated as builder.",
    );
    await expect(
      page.getByTestId("new-coding-session-seat-role-notice"),
    ).toHaveText("Its home role.");
    await waitForAnimations(page);
    await seatField.screenshot({ path: `${SHOTS}/08-seat-home-role.png` });

    // No pack: the agent whose home role has no pack on this computer.
    await page.getByTestId("new-coding-session-seat-agent").click();
    await page
      .getByTestId(`new-coding-session-seat-agent-${CREW_ROLES[1].pubkey}`)
      .click();
    await expect(page.getByTestId("new-coding-session-seat-pack")).toHaveText(
      "Architect has no role pack on this computer, so this seat carries no role skills and runs on its persona prompt alone.",
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
    // agent read as one whose role pack is missing.
    //
    // Driven through the join dialog since 2026-09-01: the launch form has no
    // seat field any more. Seating there is the *lead* question, answered with
    // an identity whose role is read from its pack, and an agent lead is a
    // governed session. Joining an existing session is where a seat is still
    // chosen field by field.
    await openApp(page);
    await openJoinDialogOnSeededSession(page);

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

  test("06 — a Solo start creates one unseated execution, and claims no seat", async ({
    page,
  }) => {
    // Until 2026-09-01 this drove the One-session tab with an agent seat and
    // read the pending screen's seat line. That shape is gone, and the removal
    // is deliberate rather than incidental: a seat holds authority only
    // through a genesis and an accepted authority chain, so an ungoverned
    // agent seat was an agent with no standing to report, to hire, or to be
    // granted anything. Team means an agent leads and the session is
    // governed; the governed sequence has its own spec
    // (`coding-session-founded-setup.spec.ts`).
    //
    // What is still reachable here — and still worth pinning — is Solo: you
    // lead, one execution under the founded genesis, and a create that claims
    // no seat rather than implying one.
    test.setTimeout(60_000);
    await openApp(page);
    await foundNewCodingSession(page);

    await expect(
      page.getByTestId("coding-session-founded-mode-solo"),
    ).toHaveAttribute("data-state", "checked");
    await expect(
      page.getByTestId("new-coding-session-lead-select"),
    ).toHaveCount(0);
    const prompt = page.getByTestId("coding-session-founded-prompt");
    await prompt.fill("Pin the unseated create.");
    await prompt.blur();
    // Leaving the field publishes the 44227; wait for it to be signed and
    // read back into the header before Start, as a person would see it.
    await expect
      .poll(
        async () =>
          (await signedEvents(page)).filter((event) => event.kind === 44227)
            .length,
        { timeout: 15_000 },
      )
      .toBe(1);
    await expect(page.getByTestId("coding-session-header")).toContainText(
      "Pin the unseated create.",
    );
    await page.getByTestId("coding-session-founded-start").click();

    await expect
      .poll(
        async () =>
          (await signedEvents(page)).filter(
            (event) =>
              event.kind === 44221 &&
              JSON.parse(event.content).action?.type === "session.create",
          ).length,
        { timeout: 25_000 },
      )
      .toBe(1);
    const create = (await signedEvents(page)).find(
      (event) => event.kind === 44221,
    );
    const action = JSON.parse(create?.content ?? "{}").action;
    // Absence is not a claim: an unseated create says nothing about a seat.
    expect(action.actor).toBeUndefined();
    expect(action.role).toBeUndefined();
    expect(action.initialTurn).toBe("Pin the unseated create.");
    // No grants and no policy follow a Solo create.
    const kinds = (await signedEvents(page)).map((event) => event.kind);
    expect(kinds).not.toContain(44228);
    expect(kinds).not.toContain(44245);

    // The create is in flight from this screen, so Start is held.
    const card = page.getByTestId("coding-session-founded-setup-card");
    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeDisabled();
    await waitForAnimations(page);
    await card.screenshot({ path: `${SHOTS}/12-pending-unseated.png` });
  });
});
