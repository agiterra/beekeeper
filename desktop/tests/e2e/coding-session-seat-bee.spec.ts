import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { SeatBeeStamp } from "@/features/coding-sessions/lib/codingSessionSeatBee";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * L12.3 — both surfaces say which `bee` a seat is running, in words.
 *
 * **Why these are harness mounts and not a driven app flow.** The seat chip
 * reads its stamps from a `seatBeeStamps` prop, and the only caller —
 * `CodingSessionUmbrellaWorkspace.tsx` — is not this lane's file, so no signed
 * 44223 can reach the chip through the running app yet. Pulse's card *is*
 * mounted in the real screen, but `CoordinatedGeneration` carries no
 * `beeStamp` and no host command answers ancestry, so the app can only ever
 * reach its `no seat's build could be compared` state — never a row.
 *
 * So each state is mounted here from the same components the app renders,
 * inside a real app page: the built stylesheet, the theme tokens, and the
 * layout are the app's own. These are pure presentational components, so the
 * server markup is byte-for-byte what React paints in the app. What this spec
 * does **not** prove is the wiring from wire to prop — that is stated in the
 * lane report as an open cross-lane request, not papered over here.
 *
 * **Why the markup is rendered out-of-process.** `CodingSessionParticipantBar`
 * and `PulseStaleBeeCard` are `.tsx` — Playwright's own TypeScript transform
 * compiles `.tsx` imports with its own automatic-JSX runtime
 * (`playwright/jsx-runtime`, not `react/jsx-runtime`), so a `.tsx` component
 * imported straight into a spec renders every JSX node as a `{__pw_type:
 * "jsx", ...}` object instead of a real React element, and
 * `react-dom/server`'s `renderToStaticMarkup` refuses it outright — a
 * Playwright-transform artifact, not a defect in either component (both
 * render correctly through the project's own loader in their `.test.mjs`
 * files next to them, and standalone under `node --experimental-strip-types
 * --import ./test-loader.mjs`). `renderFixture` below shells out to that same
 * loader in a plain Node child process — the app's own toolchain, not
 * Playwright's — and hands back the markup it printed.
 */

const SHOTS =
  "/Users/brian/Projects/beekeeper/review-2026-09-01/batch3/l12-shots";

const hashes = new Map<string, string>();

const DESKTOP_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "..",
);
const RENDER_HELPER = path.join(
  DESKTOP_ROOT,
  "tests",
  "e2e",
  "helpers",
  "renderL12BeeFixture.mjs",
);

/** Render one of the two L12 components in a real Node process, real loader. */
function renderFixture(
  caseName: "participant-bar" | "pulse-card",
  payload: unknown,
): string {
  return execFileSync(
    "node",
    [
      "--experimental-strip-types",
      "--import",
      "./test-loader.mjs",
      RENDER_HELPER,
      caseName,
      JSON.stringify(payload),
    ],
    { cwd: DESKTOP_ROOT, encoding: "utf8" },
  );
}

const BUNDLED: SeatBeeStamp = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: "0.1.0",
  sha: "23728227b",
  dirty: false,
};

const ON_PATH: SeatBeeStamp = {
  path: "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
  source: "path",
  version: "0.1.0",
  sha: "07c470be0",
  dirty: true,
};

const UNPARSED: SeatBeeStamp = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: null,
  sha: null,
  dirty: null,
};

/**
 * Paint one component into a real app page and hand back its locator.
 *
 * The app is booted first so the built stylesheet, the theme, and the font are
 * the app's own; the harness node is appended to the live document rather than
 * replacing it, so nothing about the page's own styling is simulated.
 */
async function mount(
  page: Page,
  markup: string,
  width: number,
): Promise<Locator> {
  await page.evaluate(
    ({ html, px }) => {
      const existing = document.querySelector("[data-testid='l12-harness']");
      existing?.remove();
      const host = document.createElement("div");
      host.setAttribute("data-testid", "l12-harness");
      host.className = "bg-background p-4";
      host.style.position = "fixed";
      host.style.top = "0";
      host.style.left = "0";
      host.style.zIndex = "9999";
      host.style.width = `${px}px`;
      host.innerHTML = html;
      document.body.append(host);
    },
    { html: markup, px: width },
  );
  return page.getByTestId("l12-harness");
}

async function capture(page: Page, locator: Locator, name: string) {
  await waitForAnimations(page);
  const buffer = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

test("a seat chip names the bee that seat is running, in every state", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  // 1 — the bundled sidecar: the binary beside the running app.
  const bundled = await mount(
    page,
    renderFixture("participant-bar", { stamps: { builder: BUNDLED } }),
    360,
  );
  await expect(bundled.getByTestId("coding-session-seat-bee")).toHaveText(
    "bee 23728227b (bundled)",
  );
  await capture(page, bundled, "01-seat-bee-bundled");

  // 2 — a binary found on PATH, built from a dirty tree: the case that made
  // one run answer from two binaries with neither surface saying so.
  const onPath = await mount(
    page,
    renderFixture("participant-bar", { stamps: { builder: ON_PATH } }),
    360,
  );
  await expect(onPath.getByTestId("coding-session-seat-bee")).toHaveText(
    "bee 07c470be0-dirty (found on PATH: /Users/brian/Projects/beekeeper/beekeeper/target/debug)",
  );
  await capture(page, onPath, "02-seat-bee-path-dirty");

  // 3 — the host ran `--version` and could not parse it. Unknown, never blank.
  const unknown = await mount(
    page,
    renderFixture("participant-bar", { stamps: { builder: UNPARSED } }),
    360,
  );
  await expect(unknown.getByTestId("coding-session-seat-bee")).toHaveText(
    "bee build unknown",
  );
  await capture(page, unknown, "03-seat-bee-unknown");
});

test("Pulse owes the reader every seat behind main, and says what it could not compare", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  // 4 — rows: one line per seat, with the host's own commit count.
  const rows = await mount(
    page,
    renderFixture("pulse-card", {
      reading: {
        rows: [
          {
            seatKey: "builder",
            label: "Bob · Builder",
            sha7: "07c470b",
            behind: 3,
          },
          {
            seatKey: "verifier",
            label: "Cleo · Verifier",
            sha7: "2372822",
            behind: 1,
          },
        ],
        uncomparedCount: 2,
        truncatedCount: 0,
      },
    }),
    560,
  );
  await expect(rows.getByTestId("pulse-stale-bee-row")).toHaveCount(2);
  await expect(rows.getByTestId("pulse-stale-bee-row").first()).toHaveText(
    "Bob · Builder · bee 07c470b — 3 commits behind main",
  );
  await expect(rows.getByTestId("pulse-stale-bee-uncompared")).toHaveText(
    "2 more seats' builds could not be compared",
  );
  await capture(page, rows, "04-pulse-stale-bee");

  // 5 — the state the shipped app can actually reach today: live seats exist,
  // but nothing published a stamp the host could place against `main`.
  const uncompared = await mount(
    page,
    renderFixture("pulse-card", {
      reading: { rows: [], uncomparedCount: 3, truncatedCount: 0 },
    }),
    560,
  );
  await expect(uncompared.getByTestId("pulse-stale-bee-empty")).toHaveText(
    "no seat's build could be compared",
  );
  await expect(uncompared.getByTestId("pulse-stale-bee-row")).toHaveCount(0);
  await capture(page, uncompared, "05-pulse-stale-bee-uncompared");
});
