import { expect, test, type Page } from "@playwright/test";
import { finalizeEvent } from "nostr-tools/pure";

import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "../../src/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "../../src/shared/constants/kinds";

import { installMockBridge } from "../helpers/bridge";
import { E2E_IDENTITY_OVERRIDE_STORAGE_KEY } from "../helpers/onboarding";
import {
  CANONICAL_REPO,
  CHANNEL_NAME,
  captureLocator,
  commandNames,
  createActions,
  defaultStubConfig,
  eventsMentioning,
  FOREIGN_PROVIDER_PUBKEY,
  FOREIGN_PROVIDER_SECRET,
  FOUNDER_IDENTITY,
  FOUNDER_PUBKEY,
  hintCalls,
  LOCAL_INSTANCE_ID,
  LOCAL_PROVIDER_PUBKEY,
  LOCAL_PROVIDER_SECRET,
  PROJECT_DTAG,
  recordedCommands,
  REUSE_BRANCH,
  REUSE_PATH,
  seatWorktreeRow,
  seedEvents,
  seededSessionEvents,
  SESSION_REF,
  SESSION_TITLE,
  signedEvents,
  workdirUseCalls,
  WORKSPACE_REUSE_SEAM_LANDED,
  workspaceReuseInitScript,
  type WorkspaceReuseStubConfig,
} from "./helpers/workspaceReuseAssertions";

/**
 * "New session in this workspace", driven rather than described.
 *
 * The action's whole promise is a pair of negatives: clicking the menu item
 * publishes one genesis and nothing else (since 2026-09-10 every "New coding
 * session" founds the topic on the click and lands on the founded page, where
 * the session is set up and started), and starting the session it founds
 * remembers nothing. Both are invisible in the UI — a wiring test would pass
 * over either failing — so this spec asserts them where they can be seen: at
 * the command seam the app actually calls, in the events it signs, and in the
 * *next* page the person opens.
 *
 * Three things are proved by consequence rather than by spy count:
 *
 *  - the reused directory arrives at the execution seam (the staged create
 *    hint carries it, under the same `commandId` the signed 44221 carries),
 *  - the first session's conversation is untouched (no signed event names it),
 *  - nothing is remembered (the workdir stub is a live store, so a recorded
 *    use would surface as a prefill in the following ordinary draft).
 *
 * Limit, stated once and meant: this is mock-bridge coverage. It proves what
 * this app asks its host for and what it publishes. It is not proof of native
 * behaviour on any platform, and it is emphatically not native Windows proof.
 */

const SHOTS = "test-results/workspace-reuse";
const MENU_LABEL = "New session in this workspace";

const AVAILABLE_ROW = seatWorktreeRow({
  sessionRef: SESSION_REF,
  path: REUSE_PATH,
  branch: REUSE_BRANCH,
});

/**
 * A *different* session whose recorded tree is the same directory.
 *
 * This is the only input that produces a non-empty `alsoHere`, and the line
 * it drives is a count of rows already read — never a registry, and never a
 * reason to block. Kept out of the default fixture so every other scenario
 * asserts the plain menu detail.
 */
const SECOND_SESSION_REF = "9f3c1d20-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const SAME_FOLDER_ROW = seatWorktreeRow({
  sessionRef: SECOND_SESSION_REF,
  path: REUSE_PATH,
  branch: REUSE_BRANCH,
  seatLabel: "verifier-1",
});

function availableStub(
  overrides: Partial<WorkspaceReuseStubConfig> = {},
): WorkspaceReuseStubConfig {
  return defaultStubConfig({
    seatWorktrees: [AVAILABLE_ROW],
    validationByPath: {
      [REUSE_PATH]: { exists: true, isDir: true, isAbsolute: true },
      [CANONICAL_REPO]: { exists: true, isDir: true, isAbsolute: true },
    },
    ...overrides,
  });
}

async function boot(
  page: Page,
  input: {
    stub: WorkspaceReuseStubConfig;
    providerSecret?: Uint8Array;
    providerPubkey?: string;
  },
) {
  // Identity first: React reads it on mount, and the seeded genesis must be
  // both signature-valid and *this viewer's own* for the sidebar's default
  // "My sessions" filter to show the row at all.
  await page.addInitScript(
    ({ storageKey, identity }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(identity));
    },
    {
      storageKey: E2E_IDENTITY_OVERRIDE_STORAGE_KEY,
      identity: FOUNDER_IDENTITY,
    },
  );
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        {
          pubkey: LOCAL_PROVIDER_PUBKEY,
          label: "This computer (coding sessions)",
        },
      ],
    },
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: LOCAL_PROVIDER_PUBKEY,
      instanceId: LOCAL_INSTANCE_ID,
    },
    codingSessionProviderRuntimes: [
      {
        instanceRef: "claude-primary",
        runtime: "claude",
        driver: "claude-agent-acp",
        label: "Claude Code",
        authState: "ready",
        defaultModel: "sonnet",
        allowedModels: ["default", "sonnet"],
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
    searchProfiles: [{ pubkey: FOUNDER_PUBKEY, displayName: "Tyler" }],
  });
  // Registered after the bridge so this wrapper sits in front of the bridge's
  // own `invoke` — the same order `coding-session-launch-form.spec.ts` uses.
  await page.addInitScript(workspaceReuseInitScript(), input.stub);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const seeded = seededSessionEvents({
    providerSecret: input.providerSecret,
    providerPubkey: input.providerPubkey,
  });
  await seedEvents(page, seeded.events);
  await expect(
    page.getByTestId("project-coding-session-row").first(),
  ).toBeVisible({ timeout: 20_000 });
  return seeded;
}

/**
 * The seeded session's sidebar row, by its title. Every founding this spec
 * performs adds an "Untitled session" row above it, so `.first()` would pick
 * a founded session with no recorded workspace and the action would degrade
 * to a plain channel founding — which is a different scenario, not this one.
 */
function seededSessionRow(page: Page) {
  return page
    .getByTestId("project-coding-session-row")
    .filter({ hasText: SESSION_TITLE })
    .first();
}

/** Right-click the sidebar session row and choose the workspace item. */
async function openWorkspaceMenu(page: Page) {
  await seededSessionRow(page).click({ button: "right" });
  const item = page.getByRole("menuitem", { name: MENU_LABEL });
  await expect(item).toBeVisible({ timeout: 10_000 });
  return item;
}

/** The founded page's card, with its Where field on screen. */
async function expectFoundedPage(page: Page) {
  await expect(
    page.getByTestId("coding-session-founded-workspace-founded"),
  ).toBeVisible({ timeout: 20_000 });
  await expect(page).toHaveURL(
    /#\/coding-sessions\/[0-9a-f-]{36}\/founded\/[0-9a-f-]{36}/,
  );
  await expect(page.getByTestId("coding-session-founded-where")).toBeVisible();
  // The goal reader is held behind the client's send budget
  // (`relaySendBudget.ts`, 25 sends per 5 s) and can settle ~10 s in; until
  // it does a prompt left publishes nothing, by design.
  await expect(
    page.getByTestId("new-coding-session-blocker-goal-unresolved"),
  ).toHaveCount(0, { timeout: 30_000 });
}

/** Click the workspace item: founds the session and lands on its page. */
async function openWorkspaceDraft(page: Page) {
  const item = await openWorkspaceMenu(page);
  await item.click();
  await expectFoundedPage(page);
}

/** Ordinary "New coding session" from the project's own create menu. */
async function openOrdinaryProjectDraft(page: Page) {
  await page
    .getByTestId(`project-create-${PROJECT_DTAG}`)
    .click({ force: true });
  await page.getByTestId(`project-new-coding-session-${PROJECT_DTAG}`).click();
  await expectFoundedPage(page);
}

/**
 * What a founded page must say about the workspace, on either side of the
 * seam.
 *
 * Lane W gates the disclosure behind `WORKSPACE_REUSE_SEAM_LANDED` so that,
 * until root lands the form seam, the page is an ordinary unseeded setup
 * rather than one claiming a folder its own directory field contradicts. Both
 * states are asserted here — neither is skipped, because "unseeded and silent"
 * is a truthful thing to ship and a lying page is not.
 */
async function expectWorkspaceDraftState(page: Page) {
  const where = page.getByTestId("coding-session-founded-where");
  if (WORKSPACE_REUSE_SEAM_LANDED) {
    // The summary renders inside the Where field: the folder, then the
    // directory field already on it.
    await expect(
      where.getByTestId("coding-session-workspace-reuse-path"),
    ).toHaveText(REUSE_PATH);
    await expect(page.getByTestId("coding-session-workdir-input")).toHaveValue(
      REUSE_PATH,
    );
    return;
  }
  await expect(
    page.getByTestId("coding-session-workspace-reuse"),
    "with the seam unlanded the page must claim no workspace at all",
  ).toHaveCount(0);
  await expect(
    page.getByTestId("coding-session-workdir-input"),
  ).not.toHaveValue(REUSE_PATH);
}

/**
 * Type the initial prompt, leave the field (which publishes it), and Start.
 */
async function submitDraft(page: Page, goal: string) {
  const prompt = page.getByTestId("coding-session-founded-prompt");
  await prompt.fill(goal);
  await prompt.blur();
  await expect
    .poll(
      async () =>
        (await signedEvents(page)).filter((event) => event.kind === 44227)
          .length,
      { timeout: 15_000 },
    )
    .toBe(1);
  await expect(page.getByTestId("coding-session-founded-start")).toBeEnabled();
  await page.getByTestId("coding-session-founded-start").click();
}

/**
 * The signed 44226 geneses, decoded to their session ref — carried in the
 * `csg-session` tag (`lib/codingSessionGenesis.ts`), not in a `d` tag.
 */
function geneses(events: Array<{ kind: number; tags: string[][] }>) {
  return events
    .filter((event) => event.kind === 44226)
    .map(
      (event) =>
        event.tags.find((tag) => tag[0] === "csg-session")?.[1] ?? null,
    );
}

/** Settle the actual create through signed provider facts, as the UI expects. */
async function settleCreatedDraft(
  page: Page,
  create: { commandId: string; sessionRef: string | null },
) {
  if (!create.sessionRef)
    throw new Error("submitted create has no session ref");
  const command = (await signedEvents(page)).find(
    (event) =>
      event.kind === 44221 &&
      JSON.parse(event.content).commandId === create.commandId,
  );
  const channelId = command?.tags.find((tag) => tag[0] === "h")?.[1];
  if (!channelId) throw new Error("submitted create has no channel");
  const metadataTemplate = seededSessionEvents({
    sessionRef: create.sessionRef,
    sessionId: "a4444444-2222-4333-8444-555555555555",
    title: "Second pass over the same files",
  }).events.find((event) => event.kind === KIND_CODING_SESSION_METADATA);
  if (!metadataTemplate) throw new Error("provider metadata fixture missing");
  // A project launcher can prepare its own session transport. The original
  // session's general-channel fixture must not put the new metadata there.
  const metadata = finalizeEvent(
    {
      kind: metadataTemplate.kind,
      created_at: metadataTemplate.created_at,
      tags: metadataTemplate.tags.map((tag) =>
        tag[0] === "h" ? ["h", channelId] : tag,
      ),
      content: metadataTemplate.content,
    },
    LOCAL_PROVIDER_SECRET,
  );
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: metadata.created_at,
      tags: [
        ["h", channelId],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", create.commandId],
        ["csl-key", lifecycleReceiptSemanticKey(create.commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: create.commandId,
        status: "created",
        session: JSON.parse(metadata.content).session,
        error: null,
      }),
    },
    LOCAL_PROVIDER_SECRET,
  );
  await page.evaluate(
    async ({ channelId, events }) => {
      const query = window.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__;
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!query || !seed)
        throw new Error("mock channel or event seam missing");
      const result = await query("get_channels", {});
      const channels = (
        result as { channels: Array<{ id: string; name: string }> }
      ).channels;
      const channel = channels.find((entry) => entry.id === channelId);
      if (!channel) throw new Error("created session's channel missing");
      for (const event of events) seed({ channelName: channel.name, event });
    },
    { channelId, events: [receipt, metadata] },
  );
}

test.describe("new session in this workspace", () => {
  // Tall enough that the page's Where field and the setup fields are both on
  // screen; a clipped card reads as a missing control.
  test.use({ viewport: { width: 1280, height: 1600 } });

  test("01+02 — the page reuses the directory, and remembers nothing", async ({
    page,
  }) => {
    test.skip(
      !WORKSPACE_REUSE_SEAM_LANDED,
      "root's workspaceReuse form seam is not landed: the launcher cannot be seeded yet",
    );
    // A governed-free launch still waits on provisioning, a membership
    // round-trip and the create's publish; 30s is not enough headroom.
    test.setTimeout(90_000);
    await boot(page, { stub: availableStub() });
    const before = (await signedEvents(page)).length;

    await openWorkspaceDraft(page);

    // The page says which folder, which branch, and which kind of fact that
    // branch is — then the two sentences that say what this action is.
    await expect(
      page.getByTestId("coding-session-workspace-reuse-path"),
    ).toHaveText(REUSE_PATH);
    // The live-head read is unconfigured in this fixture, so the recorded
    // branch is the honest answer and it must be labelled as recorded.
    await expect(
      page.getByTestId("coding-session-workspace-reuse-branch"),
    ).toContainText(REUSE_BRANCH);
    // Soft from here on, deliberately: the assertions below depend on root's
    // reserved-file seam (contract §1) and on the branch's provenance
    // surviving into the draft. A hard stop at the first of them would end
    // the run before the create-seam evidence — the part only this spec can
    // produce — is ever gathered. Soft failures still fail the test.
    await expect
      .soft(
        page.getByTestId("coding-session-workspace-reuse-branch"),
        "the branch must say which fact it is: recorded at creation, or on disk now",
      )
      .toContainText("recorded");
    const note = page.getByTestId("coding-session-workspace-reuse-note");
    await expect(note).toContainText("New conversation; uses these files.");
    await expect(note).toContainText(
      "Includes uncommitted changes already in this folder.",
    );
    // Vocabulary this action is not.
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).not.toContainText(/isolated|forked|inherited|continue|taken over/i);

    // The Where field itself: the directory is already the one that was
    // chosen, and no worktree will be cut.
    await expect
      .soft(
        page.getByTestId("coding-session-workdir-input"),
        "the page must open on the reused directory (root's workspaceReuse seam)",
      )
      .toHaveValue(REUSE_PATH);
    await expect
      .soft(page.getByTestId("coding-session-worktree-toggle"))
      .not.toBeChecked();

    await submitDraft(page, "Start a second pass over the same files.");

    // The create the app signed, and the hint it staged, are the same launch.
    await expect
      .poll(async () => createActions(await signedEvents(page)).length, {
        timeout: 25_000,
      })
      .toBe(1);
    const create = createActions(await signedEvents(page))[0];
    const commands = await recordedCommands(page);
    const hints = hintCalls(commands);
    expect(hints).toHaveLength(1);
    expect
      .soft(
        hints[0].path,
        "the execution seam must receive the reused directory",
      )
      .toBe(REUSE_PATH);
    expect(hints[0].commandId).toBe(create.commandId);

    // No worktree was planned or cut: reuse means the directory as it stands.
    expect(commandNames(commands)).not.toContain(
      "plan_coding_session_worktree",
    );
    expect(commandNames(commands)).not.toContain(
      "create_coding_session_worktree",
    );

    // The first conversation is untouched: nothing this app signed names it.
    const after = await signedEvents(page);
    expect(eventsMentioning(after.slice(before), SESSION_REF)).toHaveLength(0);
    expect(create.sessionRef).not.toBe(SESSION_REF);

    // 02 — nothing is remembered. The one-off directory must not enter the
    // MRU, and must not become the project's default through the hint.
    expect(workdirUseCalls(commands)).not.toContain(REUSE_PATH);
    expect(hints[0].projectRef).toBeNull();

    // Provider acceptance and metadata settle the durable create; the founded
    // page hands off to the generation route once, on its own.
    await settleCreatedDraft(page, create);
    await expect(
      page.getByTestId("coding-session-founded-workspace-founded"),
    ).toHaveCount(0, { timeout: 25_000 });
    await expect(page.getByTestId("coding-session-workspace")).toBeVisible({
      timeout: 25_000,
    });
    // …and the consequence, which is what a person would actually notice.
    await openOrdinaryProjectDraft(page);
    await expect(
      page.getByTestId("coding-session-workdir-input"),
    ).not.toHaveValue(REUSE_PATH);
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(0);
  });

  test("03 — opening the menu mutates nothing; the click founds one genesis and nothing else", async ({
    page,
  }) => {
    await boot(page, { stub: availableStub() });
    const signedBefore = (await signedEvents(page)).length;
    const commandsBefore = (await recordedCommands(page)).length;

    const item = await openWorkspaceMenu(page);
    // One row at this path, so there is no shared-directory line: an empty
    // list renders nothing rather than "no other sessions", which is a claim
    // the read cannot support. (The line itself is scenario 13.)
    await expect(item).not.toContainText("other session");
    // Opening the menu signs nothing: the reads happen, the genesis does not.
    expect(await signedEvents(page)).toHaveLength(signedBefore);
    await item.click();
    await expectFoundedPage(page);

    const window = (await recordedCommands(page)).slice(commandsBefore);
    // An allowlist, not a denylist: every command the app issued between the
    // right-click and the open page has to be named here, and each name has
    // to be a read — except the one signature the click is for. A mutation
    // nobody predicted fails this; a denylist would have let it through.
    const allowed = new Set([
      // The click's one write: the genesis is signed here, and its publish is
      // the one EVENT frame admitted below.
      "sign_event",
      // The click navigates to a new route; these are `AppShell`'s route
      // effects (huddle state, Tauri event listeners, pending deep links),
      // all reads or listener bookkeeping — not session mutations.
      "get_huddle_state",
      "plugin:event|listen",
      "plugin:event|unlisten",
      "take_pending_entity_deep_link",
      "take_pending_navigation_deep_link",
      // The founded page's stores read the relay's own key once, to trust
      // its deletion receipts (2026-09-11) — a read, not a mutation.
      "get_relay_self",
      // The resolution's own budget (contract §2).
      "list_coding_session_seat_worktrees",
      "validate_coding_session_workdir",
      "list_coding_session_worktree_branches",
      "coding_session_provider_status",
      // The launcher reading what this machine already remembers.
      "get_coding_session_workdir_state",
      // Ambient reads the launcher performs on mount. Every one is a getter:
      // `coding_session_naming_settings` reads what names sessions on this
      // computer (its writer is `set_coding_session_naming_settings`, which
      // does not appear), and `coding_session_provider_models` reads a model
      // catalog — it may probe the adapter, but it changes no session state.
      "list_managed_agents",
      "list_teams",
      "list_personas",
      "coding_session_naming_settings",
      "coding_session_provider_models",
      "coding_session_provider_runtimes",
      "get_global_agent_config",
      "get_identity",
      "list_acp_runtimes",
      // The project-scoped launcher's own reads: the project's local
      // checkouts, the team's readiness, and a relay query. All getters.
      "list_project_local_repositories",
      "team_readiness",
      // Which agents this computer installed for the project, read from setup
      // journals so the lead picker can put the project's own roles first. A
      // listing: it creates, repairs and publishes nothing.
      "project_team_list_installed_roles",
      "query_relay_filters",
    ]);
    // Ambient subscription maintenance may flush during the click window.
    // Inspect the wire frame: the generic send command alone is not a read.
    // The one EVENT admitted is the genesis; a second EVENT of any kind, or
    // an EVENT of any other kind, is the mutation this test exists to catch.
    let genesisFrames = 0;
    for (const entry of window.filter(
      (entry) => entry.command === "plugin:websocket|send",
    )) {
      const args = entry.args as { message: { type: string; data: string } };
      expect(args.message.type).toBe("Text");
      const frame = JSON.parse(args.message.data);
      expect(Array.isArray(frame)).toBe(true);
      if (frame[0] === "EVENT") {
        expect(frame[1]?.kind, "the only EVENT a click may send").toBe(44226);
        genesisFrames += 1;
        continue;
      }
      expect(["REQ", "COUNT", "CLOSE"]).toContain(frame[0]);
    }
    expect(genesisFrames).toBeLessThanOrEqual(1);
    allowed.add("plugin:websocket|send");
    const observed = commandNames(window);
    // Printed, not merely asserted: the report this spec exists to produce
    // has to name the exact set a run observed, and a passing allowlist
    // otherwise leaves that set invisible.
    console.log(`[03] commands in the click window: ${observed.join(", ")}`);
    const unexpected = observed.filter((name) => !allowed.has(name));
    expect(
      unexpected,
      `commands observed in the click window: ${commandNames(window).join(", ")}`,
    ).toEqual([]);

    // Exactly one thing was signed — the genesis — and it does not name the
    // source session: no create, no turn, no closure, no authority change.
    const signed = await signedEvents(page);
    expect(signed).toHaveLength(signedBefore + 1);
    expect(signed[signedBefore].kind).toBe(44226);
    expect(geneses(signed)).toHaveLength(1);
    expect(geneses(signed)[0]).not.toBe(SESSION_REF);
    expect(createActions(signed)).toHaveLength(0);
  });

  test("04 — a directory that is gone opens an unseeded page and says so", async ({
    page,
  }) => {
    await boot(page, {
      stub: defaultStubConfig({
        seatWorktrees: [AVAILABLE_ROW],
        validationByPath: {
          [REUSE_PATH]: { exists: false, isDir: false, isAbsolute: true },
          [CANONICAL_REPO]: { exists: true, isDir: true, isAbsolute: true },
        },
      }),
    });

    // The item still exists — availability changes what it opens, never
    // whether it is offered.
    await openWorkspaceDraft(page);
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(0);
    await expect(
      page.getByText("No such directory on this computer.").first(),
    ).toBeVisible({ timeout: 10_000 });

    // Nothing seeded — and the ordinary way to choose a folder is right there.
    await expect(
      page.getByTestId("coding-session-workdir-input"),
    ).not.toHaveValue(REUSE_PATH);
    await expect(
      page.getByTestId("coding-session-workdir-browse"),
    ).toBeEnabled();
  });

  test("05 — an execution on another provider is named, never pathed", async ({
    page,
  }) => {
    await boot(page, {
      stub: availableStub(),
      providerSecret: FOREIGN_PROVIDER_SECRET,
      providerPubkey: FOREIGN_PROVIDER_PUBKEY,
    });

    await openWorkspaceDraft(page);
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(0);
    await expect(
      page
        .getByText(
          "This session's execution runs on a provider this computer does not hold.",
        )
        .first(),
    ).toBeVisible({ timeout: 10_000 });

    // No path, anywhere: a Mac path is not another machine's checkout.
    await expect(
      page.getByTestId("coding-session-workdir-input"),
    ).not.toHaveValue(REUSE_PATH);
    await expect(
      page.getByTestId("coding-session-founded-setup-card"),
    ).not.toContainText(REUSE_PATH);
  });

  test("06 — an unrecorded session borrows nothing", async ({ page }) => {
    await boot(page, {
      stub: defaultStubConfig({
        seatWorktrees: [],
        validationByPath: {
          [CANONICAL_REPO]: { exists: true, isDir: true, isAbsolute: true },
        },
      }),
    });

    await openWorkspaceDraft(page);
    await expect(
      page
        .getByText("This computer recorded no directory for this session.")
        .first(),
    ).toBeVisible({ timeout: 10_000 });

    // The channel's remembered folder is not this session's workspace, and
    // must not be dressed as one.
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("coding-session-workdir-input"),
    ).not.toHaveValue(REUSE_PATH);
  });

  test("07 — a folder typed before submit is the folder the create uses", async ({
    page,
  }) => {
    test.skip(
      !WORKSPACE_REUSE_SEAM_LANDED,
      "root's workspaceReuse form seam is not landed: the launcher cannot be seeded yet",
    );
    test.setTimeout(90_000);
    const typed = "/Users/mock/Code/somewhere-else";
    await boot(page, {
      stub: availableStub({
        validationByPath: {
          [REUSE_PATH]: { exists: true, isDir: true, isAbsolute: true },
          [CANONICAL_REPO]: { exists: true, isDir: true, isAbsolute: true },
          [typed]: { exists: true, isDir: true, isAbsolute: true },
        },
      }),
    });

    await openWorkspaceDraft(page);
    await expect(page.getByTestId("coding-session-workdir-input")).toHaveValue(
      REUSE_PATH,
    );
    await expect(
      page.getByTestId("coding-session-workspace-reuse-path"),
    ).toHaveText(REUSE_PATH);
    // The directory field stays the founder's to change; the folder typed
    // here, not the one the page opened on, is the folder the create uses.
    await page.getByTestId("coding-session-workdir-input").fill(typed);
    await expect(page.getByTestId("coding-session-workdir-input")).toHaveValue(
      typed,
    );
    await submitDraft(page, "Run this one somewhere else.");

    await expect
      .poll(async () => createActions(await signedEvents(page)).length, {
        timeout: 25_000,
      })
      .toBe(1);
    const hints = hintCalls(await recordedCommands(page));
    expect(hints).toHaveLength(1);
    expect(hints[0].path).toBe(typed);
  });

  test("08 — leaving and re-opening founds a second session, and accumulates nothing else", async ({
    page,
  }) => {
    test.skip(
      !WORKSPACE_REUSE_SEAM_LANDED,
      "root's workspaceReuse form seam is not landed: the launcher cannot be seeded yet",
    );
    await boot(page, { stub: availableStub() });
    const signedBefore = (await signedEvents(page)).length;

    await openWorkspaceDraft(page);
    const firstUrl = page.url();
    // Back returns to where the click was: the click pushed, it did not
    // replace. The founded session stays founded — a click is a genesis and
    // there is no undo short of Discard, which is its own scenario.
    await page.goBack({ waitUntil: "domcontentloaded" });
    await expect(
      page.getByTestId("coding-session-founded-workspace-founded"),
    ).toHaveCount(0);

    await openWorkspaceDraft(page);
    // A second page for a second session, same seeded state, and nothing
    // published by the round trip beyond the two geneses.
    expect(page.url()).not.toBe(firstUrl);
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(1);
    await expect(
      page.getByTestId("coding-session-workspace-reuse-path"),
    ).toHaveText(REUSE_PATH);
    await expect(page.getByTestId("coding-session-workdir-input")).toHaveValue(
      REUSE_PATH,
    );
    const signed = await signedEvents(page);
    expect(signed).toHaveLength(signedBefore + 2);
    expect(geneses(signed)).toHaveLength(2);
    expect(new Set(geneses(signed)).size).toBe(2);
    expect(createActions(signed)).toHaveLength(0);
  });

  test("09 — a reload founds nothing, and nothing leaks after it", async ({
    page,
  }) => {
    await boot(page, { stub: availableStub() });

    await openWorkspaceDraft(page);
    // The founding request does not survive a reload — a request that did
    // would found a second genesis for the same click. The route survives
    // (hash history), so the page reopens on the same session; the mock
    // relay is in-memory, so what it shows after the reload is not this
    // test's claim. What is: no click, no genesis.
    await page.reload({ waitUntil: "domcontentloaded" });
    await expect(page).toHaveURL(
      /#\/coding-sessions\/[0-9a-f-]{36}\/founded\/[0-9a-f-]{36}/,
    );
    await expect(page.getByTestId("coding-session-header")).toBeVisible({
      timeout: 20_000,
    });
    await expect
      .poll(async () => (await recordedCommands(page)).length, {
        timeout: 10_000,
      })
      .toBeGreaterThan(0);
    expect(geneses(await signedEvents(page))).toHaveLength(0);
    expect(createActions(await signedEvents(page))).toHaveLength(0);

    // A later ordinary page is a clean page: the workspace does not
    // resurrect out of anything the earlier request touched.
    await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
    await openOrdinaryProjectDraft(page);
    expect(geneses(await signedEvents(page))).toHaveLength(1);
    await expect(
      page.getByTestId("coding-session-workspace-reuse"),
    ).toHaveCount(0);
    await expect(
      page.getByTestId("coding-session-workdir-input"),
    ).not.toHaveValue(REUSE_PATH);
  });

  test("10 — the open session's header offers the same action", async ({
    page,
  }) => {
    await boot(page, { stub: availableStub() });
    await page.getByTestId("project-coding-session-row").first().click();
    await expect(page.getByTestId("coding-session-workspace")).toBeVisible({
      timeout: 20_000,
    });
    // `CodingSessionHeaderOverflow` is rendered only when the header is in
    // Mission mode (`CodingSessionHeader.tsx:572`, `missionActions`), which
    // `CodingSessionUmbrellaWorkspace.tsx:211` sets to `isMultiExecution &&
    // lens === "mission"`. An ordinary single-execution session therefore has
    // no overflow at all — if this assertion fails, the header entry point is
    // unreachable for exactly the sessions most likely to want it, and that
    // is a surface gap, not a fixture one.
    const overflow = page.getByTestId("coding-session-overflow");
    await expect(
      overflow,
      "the open session's header must offer this action; the overflow is Mission-only today",
    ).toBeVisible({ timeout: 10_000 });
    await overflow.click();
    const item = page.getByTestId(
      "coding-session-overflow-new-session-in-workspace",
    );
    await expect(item).toBeVisible();
    // One label in one constant: the two entry points cannot drift.
    await expect(item).toContainText(MENU_LABEL);
    await item.click();
    await expectFoundedPage(page);
    await expectWorkspaceDraftState(page);
  });

  test("11 — screenshots: standard, narrow, keyboard focus at 250%, unavailable", async ({
    page,
  }) => {
    await boot(page, { stub: availableStub() });
    await openWorkspaceDraft(page);
    const card = page.getByTestId("coding-session-founded-setup-card");
    // The shot is named for the state it actually shows. A picture of the
    // ordinary page filed as "reuse draft" would be this pack telling the
    // reviewer the feature works — `expectWorkspaceDraftState` asserts which
    // of the two states this build is in, and the name follows it.
    await expectWorkspaceDraftState(page);
    const draftShot = WORKSPACE_REUSE_SEAM_LANDED
      ? "01-reuse-draft"
      : "01-unseeded-draft";
    await captureLocator(page, card, SHOTS, draftShot);

    await page.setViewportSize({ width: 640, height: 900 });
    await captureLocator(page, card, SHOTS, `${draftShot}-narrow`);
    await page.setViewportSize({ width: 1280, height: 1600 });

    // Keyboard focus on the menu item, at 250% of the root font size — the
    // zoom the desktop app implements by scaling `<html>`.
    await page.evaluate(() => {
      document.documentElement.style.fontSize = "40px";
    });
    await seededSessionRow(page).click({ button: "right" });
    const item = page.getByRole("menuitem", { name: MENU_LABEL });
    await expect(item).toBeVisible({ timeout: 10_000 });
    await item.focus();
    await expect(item).toBeFocused();
    await captureLocator(
      page,
      page.getByRole("menu").first(),
      SHOTS,
      "03-menu-focus-250",
    );
    await page.keyboard.press("Escape");
    await page.evaluate(() => {
      document.documentElement.style.fontSize = "";
    });
  });

  test("13 — the menu names other sessions recorded in the same folder", async ({
    page,
  }) => {
    await boot(page, {
      stub: availableStub({ seatWorktrees: [AVAILABLE_ROW, SAME_FOLDER_ROW] }),
    });

    const item = await openWorkspaceMenu(page);
    // Two rows, one path: the count is of rows this computer already read, so
    // it says "1 other session" and names no machine but this one. It is a
    // disclosure, not a lock — the item stays enabled and still opens a draft.
    await expect(item).toContainText(
      "1 other session on this computer recorded a tree in this exact folder.",
    );
    await expect(item).toContainText(MENU_LABEL);
    await expect(item).toBeEnabled();
  });

  test("12 — screenshot: the unavailable state", async ({ page }) => {
    await boot(page, {
      stub: defaultStubConfig({
        seatWorktrees: [AVAILABLE_ROW],
        validationByPath: {
          [REUSE_PATH]: { exists: false, isDir: false, isAbsolute: true },
          [CANONICAL_REPO]: { exists: true, isDir: true, isAbsolute: true },
        },
      }),
    });
    await seededSessionRow(page).click({ button: "right" });
    const item = page.getByRole("menuitem", { name: MENU_LABEL });
    await expect(item).toBeVisible({ timeout: 10_000 });
    await captureLocator(
      page,
      page.getByRole("menu").first(),
      SHOTS,
      "04-unavailable",
    );
  });
});
