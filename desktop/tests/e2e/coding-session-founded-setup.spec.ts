import { expect, test, type Page } from "@playwright/test";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The founded page as the whole form, driven rather than described.
 *
 * Since 2026-09-10 there is no create dialog: clicking "New coding session"
 * founds the topic on the spot — one 44226, nothing else — and lands on
 * `/coding-sessions/<channelId>/founded/<sessionRef>`, where the name, the
 * initial prompt, Solo or Team, the runtime and where it runs are edited. The
 * name publishes a 44229 when the field is left, the prompt a 44227; a Solo
 * Start is the one 44221 it always was (you lead, the prompt as the first
 * turn, under the existing genesis); a Team Start is the founded Start
 * (optional 44245 → 44221 → grants → first turn). Discard is a 44230 `closed`.
 *
 * What this spec asserts is what no unit test can: the events the app signs
 * for each click, in order, and — the honesty core of the page — that a
 * field publishes only when the founder commits it, and that a Start never
 * republishes what is already on the wire.
 *
 * A Start stops at the create's receipt, because no mock provider answers a
 * 44221; everything up to and including the create is where every claim this
 * page makes about what it publishes lives. The native policy command is
 * answered in front of the bridge with a response derived from the request,
 * exactly as `crew-front-door.spec.ts` answers the crew commands.
 */

const SHOTS = "test-results/founded-setup";

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

/** The bridge's own identity, so the founded genesis is signed by the driver. */
const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const TEAM_ID = "team-founded-setup";
const SETUP_MODE_STORAGE_KEY = "buzz.coding-session-setup-mode.v1";

const GOAL_BLOCKER = "Please specify the initial prompt to start the session.";
const LEAD_BLOCKER = "Pick an agent to lead this session, or switch to Solo.";

type RoleFixture = {
  role: string;
  name: string;
  pubkey: string;
  personaId: string;
  hasRolePack: boolean;
};

const ROLES: RoleFixture[] = [
  {
    role: "lead",
    name: "Keystone",
    pubkey: "a1".repeat(32),
    personaId: "persona-lead",
    hasRolePack: true,
  },
  {
    role: "builder",
    name: "Keystone",
    pubkey: "b2".repeat(32),
    personaId: "persona-builder",
    hasRolePack: true,
  },
  {
    // No role pack on this computer and no model of its own: the disclosure
    // that never blocks, and the blocker that does.
    role: "verifier",
    name: "Parallax",
    pubkey: "c3".repeat(32),
    personaId: "persona-verifier",
    hasRolePack: false,
  },
];

function foundedSetupInvokeInitScript(config: {
  roles: RoleFixture[];
  teamId: string;
}) {
  return (input: typeof config) => {
    type Invoke = (
      cmd: string,
      args?: Record<string, unknown>,
      options?: unknown,
    ) => Promise<unknown>;

    const rawAgent = (agent: (typeof input.roles)[number]) => ({
      pubkey: agent.pubkey,
      name: agent.name,
      persona_id: agent.personaId,
      runtime: null,
      team_id: input.teamId,
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
      // The lead publishes its own model; the others publish none.
      model: agent.role === "lead" ? "sonnet" : null,
      provider: null,
      persona_out_of_date: false,
      persona_orphaned: false,
      needs_restart: false,
      status: "stopped",
      pid: null,
      created_at: "2026-09-10T00:00:00Z",
      updated_at: "2026-09-10T00:00:00Z",
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

    const team = {
      id: input.teamId,
      name: "Founded setup team",
      description: null,
      persona_ids: input.roles.map((role) => role.personaId),
      is_builtin: false,
      source_dir: null,
      is_symlink: false,
      symlink_target: null,
      version: null,
      created_at: "2026-09-10T00:00:00Z",
      updated_at: "2026-09-10T00:00:00Z",
      crew: {
        primary: input.roles[0].personaId,
        seats: input.roles.map((role) => ({
          personaId: role.personaId,
          role: role.role,
          driver: "claude-agent-acp",
          vendor: "anthropic",
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
            case "build_coding_session_policy_event": {
              const request = (args as { request: Record<string, unknown> })
                .request;
              const policy = request.policy as Record<string, unknown>;
              const flatten = (key: string, keys: string[]) => {
                const group = policy[key] as
                  | Record<string, unknown>
                  | undefined;
                if (!group) return null;
                return Object.fromEntries(
                  keys.map((name) => [name, group[name] ?? null]),
                );
              };
              return {
                schema: "buzz-coding-session-policy-adapter/v1",
                implementation: "buzz-core",
                kind: 44245,
                content: JSON.stringify(policy),
                tags: [
                  ["h", request.channelRef],
                  ["d", policy.sessionRef],
                  ["csp-v", "buzz-coding-session-policy/v1"],
                  ["csp-genesis", policy.genesisRef],
                ],
                record: {
                  sessionRef: policy.sessionRef,
                  genesisRef: policy.genesisRef,
                  posture: policy.posture ?? null,
                  budget: flatten("budget", [
                    "turns",
                    "tokensPerSeat",
                    "tokensPerSession",
                    "costUsdPerSession",
                    "contextTier",
                  ]),
                  attention: policy.attention ?? null,
                  gates: flatten("gates", [
                    "redFirst",
                    "reviewEveryLane",
                    "requiredGates",
                    "verifierRequired",
                  ]),
                  bench: flatten("bench", [
                    "identities",
                    "providers",
                    "challengerSampleRate",
                  ]),
                  irreversible: policy.irreversible ?? null,
                  stop: flatten("stop", ["timeBoxSecs", "onMilestone"]),
                  setsAnyPolicy: Object.keys(policy).length > 3,
                },
              };
            }
            case "get_coding_session_workdir_state":
              return {
                version: 1,
                byProject: {},
                byChannel: {},
                mru: [],
                pending: {},
              };
            case "stage_coding_session_actor_seat":
              return { packStaged: true };
            case "clear_coding_session_actor_seat":
              return null;
            default:
              break;
          }
          if (!real) throw new Error("mock invoke is not installed yet");
          const result = await real(cmd, args, options);
          if (cmd === "list_managed_agents") {
            return [...input.roles.map(rawAgent), ...(result as unknown[])];
          }
          if (cmd === "list_teams") return [team, ...(result as unknown[])];
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

function runtimeFixture() {
  return {
    instanceRef: "claude-primary",
    runtime: "claude",
    driver: "claude-agent-acp",
    label: "Claude Code",
    authState: "ready" as const,
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
  };
}

async function openApp(page: Page) {
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
    codingSessionProviderRuntimes: [runtimeFixture()],
  });
  const config = { roles: ROLES, teamId: TEAM_ID };
  await page.addInitScript(foundedSetupInvokeInitScript(config), config);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

const FOUNDED_URL =
  /#\/coding-sessions\/([0-9a-f-]{36})\/founded\/([0-9a-f-]{36})/;

/**
 * Click "New coding session" in the channel popover and land on the founded
 * page. Returns the route's `sessionRef`, which is the `d` of every record
 * the page publishes from here on.
 */
async function foundSession(page: Page): Promise<string> {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page).toHaveURL(FOUNDED_URL);
  const match = page.url().match(FOUNDED_URL);
  if (!match) throw new Error(`not a founded route: ${page.url()}`);
  expect(match[1]).toBe(CHANNEL_ID);
  // The page's goal and name readers are held behind the client's send
  // budget on a fresh launch (`relaySendBudget.ts`: 25 sends per 5 s window,
  // and the boot plus the channel spend it first) — measured at ~10 s from
  // page open. Until they settle, Start carries the `goal-unresolved`
  // blocker and a field left publishes nothing, by the page's own rule. Every
  // test here drives the card only after that.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
  return match[2];
}

/** Every event this app has signed, in order, as `{kind, content, tags}`. */
async function signedEvents(page: Page) {
  return page.evaluate(() => window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []);
}

async function signedKinds(page: Page) {
  return (await signedEvents(page)).map((event) => event.kind);
}

function ofKind(
  events: Array<{ kind: number; content: string; tags: string[][] }>,
  kind: number,
) {
  return events.filter((event) => event.kind === kind);
}

/** The signed 44221 `session.create` actions, decoded. */
function creates(
  events: Array<{ kind: number; content: string; tags: string[][] }>,
) {
  return ofKind(events, 44221)
    .map((event) => JSON.parse(event.content))
    .filter((payload) => payload.action?.type === "session.create");
}

/** Leave a field the way a person does: focus moves on. */
async function commitField(page: Page, testId: string, text: string) {
  const field = page.getByTestId(testId);
  await field.fill(text);
  await field.blur();
}

test.describe("the founded page is the form", () => {
  // Tall enough that one shot carries the whole card — mode switch, both
  // text fields, runtime, Where, readiness and the footer.
  test.use({ viewport: { width: 1280, height: 1600 } });

  test("01 — the click founds one genesis and opens the page in Solo", async ({
    page,
  }) => {
    await openApp(page);
    const sessionRef = await foundSession(page);

    // Exactly one 44226 and nothing else: no name, no goal, no create. The
    // click is a genesis and only a genesis.
    const kinds = await signedKinds(page);
    expect(kinds.filter((kind) => kind === 44226)).toHaveLength(1);
    for (const kind of [44227, 44229, 44221, 44245, 44228, 44220]) {
      expect(
        kinds,
        `kind ${kind} must not be signed by the click`,
      ).not.toContain(kind);
    }
    const genesis = ofKind(await signedEvents(page), 44226)[0];
    expect(genesis.tags.find((tag) => tag[0] === "h")?.[1]).toBe(CHANNEL_ID);
    // A genesis names its session in `csg-session` and in its content
    // (`lib/codingSessionGenesis.ts`), not in a `d` tag — that is the name,
    // goal and closure records' shape.
    expect(genesis.tags.find((tag) => tag[0] === "csg-session")?.[1]).toBe(
      sessionRef,
    );
    expect(JSON.parse(genesis.content).sessionRef).toBe(sessionRef);

    // The old dialog and the old *Team* card are both gone (the testid below
    // is the retired one on purpose — the setup card is asserted next).
    await expect(page.getByTestId("new-coding-session-form")).toHaveCount(0);
    await expect(
      page.getByTestId(["coding-session-founded", "team-card"].join("-")),
    ).toHaveCount(0);

    const card = page.getByTestId("coding-session-founded-setup-card");
    await expect(card).toBeVisible();
    // The founder's own view has no founder line: the header says "not
    // started" and the card says the rest.
    await expect(
      page.getByTestId("coding-session-founded-founder"),
    ).toHaveCount(0);

    // First run on this computer: Solo. The mode is a fieldset of two native
    // radios (a fieldset groups them for assistive tech; no ARIA role needed).
    const mode = page.getByTestId("coding-session-founded-mode");
    await expect(mode).toHaveAttribute("aria-label", "Solo or Team");
    await expect(mode.locator("input[type=radio]")).toHaveCount(2);
    await expect(
      page.getByTestId("coding-session-founded-mode-solo"),
    ).toHaveAttribute("data-state", "checked");
    await expect(
      page.getByTestId("coding-session-founded-mode-team"),
    ).toHaveAttribute("data-state", "unchecked");

    // Solo shows the runtime and Where, and none of the team fields.
    await expect(page.getByTestId("coding-session-founded-name")).toBeVisible();
    await expect(
      page.getByTestId("coding-session-founded-prompt"),
    ).toBeVisible();
    await expect(page.getByTestId("coding-session-model-picker")).toBeVisible();
    await expect(
      page.getByTestId("coding-session-founded-where"),
    ).toBeVisible();
    await expect(
      page.getByTestId("coding-session-worktree-toggle"),
    ).toBeVisible();
    await expect(
      page.getByTestId("new-coding-session-lead-select"),
    ).toHaveCount(0);
    await expect(page.getByTestId("new-coding-session-bench")).toHaveCount(0);
    await expect(page.getByTestId("new-coding-session-policy")).toHaveCount(0);
    await expect(
      page.getByTestId("coding-session-founded-discard"),
    ).toBeVisible();
    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeVisible();

    await waitForAnimations(page);
    await card.screenshot({ path: `${SHOTS}/01-solo-card.png` });
  });

  test("02 — a blank prompt refuses on press, under the field, and creates nothing", async ({
    page,
  }) => {
    await openApp(page);
    await foundSession(page);

    // A blank prompt does not disable the button; pressing it is what shows
    // the reason, under the prompt field (Andy, 2026-09-10).
    await expect(
      page.getByTestId("new-coding-session-blocker-goal"),
    ).toHaveCount(0);
    const start = page.getByTestId("coding-session-founded-start");
    await expect(start).toBeEnabled();
    await start.click();
    await expect(
      page.getByTestId("new-coding-session-blocker-goal"),
    ).toHaveText(GOAL_BLOCKER);

    const kinds = await signedKinds(page);
    expect(kinds.filter((kind) => kind === 44226)).toHaveLength(1);
    expect(kinds).not.toContain(44221);
    // Nothing was published for a prompt the founder never typed.
    expect(kinds).not.toContain(44227);
  });

  test("03 — name and prompt publish when left; a Solo Start is one 44221 and republishes neither", async ({
    page,
  }) => {
    test.setTimeout(90_000);
    await openApp(page);
    const sessionRef = await foundSession(page);

    const name = "Reconnect audit";
    const prompt = "Close ledger item 103, finding 12.";

    // Name: typed, then left → one 44229 with the typed text.
    await commitField(page, "coding-session-founded-name", name);
    await expect
      .poll(async () => ofKind(await signedEvents(page), 44229).length, {
        timeout: 15_000,
      })
      .toBe(1);
    const nameEvent = ofKind(await signedEvents(page), 44229)[0];
    expect(nameEvent.content).toBe(name);
    expect(nameEvent.tags.find((tag) => tag[0] === "d")?.[1]).toBe(sessionRef);
    await expect(
      page.getByTestId("coding-session-founded-name-error"),
    ).toHaveCount(0);
    // The header renames from the wire, not from the field.
    await expect(page.getByTestId("coding-session-header")).toContainText(name);

    // Prompt: typed, then left → one 44227.
    await commitField(page, "coding-session-founded-prompt", prompt);
    await expect
      .poll(async () => ofKind(await signedEvents(page), 44227).length, {
        timeout: 15_000,
      })
      .toBe(1);
    const goalEvent = ofKind(await signedEvents(page), 44227)[0];
    expect(goalEvent.content).toBe(prompt);
    expect(goalEvent.tags.find((tag) => tag[0] === "d")?.[1]).toBe(sessionRef);
    await expect(
      page.getByTestId("coding-session-founded-prompt-error"),
    ).toHaveCount(0);

    // In that order: the name was committed first.
    const order = (await signedKinds(page)).filter((kind) =>
      [44226, 44229, 44227].includes(kind),
    );
    expect(order).toEqual([44226, 44229, 44227]);

    // Leaving a field with unchanged text publishes nothing.
    await page.getByTestId("coding-session-founded-name").focus();
    await page.getByTestId("coding-session-founded-name").blur();
    await page.getByTestId("coding-session-founded-prompt").focus();
    await page.getByTestId("coding-session-founded-prompt").blur();
    expect(ofKind(await signedEvents(page), 44229)).toHaveLength(1);
    expect(ofKind(await signedEvents(page), 44227)).toHaveLength(1);

    // Solo shows neither Details nor Launch details: one create is the whole
    // story, and its blockers say the rest (Andy, 2026-09-10).
    await expect(
      page.getByTestId("new-coding-session-launch-details"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("new-coding-session-readiness-details"),
    ).toHaveCount(0);

    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeEnabled();
    await page.getByTestId("coding-session-founded-start").click();

    await expect
      .poll(async () => creates(await signedEvents(page)).length, {
        timeout: 25_000,
      })
      .toBe(1);
    const create = creates(await signedEvents(page))[0];
    // You lead, under the existing genesis, the prompt as the first turn, and
    // no second name — the 44229 already on the wire is the title.
    expect(create.action.sessionRef).toBe(sessionRef);
    expect(create.action.genesisRef).toMatch(/^[0-9a-f]{64}$/);
    expect(create.action.initialTurn).toBe(prompt);
    expect(create.action.title).toBeNull();
    expect(create.action.actor).toBeUndefined();
    expect(create.action.role).toBeUndefined();

    // Start flushed nothing: both records were already committed, so neither
    // is republished. One 44226, one 44229, one 44227, one 44221 — no 44245,
    // no grants, no first-turn command.
    const kinds = await signedKinds(page);
    expect(kinds.filter((kind) => kind === 44226)).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 44229)).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 44227)).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 44221)).toHaveLength(1);
    expect(kinds).not.toContain(44245);
    expect(kinds).not.toContain(44228);
    expect(kinds).not.toContain(44220);
  });

  test("04 — Team: an agent must lead, the Start signs the crew sequence, and the mode is remembered", async ({
    page,
  }) => {
    test.setTimeout(120_000);
    await openApp(page);
    await foundSession(page);

    await page.getByTestId("coding-session-founded-mode-team").click();
    await expect(
      page.getByTestId("coding-session-founded-mode-team"),
    ).toHaveAttribute("data-state", "checked");

    // The team fields appear…
    const leadSelect = page.getByTestId("new-coding-session-lead-select");
    await expect(leadSelect).toBeVisible();
    await expect(page.getByTestId("new-coding-session-bench")).toBeVisible();
    await expect(page.getByTestId("new-coding-session-policy")).toBeVisible();
    // …and there is no "You" option: Team means an agent leads.
    await expect(
      leadSelect.locator("option", { hasText: /^You$/ }),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("new-coding-session-governed-switch"),
    ).toHaveCount(0);

    // Nobody picked: nothing is said until Start is pressed; the press says
    // it under the field and signs nothing.
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("new-coding-session-launch-details"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeEnabled();
    await page.getByTestId("coding-session-founded-start").click();
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveText(LEAD_BLOCKER);
    expect(ofKind(await signedEvents(page), 44221)).toHaveLength(0);

    await commitField(
      page,
      "coding-session-founded-prompt",
      "Land the founded form and prove the sequence.",
    );
    await expect
      .poll(async () => ofKind(await signedEvents(page), 44227).length, {
        timeout: 15_000,
      })
      .toBe(1);

    await leadSelect.selectOption(ROLES[0].pubkey);
    await expect(
      page.getByTestId("new-coding-session-blocker-lead"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("new-coding-session-lead-identity"),
    ).toContainText("Keystone · a1a1a1a1…a1a1");

    // Bench the builder, and set one policy field so a 44245 goes out.
    await page
      .getByTestId(`new-coding-session-bench-identity-${ROLES[1].pubkey}`)
      .click();
    await page.getByTestId("new-coding-session-policy").click();
    await page
      .getByTestId("new-coding-session-policy-posture")
      .selectOption("overnight");
    // Picking the lead cleared the refused press, so "Launch details" is
    // gone again; the wire below is the proof of what the Start signs.
    await expect(
      page.getByTestId("new-coding-session-launch-details"),
    ).toHaveCount(0);

    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeEnabled();
    await page.getByTestId("coding-session-founded-start").click();

    // Policy before the create; the create is the lead's, and only the lead's.
    await expect
      .poll(
        async () =>
          (await signedKinds(page)).filter((kind) =>
            [44245, 44221].includes(kind),
          ),
        { timeout: 25_000 },
      )
      .toEqual([44245, 44221]);
    const created = creates(await signedEvents(page));
    expect(created).toHaveLength(1);
    expect(created[0].action.actor).toBe(ROLES[0].pubkey);
    expect(created[0].action.role).toBe("lead");
    expect(created[0].action.model).toBe("sonnet");
    const policy = JSON.parse(
      ofKind(await signedEvents(page), 44245)[0].content,
    );
    expect(policy.posture).toBe("overnight");
    expect(policy.bench.identities).toEqual([ROLES[1].pubkey]);
    // No second genesis, and the prompt was not republished by Start.
    const kinds = await signedKinds(page);
    expect(kinds.filter((kind) => kind === 44226)).toHaveLength(1);
    expect(kinds.filter((kind) => kind === 44227)).toHaveLength(1);

    // The mode this computer used last is what the next page opens in.
    const stored = await page.evaluate(
      (key) => window.localStorage.getItem(key),
      SETUP_MODE_STORAGE_KEY,
    );
    expect(stored === "team" || stored === JSON.stringify("team")).toBe(true);
    await page.reload({ waitUntil: "domcontentloaded" });
    await foundSession(page);
    await expect(
      page.getByTestId("coding-session-founded-mode-team"),
    ).toHaveAttribute("data-state", "checked");
    await expect(
      page.getByTestId("new-coding-session-lead-select"),
    ).toBeVisible();
  });

  test("05 — the page at a narrow width keeps its blockers and its plan", async ({
    page,
  }) => {
    await openApp(page);
    await foundSession(page);
    await page.getByTestId("coding-session-founded-mode-team").click();
    await page.setViewportSize({ width: 720, height: 1600 });
    await commitField(
      page,
      "coding-session-founded-prompt",
      "Check the narrow measure.",
    );
    await page
      .getByTestId("new-coding-session-lead-select")
      .selectOption(ROLES[2].pubkey);
    // Parallax has no role pack on this computer — a disclosure, behind
    // Details, that never blocks — and declares no model, which does.
    await expect(
      page.getByTestId("new-coding-session-readiness-details"),
    ).toBeVisible();
    await page.getByTestId("new-coding-session-readiness-details").click();
    await expect(
      page.getByTestId("new-coding-session-unknown-role-pack-missing"),
    ).toContainText("persona prompt alone");
    await expect(
      page.getByTestId("new-coding-session-blocker-model"),
    ).toBeVisible();
    await expect(
      page.getByTestId("coding-session-founded-start"),
    ).toBeDisabled();

    // Launch details appear only after a refused press; with an inline
    // blocker holding Start, there is no press and no plan to show.
    await expect(
      page.getByTestId("new-coding-session-launch-details"),
    ).toHaveCount(0);
    await waitForAnimations(page);
    await page.screenshot({
      path: `${SHOTS}/05-narrow.png`,
      fullPage: true,
    });
  });

  test("06 — Discard closes the founded session with a 44230 and the page says so", async ({
    page,
  }) => {
    await openApp(page);
    const sessionRef = await foundSession(page);

    await page.getByTestId("coding-session-founded-discard").click();
    // The existing close confirmation, with its own copy.
    await page.getByTestId("coding-session-closure-confirm").click();

    await expect
      .poll(async () => ofKind(await signedEvents(page), 44230).length, {
        timeout: 15_000,
      })
      .toBe(1);
    const closure = JSON.parse(
      ofKind(await signedEvents(page), 44230)[0].content,
    );
    expect(closure.action).toBe("closed");
    expect(closure.sessionRef).toBe(sessionRef);
    expect(closure.genesisRef).toMatch(/^[0-9a-f]{64}$/);

    // The accepted closure reaches the page through its closures read, and
    // the closed state offers no Start.
    await expect(
      page.getByTestId("coding-session-founded-workspace-closed"),
    ).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("coding-session-founded-start")).toHaveCount(
      0,
    );
    await expect(
      page.getByTestId("coding-session-founded-discard"),
    ).toHaveCount(0);
    // Nothing else was signed on the way out.
    const kinds = await signedKinds(page);
    expect(kinds).not.toContain(44221);
    expect(kinds).not.toContain(44227);
    expect(kinds).not.toContain(44229);
    await waitForAnimations(page);
    await page.screenshot({ path: `${SHOTS}/06-closed.png`, fullPage: true });
  });
});
