// Manage Host's page against fixtures: layout, states and the actions it sends.
//
// The same bridge as host-manage-ui-check.mjs, with the menu bar binary replaced by
// the fixtures below. No Host, no relay, no `bee`, no key and no model: this is for
// what a page alone decides. The walkthrough that proves the actions work is
// `scripts/host-manage-check.py --ui`.
//
// The page is served with the CSP from tauri.conf.json, which a file:// page does
// not get, so a style or script the real window would refuse fails here.
//
//   NODE_PATH=<desktop/node_modules> node scripts/host-manage-ui-fixture-check.mjs
//
// HM_BROWSER: chromium (default) or webkit. HM_SHOTS: where to save screenshots.
// HM_PAGE_DIR with HM_SHOTS_ONLY=1: an earlier page, screenshotted and not checked.
import fs from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(path.join(process.env.NODE_PATH, "x.js"));
const playwright = require("@playwright/test");

const here = path.dirname(fileURLToPath(import.meta.url));
const crate = path.join(here, "../desktop/src-tauri/crates/beekeeper-menubar");
const pageDir = process.env.HM_PAGE_DIR || path.join(crate, "frontend");
const csp = JSON.parse(fs.readFileSync(path.join(crate, "tauri.conf.json"))).app
  .security.csp;
const shots = process.env.HM_SHOTS;
const shotsOnly = process.env.HM_SHOTS_ONLY === "1";
if (shots) fs.mkdirSync(shots, { recursive: true });

// ── Fixtures ─────────────────────────────────────────────────────────────────

const hex = (seed) => seed.repeat(64).slice(0, 64);
const OWNER = hex("3d3b");
const CHANNEL = "0881bfba-0000-4000-8000-00000000c001";

const longRuntimeDetail =
  "The adapter answered with this list. The account behind it is unconfirmed until a signed turn result.";

const runtime = {
  instanceRef: "claude-primary",
  runtime: "claude",
  driver: "claude-acp",
  commissionedDefault: "default",
  commissionedModels: ["default"],
  discoverModels: true,
  credentialEnv: [],
  readiness: "adapter_answered",
  readinessDetail: longRuntimeDetail,
  selectableModels: [
    {
      id: "default",
      name: "Default (recommended)",
      efforts: ["low", "medium", "high"],
      isDefault: true,
      isLabel: true,
    },
    {
      id: "claude-a-model-with-a-long-identifier-1m",
      name: "A Model With A Very Long Display Name For A Narrow Menu",
      efforts: [],
      isDefault: false,
      isLabel: false,
    },
    {
      id: "sonnet",
      name: "Sonnet",
      efforts: ["low", "high"],
      isDefault: false,
      isLabel: false,
    },
  ],
  commissionedNotOffered: [],
};

function setupRow(name, effectiveState, extra = {}) {
  const setupId = `${name.toLowerCase().replaceAll(" ", "-")}-id`;
  const seed = setupId.charCodeAt(0).toString(16);
  return {
    effectiveState,
    executionHeld: extra.executionHeld || false,
    setup: {
      setupId,
      spec: {
        project: `30621:${OWNER}:everyday-workspace`,
        channel: CHANNEL,
        role: "lead",
        displayName: name,
        providerInstance: "claude-primary",
        model: "sonnet",
        parallelism: 1,
        access: { mode: "owner_only" },
        ...extra.spec,
      },
      agentPubkey: extra.noIdentity ? undefined : hex(seed),
      projectContext: "bound",
      steps: extra.steps || [],
      attempts: extra.attempts || [],
      pendingAction: extra.pendingAction,
      modelChange: extra.modelChange,
      lastRefusal: extra.lastRefusal,
      readback: extra.noIdentity
        ? undefined
        : {
            definition: {
              eventId: hex("3a57"),
              model: extra.spec?.effort
                ? `${extra.spec.model}[${extra.spec.effort}]`
                : extra.spec?.model || "sonnet",
              respondTo: "owner-only",
            },
            provider: { model: "sonnet", status: "idle" },
            turnEvidence:
              "This reading does not show a turn. Only a signed turn result proves the runtime's account works.",
          },
    },
  };
}

const setups = {
  running: setupRow("Workspace Lead", "running", {
    spec: { model: "sonnet", effort: "low" },
  }),
  stopped: setupRow("Manage Host Acceptance", "stopped"),
  allowlist: setupRow("Shared Reviewer", "stopped", {
    spec: {
      role: "reviewer",
      access: { mode: "allowlist", pubkeys: [hex("aa"), hex("bb")] },
    },
  }),
  attention: setupRow("Interrupted Agent", "needs_attention", {
    lastRefusal: {
      code: "MODEL_AUTH_UNAVAILABLE",
      message: "The runtime's credential was not available.",
      uncertain: true,
    },
    pendingAction: { action: "start", attempted: false },
  }),
  stopHeld: setupRow("Stop Held Agent", "interrupted", {
    pendingAction: { action: "stop", attempted: false },
    executionHeld: true,
  }),
  unknownStop: setupRow("Unknown Outcome Agent", "running", {
    pendingAction: { action: "start", attempted: true },
  }),
  applying: setupRow("Applying Agent", "applying", {
    steps: [{ step: "install", state: "started", at: "now" }],
    noIdentity: true,
  }),
  planned: setupRow("Planned Agent", "planned", { noIdentity: true }),
  cancelledHeld: setupRow("Cancelled Still Running", "cancelled_held", {
    executionHeld: true,
  }),
  switched: (() => {
    // After `bee host manage set-model`: the spec moved, the saved reading did not.
    const row = setupRow("Switched Agent", "running", {
      spec: { model: "opus", effort: "high" },
    });
    row.setup.readback.readAt = "2026-11-04T10:00:00Z";
    row.setup.readback.definition.model = "sonnet";
    return row;
  })(),
  changing: setupRow("Changing Agent", "running", {
    modelChange: { model: "opus", operationId: "op" },
  }),
  notHeld: setupRow("Closed Session Agent", "not_held"),
  providerDown: setupRow("Waiting Agent", "provider_not_running"),
};

function inventory(overrides = {}) {
  const rows = overrides.setups || [
    setups.running,
    setups.stopped,
    setups.allowlist,
    setups.attention,
  ];
  return {
    readAt: "2026-11-04T00:00:00Z",
    host: {
      instance: "dev",
      hostPid: 1,
      hostVersion: "fixture",
      relayUrl: "wss://relay.example.test",
      providerPubkey: hex("19"),
      ownerPubkey: OWNER,
      provider: { state: "live", pid: 2 },
      relayConnection: { state: "unknown", why: "the Host does not hold it" },
      ...overrides.host,
    },
    runtimes: overrides.runtimes || [runtime],
    projects: [
      {
        coordinate: `30621:${OWNER}:everyday-workspace`,
        checkoutPresent: true,
        agentsRepoPresent: true,
        usable: true,
      },
    ],
    agents: [
      ...rows
        .filter((row) => row.setup.agentPubkey)
        .map((row) => ({
          actor: row.setup.agentPubkey,
          displayName: row.setup.spec.displayName,
          roles: ["lead"],
          runtimes: ["claude-primary"],
          projects: [row.setup.spec.project],
          sessionCount: 3,
          openSessions: row.effectiveState === "running" ? 1 : 0,
          turnsInFlight: 0,
          priorOpenSessions: row.setup.spec.displayName.startsWith("Workspace")
            ? 3
            : 0,
          priorTurnsInFlight: 0,
          custody: "host_enrolled",
          setupId: row.setup.setupId,
        })),
      {
        actor: hex("ed"),
        roles: ["builder"],
        runtimes: ["claude-primary"],
        projects: [`30621:${OWNER}:other-project`],
        sessionCount: 9,
        openSessions: 2,
        turnsInFlight: 1,
        priorOpenSessions: 0,
        priorTurnsInFlight: 0,
        custody: "sessions_only",
      },
    ],
    setups: rows,
    recovery: overrides.recovery || {
      rows: [
        {
          commandId: "c",
          actor: hex("ab"),
          role: "builder",
          projectRef: `30621:${OWNER}:other-project`,
          sessionId: "aaaaaaaa-0000-4000-8000-000000000001",
          generation: 1,
          stopCommand:
            "bee sessions stop --channel 11111111-2222-3333-4444-555555555555 --session aaaaaaaa",
          fenced: false,
          projectTrustedHere: true,
          sessionPresent: true,
          code: "SEAT_CUSTODY_PENDING",
          remedy: "Nothing here can finish this. Stop it with its own command.",
        },
      ],
      writeOperation: "No button here: custody is written by the provider.",
    },
    warnings: overrides.warnings,
  };
}

// ── Bridge ───────────────────────────────────────────────────────────────────

const server = http.createServer((request, response) => {
  const name = path.basename(new URL(request.url, "http://x").pathname);
  const file = path.join(pageDir, name);
  if (!fs.existsSync(file)) {
    response.writeHead(404).end();
    return;
  }
  const types = {
    ".html": "text/html",
    ".css": "text/css",
    ".js": "text/javascript",
  };
  response.writeHead(200, {
    "Content-Type": `${types[path.extname(name)] || "text/plain"}; charset=utf-8`,
    "Content-Security-Policy": csp,
  });
  response.end(fs.readFileSync(file));
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;

const browserName = process.env.HM_BROWSER || "chromium";
const browser = await playwright[browserName].launch();

let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(
    `${ok ? "UI PASS" : "UI FAIL"} ${name}${ok ? "" : ` :: ${JSON.stringify(detail).slice(0, 600)}`}`,
  );
  if (!ok) failures += 1;
};

async function openPage({
  world = () => ({ ok: true, value: inventory() }),
  width = 1440,
  height = 900,
  colorScheme = "dark",
  context: shared,
  signer = { matches: true },
} = {}) {
  const context =
    shared ||
    (await browser.newContext({
      viewport: { width, height },
      colorScheme,
    }));
  const page = await context.newPage();
  const session = { page, context, actions: [], problems: [], world };
  page.__session = session;
  page.on("pageerror", (error) => session.problems.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("favicon"))
      session.problems.push(message.text());
  });
  await page.exposeFunction("__manageBridge", (name, args) => {
    if (name === "manage_inventory") return session.world();
    if (name === "manage_interrupt") return false;
    const action = args.action;
    session.actions.push(action);
    switch (action.kind) {
      case "signer":
        return {
          ok: true,
          value: {
            matches: signer.matches,
            signer: signer.matches ? OWNER : hex("99"),
            hostOwner: OWNER,
            source: "key file",
          },
        };
      case "channels":
        return {
          ok: true,
          value: [{ id: CHANNEL, name: "everyday-workspace", private: true }],
        };
      case "roles":
        return {
          ok: true,
          value: {
            lead: "lead",
            roles: [{ slug: "lead" }, { slug: "reviewer" }],
          },
        };
      case "preflight":
        return {
          ok: true,
          value: {
            checks: [
              {
                code: "ROLE_RESOLVED",
                ok: true,
                message: "The role resolves.",
              },
              {
                code: "CHANNEL_OWNED",
                ok: true,
                warning: true,
                message: "You own this channel, and it is empty.",
              },
            ],
          },
        };
      case "plan":
        return { ok: true, value: { setupId: "new-setup" } };
      case "inspect":
        return {
          ok: true,
          value: {
            effectiveState: "running",
            live: { availability: "verified_unexpired_provider_lease" },
          },
        };
      default:
        return { ok: true, value: {} };
    }
  });
  await page.addInitScript(() => {
    window.__TAURI__ = {
      core: { invoke: (name, args) => window.__manageBridge(name, args) },
    };
    window.__violations = [];
    document.addEventListener("securitypolicyviolation", (event) =>
      window.__violations.push(
        `${event.violatedDirective} ${event.blockedURI}`,
      ),
    );
  });
  await page.goto(`${origin}/manage-host.html`);
  await page.waitForFunction(
    () =>
      !document.getElementById("host-line").textContent.startsWith("Asking"),
  );
  return session;
}

// Playwright's WebKit screenshot injects a <style>, which the CSP rightly reports.
// That is the camera, not the page, so what a screenshot raises is not counted.
const shot = async (page, name, fullPage = true) => {
  if (!shots) return;
  const session = page.__session;
  const problems = session.problems.length;
  const violations = await page.evaluate(() => window.__violations.length);
  await page.screenshot({
    path: path.join(shots, `${name}.png`),
    fullPage,
    caret: "initial",
  });
  await page.evaluate((count) => {
    window.__violations.length = count;
  }, violations);
  session.problems.length = problems;
};
const byTest = (page, id) => page.locator(`[data-testid="${id}"]`);
const close = (session) => session.context.close();

// ── The earlier page, for a before picture only ──────────────────────────────

if (shotsOnly) {
  for (const [name, width, height] of [
    ["wide", 1909, 1040],
    ["narrow", 640, 900],
  ]) {
    const session = await openPage({ width, height });
    await shot(session.page, `before-${name}`, false);
    await close(session);
  }
  await browser.close();
  server.close();
  process.exit(0);
}

// ── Overview ─────────────────────────────────────────────────────────────────

{
  const session = await openPage({ width: 1909, height: 1040 });
  const { page } = session;
  await shot(page, "after-wide-overview", false);
  const main = await page.locator("main").boundingBox();
  check(
    "the page is centered and uses the wide window",
    main.width > 1400 && Math.abs(main.x - (1909 - main.x - main.width)) <= 2,
    main,
  );
  const rail = await page.locator(".rail").boundingBox();
  const pane = await page.locator(".agents-pane").boundingBox();
  check(
    "agents lead, with the owner key beside them",
    pane.x < rail.x && pane.width > rail.width,
    { pane, rail },
  );
  const cards = page.locator('[data-testid="setups"] .card');
  const first = await cards.nth(0).boundingBox();
  const second = await cards.nth(1).boundingBox();
  check(
    "a wide window shows cards side by side",
    Math.abs(first.y - second.y) < 2 && second.x > first.x,
    { first, second },
  );
  check(
    "the flow is closed until someone adds an agent",
    !(await page.locator("#setup-form").isVisible()),
  );
  const firstCard = await cards.nth(0).locator("h3").textContent();
  check(
    "a card that needs attention comes first",
    firstCard === "Interrupted Agent",
    firstCard,
  );
  const hostLine = await byTest(page, "host-line").textContent();
  check(
    "the header says what is known and what is not",
    /Provider running/.test(hostLine) &&
      /relay\.example\.test/.test(hostLine) &&
      /Relay link unknown to the Host/.test(hostLine) &&
      /Owner 3d3b3d3b/.test(hostLine),
    hostLine,
  );
  check(
    "nothing is read or signed by opening the window",
    session.actions.filter((action) => action.kind !== "roles").length === 0,
    session.actions,
  );
  check(
    "the real CSP refuses nothing on this page",
    (await page.evaluate(() => window.__violations)).length === 0,
    await page.evaluate(() => window.__violations),
  );
  await close(session);
}

// ── Each state offers what it should, and says why ───────────────────────────

const STATES = [
  ["running", setups.running, "running", ["Inspect", "Stop"], /fresh session/],
  ["stopped", setups.stopped, "stopped", ["Start", "Inspect"], /fresh session/],
  [
    "session closed",
    setups.notHeld,
    "not running: its session has closed",
    ["Start", "Inspect"],
    /fresh session/,
  ],
  [
    "needs attention",
    setups.attention,
    "needs attention",
    ["Retry", "Inspect", "Cancel"],
    null,
  ],
  [
    "stop held",
    setups.stopHeld,
    "interrupted, retry to resume",
    ["Retry stop", "Inspect", "Stop", "Cancel"],
    /fresh session/,
  ],
  [
    "an outcome unknown",
    setups.unknownStop,
    "running",
    ["Inspect"],
    /outcome is unknown/,
  ],
  ["applying", setups.applying, "working now", ["Interrupt"], null],
  ["planned", setups.planned, "planned", ["Apply", "Cancel"], null],
  [
    "cancelled and running",
    setups.cancelledHeld,
    "cancelled, and still running",
    ["Resume", "Inspect", "Stop"],
    /fresh session/,
  ],
  [
    "provider stopped",
    setups.providerDown,
    "not running: the session provider is stopped",
    ["Inspect"],
    /until the session provider is running/,
  ],
];
{
  const session = await openPage({
    world: () => ({
      ok: true,
      value: inventory({ setups: STATES.map((row) => row[1]) }),
    }),
    width: 1440,
    height: 1400,
  });
  const { page } = session;
  await page.fill("#key-file", "/fixture/owner-key");
  await page.click("#check-key");
  await byTest(page, "signer")
    .filter({ hasText: "this Host's owner" })
    .waitFor();
  await shot(page, "after-wide-all-states");
  for (const [name, row, badge, buttons, hint] of STATES) {
    const card = byTest(page, `setup-${row.setup.setupId}`);
    const seenBadge = await card
      .locator('[data-testid="setup-state"]')
      .textContent();
    const seenButtons = await card.locator("button").allTextContents();
    const hintNode = card.locator('[data-testid="setup-hint"]');
    const seenHint = (await hintNode.count())
      ? await hintNode.textContent()
      : null;
    check(`${name}: the badge says what it is`, seenBadge === badge, seenBadge);
    check(
      `${name}: it offers exactly its actions`,
      JSON.stringify(seenButtons) === JSON.stringify(buttons),
      seenButtons,
    );
    check(
      `${name}: the hint is the relevant one`,
      hint ? hint.test(seenHint || "") : seenHint === null,
      seenHint,
    );
  }
  const stop = byTest(page, `stop-${setups.running.setup.setupId}`);
  const start = byTest(page, `start-${setups.running.setup.setupId}`);
  check(
    "Stop is drawn as destructive and apart from the others",
    (await stop.getAttribute("class")).includes("danger") &&
      (await stop.getAttribute("class")).includes("apart"),
  );
  check("Start is not on a running card", (await start.count()) === 0);
  await close(session);
}

// ── Truth on a card ──────────────────────────────────────────────────────────

{
  const session = await openPage({ width: 1440, height: 1200 });
  const { page } = session;
  const card = byTest(page, `setup-${setups.running.setup.setupId}`);
  const text = await card.locator("dl.facts").textContent();
  check(
    "the model and effort read as requested, never as what ran",
    /sonnet\s*requested/.test(text) && /low\s*requested/.test(text),
    text,
  );
  check(
    "the list says what is unknown about the model and the effort",
    /Only a signed turn result names the model that ran, and no runtime reports the effort/.test(
      await byTest(page, "model-truth").textContent(),
    ),
  );
  const none = await byTest(page, `setup-${setups.stopped.setup.setupId}`)
    .locator("dl.facts")
    .textContent();
  check(
    "no effort chosen is not filled in with a guess",
    /None chosen/.test(none),
    none,
  );
  const aligned = await card
    .locator("dl.facts")
    .evaluate((list) =>
      [...list.children].every(
        (cell) =>
          cell.children[0].tagName === "DT" &&
          cell.children[1].tagName === "DD",
      ),
    );
  check("each fact keeps its name directly above its value", aligned);
  check(
    "owner only and an allowlist read as who may instruct, and count people",
    /Only the owner/.test(text) &&
      /Owner and 2 people/.test(
        await byTest(
          page,
          `setup-${setups.allowlist.setup.setupId}`,
        ).textContent(),
      ),
  );
  check(
    "an adopted identity's older sessions are named, and not offered to stop",
    /3 open from before adoption.*not this Host's to stop/.test(
      await card.locator('[data-testid="setup-prior"]').textContent(),
    ),
  );
  check(
    "the full pubkey is behind the disclosure",
    !(await card.locator('[data-testid="setup-identity"]').isVisible()),
  );
  await card.locator("summary", { hasText: "Technical details" }).click();
  check(
    "disclosed: the full identity, unshortened",
    (await card.locator('[data-testid="setup-identity"]').textContent())
      .length === 64,
  );
  check(
    "disclosed: the signed definition and what a reading does not prove",
    /Signed definition .*access owner-only.*Provider signed model sonnet.*does not show a turn/.test(
      await card.locator('[data-testid="setup-readback"]').textContent(),
    ),
  );
  await shot(page, "after-wide-technical-details");

  // A read of the same inventory must leave a person's place alone.
  await card.locator("summary", { hasText: "Technical details" }).focus();
  const before = await card.evaluate((node) => node.isConnected);
  await page.evaluate(() => document.getElementById("refresh").click());
  await page.waitForTimeout(200);
  check(
    "a read that changes nothing does not redraw",
    before && (await card.evaluate((node) => node.isConnected)),
  );
  // And a read that does change it keeps the disclosure and the focus.
  const start = byTest(page, `start-${setups.stopped.setup.setupId}`);
  await page.fill("#key-file", "/fixture/owner-key");
  await page.click("#check-key");
  await byTest(page, "signer")
    .filter({ hasText: "this Host's owner" })
    .waitFor();
  await start.focus();
  session.world = () => ({
    ok: true,
    value: inventory({
      setups: [
        setups.running,
        {
          ...setups.stopped,
          setup: { ...setups.stopped.setup, updatedAt: "later" },
        },
        setups.allowlist,
        setups.attention,
      ],
    }),
  });
  await page.evaluate(() => document.getElementById("refresh").click());
  await page.waitForTimeout(200);
  check(
    "a redraw keeps an open disclosure open",
    await card.locator('[data-testid="setup-identity"]').isVisible(),
  );
  check(
    "a redraw gives keyboard focus back",
    (await page.evaluate(() => document.activeElement?.dataset.testid)) ===
      `start-${setups.stopped.setup.setupId}`,
  );
  await close(session);
}

// ── A model change the saved reading has not caught up with ──────────────────

{
  const session = await openPage({
    world: () => ({
      ok: true,
      value: inventory({
        setups: [setups.switched, setups.running, setups.changing],
      }),
    }),
  });
  const { page } = session;
  const card = byTest(page, `setup-${setups.switched.setup.setupId}`);
  const facts = await card.locator("dl.facts").textContent();
  check(
    "after a switch the card reads the setup's current choice, not the old reading",
    /opus\s*requested/.test(facts) &&
      /high\s*requested/.test(facts) &&
      !/sonnet/.test(facts),
    facts,
  );
  await card.locator("summary", { hasText: "Technical details" }).click();
  check(
    "the old reading is dated and still shown",
    /Read 2026-11-04T10:00:00Z.*model sonnet/.test(
      await card.locator('[data-testid="setup-readback"]').textContent(),
    ),
  );
  check(
    "and is called out as older than the current choice, with the way to reread",
    /names sonnet.*now requests opus\[high\].*Inspect reads/.test(
      await card
        .locator('[data-testid="setup-readback-outdated"]')
        .textContent(),
    ),
  );
  await shot(page, "after-model-switched-reading");
  check(
    "a reading that agrees with an effort's spelling is not called outdated",
    (await byTest(page, `setup-${setups.running.setup.setupId}`)
      .locator('[data-testid="setup-readback-outdated"]')
      .count()) === 0,
  );
  const held = byTest(page, `setup-${setups.changing.setup.setupId}`);
  check(
    "a change still being recorded is disclosed on the card",
    /change to opus is still being recorded.*set-model/.test(
      await held.locator('[data-testid="setup-model-change"]').textContent(),
    ),
  );
  await close(session);
}

// ── Locked, then unlocked, by the owner key ──────────────────────────────────

{
  const session = await openPage({ width: 880, height: 780 });
  const { page, context } = session;
  await shot(page, "after-default-locked", false);
  const start = byTest(page, `start-${setups.stopped.setup.setupId}`);
  check("Start waits for the key", await start.isDisabled());
  check(
    "and says so on the button",
    (await start.getAttribute("title")) === "Check the owner key first.",
  );
  check(
    "and in words beside the actions",
    /need the owner key/.test(await byTest(page, "lock-note").textContent()),
  );
  check(
    "Cancel needs no key, so it stays available",
    await byTest(page, `cancel-${setups.attention.setup.setupId}`).isEnabled(),
  );
  await byTest(page, "key-status").click();
  check(
    "the header's key status goes to the field",
    await page.evaluate(() => document.activeElement?.id === "key-file"),
  );
  await byTest(page, "check-key-now").click();
  await byTest(page, "signer")
    .filter({ hasText: "this Host's owner" })
    .waitFor();
  check(
    "with no path, the check still runs and bee uses its default file",
    JSON.stringify(session.actions.find((a) => a.kind === "signer")) ===
      JSON.stringify({ kind: "signer", keyFile: null }),
    session.actions,
  );
  session.actions.length = 0;
  await page.fill("#key-file", "/fixture/owner-key");
  await byTest(page, "check-key-now").click();
  await byTest(page, "signer")
    .filter({ hasText: "this Host's owner" })
    .waitFor();
  const signer = session.actions.find((action) => action.kind === "signer");
  check(
    "the check sends only the path",
    JSON.stringify(signer) ===
      JSON.stringify({ kind: "signer", keyFile: "/fixture/owner-key" }),
    signer,
  );
  check("Start is available once it matches", await start.isEnabled());
  check(
    "the lock note is gone",
    !(await byTest(page, "lock-note").isVisible()),
  );
  check(
    "the header reads as checked",
    /Owner key checked/.test(await byTest(page, "key-status").textContent()),
  );

  await start.click();
  await page.waitForFunction(
    () => document.getElementById("result").textContent === "Done.",
  );
  check(
    "Start sends the same action it always did",
    JSON.stringify(session.actions.at(-1)) ===
      JSON.stringify({
        kind: "start",
        setup: setups.stopped.setup.setupId,
        keyFile: "/fixture/owner-key",
      }),
    session.actions.at(-1),
  );
  await byTest(page, `stop-${setups.running.setup.setupId}`).click();
  await page.waitForFunction(
    () => document.getElementById("result").textContent === "Done.",
  );
  check(
    "Stop sends the same action it always did",
    session.actions.at(-1).kind === "stop" &&
      session.actions.at(-1).setup === setups.running.setup.setupId,
    session.actions.at(-1),
  );
  await byTest(page, `inspect-${setups.running.setup.setupId}`).click();
  await byTest(page, "inspect-result").waitFor();
  check(
    "Inspect opens its reading on that agent's card",
    /verified_unexpired_provider_lease/.test(
      await byTest(page, `setup-${setups.running.setup.setupId}`)
        .locator('[data-testid="inspect-result"]')
        .textContent(),
    ),
  );

  // The path is remembered. The key is not read again without being asked.
  const signersBefore = session.actions.filter(
    (a) => a.kind === "signer",
  ).length;
  const again = await openPage({ context });
  check(
    "a matched path is remembered for the next window",
    (await again.page.inputValue("#key-file")) === "/fixture/owner-key",
  );
  check(
    "a remembered path is not checked until someone asks",
    again.actions.every((action) => action.kind !== "signer") &&
      session.actions.filter((a) => a.kind === "signer").length ===
        signersBefore &&
      (await byTest(
        again.page,
        `start-${setups.stopped.setup.setupId}`,
      ).isDisabled()),
  );
  await close(session);

  const wrong = await openPage({ signer: { matches: false } });
  await wrong.page.fill("#key-file", "/fixture/not-the-owner");
  await wrong.page.click("#check-key");
  await byTest(wrong.page, "signer")
    .filter({ hasText: "OWNER_SIGNER_MISMATCH" })
    .waitFor();
  check(
    "a key that is not the owner's keeps changes locked and says so",
    (await byTest(
      wrong.page,
      `start-${setups.stopped.setup.setupId}`,
    ).isDisabled()) &&
      /not this Host's owner/.test(
        await byTest(wrong.page, "lock-note").textContent(),
      ),
  );
  await shot(wrong.page, "after-wrong-key", false);
  const reopened = await openPage({ context: wrong.context });
  check(
    "a key that did not match is not remembered",
    (await reopened.page.inputValue("#key-file")) === "",
  );
  await close(wrong);
}

// ── Add agent ────────────────────────────────────────────────────────────────

for (const [label, width, height] of [
  ["wide", 1440, 1000],
  ["narrow", 620, 900],
]) {
  const session = await openPage({ width, height });
  const { page } = session;
  await byTest(page, "add-agent").click();
  check(
    `${label}: Add agent opens with focus in the form`,
    await page.evaluate(() => document.activeElement?.id === "project"),
  );
  check(
    `${label}: Add agent is exposed as expanded`,
    (await byTest(page, "add-agent").getAttribute("aria-expanded")) === "true",
  );
  await page
    .locator('#role option[value="lead"]')
    .waitFor({ state: "attached" });
  const blocked = await byTest(page, "setup-blocked").textContent();
  check(
    `${label}: a disabled Set up names what is still needed`,
    (await byTest(page, "set-up").isDisabled()) &&
      /check the owner key/.test(blocked) &&
      /name the agent/.test(blocked),
    blocked,
  );
  if (label === "wide") {
    const flow = await byTest(page, "add-flow").boundingBox();
    const group = await page.locator("fieldset.group").nth(1).boundingBox();
    check(
      "wide: the form's two groups use the whole panel",
      flow.x + flow.width - (group.x + group.width) < 40,
      { flow, group },
    );
  }
  await shot(page, `after-${label}-add-agent-locked`);

  await page.fill("#key-file", "/fixture/owner-key");
  await page.click("#check-key");
  await byTest(page, "signer")
    .filter({ hasText: "this Host's owner" })
    .waitFor();
  await page.fill("#display-name", "New Agent");

  // A choice that decides what runs must be readable where it is made.
  const fits = await page.evaluate(() => {
    const canvas = document.createElement("canvas").getContext("2d");
    const select = document.getElementById("model");
    const style = getComputedStyle(select);
    canvas.font = `${style.fontSize} ${style.fontFamily}`;
    return [...select.options].map((option) => ({
      label: option.textContent,
      needs: Math.ceil(canvas.measureText(option.textContent).width),
      has:
        select.clientWidth -
        Number.parseFloat(style.paddingLeft) -
        Number.parseFloat(style.paddingRight),
    }));
  });
  const widest = fits.reduce((a, b) => (a.needs > b.needs ? a : b));
  const defaultLabel = fits.find((row) => row.label.startsWith("Default"));
  check(
    `${label}: the provider's default model label fits its menu`,
    defaultLabel.needs <= defaultLabel.has,
    defaultLabel,
  );
  // A label too long for its menu is still read in full, beneath it.
  await page.selectOption("#model", "claude-a-model-with-a-long-identifier-1m");
  const written = await byTest(page, "model-full").isVisible();
  check(
    `${label}: the longest label is in its menu or written out beneath it`,
    widest.needs <= widest.has
      ? !written
      : written &&
          (await byTest(page, "model-full").textContent()) === widest.label,
    { widest, written },
  );
  await page.selectOption("#model", "default");
  check(
    `${label}: a label that fits is not written twice`,
    !(await byTest(page, "model-full").isVisible()),
  );
  check(
    `${label}: the default model label does not repeat itself`,
    defaultLabel.label === "Default (recommended), provider default",
    defaultLabel,
  );
  check(
    `${label}: an effort is required when the model lists them, and none is preselected`,
    (await page.inputValue("#effort")) === "" &&
      (await byTest(page, "set-up").isDisabled()) &&
      /choose an effort/.test(
        await byTest(page, "setup-blocked").textContent(),
      ),
  );
  check(
    `${label}: \`default\` is called out as no particular model`,
    /names no model/.test(await byTest(page, "model-note").textContent()),
  );
  await page.selectOption("#effort", "low");
  check(
    `${label}: complete choices enable Set up`,
    await byTest(page, "set-up").isEnabled(),
  );

  await byTest(page, "access-allowlist").check();
  check(
    `${label}: an allowlist states it grants no channel access`,
    /grants no access/.test(await byTest(page, "allowlist-note").textContent()),
  );
  check(
    `${label}: who may instruct is never described as channel access`,
    /never changes who can post in the channel/.test(
      await page.locator("#access-hint").textContent(),
    ),
  );
  await page.fill("#allowlist", `${hex("aa")}\n${hex("bb")}`);
  await byTest(page, "preflight").click();
  await byTest(page, "preflight-checks").waitFor();
  check(
    `${label}: a check's result stays beside the form`,
    await page.evaluate(
      () =>
        document.getElementById("result").parentElement.id ===
        "flow-result-slot",
    ),
  );
  await shot(page, `after-${label}-add-agent-ready`);
  const preflight = session.actions.at(-1);
  check(
    `${label}: Check choices sends the spec it always did`,
    preflight.kind === "preflight" &&
      preflight.keyFile === "/fixture/owner-key" &&
      preflight.spec.displayName === "New Agent" &&
      preflight.spec.channel === CHANNEL &&
      preflight.spec.role === "lead" &&
      preflight.spec.model === "default" &&
      preflight.spec.effort === "low" &&
      preflight.spec.parallelism === 1 &&
      preflight.spec.providerInstance === "claude-primary" &&
      preflight.spec.access.mode === "allowlist" &&
      preflight.spec.access.pubkeys.length === 2,
    preflight,
  );

  await page.locator("summary", { hasText: "Add a project" }).click();
  await shot(page, `after-${label}-add-project-or-channel`);

  await byTest(page, "set-up").click();
  await page.waitForFunction(() =>
    document.getElementById("result").textContent.startsWith("Running."),
  );
  await page.waitForFunction(() => document.getElementById("add-flow").hidden);
  check(
    `${label}: plan then apply, with the setup the plan named`,
    JSON.stringify(session.actions.slice(-2).map((a) => a.kind)) ===
      JSON.stringify(["plan", "apply"]) &&
      session.actions.at(-1).setup === "new-setup",
    session.actions.slice(-2),
  );
  check(
    `${label}: the flow closes and its result moves above the agents`,
    await page.evaluate(
      () =>
        document.getElementById("result").parentElement.id === "result-home" &&
        !document.getElementById("result-home").hidden,
    ),
  );
  check(
    `${label}: focus returns to Add agent`,
    await page.evaluate(() => document.activeElement?.id === "add-agent"),
  );
  await byTest(page, "add-agent").click();
  await page.keyboard.press("Escape");
  check(
    `${label}: Escape closes the flow and returns focus`,
    await page.evaluate(
      () =>
        document.getElementById("add-flow").hidden &&
        document.activeElement?.id === "add-agent",
    ),
  );
  check(
    `${label}: the page raised no error`,
    session.problems.length === 0,
    session.problems,
  );
  await close(session);
}

// ── Windows narrower than the design and states without agents ───────────────

for (const [label, width, height] of [
  ["minimum window", 620, 480],
  ["narrow", 640, 900],
  ["default window", 880, 780],
  ["between breakpoints", 1000, 780],
]) {
  const session = await openPage({ width, height });
  const { page } = session;
  await page.waitForTimeout(50);
  const widths = await page.evaluate(() => ({
    scroll: document.documentElement.scrollWidth,
    client: document.documentElement.clientWidth,
  }));
  check(
    `${label}: no sideways scrolling`,
    widths.scroll <= widths.client,
    widths,
  );
  const overflowing = await page.evaluate(() =>
    [...document.querySelectorAll("button, select, input, h3, .badge")]
      .filter((node) => {
        if (!node.offsetParent || node.closest(".scroll")) return false;
        const box = node.getBoundingClientRect();
        return box.right > document.documentElement.clientWidth + 0.5;
      })
      .map((node) => node.textContent || node.id),
  );
  check(
    `${label}: no control leaves the window`,
    overflowing.length === 0,
    overflowing,
  );
  if (label === "narrow") await shot(page, "after-narrow-overview");
  if (label === "default window") await shot(page, "after-default-overview");
  await close(session);
}

{
  const session = await openPage({
    world: () => ({
      ok: true,
      value: inventory({
        setups: [],
        recovery: { rows: [], writeOperation: "" },
      }),
    }),
  });
  const { page } = session;
  check(
    "no agents: an honest empty state",
    await byTest(page, "no-agents").isVisible(),
  );
  check(
    "no agents: nothing claims a model truth with no model",
    !(await byTest(page, "model-truth").isVisible()),
  );
  await shot(page, "after-empty", false);
  await byTest(page, "add-agent-empty").click();
  check(
    "no agents: its button opens the flow",
    await page.locator("#setup-form").isVisible(),
  );
  await close(session);

  const down = await openPage({
    world: () => ({
      ok: false,
      code: "HOST_UNAVAILABLE",
      message: "The Host's socket did not answer.",
    }),
  });
  check(
    "Host away: the header says nothing is current",
    /nothing below is current/.test(
      await byTest(down.page, "host-line").textContent(),
    ),
  );
  check(
    "Host away: its refusal is named",
    /HOST_UNAVAILABLE/.test(
      await byTest(down.page, "host-refusal").textContent(),
    ),
  );
  check(
    "Host away: no agent list that could be read as current",
    (await down.page.locator('[data-testid="setups"] .card').count()) === 0,
  );
  await shot(down.page, "after-host-away", false);
  await close(down);

  const stopped = await openPage({
    world: () => ({
      ok: true,
      value: inventory({
        host: {
          provider: { state: "notSupervised" },
          ownerPubkey: undefined,
          ownerUnverified: "no attestation was readable",
        },
        setups: [setups.providerDown],
        runtimes: [
          {
            ...runtime,
            readiness: "provider_not_live",
            readinessDetail:
              "The session provider is not running, so nothing is on offer now.",
            selectableModels: [],
          },
        ],
        warnings: [
          {
            code: "JOURNAL_MALFORMED",
            message: "One setup's journal could not be read.",
          },
        ],
      }),
    }),
  });
  const line = await byTest(stopped.page, "host-line").textContent();
  check(
    "provider stopped: the header says so and the owner is unverified, with why",
    /Provider stopped/.test(line) &&
      /Owner unverified/.test(line) &&
      /no attestation was readable/.test(
        await byTest(stopped.page, "notices").textContent(),
      ),
    line,
  );
  check(
    "provider stopped: a Host warning is shown",
    /JOURNAL_MALFORMED/.test(
      await byTest(stopped.page, "notices").textContent(),
    ),
  );
  check(
    "provider stopped: the runtime offers nothing",
    /Nothing on offer now/.test(
      await byTest(stopped.page, "runtimes").textContent(),
    ),
  );
  await shot(stopped.page, "after-provider-stopped", false);
  await close(stopped);
}

// ── Keyboard, labels and announcements ───────────────────────────────────────

{
  const session = await openPage({ width: 1440, height: 1000 });
  const { page } = session;
  const running = byTest(page, `setup-${setups.running.setup.setupId}`);

  // Every control, in the order Tab reaches it, by keyboard alone.
  const reached = [];
  await page.evaluate(() => document.body.focus());
  for (let i = 0; i < 40; i += 1) {
    await page.keyboard.press("Tab");
    const at = await page.evaluate(() => {
      const node = document.activeElement;
      if (!node || node === document.body) return null;
      const style = getComputedStyle(node);
      return {
        id: node.id || node.dataset.testid || node.tagName,
        tag: node.tagName,
        focusRing:
          style.outlineStyle !== "none" &&
          Number.parseFloat(style.outlineWidth) >= 2,
      };
    });
    if (!at) break;
    reached.push(at);
  }
  const ids = reached.map((row) => row.id);
  for (const id of [
    "key-status",
    "refresh",
    "add-agent",
    "key-file",
    "check-key",
  ]) {
    check(`Tab reaches ${id}`, ids.includes(id), ids);
  }
  check(
    "Tab reaches a disclosure and an enabled agent action",
    reached.some((row) => row.tag === "SUMMARY") &&
      ids.includes(`cancel-${setups.attention.setup.setupId}`),
    ids,
  );
  const unringed = reached.filter((row) => !row.focusRing).map((row) => row.id);
  check(
    "every control shows a visible focus ring",
    unringed.length === 0,
    unringed,
  );
  check(
    "the order starts in the header and reaches the agents before the owner key",
    ids.indexOf("add-agent") <
      ids.indexOf(`cancel-${setups.attention.setup.setupId}`) &&
      ids.indexOf(`cancel-${setups.attention.setup.setupId}`) <
        ids.indexOf("key-file"),
    ids,
  );

  // A disclosure opens from the keyboard.
  const summary = running.locator("summary", { hasText: "Technical details" });
  await summary.focus();
  await page.keyboard.press("Enter");
  check(
    "Enter opens Technical details",
    await running.locator('[data-testid="setup-identity"]').isVisible(),
  );

  // Names a screen reader would read, not only what the eye can place.
  check(
    "each agent's card is a named landmark item",
    (await page
      .getByRole("article", { name: setups.running.setup.spec.displayName })
      .count()) === 1,
  );
  check(
    "an action names the agent it acts on",
    (await page
      .getByRole("button", {
        name: `Stop ${setups.running.setup.spec.displayName}`,
      })
      .count()) === 1,
  );
  check(
    "the owner key field has a label",
    (await page.getByRole("textbox", { name: /Key file/ }).count()) === 1,
  );
  await byTest(page, "add-agent").click();
  for (const [role, name] of [
    ["combobox", /^Project/],
    ["combobox", /^Role/],
    ["combobox", /^Channel/],
    ["combobox", /^Runtime/],
    ["combobox", /^Model/],
    ["textbox", /^Agent name/],
    ["radio", /Only the owner/],
    ["radio", /The owner and these people/],
  ]) {
    check(
      `the form names its ${role} ${name}`,
      (await page.getByRole(role, { name }).count()) === 1,
    );
  }
  check(
    "the instruct choice is a named radio group",
    (await page
      .getByRole("radiogroup", { name: "Who may instruct it" })
      .count()) === 1,
  );
  check(
    "Set up is described by what it still needs",
    (await byTest(page, "set-up").getAttribute("aria-describedby")) ===
      "setup-blocked",
  );
  await byTest(page, "close-add").click();
  await close(session);

  // Status reaches an announcer, without a person looking at the card.
  let world = inventory({ setups: [setups.stopped] });
  const live = await openPage({ world: () => ({ ok: true, value: world }) });
  const announced = () => byTest(live.page, "announcer").textContent();
  check(
    "the announcer is a polite status region",
    (await byTest(live.page, "announcer").getAttribute("role")) === "status",
  );
  check(
    "opening the window announces no state change",
    (await announced()) === "",
  );
  world = inventory({
    setups: [{ ...setups.stopped, effectiveState: "running" }],
  });
  await live.page.click("#refresh");
  await byTest(live.page, "announcer").filter({ hasText: "running" }).waitFor();
  check(
    "a state change is announced with the agent's name",
    /Manage Host Acceptance: running/.test(await announced()),
    await announced(),
  );
  check(
    "the result area and the key result are live regions",
    (await byTest(live.page, "result").getAttribute("aria-live")) ===
      "polite" &&
      (await byTest(live.page, "signer").getAttribute("role")) === "status",
  );
  await close(live);

  const down = await openPage({
    world: () => ({
      ok: false,
      code: "HOST_UNAVAILABLE",
      message: "No answer.",
    }),
  });
  check(
    "a Host refusal is an alert",
    (await byTest(down.page, "host-refusal").getAttribute("role")) === "alert",
  );
  await close(down);

  const wrong = await openPage({ signer: { matches: false } });
  await wrong.page.fill("#key-file", "/fixture/not-the-owner");
  await wrong.page.click("#check-key");
  await byTest(wrong.page, "signer")
    .filter({ hasText: "OWNER_SIGNER_MISMATCH" })
    .waitFor();
  check(
    "a key that is not the owner's says so in words, not only in red",
    /not the owner's/.test(
      await byTest(wrong.page, "key-status").textContent(),
    ),
  );
  await close(wrong);
}

{
  // Meaning carried by colour alone would disappear here.
  const session = await openPage({ width: 1440, height: 1000 });
  const words = await session.page.evaluate(() =>
    [...document.querySelectorAll(".badge, .host-fact.state, .key-status")].map(
      (node) => node.textContent.trim(),
    ),
  );
  check(
    "every coloured state also carries a word",
    words.length > 0 && words.every(Boolean),
    words,
  );
  await close(session);
}

// ── Contrast ─────────────────────────────────────────────────────────────────

for (const colorScheme of ["dark", "light"]) {
  const session = await openPage({ colorScheme, width: 1440, height: 1200 });
  const { page } = session;
  await shot(page, `after-wide-${colorScheme}`, false);
  const failing = await page.evaluate(() => {
    const parse = (text) =>
      text
        .match(/[\d.]+/g)
        .slice(0, 4)
        .map(Number);
    const channel = (value) => {
      const v = value / 255;
      return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    };
    const luminance = ([r, g, b]) =>
      0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
    const background = (node) => {
      for (let at = node; at; at = at.parentElement) {
        const color = parse(getComputedStyle(at).backgroundColor);
        if (color.length < 4 || color[3] > 0) return color;
      }
      return parse(getComputedStyle(document.body).backgroundColor);
    };
    const rows = [];
    for (const node of document.querySelectorAll(
      "h1, h2, h3, p, dt, dd, button, .badge, summary, label, .host-fact, .tag",
    )) {
      if (!node.offsetParent || !node.textContent.trim()) continue;
      const fg = luminance(parse(getComputedStyle(node).color));
      const bg = luminance(background(node));
      const ratio = (Math.max(fg, bg) + 0.05) / (Math.min(fg, bg) + 0.05);
      if (ratio < 4.5)
        rows.push(
          `${node.tagName}.${node.className}: ${ratio.toFixed(2)} "${node.textContent.slice(0, 30)}"`,
        );
    }
    return rows;
  });
  check(
    `${colorScheme}: all visible text meets 4.5:1`,
    failing.length === 0,
    failing,
  );
  await close(session);
}

await browser.close();
server.close();
console.log(failures ? `${failures} failed` : "all passed");
process.exit(failures ? 1 : 0);
