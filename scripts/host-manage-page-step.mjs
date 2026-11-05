// One step of the recovery scenarios, through the Manage Host window's real page.
//
// Loads the embedded HTML and JS with Tauri's `invoke` replaced by the menu bar
// binary's own action path, checks the key, reads one setup's card and optionally
// clicks one of its buttons, then prints what a person would see as JSON.
// Input: one JSON argument {setup, click?, keyFile}.
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(path.join(process.env.NODE_PATH, "x.js"));
const { chromium } = require("@playwright/test");
const here = path.dirname(fileURLToPath(import.meta.url));
// HM_PAGE_FILE: an exact earlier page, for running a control against it.
const pageFile =
  process.env.HM_PAGE_FILE ||
  path.join(
    here,
    "../desktop/src-tauri/crates/beekeeper-menubar/frontend/manage-host.html",
  );
const input = JSON.parse(process.argv[2]);

function menubar(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.env.MENUBAR_BIN, args, {
      env: process.env,
      stdio: ["ignore", "pipe", "ignore"],
    });
    let out = "";
    child.stdout.on("data", (chunk) => {
      out += chunk;
    });
    child.on("error", reject);
    child.on("close", () => {
      try {
        resolve(JSON.parse(out.trim().split("\n").pop()));
      } catch {
        reject(new Error(`no JSON from the menu bar: ${out}`));
      }
    });
  });
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 920, height: 1000 } });
await page.exposeFunction("__manageBridge", (name, args) => {
  if (name === "manage_inventory") return menubar(["--manage-inventory"]);
  if (name === "manage_run")
    return menubar(["--manage-action", JSON.stringify(args.action)]);
  return false;
});
await page.addInitScript(() => {
  window.__TAURI__ = {
    core: { invoke: (name, args) => window.__manageBridge(name, args) },
  };
});
await page.goto(`file://${pageFile}`);

const card = page.locator(`[data-testid="setup-${input.setup}"]`);
async function read() {
  await card.waitFor({ timeout: 20000 });
  const text = async (id) => {
    const node = card.locator(`[data-testid="${id}"]`);
    return (await node.count())
      ? (await node.first().textContent()).trim()
      : null;
  };
  return {
    badge: await text("setup-state"),
    refusal: await text("setup-refusal"),
    context: await text("setup-context"),
    identity: await text("setup-identity"),
    buttons: await card.locator("button").allTextContents(),
  };
}

async function text2(scope, id) {
  const node = scope.locator(`[data-testid="${id}"]`);
  return (await node.count())
    ? (await node.first().textContent()).trim()
    : null;
}

// Setup lives behind Add agent. An earlier page (HM_PAGE_FILE) shows it directly.
async function openAddAgent() {
  if (!(await page.locator("#project").isVisible()))
    await page.click("#add-agent");
}

const result = {};
try {
  await page.fill("#key-file", input.keyFile);
  await page.click("#check-key");
  await page.locator('[data-testid="signer"].good').waitFor({ timeout: 20000 });
  result.before = await read();
  if (input.readEffort) {
    await openAddAgent();
    await page.selectOption("#project", input.readEffort.project);
    await page.waitForTimeout(800);
    await page.selectOption("#model", input.readEffort.model);
    const effort = page.locator("#effort");
    result.effortHidden = await effort.evaluate((node) => node.hidden);
    result.effortOptions = await effort.locator("option").allTextContents();
    result.effortValue = await effort.inputValue();
  }
  if (input.setUp) {
    // A person's path: the form, the effort list, the button. Nothing injected.
    const form = input.setUp;
    await openAddAgent();
    await page.selectOption("#project", form.project);
    await page.waitForFunction(
      () => document.getElementById("role").value,
      null,
      { timeout: 20000 },
    );
    await page.selectOption("#channel", form.channel);
    await page.fill("#display-name", form.name);
    await page.selectOption("#model", form.model);
    result.setUpEnabledBeforeEffort = await page.locator("#set-up").isEnabled();
    await page.selectOption("#effort", form.effort);
    result.setUpEnabledAfterEffort = await page.locator("#set-up").isEnabled();
    await page.click("#set-up");
    await page.waitForFunction(
      () => !document.getElementById("check-key").disabled,
      null,
      { timeout: 200000 },
    );
    result.result = (await page.locator("#result").textContent()).trim();
  }
  if (input.click) {
    const button = card.locator("button", { hasText: input.click }).first();
    if (await button.count()) {
      await button.click();
      if (input.watchProgress) {
        // Without a Refresh: the card must show this by itself, while it runs.
        const interrupt = card.locator("button", { hasText: "Interrupt" });
        await interrupt.waitFor({ timeout: 8000 }).catch(() => {});
        result.interruptShown = await interrupt.count();
        await card
          .locator('[data-testid="setup-step"]')
          .waitFor({ timeout: 8000 })
          .catch(() => {});
        result.stepShown = await text2(card, "setup-step");
      }
      // The window disables its actions while one runs, and redraws when it ends.
      await page.waitForFunction(
        () => document.getElementById("result").textContent !== "Working…",
        null,
        { timeout: 200000 },
      );
      await page.waitForFunction(
        () => !document.getElementById("check-key").disabled,
        null,
        { timeout: 200000 },
      );
      await page.click("#refresh");
      await page.waitForTimeout(800);
      result.clicked = true;
    } else {
      result.clicked = false;
    }
    result.result = (await page.locator("#result").textContent()).trim();
    result.after = await read();
  }
} catch (error) {
  result.error = error.message.split("\n")[0];
} finally {
  await browser.close();
}
console.log(JSON.stringify(result));
