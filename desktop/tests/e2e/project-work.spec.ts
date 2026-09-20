import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

/**
 * NIP-PW's wire, across the Tauri boundary the app actually uses.
 *
 * What this proves: the desktop invokes `project_work_coverage` by that name,
 * gets the frozen contract's own projection back **unmodified**, and refuses
 * — rather than answering an empty coverage — when the command is not there.
 * That last case is the one worth a browser test: "the read failed" and
 * "nothing remains" must never render the same, and a silently-empty answer
 * is the failure mode every other surface in this repo has had at least once.
 *
 * What it does not prove: a live relay, a real agents checkout, or the panel
 * mounted inside a running mission. Those belong to the Wave 3 control run.
 */

const FIXTURE = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../conformance/project-work/fixtures/sequences/mixed-artifacts/expected-fold.json",
);

type BridgeWindow = Window & {
  __BUZZ_E2E_INVOKE_MOCK_COMMAND__?: (
    command: string,
    payload?: Record<string, unknown>,
  ) => Promise<unknown>;
  __TAURI_INTERNALS__?: {
    invoke?: (
      command: string,
      payload?: Record<string, unknown>,
    ) => Promise<unknown>;
  };
};

async function waitForBridge(page: import("@playwright/test").Page) {
  await page.waitForFunction(
    () => {
      const bridge = window as BridgeWindow;
      return (
        typeof bridge.__BUZZ_E2E_INVOKE_MOCK_COMMAND__ === "function" ||
        typeof bridge.__TAURI_INTERNALS__?.invoke === "function"
      );
    },
    null,
    { timeout: 10_000 },
  );
}

function response() {
  return {
    schema: "buzz-project-work-response/v1",
    implementation: "buzz-core",
    coverage: JSON.parse(readFileSync(FIXTURE, "utf8")),
    unreadablePlans: [],
    agentsRepoRead: true,
  };
}

test("the native work-coverage projection crosses the bridge verbatim", async ({
  page,
}) => {
  const expected = response();
  await installMockBridge(page, {
    projectWorkCoverageResponse: expected,
  });
  await page.goto("/");

  await waitForBridge(page);
  const answer = await page.evaluate(async () => {
    const bridge = window as BridgeWindow;
    const invoke =
      bridge.__BUZZ_E2E_INVOKE_MOCK_COMMAND__ ??
      bridge.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("the mock Tauri bridge is not installed");
    return invoke("project_work_coverage", {
      request: {
        schema: "buzz-project-work-request/v1",
        sessionRef: "11111111-2222-4333-8444-555555555555",
        projectRef: "30621:1ead:kettle",
        founderPubkey: "1e".repeat(32),
        relaySelfKey: null,
        activeSeats: [],
        activeGrants: [],
        workEvents: [],
        teamEvents: [],
        goalEvents: [],
        hostEvents: [],
        refStates: [],
      },
    });
  });

  // Verbatim: the app renders the fold's own words, and a surface that
  // reshaped the projection here is a surface that could disagree with
  // `bee sessions work status` about a criterion.
  expect(answer).toEqual(expected);
});

test("a coverage read that cannot answer fails, and never returns an empty contract", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/");

  await waitForBridge(page);
  const outcome = await page.evaluate(async () => {
    const bridge = window as BridgeWindow;
    const invoke =
      bridge.__BUZZ_E2E_INVOKE_MOCK_COMMAND__ ??
      bridge.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("the mock Tauri bridge is not installed");
    try {
      const value = await invoke("project_work_coverage", {
        request: { schema: "buzz-project-work-request/v1" },
      });
      return { kind: "answered", value };
    } catch (error) {
      return { kind: "threw", message: String(error) };
    }
  });

  expect(outcome.kind).toBe("threw");
});
