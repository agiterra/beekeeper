import { mkdirSync, writeFileSync } from "node:fs";

import { expect, type Page, test } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";
import {
  codingSessionStreamSigner,
  STREAM_CHANNEL_NAME,
  seedStreamEvents,
  type StreamTarget,
} from "./helpers/codingSessionStreamFixture";

/**
 * SV-118 streaming harness: what one streamed event costs on screen when the
 * channel already holds a large history.
 *
 * Fixture (synthetic and sanitized — no real transcript content): GENERATIONS
 * sessions of EVENTS_PER_GENERATION signed events each, shaped like real Claude
 * turns (prompt, Bash calls and results, answers, result). The newest session
 * is opened, then STREAMED events are delivered into it one at a time, each an
 * answer carrying a unique marker.
 *
 * Measured per streamed event, inside the page: delivery → the marker's text
 * node is in the DOM → the next animation frame (painted). Long tasks
 * (PerformanceObserver `longtask`) over the whole streaming window. React
 * commits per event come from a SEPARATE pass with a counting devtools hook,
 * so its overhead never touches the timing passes; that pass's latency is
 * reported too, which identifies the overhead.
 *
 * Run it against the build under test (DevTools closed, no console logging
 * per event):
 *   pnpm build:e2e && npx playwright test --config=playwright.perf.config.ts \
 *     coding-session-stream.perf.ts
 * Results print as one JSON line and land in test-results/sv118-perf.json.
 */

const GENERATIONS = 12;
const EVENTS_PER_GENERATION = 250;
const STREAMED = 40;
const TIMING_PASSES = 2;

const signer = codingSessionStreamSigner(new Uint8Array(32).fill(13));

function targetFor(index: number): StreamTarget {
  return {
    driver: "claude-agent-acp",
    instanceId: "5b1185b1185b1185",
    sessionId: `5b118000-0000-4000-8000-${String(index).padStart(12, "0")}`,
    generation: 1,
  };
}

/** One generation of plausible history, oldest generation first. */
function history(index: number): RelayEvent[] {
  const target = targetFor(index);
  const base = 1_800_700_000 + index * 1_000;
  const events = [signer.metadata(target, base, `Perf session ${index}`)];
  let seq = 0;
  let turn = 0;
  while (seq < EVENTS_PER_GENERATION) {
    turn += 1;
    const turnId = `perf-${index}-${turn}`;
    const push = (item: unknown) => {
      seq += 1;
      events.push(signer.transcript(target, seq, turnId, item, base + seq));
    };
    push({ kind: "user_prompt", content: `Task ${turn} for session ${index}` });
    for (let tool = 0; tool < 4; tool += 1) {
      const toolId = `t-${turn}-${tool}`;
      push({
        kind: "tool_call",
        tool: {
          toolName: "Bash",
          toolId,
          input: {
            command: `cd /work/repo && git status --short && cargo test -p crate${tool} -- --nocapture | tail -${20 + tool}`,
          },
        },
      });
      push({
        kind: "tool_result",
        toolId,
        toolName: "Bash",
        content: `test result: ok. ${tool + 10} passed; 0 failed`,
        isError: false,
      });
    }
    push({
      kind: "assistant_text",
      text: `Turn ${turn}: all four suites passed, and the **status** is clean.`,
    });
    push({
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 30_000,
      result: "",
      costUsd: 0.02,
    });
  }
  return events;
}

type PassResult = {
  latencyMs: number[];
  longTasks: number[];
  commits: number[] | null;
};

async function runPass(page: Page, countCommits: boolean): Promise<PassResult> {
  // An uncaught page error fails the measurement with its own message, not
  // as a missing element two minutes later.
  const pageErrors: string[] = [];
  page.on("pageerror", (error) => {
    pageErrors.push(`${error.message}\n${error.stack ?? ""}`);
  });
  page.on("console", (message) => {
    if (message.type() === "error") pageErrors.push(message.text());
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  if (countCommits) {
    // A counting devtools hook. Production React reports every commit to it.
    await page.addInitScript(() => {
      const w = window as unknown as Record<string, unknown>;
      w.__sv118Commits = 0;
      w.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
        isDisabled: false,
        supportsFiber: true,
        renderers: new Map(),
        inject: () => 1,
        checkDCE: () => {},
        onCommitFiberRoot: () => {
          w.__sv118Commits = (w.__sv118Commits as number) + 1;
        },
        onCommitFiberUnmount: () => {},
        onPostCommitFiberRoot: () => {},
        onScheduleFiberRoot: () => {},
      };
    });
  }
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: signer.pubkey, label: "SV-118 perf provider" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${STREAM_CHANNEL_NAME}`).click();
  for (let index = 0; index < GENERATIONS; index += 1) {
    await seedStreamEvents(page, history(index));
  }
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect
    .poll(
      async () =>
        pageErrors.length > 0
          ? `page error: ${pageErrors.join("\n---\n").slice(0, 4000)}`
          : await trigger.getAttribute("aria-label").catch(() => null),
      { timeout: 120_000 },
    )
    .toBe(`Coding sessions (${GENERATIONS})`);
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText(`session ${GENERATIONS - 1}`, {
    timeout: 60_000,
  });
  // Let opening settle before the measured window.
  await page.waitForTimeout(2_000);

  const open = targetFor(GENERATIONS - 1);
  const streamed: { event: RelayEvent; marker: string }[] = [];
  const turnId = "perf-streamed";
  const openBase = 1_800_700_000 + (GENERATIONS - 1) * 1_000;
  streamed.push({
    event: signer.transcript(
      open,
      EVENTS_PER_GENERATION + 50,
      turnId,
      { kind: "user_prompt", content: "Streamed turn" },
      openBase + EVENTS_PER_GENERATION + 50,
    ),
    marker: "Streamed turn",
  });
  for (let i = 0; i < STREAMED; i += 1) {
    const seq = EVENTS_PER_GENERATION + 51 + i;
    const marker = `marker${String(i).padStart(3, "0")}x`;
    streamed.push({
      event: signer.transcript(
        open,
        seq,
        turnId,
        { kind: "user_prompt", content: `Streamed ${marker}`, steered: true },
        openBase + seq,
      ),
      marker,
    });
  }

  return page.evaluate(
    async ({ items, channelName }) => {
      const w = window as unknown as Record<string, unknown>;
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      const root = document.querySelector(
        '[data-testid="coding-session-workspace"]',
      );
      if (!root) throw new Error("workspace is not mounted");
      const longTasks: number[] = [];
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) longTasks.push(entry.duration);
      });
      observer.observe({ type: "longtask", buffered: false });
      const latencyMs: number[] = [];
      const commits: number[] = [];
      // Only the mutated nodes are inspected, so the probe's own cost does
      // not grow with the transcript.
      const contains = (node: Node, marker: string) =>
        (node.textContent ?? "").includes(marker);
      for (const { event, marker } of items) {
        const commitsBefore = (w.__sv118Commits as number | undefined) ?? 0;
        const start = performance.now();
        const visible = new Promise<void>((resolve) => {
          const mutations = new MutationObserver((records) => {
            for (const record of records) {
              const hit =
                (record.type === "characterData" &&
                  contains(record.target, marker)) ||
                [...record.addedNodes].some((node) => contains(node, marker));
              if (hit) {
                mutations.disconnect();
                requestAnimationFrame(() => resolve());
                return;
              }
            }
          });
          mutations.observe(root, {
            subtree: true,
            childList: true,
            characterData: true,
          });
        });
        seed({ channelName, event });
        await Promise.race([
          visible,
          new Promise((_, reject) =>
            setTimeout(
              () => reject(new Error(`${marker} never showed`)),
              15_000,
            ),
          ),
        ]);
        latencyMs.push(performance.now() - start);
        commits.push(
          ((w.__sv118Commits as number | undefined) ?? 0) - commitsBefore,
        );
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      observer.disconnect();
      return {
        latencyMs: latencyMs.slice(1),
        longTasks,
        commits: w.__sv118Commits === undefined ? null : commits.slice(1),
      };
    },
    {
      items: streamed.map(({ event, marker }) => ({ event, marker })),
      channelName: STREAM_CHANNEL_NAME,
    },
  );
}

function quantiles(values: number[]) {
  const sorted = [...values].sort((a, b) => a - b);
  const at = (q: number) =>
    sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] ?? 0;
  return {
    n: sorted.length,
    p50: +at(0.5).toFixed(1),
    p90: +at(0.9).toFixed(1),
    max: +(sorted.at(-1) ?? 0).toFixed(1),
  };
}

test("MEASURE: SV-118 streamed event → visible, with a large channel history", async ({
  browser,
}) => {
  test.setTimeout(600_000);
  const timing: PassResult[] = [];
  for (let pass = 0; pass < TIMING_PASSES; pass += 1) {
    const page = await browser.newPage();
    timing.push(await runPass(page, false));
    await page.close();
  }
  const page = await browser.newPage();
  const counted = await runPass(page, true);
  await page.close();

  const latency = timing.flatMap((pass) => pass.latencyMs);
  const longTasks = timing.flatMap((pass) => pass.longTasks);
  const summary = {
    fixture: {
      generations: GENERATIONS,
      eventsPerGeneration: EVENTS_PER_GENERATION,
      streamed: STREAMED,
      timingPasses: TIMING_PASSES,
    },
    eventToVisibleMs: quantiles(latency),
    longTasks: {
      count: longTasks.length,
      totalMs: +longTasks.reduce((sum, d) => sum + d, 0).toFixed(1),
      maxMs: +Math.max(0, ...longTasks).toFixed(1),
      perPass: timing.map((pass) => pass.longTasks.length),
    },
    reactCommitsPerEvent: quantiles(counted.commits ?? []),
    instrumentedPassEventToVisibleMs: quantiles(counted.latencyMs),
  };
  mkdirSync("test-results", { recursive: true });
  writeFileSync(
    "test-results/sv118-perf.json",
    JSON.stringify(summary, null, 2),
  );
  console.log(`SV118_PERF ${JSON.stringify(summary)}`);
});
