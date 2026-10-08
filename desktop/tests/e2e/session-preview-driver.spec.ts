import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

/**
 * The Browser's preview driver (SV-33 S1/S2, C4) against a real WebKit.
 *
 * Runs in `smoke-webkit` only. It loads the vendored Playwright injected
 * script and `driver.js` exactly as `session_preview::driver::driver_source`
 * composes them, then drives a fixture page through the driver's own `run`
 * entry point: locators, input, ambiguity, refs, keys, checks and scroll.
 *
 * What it proves: the driver's logic on the WebKit engine. What it does not:
 * Playwright's WebKit is not the WKWebView the app ships, and this page
 * evaluates in the main world, not the isolated `beekeeper-preview-driver`
 * content world (that, and every native path, is the real-app proof).
 * Ported from lane L1's harness (`drv/run.cjs`, 29 checks).
 */

const ROOT = new URL("../../", import.meta.url);
const INJECTED = readFileSync(
  new URL("vendor/playwright-injected/injectedScriptSource.js", ROOT),
  "utf8",
);
const DRIVER = readFileSync(
  new URL("src-tauri/src/session_preview/driver/driver.js", ROOT),
  "utf8",
);
const FIXTURE = fileURLToPath(
  new URL("./fixtures/session-preview-driver.html", import.meta.url),
);
// Byte-for-byte `INJECTED_OPTIONS` in `session_preview/driver.rs`.
const OPTIONS =
  '{"isUnderTest":false,"sdkLanguage":"javascript","frameSeq":0,"testIdAttributeName":"data-testid","stableRafCount":1,"browserName":"webkit","shouldPrependErrorPrefix":false,"isUtilityWorld":true,"customEngines":[]}';
// The same wrapper `driver_source()` formats.
const SOURCE = `(() => {\nif (globalThis.__beekeeperPreviewDriver) return;\nconst createInjected = () => {\nconst module = {};\n${INJECTED}\nreturn new (module.exports.InjectedScript())(globalThis, ${OPTIONS});\n};\n${DRIVER}\nglobalThis.__beekeeperPreviewDriver = globalThis.__beekeeperPreviewDriverFactory(createInjected);\ndelete globalThis.__beekeeperPreviewDriverFactory;\n})();\n`;

type DriverResult = {
  ok: boolean;
  code?: string;
  count?: number;
  aria?: string;
  satisfied?: boolean;
  scrollY?: number;
  [key: string]: unknown;
};

declare global {
  interface Window {
    __beekeeperPreviewDriver?: {
      run: (op: Record<string, unknown>) => Promise<DriverResult>;
    };
    __keys?: string[];
    pageSecret?: number;
  }
}

function runner(page: Page) {
  return (op: Record<string, unknown>, generation = 1) =>
    page.evaluate(
      async (input) =>
        JSON.parse(
          JSON.stringify(await window.__beekeeperPreviewDriver?.run(input)),
        ) as DriverResult,
      { generation, ...op },
    );
}

test.skip(
  ({ browserName }) => browserName !== "webkit",
  "the preview driver targets WebKit; smoke-webkit only",
);

test("the preview driver drives a page in WebKit", async ({ page }) => {
  await page.goto(`file://${FIXTURE}`);
  await page.evaluate(SOURCE);
  const run = runner(page);

  const snap = await run({ verb: "snapshot" });
  expect(snap.ok, "snapshot ok").toBe(true);
  expect(snap.aria, "aria refs are generation-stamped").toMatch(
    /\[ref=e\d+@g1\]/,
  );
  expect(snap.aria).toContain('button "Save"');

  expect(
    (await run({ verb: "click", target: { role: "button", name: "Save" } })).ok,
  ).toBe(true);
  expect(await page.title(), "click by role+name").toBe("clicked");

  expect(
    (await run({ verb: "type", target: { label: "Email" }, text: "a@b" })).ok,
  ).toBe(true);
  expect(await page.inputValue("#email"), "type by label").toBe("a@b");
  expect(
    (
      await run({
        verb: "type",
        target: { placeholder: "you@x" },
        text: "c@d",
        clear: true,
      })
    ).ok,
  ).toBe(true);
  expect(await page.inputValue("#email"), "type --clear").toBe("c@d");
  expect(
    (await run({ verb: "type", target: { label: "Name" }, text: "Bo" })).ok,
  ).toBe(true);
  expect(await page.inputValue("#name"), "wrapping label").toBe("Bo");
  expect(
    (await run({ verb: "type", target: { label: "Notes" }, text: "x y" })).ok,
    "aria-label textarea",
  ).toBe(true);

  const ambiguous = await run({ verb: "click", target: { text: "Dup" } });
  expect(ambiguous).toMatchObject({
    ok: false,
    code: "preview_target_ambiguous",
    count: 2,
  });
  expect(
    (await run({ verb: "click", target: { text: "Dup", nth: 1 } })).ok,
    "nth",
  ).toBe(true);
  expect(
    await run({ verb: "click", target: { role: "button", name: "Nope" } }),
  ).toMatchObject({
    ok: false,
    code: "preview_target_not_found",
  });
  expect(
    (await run({ verb: "click", target: { testId: "dup2" } })).ok,
    "testId",
  ).toBe(true);
  expect(
    await run({ verb: "click", target: { role: "button", text: "x" } }),
    "two locator families are refused",
  ).toMatchObject({ ok: false, code: "preview_bad_request" });

  const refLine = (snap.aria ?? "")
    .split("\n")
    .find((line) => line.includes('button "Save"'));
  const ref = /\[ref=(e\d+@g1)\]/.exec(refLine ?? "")?.[1];
  expect(ref, "the Save button has a ref").toBeTruthy();
  expect(
    (await run({ verb: "click", target: { ref } })).ok,
    "click by ref",
  ).toBe(true);
  expect(
    await run({ verb: "click", target: { ref } }, 2),
    "a ref from an older generation is stale",
  ).toMatchObject({ ok: false, code: "preview_stale_ref" });

  expect(
    (await run({ verb: "press", key: "Meta+a", target: { label: "Email" } }))
      .ok,
  ).toBe(true);
  expect(
    await page.evaluate(() => window.__keys?.slice(-1)),
    "Meta+a reached the page",
  ).toEqual(["a+M"]);
  expect(
    (await run({ verb: "press", key: "Enter", target: { label: "Email" } })).ok,
  ).toBe(true);
  expect(await page.title(), "Enter submits the form").toBe("submitted:c@d");

  const appeared = {
    verb: "check",
    target: { role: "button", name: "Appeared" },
  };
  expect(await run(appeared), "not there yet").toMatchObject({
    ok: true,
    satisfied: false,
  });
  await page.waitForTimeout(500);
  expect(await run(appeared), "there after it renders").toMatchObject({
    ok: true,
    satisfied: true,
  });
  expect(await run({ verb: "check", text: "Local dev server" })).toMatchObject({
    ok: true,
    satisfied: true,
  });
  expect(await run({ verb: "check", urlIncludes: "nothere" })).toMatchObject({
    ok: true,
    satisfied: false,
  });

  expect(await run({ verb: "scroll", dy: 600 })).toMatchObject({
    ok: true,
    scrollY: 600,
  });
  expect(await run({ verb: "scroll", to: "top" })).toMatchObject({
    ok: true,
    scrollY: 0,
  });

  // Main world here, so the page's globals are visible; the isolated-world
  // proof (`pageSecret: undefined`) is the native probe's, not this spec's.
  expect(await page.evaluate(() => typeof window.pageSecret)).toBe("number");
  expect(await run({ verb: "fly" }), "unknown verb").toMatchObject({
    ok: false,
    code: "preview_bad_request",
  });
});
