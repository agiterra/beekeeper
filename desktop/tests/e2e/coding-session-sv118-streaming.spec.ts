import { expect, type Page, test } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";
import {
  codingSessionStreamSigner,
  STREAM_CHANNEL_NAME,
  seedStreamEvents,
} from "./helpers/codingSessionStreamFixture";

// SV-118 (ledger 355/356): the catalog keeps each generation's projection
// and re-presents only what a streamed event changed. These are the two ways
// a retained projection could show something untrue on screen: a tool result
// that completes an EARLIER item (the array's length and ends do not move),
// and a late event that lands BEFORE items already shown. Both must reach the
// fold row exactly as a full rebuild would show them, and a second session
// streaming in the same channel must not disturb the open one.

const signer = codingSessionStreamSigner(new Uint8Array(32).fill(11));
const session = {
  driver: "claude-agent-acp",
  instanceId: "5b1185b1185b1185",
  sessionId: "5b118000-0000-4000-8000-000000000118",
  generation: 1,
};
const neighbour = {
  ...session,
  sessionId: "5b118000-0000-4000-8000-000000000119",
};
const done = {
  kind: "result",
  subtype: "success",
  isError: false,
  durationMs: 12_000,
  result: "",
  costUsd: 0.01,
};

function call(toolId: string, command: string) {
  return {
    kind: "tool_call",
    tool: { toolName: "Bash", toolId, input: { command } },
  };
}

function result(toolId: string, isError: boolean) {
  return {
    kind: "tool_result",
    toolId,
    toolName: "Bash",
    content: isError ? "exit 1" : "ok",
    isError,
  };
}

async function openSession(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: signer.pubkey, label: "SV-118 provider" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${STREAM_CHANNEL_NAME}`).click();
  // Turn one with its tool result held back: seq 3 arrives last.
  await seedStreamEvents(page, [
    signer.metadata(session, 1_800_600_000, "SV-118 streaming"),
    signer.transcript(session, 1, "turn-1", {
      kind: "user_prompt",
      content: "Check the build.",
    }),
    signer.transcript(session, 2, "turn-1", call("build", "make build")),
    signer.transcript(session, 4, "turn-1", {
      kind: "assistant_text",
      text: "Answer one: the build ran.",
    }),
    signer.transcript(session, 5, "turn-1", done),
  ]);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Answer one");
  return workspace;
}

test("SV-118: late and patching events reach the open transcript as a rebuild would show them", async ({
  page,
}) => {
  test.setTimeout(90_000);
  const workspace = await openSession(page);
  const turns = workspace.getByTestId("coding-session-turn");
  await expect(turns).toHaveCount(1);
  const first = turns.first();
  const firstFold = first.getByTestId("coding-session-worked-fold");
  await expect(firstFold).toContainText("1 step did not finish");

  // The held result arrives late, BEFORE items already on screen: the
  // generation is rebuilt and the call reads failed, not unfinished.
  await seedStreamEvents(page, [
    signer.transcript(session, 3, "turn-1", result("build", true)),
  ]);
  await expect(firstFold).toContainText("1 step failed");
  await expect(firstFold).not.toContainText("did not finish");

  // Turn two streams in order; its result is appended after the answer and
  // completes a call in the MIDDLE of the generation.
  await seedStreamEvents(page, [
    signer.transcript(session, 6, "turn-2", {
      kind: "user_prompt",
      content: "Run the tests.",
    }),
    signer.transcript(session, 7, "turn-2", call("tests", "make test")),
    signer.transcript(session, 8, "turn-2", {
      kind: "assistant_text",
      text: "Answer two: the tests ran.",
    }),
    signer.transcript(session, 9, "turn-2", done),
  ]);
  await expect(turns).toHaveCount(2);
  const second = turns.nth(1);
  const secondFold = second.getByTestId("coding-session-worked-fold");
  await expect(secondFold).toContainText("1 step did not finish");
  await seedStreamEvents(page, [
    signer.transcript(session, 10, "turn-2", result("tests", false)),
  ]);
  await expect(secondFold).not.toContainText("did not finish");
  await expect(secondFold).not.toContainText("failed");
  // The earlier turn kept what it showed.
  await expect(firstFold).toContainText("1 step failed");

  // A second session streaming in the same channel leaves this one alone.
  // The open session's next event is delivered after the neighbour's, so
  // seeing it proves the neighbour's events were already processed.
  await seedStreamEvents(page, [
    signer.metadata(neighbour, 1_800_600_100, "SV-118 neighbour"),
    signer.transcript(neighbour, 1, "n-1", {
      kind: "assistant_text",
      text: "Neighbour text that must not appear here.",
    }),
    signer.transcript(session, 11, "turn-3", {
      kind: "user_prompt",
      content: "Third prompt after the neighbour.",
    }),
  ]);
  await expect(workspace).toContainText("Third prompt after the neighbour.");
  await expect(turns).toHaveCount(3);
  await expect(workspace).not.toContainText("Neighbour text");
  await expect(workspace).toContainText("Answer one");
  await expect(workspace).toContainText("Answer two");
  await expect(firstFold).toContainText("1 step failed");
});
