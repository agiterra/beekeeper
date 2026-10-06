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
  __BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__?: (
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
        typeof bridge.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ === "function" ||
        typeof bridge.__TAURI_INTERNALS__?.invoke === "function"
      );
    },
    null,
    // Stays under Playwright's own per-test timeout so a slow bridge is
    // reported as a bridge failure rather than as an unattributable test
    // timeout.
    { timeout: 20_000 },
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
      bridge.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ ??
      bridge.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("the mock Tauri bridge is not installed");
    return invoke("project_work_coverage", {
      request: {
        schema: "buzz-project-work-request/v1",
        sessionRef: "11111111-2222-4333-8444-555555555555",
        projectRef: "30621:1ead:kettle",
        founderPubkey: "1e".repeat(32),
        // Required since lane 213: the assembler folds the 44244 records
        // with the canonical team fold, which is scoped by these.
        channelRef: "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2",
        genesisRef: "ab".repeat(32),
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
  // A mock configuration of the same *shape* as the other two cases, with
  // only the coverage response withheld — which is the fact under test. A
  // bare `installMockBridge(page)` and a `{}` both leave this case's bridge
  // globals undefined when it runs after another test in the same worker
  // (in isolation either passes), and `waitForBridge` then burns its whole
  // budget: that is the cold "waitForBridge timed out" seen in a landing,
  // and it is this spec's setup rather than the shared helper.
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      model: null,
      preferred_runtime: null,
      provider: null,
    },
  });
  await page.goto("/");

  await waitForBridge(page);
  const outcome = await page.evaluate(async () => {
    const bridge = window as BridgeWindow;
    const invoke =
      bridge.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ ??
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

test("a request missing what the native fold requires is refused, not answered", async ({
  page,
}) => {
  // Lane 213 added two required fields to the Rust struct and every
  // TypeScript unit test stayed green while the view would have failed at
  // runtime. The mock bridge holds the same contract, so the next drift of
  // this class fails here.
  await installMockBridge(page, { projectWorkCoverageResponse: response() });
  await page.goto("/");
  await waitForBridge(page);

  const outcome = await page.evaluate(async () => {
    const bridge = window as BridgeWindow;
    const invoke =
      bridge.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ ??
      bridge.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("the mock Tauri bridge is not installed");
    try {
      await invoke("project_work_coverage", {
        request: {
          schema: "buzz-project-work-request/v1",
          sessionRef: "11111111-2222-4333-8444-555555555555",
          projectRef: "30621:1ead:kettle",
          founderPubkey: "1e".repeat(32),
          workEvents: [],
          teamEvents: [],
        },
      });
      return { kind: "answered", message: "" };
    } catch (error) {
      return { kind: "threw", message: String(error) };
    }
  });

  expect(outcome.kind).toBe("threw");
  expect(outcome.message).toContain("channelRef");
  expect(outcome.message).toContain("genesisRef");
});

test("R1: one read answers with a body and the hash of those bytes", async ({
  page,
}) => {
  // Astra's Wave 2 re-check, R1: the approval surfaces must not be able to
  // join a body from one read to a hash from another. Across the bridge the
  // shape itself forbids it — `get_workflow_definition` answers with both, so
  // there is no second response to disagree with.
  const read = {
    id: "9a7c4f1e-0000-4000-8000-000000000001",
    revision: "1e".repeat(32),
    name: "verify",
    owner_pubkey: "2e".repeat(32),
    channel_id: "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2",
    definition: {
      name: "verify",
      steps: [{ id: "verify", action: "run_on_host", command: ["just", "ci"] }],
    },
    definition_hash: "aa".repeat(32),
    definition_hash_unavailable: null,
    created_at: 10,
  };
  await installMockBridge(page, { workflowDefinitionRead: read });
  await page.goto("/");
  await waitForBridge(page);

  const answer = await page.evaluate(async () => {
    const bridge = window as BridgeWindow;
    const invoke =
      bridge.__BEEKEEPER_E2E_INVOKE_MOCK_COMMAND__ ??
      bridge.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("the mock Tauri bridge is not installed");
    return invoke("get_workflow_definition", {
      workflowId: "9a7c4f1e-0000-4000-8000-000000000001",
    });
  });

  expect(answer).toEqual(read);
  // The body and the hash arrive together, from one call.
  expect((answer as typeof read).definition_hash).toBe("aa".repeat(32));
});
