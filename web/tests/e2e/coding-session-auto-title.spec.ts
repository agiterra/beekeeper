/**
 * SV-31 on the web observer: a session nobody named reads its provider's
 * generated title (kind 44252) with a muted "Auto-named" marker whose tooltip
 * names the provider and model; the founder's rename (44229) replaces it and
 * the marker goes; a title signed by a provider with no execution in the
 * session never names it — the founding execution's title stands, and the
 * page says one title was ignored.
 *
 * Every coding-session event is REALLY signed with the domain tests' fixture
 * helpers (the observer verifies signatures and drops anything else). The
 * relay is the mocked-WebSocket pattern of `coding-session-prose-join.spec.ts`,
 * with a hook to push one event into the open subscriptions mid-test, the way
 * a live relay delivers a rename.
 *
 * Screenshots (`test-results/sv31/`), scoped to the session summary and
 * hash-distinct: `sv31-web-auto-named.png`, `sv31-web-renamed.png`,
 * `sv31-web-foreign-signer-fallback.png`.
 */
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { expect, type Page, test } from "@playwright/test";

import {
  CHANNEL_ID,
  createEvent,
  generatedTitleEvent,
  genesisEvent,
  leaseEvent,
  metadataEvent,
  nameEvent,
  newSigner,
  receiptEvent,
  SESSION_REF,
} from "../../src/features/coding-sessions/domain/testFixtures.mjs";

type RelayEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

type Signer = ReturnType<typeof newSigner>;

const SHOT_DIR = resolve(process.cwd(), "test-results", "sv31");
const SESSION_URL = `/repos/linked-repo/sessions/${SESSION_REF}`;
const shots = new Map<string, string>();

function repoEvent(): RelayEvent {
  return {
    id: "repo-auto-title".padEnd(64, "0"),
    pubkey: "a".repeat(64),
    created_at: 1_700_000_000,
    kind: 30617,
    tags: [
      ["d", "linked-repo"],
      ["name", "Linked repo"],
      ["buzz-channel", CHANNEL_ID],
    ],
    content: "",
    sig: "0".repeat(128),
  };
}

async function installMockRelay(page: Page, events: RelayEvent[]) {
  await page.addInitScript((seeded: RelayEvent[]) => {
    type Filter = { kinds?: number[] };
    const sockets: MockWebSocket[] = [];
    class MockWebSocket {
      static readonly CONNECTING = 0;
      static readonly OPEN = 1;
      static readonly CLOSING = 2;
      static readonly CLOSED = 3;
      readyState = 1;
      private handlers = new Map<string, ((event: unknown) => void)[]>();
      private subscriptions = new Map<string, Filter[]>();
      constructor(public url: string) {
        sockets.push(this);
        setTimeout(() => this.emit("open", {}), 0);
      }
      addEventListener(type: string, fn: (event: unknown) => void) {
        this.handlers.set(type, [...(this.handlers.get(type) ?? []), fn]);
      }
      removeEventListener() {}
      private emit(type: string, event: unknown) {
        for (const fn of this.handlers.get(type) ?? []) fn(event);
      }
      private matches(filters: Filter[], event: RelayEvent) {
        return filters.some(
          (filter) => !filter.kinds || filter.kinds.includes(event.kind),
        );
      }
      push(event: RelayEvent) {
        for (const [subId, filters] of this.subscriptions) {
          if (!this.matches(filters, event)) continue;
          this.emit("message", {
            data: JSON.stringify(["EVENT", subId, event]),
          });
        }
      }
      send(raw: string) {
        const message = JSON.parse(raw) as [string, string, ...Filter[]];
        if (message[0] === "CLOSE") {
          this.subscriptions.delete(message[1]);
          return;
        }
        if (message[0] !== "REQ") return;
        const [, subId, ...filters] = message;
        this.subscriptions.set(subId, filters);
        for (const event of seeded) {
          if (!this.matches(filters, event)) continue;
          this.emit("message", {
            data: JSON.stringify(["EVENT", subId, event]),
          });
        }
        this.emit("message", { data: JSON.stringify(["EOSE", subId]) });
      }
      close() {
        this.readyState = 3;
      }
    }
    const scope = window as unknown as {
      WebSocket: unknown;
      __pushRelayEvent__: (event: RelayEvent) => void;
    };
    scope.WebSocket = MockWebSocket;
    scope.__pushRelayEvent__ = (event) => {
      for (const socket of sockets) socket.push(event);
    };
  }, events);
}

/** Wait for every running animation, then a frame, before capturing. */
async function waitForAnimations(page: Page) {
  await page.evaluate(async () => {
    await Promise.all(
      document.getAnimations().map((animation) => animation.finished),
    );
    await new Promise((done) => requestAnimationFrame(() => done(null)));
  });
}

/** A governed session: genesis, the founder's create, receipt, metadata, lease. */
function governedSession(founder: Signer, provider: Signer): RelayEvent[] {
  const nowSeconds = Math.floor(Date.now() / 1000);
  const genesis = genesisEvent(founder, { sessionRef: SESSION_REF });
  return [
    repoEvent(),
    genesis as RelayEvent,
    createEvent(founder, {
      sessionRef: SESSION_REF,
      genesisRef: genesis.id,
      providerAuthorityPubkey: provider.pubkey,
      title: "Fix the login redirect",
    }) as RelayEvent,
    receiptEvent(provider, { status: "created" }) as RelayEvent,
    metadataEvent(provider, {
      status: "running",
      sessionRef: SESSION_REF,
      runtime: "claude-agent-acp",
      model: "opus",
      title: "Fix the login redirect",
      created_at: nowSeconds - 40,
    }) as RelayEvent,
    leaseEvent(provider, {
      state: "live",
      leaseSequence: 3,
      created_at: nowSeconds - 5,
    }) as RelayEvent,
  ];
}

async function captureSummary(page: Page, name: string) {
  const summary = page.getByTestId("coding-session-detail-summary");
  await expect(summary).toBeVisible();
  // The tooltip must be closed: the shot is of the page, not of a hover.
  await page.mouse.move(0, 0);
  await expect(
    page.getByTestId("coding-session-title-origin-detail"),
  ).toHaveCount(0);
  await waitForAnimations(page);
  const png = await summary.screenshot();
  mkdirSync(SHOT_DIR, { recursive: true });
  writeFileSync(resolve(SHOT_DIR, `${name}.png`), png);
  shots.set(name, createHash("sha256").update(png).digest("hex"));
}

test("an auto-named session says so, and a rename replaces it with no marker", async ({
  page,
}) => {
  const founder = newSigner();
  const provider = newSigner();
  await installMockRelay(page, [
    ...governedSession(founder, provider),
    generatedTitleEvent(provider, {
      title: "Login redirect fix",
      model: "claude-haiku-4-5",
    }) as RelayEvent,
  ]);
  await page.goto(SESSION_URL);

  const name = page.getByTestId("coding-session-detail-name");
  const marker = page.getByTestId("coding-session-title-origin");
  await expect(name).toHaveText("Login redirect fix");
  await expect(marker).toHaveText("Auto-named");

  await marker.hover();
  const tooltip = page.getByTestId("coding-session-title-origin-detail");
  await expect(tooltip).toContainText(
    "Named automatically from the first message by Claude Code · opus",
  );
  await expect(tooltip).toContainText(provider.pubkey.slice(0, 8));
  await expect(tooltip).toContainText("· claude-haiku-4-5");

  await captureSummary(page, "sv31-web-auto-named");

  // The founder renames it, live. A person's name outranks every generated
  // title however old, and carries no marker.
  const rename = nameEvent(founder, {
    sessionRef: SESSION_REF,
    name: "Auth rework",
    created_at: Math.floor(Date.now() / 1000),
  });
  await page.evaluate((event) => {
    (
      window as unknown as { __pushRelayEvent__: (event: unknown) => void }
    ).__pushRelayEvent__(event);
  }, rename);

  await expect(name).toHaveText("Auth rework");
  await expect(marker).toHaveCount(0);
  await captureSummary(page, "sv31-web-renamed");

  // The session list reads the same resolver.
  await page.goto("/repos/linked-repo/sessions");
  await expect(page.getByTestId("coding-session-row-name")).toHaveText(
    "Auth rework",
  );
  await expect(page.getByTestId("coding-session-title-origin")).toHaveCount(0);
});

test("a title from a provider with no execution here never names the session", async ({
  page,
}) => {
  const founder = newSigner();
  const provider = newSigner();
  const stranger = newSigner();
  await installMockRelay(page, [
    ...governedSession(founder, provider),
    generatedTitleEvent(stranger, {
      title: "Not this provider's to name",
      created_at: 1,
    }) as RelayEvent,
  ]);
  await page.goto(SESSION_URL);

  await expect(page.getByTestId("coding-session-detail-name")).toHaveText(
    "Fix the login redirect",
  );
  await expect(page.getByTestId("coding-session-title-origin")).toHaveCount(0);
  await expect(page.getByTestId("coding-session-name-set-aside")).toHaveText(
    "Ignored 1 generated title from a provider outside this session.",
  );
  await captureSummary(page, "sv31-web-foreign-signer-fallback");

  await page.goto("/repos/linked-repo/sessions");
  await expect(page.getByTestId("coding-session-row-name")).toHaveText(
    "Fix the login redirect",
  );
  await expect(page.getByTestId("coding-session-title-origin")).toHaveCount(0);
});

test.afterAll(() => {
  // Runs once both tests above have written their shots: three states, three
  // distinct images. Identical hashes mean two shots captured one state.
  if (shots.size !== 3) return;
  const hashes = [...shots.values()];
  expect(new Set(hashes).size).toBe(hashes.length);
  for (const [name, hash] of shots) console.log(`${hash}  ${name}.png`);
});
