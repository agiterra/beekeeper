import { mkdirSync, writeFileSync } from "node:fs";

import { expect, type Page, test } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";
import { seedStreamEvents } from "./helpers/codingSessionStreamFixture";
import {
  TEAM_PROVIDER_PUBKEY,
  TEAM_TITLE,
  teamStreamFixture,
} from "./helpers/codingSessionTeamStreamFixture";

/**
 * SV-100 team streaming harness: what one streamed event costs on screen in a
 * TEAM umbrella — SEATS executions with TURNS_PER_SEAT settled turns each, so
 * many umbrella turn blocks — in the Conversation lens with the minimap drawn.
 *
 * Fixture: synthetic and sanitized (`helpers/codingSessionTeamStreamFixture.ts`).
 * The last seat is running with an open turn; STREAMED steered prompts, each
 * carrying a unique marker, are delivered into that turn one at a time.
 *
 * Measured per streamed event, inside the page: delivery → the marker's text
 * node is in the DOM → the next animation frame. Long tasks over the
 * streaming window. React commits per event come from a SEPARATE pass with a
 * counting devtools hook; production React exposes no render durations, so
 * render/commit TIME is not measured here. That pass's latency identifies the
 * hook's overhead.
 *
 * Run against the build under test (DevTools closed):
 *   pnpm build:e2e && npx playwright test --config=playwright.perf.config.ts \
 *     coding-session-team-stream.perf.ts
 * Results print as one JSON line and land in test-results/sv100-perf.json.
 */

const SEATS = 4;
const TURNS_PER_SEAT = 60;
const STREAMED = 40;
const TIMING_PASSES = 2;
const CHANNEL_NAME = "engineering";

type PassResult = {
  latencyMs: number[];
  longTasks: number[];
  commits: number[] | null;
  /** Main-thread time over the streaming window, from CDP `Performance`. */
  mainThreadMs: Record<string, number>;
};

/** CDP's cumulative durations (seconds) we difference over the window. */
const MAIN_THREAD_METRICS = [
  "ScriptDuration",
  "TaskDuration",
  "LayoutDuration",
  "RecalcStyleDuration",
] as const;

async function runPass(page: Page, countCommits: boolean): Promise<PassResult> {
  // An uncaught page error fails the measurement with its own message.
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
      w.__sv100Commits = 0;
      w.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
        isDisabled: false,
        supportsFiber: true,
        renderers: new Map(),
        inject: () => 1,
        checkDCE: () => {},
        onCommitFiberRoot: () => {
          w.__sv100Commits = (w.__sv100Commits as number) + 1;
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
        { pubkey: TEAM_PROVIDER_PUBKEY, label: "SV-100 perf provider" },
      ],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  const fixture = teamStreamFixture(SEATS, TURNS_PER_SEAT);
  await seedStreamEvents(page, fixture.events);
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect
    .poll(
      async () =>
        pageErrors.length > 0
          ? `page error: ${pageErrors.join("\n---\n").slice(0, 4000)}`
          : await trigger.getAttribute("aria-label").catch(() => null),
      { timeout: 120_000 },
    )
    .toBe("Coding sessions (1)");
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(page.getByText(TEAM_TITLE).first()).toBeVisible({
    timeout: 60_000,
  });
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible();
  await expect(page.getByTestId("coding-session-minimap-strip")).toBeVisible({
    timeout: 30_000,
  });
  // Let opening settle before the measured window.
  await page.waitForTimeout(2_000);

  const streamed: { event: RelayEvent; marker: string }[] = [];
  for (let i = 0; i < STREAMED; i += 1) {
    const marker = `marker${String(i).padStart(3, "0")}x`;
    streamed.push({
      event: fixture.liveWrite(fixture.liveTurn, {
        kind: "user_prompt",
        content: `Streamed ${marker}`,
        steered: true,
      }),
      marker,
    });
  }

  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable");
  const readMetrics = async () => {
    const { metrics } = await cdp.send("Performance.getMetrics");
    return new Map(metrics.map((metric) => [metric.name, metric.value]));
  };
  // SV100_PROFILE=1: also take a CPU profile of the streaming window (an
  // instrumented pass — never read its timings as the measurement).
  const profiling = process.env.SV100_PROFILE === "1" && !countCommits;
  if (profiling) {
    await cdp.send("Profiler.enable");
    await cdp.send("Profiler.start");
  }
  const before = await readMetrics();
  const measured = await page.evaluate(
    async ({ items, channelName }) => {
      const w = window as unknown as Record<string, unknown>;
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      const root = document.querySelector(
        '[data-testid="coding-session-umbrella-workspace"]',
      );
      if (!root) throw new Error("umbrella workspace is not mounted");
      const longTasks: number[] = [];
      const observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) longTasks.push(entry.duration);
      });
      observer.observe({ type: "longtask", buffered: false });
      const latencyMs: number[] = [];
      const commits: number[] = [];
      // Only mutated nodes are inspected, so the probe's cost does not grow
      // with the transcript.
      const contains = (node: Node, marker: string) =>
        (node.textContent ?? "").includes(marker);
      for (const { event, marker } of items) {
        const commitsBefore = (w.__sv100Commits as number | undefined) ?? 0;
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
          ((w.__sv100Commits as number | undefined) ?? 0) - commitsBefore,
        );
        await new Promise((resolve) => setTimeout(resolve, 100));
      }
      observer.disconnect();
      return {
        latencyMs,
        longTasks,
        commits: w.__sv100Commits === undefined ? null : commits,
      };
    },
    {
      items: streamed.map(({ event, marker }) => ({ event, marker })),
      channelName: CHANNEL_NAME,
    },
  );
  const after = await readMetrics();
  if (profiling) {
    const { profile } = await cdp.send("Profiler.stop");
    mkdirSync("test-results", { recursive: true });
    writeFileSync(
      `test-results/sv100-profile-${Date.now()}.cpuprofile`,
      JSON.stringify(profile),
    );
  }
  // Per streamed event. The window also holds the probe's 100 ms gaps, which
  // are idle, so these are work, not wall time.
  const mainThreadMs = Object.fromEntries(
    MAIN_THREAD_METRICS.map((name) => [
      name,
      +(
        (((after.get(name) ?? 0) - (before.get(name) ?? 0)) * 1_000) /
        STREAMED
      ).toFixed(2),
    ]),
  );
  await cdp.detach();
  return { ...measured, mainThreadMs };
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

test("MEASURE: SV-100 team umbrella streamed event → visible, minimap on", async ({
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
      seats: SEATS,
      turnsPerSeat: TURNS_PER_SEAT,
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
    mainThreadMsPerEvent: timing.map((pass) => pass.mainThreadMs),
    reactCommitsPerEvent: quantiles(counted.commits ?? []),
    instrumentedPassEventToVisibleMs: quantiles(counted.latencyMs),
  };
  mkdirSync("test-results", { recursive: true });
  writeFileSync(
    "test-results/sv100-perf.json",
    JSON.stringify(summary, null, 2),
  );
  console.log(`SV100_PERF ${JSON.stringify(summary)}`);
});
