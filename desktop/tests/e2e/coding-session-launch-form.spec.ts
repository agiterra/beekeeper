import { expect, test, type Page } from "@playwright/test";
import { CODING_SESSION_POLICY_STATED_NOT_ENFORCED } from "../../src/features/coding-sessions/lib/codingSessionPolicy";
import { generateSecretKey, getPublicKey } from "nostr-tools/pure";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The one launch form, driven rather than described.
 *
 * The dialog was two tabs until 2026-09-01, and finding 12 is what two tabs
 * cost: the *Team* tab had no provider control, so the lead ran on whatever
 * the *One session* tab happened to be showing, and that tab's model leaked
 * into every unpinned seat. This spec drives the replacement end to end and
 * asserts the two things a unit test cannot: that the events a governed launch
 * signs are the ones the form promised, in the order it promised them, and
 * that **only the lead is created**.
 *
 * A governed launch stops at the create's receipt, because no mock provider
 * answers a 44221 — so what is driven here is everything up to and including
 * the create, which is where every claim this form makes about what it
 * publishes lives. The native policy command is answered in front of the bridge with a response derived from the
 * request, exactly as `crew-front-door.spec.ts` answers the crew commands: the
 * *rules* that command applies are proved by
 * `desktop/src-tauri/src/commands/coding_session_policy_tests.rs` and pinned to
 * the decoder by an adapter-generated fixture, so what this spec has to prove
 * is only that the form calls it and publishes what it returns.
 */

const SHOTS = "test-results/b3-launch";

const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

/** The bridge's own identity, so the seeded genesis is founded by the driver. */
const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const CHANNEL_NAME = "engineering";
const TEAM_ID = "team-launch-form";

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
    // Deliberately the same display name as the lead: two Keystones is the
    // case the identity line exists for, and the launch form is where picking
    // the wrong one costs a whole session.
    role: "builder",
    name: "Keystone",
    pubkey: "b2".repeat(32),
    personaId: "persona-builder",
    hasRolePack: true,
  },
  {
    role: "verifier",
    name: "Parallax",
    pubkey: "c3".repeat(32),
    personaId: "persona-verifier",
    hasRolePack: false,
  },
];

function launchInvokeInitScript(config: {
  roles: RoleFixture[];
  teamId: string;
  seatFailure?: string;
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
      // The lead publishes its own model; the builder publishes none, which is
      // the seat finding 12 used to fill from the other tab.
      model: agent.role === "lead" ? "sonnet" : null,
      provider: null,
      persona_out_of_date: false,
      persona_orphaned: false,
      needs_restart: false,
      status: "stopped",
      pid: null,
      created_at: "2026-09-01T00:00:00Z",
      updated_at: "2026-09-01T00:00:00Z",
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
      name: "Launch form team",
      description: null,
      persona_ids: input.roles.map((role) => role.personaId),
      is_builtin: false,
      source_dir: null,
      is_symlink: false,
      symlink_target: null,
      version: null,
      created_at: "2026-09-01T00:00:00Z",
      updated_at: "2026-09-01T00:00:00Z",
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

    const state = { policyRequests: [] as unknown[] };
    (window as unknown as { __B3__: typeof state }).__B3__ = state;

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
              state.policyRequests.push(request);
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
              if (input.seatFailure) throw new Error(input.seatFailure);
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

async function openApp(page: Page, seatFailure?: string) {
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
  const config = { roles: ROLES, teamId: TEAM_ID, seatFailure };
  await page.addInitScript(launchInvokeInitScript(config), config);
  await page.goto("/", { waitUntil: "domcontentloaded" });
}

async function openDialog(page: Page) {
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-sessions-new").click();
  await expect(page.getByTestId("new-coding-session-form")).toBeVisible({
    timeout: 15_000,
  });
}

/** Every event this app has signed, in order, as `{kind, content, tags}`. */
async function signedEvents(page: Page) {
  return page.evaluate(() => window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []);
}

test.describe("the one launch form", () => {
  // Tall enough that a single shot evidences the primary form — the ruling's
  // field list, the readiness split and the visible blockers. At 900 px both
  // shots were clipped and neither showed the policy controls open
  // (REVIEW-B3 F12).
  test.use({ viewport: { width: 1280, height: 1600 } });

  test("01 — one form: goal, lead, governed, bench, policy, readiness", async ({
    page,
  }) => {
    await openApp(page);
    await openDialog(page);

    // The two tabs are gone. A form that still offered them would be offering
    // the two half-answers this change exists to remove.
    await expect(
      page.getByTestId("new-coding-session-tab-session"),
    ).toHaveCount(0);
    await expect(page.getByTestId("new-coding-session-tab-crew")).toHaveCount(
      0,
    );

    // The primary path keeps setup details out of view while preserving the
    // mounted controls behind the disclosure for saved setups and edits.
    await expect(
      page.getByTestId("new-coding-session-setup-summary"),
    ).toContainText("Current setup");
    await expect(
      page.getByTestId("new-coding-session-configuration"),
    ).not.toHaveAttribute("open");
    await expect(page.getByTestId("new-coding-session-bench")).toBeHidden();
    await expect(page.getByTestId("new-coding-session-policy")).toBeHidden();

    // Leading it yourself: not governed, and the switch says why rather than
    // pretending to be a decision.
    await expect(page.getByTestId("new-coding-session-governed")).toContainText(
      "Not governed",
    );
    await expect(
      page.getByTestId("new-coding-session-governed-reason"),
    ).toContainText("Pick an agent to lead");
    // No goal yet, so the button is off and the reason is on screen.
    await expect(page.getByTestId("new-coding-session-submit")).toBeDisabled();
    await expect(
      page.getByTestId("new-coding-session-blocker-goal"),
    ).toContainText("Write the goal");

    await page.getByTestId("new-coding-session-edit-setup").click();

    await page
      .getByTestId("new-coding-session-goal")
      .fill("Close ledger item 103, finding 12.");
    await page
      .getByTestId("new-coding-session-lead-select")
      .selectOption(ROLES[0].pubkey);

    // Picking an agent turns the session governed, by construction.
    await expect(page.getByTestId("new-coding-session-governed")).toContainText(
      "Governed",
    );
    await expect(
      page.getByTestId("new-coding-session-governed-reason"),
    ).toContainText("always governed");
    // The identity line carries the whole name and the canonical short pubkey,
    // so the two Keystones in this fixture are distinguishable.
    await expect(
      page.getByTestId("new-coding-session-lead-identity"),
    ).toContainText("Keystone · a1a1a1a1…a1a1");

    // The bench is who the lead *may* hire, and says nobody here is seated.
    const bench = page.getByTestId("new-coding-session-bench");
    await expect(bench).toContainText("Nobody here is seated by this launch");
    await expect(
      bench.getByTestId(`new-coding-session-bench-identity-${ROLES[1].pubkey}`),
    ).toBeVisible();
    await expect(bench).toContainText("b2b2b2b2…b2b2");

    // The policy block carries the disclosure the record earns — opened, so
    // the shot evidences the controls rather than a collapsed summary.
    await expect(page.getByTestId("new-coding-session-policy")).toContainText(
      "none set",
    );
    await page.getByTestId("new-coding-session-policy").click();
    // L21 finding 40: this asserted "a stated intention, not an enforced
    // limit" — copy that 06a41fff7 replaced when `gates.verifierRequired`
    // became enforced. The spec has been red on `main` ever since and `just
    // ci` does not run Playwright, so nothing said so. Asserted against the
    // constant now, so the two cannot drift again.
    await expect(
      page.getByTestId("new-coding-session-policy-disclosure"),
    ).toContainText(CODING_SESSION_POLICY_STATED_NOT_ENFORCED);

    // The technical plan stays collapsed on the primary path, but remains
    // available without changing what Start will publish.
    await expect(
      page.getByTestId("new-coding-session-launch-details"),
    ).not.toHaveAttribute("open");
    await expect(
      page.getByTestId("new-coding-session-plan-genesis"),
    ).toBeHidden();
    await page
      .getByTestId("new-coding-session-launch-details")
      .locator("summary")
      .click();

    // The plan names the events pressing the button publishes, with kinds.
    await expect(
      page.getByTestId("new-coding-session-plan-genesis"),
    ).toHaveAttribute("data-kind", "44226");
    await expect(
      page.getByTestId("new-coding-session-plan-create"),
    ).toContainText("the only seat this launch creates");
    await expect(page.getByTestId("new-coding-session-submit")).toBeEnabled();

    // All five plan lines remain available, so the disclosure cannot truncate
    // the actual launch behavior when a person asks to inspect it.
    for (const id of ["genesis", "goal", "create", "grants", "turn"]) {
      await expect(
        page.getByTestId(`new-coding-session-plan-${id}`),
      ).toBeVisible();
    }
    await page
      .getByTestId("new-coding-session-launch-details")
      .locator("summary")
      .click();
    await page
      .getByTestId("new-coding-session-configuration")
      .locator(":scope > summary")
      .click();
    await page
      .getByTestId("new-coding-session-form")
      .evaluate((node) => node.scrollTo(0, 0));
    await expect(
      page.getByTestId("new-coding-session-configuration"),
    ).not.toHaveAttribute("open");
    await waitForAnimations(page);
    await page.screenshot({ path: `${SHOTS}/launch-form.png`, fullPage: true });
  });

  test("seat setup refusal stays visible beside Start in a short window", async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1000, height: 600 });
    await openApp(
      page,
      "Could not fetch the lead role pack: credential helper failed.",
    );
    await openDialog(page);
    await page
      .getByTestId("new-coding-session-goal")
      .fill("Inspect this project.");
    await page.getByTestId("new-coding-session-edit-setup").click();
    await page
      .getByTestId("new-coding-session-lead-select")
      .selectOption(ROLES[0].pubkey);
    await page.getByTestId("new-coding-session-submit").click();
    const failure = page.getByTestId("new-coding-session-setup-error");
    await expect(failure).toContainText("credential helper failed");
    await expect(failure).toBeInViewport();
    await expect(page.getByTestId("new-coding-session-submit")).toBeEnabled();
    await expect(page.getByTestId("new-coding-session-goal")).toHaveValue(
      "Inspect this project.",
    );
    expect(
      (await signedEvents(page)).filter((event) => event.kind === 44221),
    ).toHaveLength(0);
  });

  test("02 — a governed launch signs 44226, 44227, 44245 and one 44221, and waits for its receipt", async ({
    page,
  }) => {
    await openApp(page);
    await openDialog(page);
    await page.getByTestId("new-coding-session-edit-setup").click();

    await page
      .getByTestId("new-coding-session-goal")
      .fill("Land the one form and prove the sequence.");
    await page
      .getByTestId("new-coding-session-lead-select")
      .selectOption(ROLES[0].pubkey);
    // Bench the builder, and set one policy field so a 44245 is published.
    await page
      .getByTestId(`new-coding-session-bench-identity-${ROLES[1].pubkey}`)
      .click();
    await page.getByTestId("new-coding-session-policy").click();
    await page
      .getByTestId("new-coding-session-policy-posture")
      .selectOption("overnight");
    // L21 finding 39: `gates.verifierRequired` is enforced at the 44244 fold's
    // completion check and was the one enforced field the launch form could
    // not set — the disclosure named it while nothing offered it.
    // L22: the switch now decides which arm of the push rule lands a seat's
    // work, so its labels say so. L21 rejected exactly these words because
    // they claimed a push effect the switch did not yet have; the relay reads
    // the flag now, and a label that still described only the completion
    // check would understate what the founder is choosing.
    const verifierSwitch = page.getByTestId(
      "new-coding-session-policy-verifier-required",
    );
    await expect(verifierSwitch).toContainText(
      "Observed gates land this mission\u2019s work",
    );
    await expect(verifierSwitch).toContainText("A verifier must also clear it");
    await expect(verifierSwitch).toContainText("Not set");
    await expect(page.getByTestId("new-coding-session-policy")).toContainText(
      "observed",
    );
    await verifierSwitch.selectOption("true");
    await expect(
      page.getByTestId("new-coding-session-plan-policy"),
    ).toHaveAttribute("data-kind", "44245");

    await page.getByTestId("new-coding-session-submit").click();

    // Genesis, goal and policy are all signed before the first create: a seat
    // should be able to read the mission it was created for off the wire.
    await expect
      .poll(
        async () =>
          (await signedEvents(page))
            .map((event) => event.kind)
            .filter((kind) => kind >= 44220 && kind <= 44245),
        { timeout: 20_000 },
      )
      .toEqual(expect.arrayContaining([44226, 44227, 44245, 44221]));

    const beforeReceipt = (await signedEvents(page)).map((event) => event.kind);
    const order = beforeReceipt.filter((kind) =>
      [44226, 44227, 44245, 44221].includes(kind),
    );
    expect(order.indexOf(44226)).toBe(0);
    expect(order.indexOf(44227)).toBeLessThan(order.indexOf(44221));
    expect(order.indexOf(44245)).toBeLessThan(order.indexOf(44221));

    // Exactly one create — the lead. A launch that seated the bench would be
    // the lie the Team tab used to tell: four rows, one live agent.
    const creates = (await signedEvents(page)).filter(
      (event) =>
        event.kind === 44221 &&
        JSON.parse(event.content).action.type === "session.create",
    );
    expect(creates).toHaveLength(1);
    const createAction = JSON.parse(creates[0].content).action;
    expect(createAction.actor).toBe(ROLES[0].pubkey);
    expect(createAction.role).toBe("lead");
    // The lead's own model, never another control's.
    expect(createAction.model).toBe("sonnet");

    // The policy that went out is the one the form asked the native builder
    // for, verbatim.
    const policyEvent = (await signedEvents(page)).find(
      (event) => event.kind === 44245,
    );
    expect(policyEvent).toBeTruthy();
    const policy = JSON.parse(policyEvent?.content ?? "{}");
    expect(policy.posture).toBe("overnight");
    expect(policy.gates.verifierRequired).toBe(true);
    expect(policy.bench.identities).toEqual([ROLES[1].pubkey]);
    expect(policyEvent?.tags?.[0]?.[0]).toBe("h");

    // The seat step is where a governed launch waits: no mock provider answers
    // a 44221, so the create's receipt never lands and the grants that follow
    // it are not reachable from here. That is the correct behaviour with no
    // provider, and it is what this spec asserts rather than faking a receipt
    // the app has no reason to believe. The 44228 pair is driven end to end by
    // `codingSessionCrewLaunch.test.mjs` ("a failed grant is named rather than
    // swallowed", "the lead seat holds operator authority") against the same
    // sequence this form calls.
    await expect(
      page.getByTestId("new-coding-session-crew-steps"),
    ).toContainText("Seat Keystone as lead");
    await expect(page.getByTestId("crew-step-genesis")).toHaveAttribute(
      "data-state",
      "done",
    );
    await expect(page.getByTestId("crew-step-policy")).toHaveAttribute(
      "data-state",
      "done",
    );
    await expect(page.getByTestId("crew-step-create:0")).toHaveAttribute(
      "data-state",
      "running",
    );
  });

  test("03 — an identity that declares no model blocks the launch until one is picked", async ({
    page,
  }) => {
    // REVIEW-B3 F1, on the live path. Before this the form settled the model
    // picker's default for an identity that declared none, published it on the
    // create, and the disclosure written for exactly that case could never
    // fire because the model was never null.
    await openApp(page);
    await openDialog(page);
    await page.getByTestId("new-coding-session-edit-setup").click();
    await page
      .getByTestId("new-coding-session-goal")
      .fill("Prove the model comes from the identity.");
    // ROLES[1] publishes `model: null`.
    await page
      .getByTestId("new-coding-session-lead-select")
      .selectOption(ROLES[1].pubkey);
    await expect(
      page.getByTestId("new-coding-session-lead-identity"),
    ).toContainText("no model of its own");
    await expect(
      page.getByTestId("new-coding-session-blocker-model"),
    ).toContainText("declares no model and nothing has routed one");
    await expect(page.getByTestId("new-coding-session-submit")).toBeDisabled();

    // Picking one by hand is an override, and an override owes its reason.
    await page.getByTestId("coding-session-model-picker").click();
    await page
      .getByTestId("coding-session-model-row")
      .filter({ hasText: /haiku/i })
      .first()
      .getByTestId("coding-session-model-row-select")
      .click();
    await expect(
      page.getByTestId("new-coding-session-blocker-model"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("new-coding-session-blocker-override-reason"),
    ).toContainText("an override with no reason");
    await expect(page.getByTestId("new-coding-session-submit")).toBeDisabled();
    await page
      .getByTestId("new-coding-session-model-override")
      .fill("The pack's own model is not installed on this runtime.");
    await expect(page.getByTestId("new-coding-session-submit")).toBeEnabled();
  });

  test("04 — the form at a narrow width keeps its blockers and its plan", async ({
    page,
  }) => {
    await openApp(page);
    await openDialog(page);
    await page.getByTestId("new-coding-session-edit-setup").click();
    await page.setViewportSize({ width: 720, height: 1600 });
    await page
      .getByTestId("new-coding-session-goal")
      .fill("Check the narrow measure.");
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
    await page
      .getByTestId("new-coding-session-launch-details")
      .locator("summary")
      .click();
    await page.getByTestId("coding-session-model-picker").click();
    await page
      .getByTestId("coding-session-model-row")
      .filter({ hasText: /haiku/i })
      .first()
      .getByTestId("coding-session-model-row-select")
      .click();
    await page
      .getByTestId("new-coding-session-model-override")
      .fill("Parallax carries no model of its own.");
    await expect(page.getByTestId("new-coding-session-submit")).toBeEnabled();
    // The dialog's own box scrolls (`max-h-[65vh]`), so a taller viewport alone
    // cannot show the plan — scroll it to the end so this shot carries the
    // readiness split and **all five** plan lines rather than a truncation
    // (REVIEW-B3 F12).
    await page
      .getByTestId("new-coding-session-form")
      .evaluate((node) => node.scrollTo(0, node.scrollHeight));
    for (const id of ["genesis", "goal", "create", "grants", "turn"]) {
      await expect(
        page.getByTestId(`new-coding-session-plan-${id}`),
      ).toBeVisible();
    }
    await waitForAnimations(page);
    await page.screenshot({
      path: `${SHOTS}/launch-form-narrow.png`,
      fullPage: true,
    });
  });
});
