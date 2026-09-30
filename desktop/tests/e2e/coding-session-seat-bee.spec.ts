import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import type { PackRef } from "@/features/coding-sessions/lib/codingSessionPackRef";
import type { SeatBeeStamp } from "@/features/coding-sessions/lib/codingSessionSeatBee";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * L12.3 — both surfaces say which `bee` a seat is running, in words.
 *
 * **Why the first two tests are harness mounts and not a driven app flow.**
 * At L12's own cut, the seat chip's `seatBeeStamps` prop had no production
 * caller — `CodingSessionUmbrellaWorkspace.tsx` was not that lane's file, so
 * no signed 44223 could reach the chip through the running app. Pulse's card
 * *is* mounted in the real screen, but `CoordinatedGeneration` carries no
 * `beeStamp` and no host command answers ancestry, so the app can only ever
 * reach its `no seat's build could be compared` state — never a row; that
 * remains true today and is why the Pulse card is still a harness mount.
 *
 * So each state is mounted here from the same components the app renders,
 * inside a real app page: the built stylesheet, the theme tokens, and the
 * layout are the app's own. These are pure presentational components, so the
 * server markup is byte-for-byte what React paints in the app.
 *
 * **L17 closed the seat-chip half of that gap**: `CodingSessionUmbrellaWorkspace.tsx`
 * now derives `seatBeeStamps` from the umbrella's own executions
 * (`codingSessionSeatBee.ts`'s `deriveSeatBeeStamps`) and threads it through
 * `CodingSessionUmbrellaHeaderRow.tsx` to the participant bar — the exact
 * production wiring the paragraph above says was missing. The test below
 * ("a real, decoded kind:44223 beeStamp reaches the chip through the running
 * app") is the proof: no harness mount, no hand-built `seatBeeStamps` map — a
 * real signed 44223 is seeded into the mock relay and the chip is read off
 * the actual app screen. The Pulse-card half (ancestry) is unchanged and
 * still out of scope, per the finalizer's own accounting.
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

// These land in the repo's own `test-results/`, like every other spec's
// shots. The absolute path this replaced was a review directory on one
// laptop, so the whole file errored `ENOENT`/`EACCES` for anyone else and
// the evidence it claims to produce existed on exactly one machine.
const SHOTS = "test-results/seat-bee";

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

const PACK_REF: PackRef = {
  repo: `30617:${"a".repeat(64)}:agiterra-packs`,
  sha: "23728227ba1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a",
  role: "builder",
  path: "personas/roles/builder",
};

test("LANE-L23: a seat chip names the pack that seat staged, and discloses its absence", async ({
  page,
}) => {
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  // 1 — a staged pack: role and short sha, words only.
  const staged = await mount(
    page,
    renderFixture("participant-bar", { packs: { builder: PACK_REF } }),
    360,
  );
  await expect(staged.getByTestId("coding-session-seat-pack")).toHaveText(
    "pack builder@23728227",
  );
  await capture(page, staged, "07-seat-pack-staged");

  // 2 — no 30624 source for the project (or an older host): disclosed, not
  // silent, unlike the bee line's own absence.
  const none = await mount(
    page,
    renderFixture("participant-bar", { packs: { builder: null } }),
    360,
  );
  await expect(none.getByTestId("coding-session-seat-pack")).toHaveText(
    "no pack staged",
  );
  await capture(page, none, "08-seat-pack-none");
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

// ── L17 gap 1: the real workspace path, not the harness mount ───────────────

/**
 * A real two-seat crew, seeded with real signed events and driven through the
 * running app rather than mounted out-of-process. Where the five states above
 * prove the presentational components render every reading correctly, this
 * proves the wiring this spec's own module doc named as untested: a real
 * kind:44223 with `beeStamp` reaches the chip through
 * `CodingSessionUmbrellaWorkspace.tsx` → `deriveSeatBeeStamps` →
 * `CodingSessionUmbrellaHeaderRow.tsx` → `CodingSessionParticipantBar`, with
 * nothing hand-assembled in the spec.
 */

const FOUNDER_IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};
const FOUNDER_SECRET = Uint8Array.from(
  (FOUNDER_IDENTITY.privateKey.match(/.{2}/g) ?? []).map((byte) =>
    Number.parseInt(byte, 16),
  ),
);
const CHANNEL_NAME = "engineering";
/** `engineering` in the mock channel fixture. The `h` tag must match exactly. */
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "9c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const BASE_CREATED_AT = 1_800_100_000;

const BUILDER_SECRET = generateSecretKey();
const BUILDER_PUBKEY = getPublicKey(BUILDER_SECRET);
const BUILDER_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "l17-builder",
  sessionId: "33333333-4444-5555-6666-777777777777",
  generation: 1,
};

const REFUTER_SECRET = generateSecretKey();
const REFUTER_PUBKEY = getPublicKey(REFUTER_SECRET);
const REFUTER_TARGET = {
  driver: "codex-acp",
  instanceId: "l17-refuter",
  sessionId: "44444444-5555-6666-7777-888888888888",
  generation: 1,
};

const CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: true,
  context: false,
  diff: false,
  plan: true,
};

function seatEvents(input: {
  target: typeof BUILDER_TARGET;
  commandId: string;
  providerSecret: Uint8Array;
  providerPubkey: string;
  title: string;
  beeStamp?: SeatBeeStamp;
  packRef?: PackRef;
}): RelayEvent[] {
  const genesis = finalizeEvent(
    (() => {
      const built = buildCodingSessionGenesisEvent({
        channelId: CHANNEL_ID,
        sessionRef: SESSION_REF,
      });
      return {
        kind: built.kind,
        created_at: BASE_CREATED_AT - 3,
        tags: built.tags,
        content: built.content,
      };
    })(),
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const create = finalizeEvent(
    (() => {
      const built = buildCodingSessionCreateEvent({
        channelId: CHANNEL_ID,
        commandId: input.commandId,
        projectRef: null,
        repoRef: null,
        sessionRef: SESSION_REF,
        genesisRef: genesis.id,
        providerInstanceRef: input.commandId,
        providerAuthorityPubkey: input.providerPubkey,
        model: "sonnet",
        title: input.title,
        initialTurn: null,
      });
      return {
        kind: built.kind,
        created_at: BASE_CREATED_AT - 2,
        tags: built.tags,
        content: built.content,
      };
    })(),
    FOUNDER_SECRET,
  ) as unknown as RelayEvent;
  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: BASE_CREATED_AT - 1,
      tags: [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", input.commandId],
        ["csl-key", lifecycleReceiptSemanticKey(input.commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: input.commandId,
        status: "created",
        session: input.target,
        error: null,
      }),
    },
    input.providerSecret,
  ) as unknown as RelayEvent;
  const metadata = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: BASE_CREATED_AT,
      tags: [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: null,
        repoRef: null,
        title: input.title,
        agentRef: null,
        provider: input.target.driver,
        runtime: input.target.driver,
        model: "sonnet",
        status: "running",
        branch: null,
        capabilities: CAPABILITIES,
        sessionRef: SESSION_REF,
        ...(input.beeStamp ? { beeStamp: input.beeStamp } : {}),
        ...(input.packRef ? { packRef: input.packRef } : {}),
      }),
    },
    input.providerSecret,
  ) as unknown as RelayEvent;
  return [genesis, create, receipt, metadata];
}

test("a real, decoded kind:44223 beeStamp reaches the chip through the running app", async ({
  page,
}) => {
  // Before the bridge installs: it reads this at boot and signs as this key,
  // so the two seeded seats below are founded by the person driving the app.
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, FOUNDER_IDENTITY);
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: BUILDER_PUBKEY, label: "builder" },
        { pubkey: REFUTER_PUBKEY, label: "refuter" },
      ],
    },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toBeVisible();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    {
      channelName: CHANNEL_NAME,
      events: [
        ...seatEvents({
          target: BUILDER_TARGET,
          commandId: "l17-seat-builder",
          providerSecret: BUILDER_SECRET,
          providerPubkey: BUILDER_PUBKEY,
          title: "Advance Buzz live sessions",
          beeStamp: {
            path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
            source: "bundled",
            version: "0.1.0",
            sha: "23728227b",
            dirty: false,
          },
          packRef: PACK_REF,
        }),
        // No `beeStamp` and no `packRef` at all — an older host. The bee
        // chip stays silent for this seat (never "bee build unknown"), but
        // the pack line still shows: `derivePackRefs` covers every
        // execution the umbrella carries, so this seat's honest reading is
        // `no pack staged`, not silence — LANE-L23's own disclosure rule.
        ...seatEvents({
          target: REFUTER_TARGET,
          commandId: "l17-seat-refuter",
          providerSecret: REFUTER_SECRET,
          providerPubkey: REFUTER_PUBKEY,
          title: "Advance Buzz live sessions",
        }),
      ],
    },
  );

  await expect(
    page.getByTestId("channel-coding-sessions-trigger"),
  ).toHaveAttribute("aria-label", "Coding sessions (1)", { timeout: 15_000 });
  await page.getByTestId("channel-coding-sessions-trigger").click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(
    page.getByTestId("coding-session-umbrella-workspace"),
  ).toBeVisible({ timeout: 15_000 });

  // Multi-execution sessions default to Conversation; the participant bar
  // this lane wired is Mission-only.
  await page.getByRole("button", { name: "Mission lens" }).click();
  const bar = page.getByTestId("coding-session-participant-bar");
  await expect(bar).toBeVisible({ timeout: 15_000 });

  const chips = bar.getByTestId("coding-session-seat-bee");
  await expect(chips).toHaveCount(1);
  await expect(chips).toHaveText("bee 23728227b (bundled)");
  await capture(page, bar, "06-seat-bee-real-workspace");

  // LANE-L23: the pack line reaches the same chip through the same real
  // pipeline — `derivePackRefs` folds every execution the umbrella carries,
  // so the builder shows its staged pack and the refuter (no `packRef` on
  // its own 44223) shows the honest `no pack staged`, not silence.
  const packLines = bar.getByTestId("coding-session-seat-pack");
  await expect(packLines).toHaveCount(2);
  const packLineTexts = (await packLines.allTextContents()).sort();
  expect(packLineTexts).toEqual(
    ["no pack staged", "pack builder@23728227"].sort(),
  );
  // No second capture here: the pack lines are already on the same `bar`
  // element `06-seat-bee-real-workspace` captured above — a second shot of
  // the same locator with nothing changed would be the identical-pixels
  // regression this repo's own screenshot rule guards against.
});
