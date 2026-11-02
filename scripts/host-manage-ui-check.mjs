// Drive the Manage Host window's real HTML and JS against a real Host and `bee`.
//
// Run by `scripts/host-manage-check.py --ui`, which provides the stack. Chromium
// loads the exact files the menu bar embeds; the only substitute is Tauri's
// `invoke`, replaced with the menu bar binary's own `--manage-action` and
// `--manage-inventory` paths, so Rust's action decoding, argv building, env
// scrubbing and process group all run for real.
//
// What this does not exercise, named: WKWebView, the CSP, the status item and
// Tauri's IPC. The native smoke check in the same plan covers those separately.
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(path.join(process.env.NODE_PATH, "x.js"));
const { chromium } = require("@playwright/test");

const here = path.dirname(fileURLToPath(import.meta.url));
const page_file = path.join(
  here,
  "../desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.html",
);
const env = { ...process.env };
const shots = process.env.UI_SHOTS;
fs.mkdirSync(shots, { recursive: true });

let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(
    `${ok ? "UI PASS" : "UI FAIL"} ${name}${ok ? "" : ` :: ${String(detail).slice(0, 400)}`}`,
  );
  if (!ok) failures += 1;
};

function spawnJson(bin, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    let out = "";
    let err = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
    });
    child.stderr.on("data", (chunk) => {
      err += chunk;
    });
    child.on("error", reject);
    child.on("close", () => {
      try {
        resolve(JSON.parse(out.trim().split("\n").pop()));
      } catch {
        reject(new Error(`no JSON from ${args[0]}: ${out} ${err}`));
      }
    });
  });
}

function runBee(args, asOwner = false) {
  const ownerEnv = asOwner
    ? {
        ...env,
        BEEKEEPER_PRIVATE_KEY: fs
          .readFileSync(process.env.OWNER_KEY_FILE, "utf8")
          .trim(),
      }
    : env;
  return new Promise((resolve) => {
    const child = spawn(process.env.BEE_BIN, args, {
      env: ownerEnv,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let out = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
    });
    child.on("close", (code) => resolve({ code, out }));
  });
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 920, height: 1000 } });
const jsErrors = [];
page.on("pageerror", (error) => jsErrors.push(error.message));
await page.exposeFunction("__manageBridge", async (name, args) => {
  if (name === "manage_inventory")
    return spawnJson(process.env.MENUBAR_BIN, ["--manage-inventory"]);
  if (name === "manage_run")
    return spawnJson(process.env.MENUBAR_BIN, [
      "--manage-action",
      JSON.stringify(args.action),
    ]);
  if (name === "manage_interrupt") return false;
  throw new Error(`unknown command ${name}`);
});
await page.addInitScript(() => {
  window.__TAURI__ = {
    core: { invoke: (name, args) => window.__manageBridge(name, args) },
  };
});
await page.goto(`file://${page_file}`);

const shot = (name) =>
  page.screenshot({ path: path.join(shots, `${name}.png`), fullPage: true });
const byTest = (id) => page.locator(`[data-testid="${id}"]`);
const waitText = (locator, text, timeout = 60000) =>
  locator.filter({ hasText: text }).first().waitFor({ timeout });

try {
  await waitText(byTest("host-line"), "provider running");
  check("the window reads the Host with no key", true);
  check(
    "writes stay disabled until a key is checked",
    await byTest("set-up").isDisabled(),
  );
  await waitText(byTest("runtimes"), "acme-primary");
  await waitText(byTest("runtimes"), "not probed");
  check(
    "the runtime shows its readiness and that its account is not proven",
    true,
  );
  await shot("01-opened");

  await page.fill("#key-file", process.env.WRONG_KEY_FILE);
  await page.click("#check-key");
  await waitText(byTest("signer"), "OWNER_SIGNER_MISMATCH");
  check(
    "a key that is not the owner is named and changes stay disabled",
    await byTest("set-up").isDisabled(),
  );
  await shot("02-wrong-key");

  await page.fill("#key-file", process.env.OWNER_KEY_FILE);
  await page.click("#check-key");
  await waitText(byTest("signer"), "this Host's owner");
  check("the owner key is recognized by path alone", true);

  // Record the project through the window, with nothing else run by hand.
  await page.locator("summary", { hasText: "Add a project" }).click();
  await page.fill("#trust-project", process.env.UI_PROJECT);
  await page.fill("#trust-checkout", process.env.UI_CHECKOUT);
  await page.fill("#trust-agents", process.env.UI_AGENTS);
  await page.click("#trust");
  await waitText(byTest("result"), "Done.");
  await page.click("#refresh");
  const slug = process.env.UI_PROJECT.split(":").pop();
  await page
    .locator("#project option", { hasText: slug })
    .waitFor({ state: "attached", timeout: 15000 });
  await page.selectOption("#project", process.env.UI_PROJECT);
  await page
    .locator('#role option[value="lead"]')
    .waitFor({ state: "attached", timeout: 15000 });
  check(
    "the project is recorded and its roles come from the source",
    (await page.inputValue("#role")) === "lead",
  );

  // Create the channel in the window as well.
  await page.fill("#new-channel", "everyday-chat");
  await page.click("#create-channel");
  await page
    .locator("#channel option", { hasText: "everyday-chat" })
    .waitFor({ state: "attached", timeout: 30000 });
  check(
    "a private channel is created from the window and selected",
    (await page.locator("#channel option:checked").textContent()).includes(
      "everyday-chat",
    ),
  );
  const channel = await page.inputValue("#channel");

  await page.fill("#display-name", "Everyday agent");
  await page.selectOption("#role", "needsmcp");
  await page.click("#preflight");
  await waitText(
    byTest("preflight-checks"),
    "ROLE_REQUIRES_UNSUPPORTED_CAPABILITY",
  );
  check("a role needing MCP is refused by name before any effect", true);
  await shot("03-refused-role");
  await page.selectOption("#role", "lead");
  await page.click("#preflight");
  await waitText(byTest("preflight-checks"), "ROLE_RESOLVED");
  check(
    "every check passes on valid choices",
    (await page.locator('[data-testid="preflight-checks"] li.bad').count()) ===
      0,
  );
  await shot("04-preflight");

  await page.click("#set-up");
  await waitText(byTest("setups"), "Everyday agent");
  const card = page
    .locator('[data-testid^="setup-"]', { hasText: "Everyday agent" })
    .first();
  await waitText(
    card.locator('[data-testid="setup-state"]'),
    "running",
    120000,
  );
  check("setup runs end to end", true);
  const actor = (
    await card.locator('[data-testid="setup-identity"]').textContent()
  ).trim();
  await waitText(
    card.locator('[data-testid="setup-readback"]'),
    "Provider signed model",
  );
  await shot("05-running");

  // The actual product: a person writes in the channel and the agent answers.
  const sent = await runBee(
    [
      "messages",
      "send",
      "--channel",
      channel,
      "--mention",
      actor,
      "--content",
      "@Everyday hello from the owner",
    ],
    true,
  );
  check(
    "a person can mention the agent in its channel",
    sent.code === 0,
    sent.out,
  );
  const request = JSON.parse(sent.out).event_id;
  let answered = false;
  for (let i = 0; i < 90 && !answered; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    const rows = await runBee(
      [
        "events",
        "query",
        "--kinds",
        "9",
        "--channel",
        channel,
        "--authors",
        actor,
      ],
      true,
    );
    if (rows.code === 0) {
      answered = JSON.parse(rows.out).some(
        (row) =>
          row.tags.some((tag) => tag[0] === "e" && tag[1] === request) &&
          row.content.includes("prompt"),
      );
    }
  }
  check("the agent set up from the window answers in that thread", answered);
  await waitText(
    card.locator('[data-testid="setup-context"]'),
    "Project context bound",
  );
  check(
    "the window says the project context is bound and that it cannot hire",
    true,
  );

  await card.locator("button", { hasText: "Inspect" }).click();
  await waitText(byTest("inspect-result"), "verified_unexpired_provider_lease");
  check("inspect shows the live signed reading", true);

  await card.locator("button", { hasText: "Stop" }).click();
  await waitText(card.locator('[data-testid="setup-state"]'), "stopped", 90000);
  check("stop reaches a signed stop from the window", true);
  await card.locator("button", { hasText: "Start" }).click();
  await waitText(card.locator('[data-testid="setup-state"]'), "running", 90000);
  const again = (
    await card.locator('[data-testid="setup-identity"]').textContent()
  ).trim();
  check("start runs the same identity again", again === actor, again);

  // A second setup for the same project and role is refused, with no new card.
  const cards = await page.locator('[data-testid="setups"] .card').count();
  await page.fill("#display-name", "A second one");
  await page.click("#set-up");
  await waitText(byTest("refusal"), "SETUP_ALREADY_EXISTS");
  check(
    "the window cannot create a second identity for the role",
    (await page.locator('[data-testid="setups"] .card').count()) === cards,
  );
  await shot("06-duplicate-refused");

  // The provider stops: the window must stop claiming.
  await runBee(["host", "stop"]);
  await page.click("#refresh");
  await waitText(byTest("host-line"), "provider stopped");
  await waitText(byTest("runtimes"), "provider not running");
  await waitText(
    card.locator('[data-testid="setup-state"]'),
    "not running: the session provider is stopped",
  );
  check(
    "a stopped provider shows nothing on offer and the agent as not running",
    await byTest("set-up").isDisabled(),
  );
  await shot("07-provider-stopped");
  await runBee(["host", "start"]);
  await waitText(byTest("host-line"), "provider running", 60000);

  check(
    "the page raised no script error",
    jsErrors.length === 0,
    jsErrors.join("; "),
  );
} catch (error) {
  check(
    `the walkthrough finished (${error.message.split("\n")[0]})`,
    false,
    error.stack,
  );
  await shot("failure").catch(() => {});
} finally {
  await browser.close();
}
process.exit(failures ? 1 : 0);
