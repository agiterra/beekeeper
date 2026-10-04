/**
 * SV-36 on the web observer: three paragraph pieces of one answer read as one
 * assistant row, with a muted "Writing…" line while the turn is open and the
 * provider holds a live lease; the turn's `result` clears it.
 *
 * Every coding-session event here is REALLY signed (the observer verifies
 * signatures and drops anything else), minted with the domain tests' fixture
 * helpers. The relay is the mocked-WebSocket pattern of `sessions.spec.ts`,
 * extended to read every filter of a REQ and to push one event into the open
 * subscriptions mid-test, the way a live relay delivers a new item.
 *
 * Screenshots: `test-results/sv36/sv36-web-arriving.png` and
 * `sv36-web-settled.png`, scoped to the transcript, hash-distinct.
 */
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { expect, type Page, test } from "@playwright/test";

import {
  CHANNEL_ID,
  createEvent,
  leaseEvent,
  metadataEvent,
  newSigner,
  receiptEvent,
  SESSION_REF,
  transcriptEvent,
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

const SHOT_DIR = resolve(process.cwd(), "test-results", "sv36");

const PARAGRAPHS = [
  "First, the plan: read the failing test and the code it covers.\n\n",
  "Second, the fix: the parser dropped the last token of a quoted string.\n\n",
  "Third, the proof: the test passes and nothing else changed.",
];

function repoEvent(): RelayEvent {
  return {
    id: "repo-prose-join".padEnd(64, "0"),
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

/** The row's body is the plain concatenation, byte for byte (no trim). */
async function expectExactText(row: ReturnType<Page["locator"]>) {
  const body = row.getByTestId("coding-session-transcript-row-text");
  await expect(body).toBeVisible();
  expect(await body.evaluate((element) => element.textContent)).toBe(
    PARAGRAPHS.join(""),
  );
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

test("three paragraph pieces read as one answer that is still arriving, then settle", async ({
  page,
}) => {
  const operator = newSigner();
  const provider = newSigner();
  const nowSeconds = Math.floor(Date.now() / 1000);
  const turnId = "turn-1";
  const piece = (eventSeq: number, item: Record<string, unknown>) =>
    transcriptEvent(provider, {
      eventSeq,
      turnId,
      item,
      timestamp: (nowSeconds - 30 + eventSeq) * 1000,
      created_at: nowSeconds - 30 + eventSeq,
    }) as RelayEvent;

  const seeded: RelayEvent[] = [
    repoEvent(),
    createEvent(operator, {
      sessionRef: SESSION_REF,
      providerAuthorityPubkey: provider.pubkey,
      title: "Prose join",
    }),
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, {
      status: "running",
      sessionRef: SESSION_REF,
      title: "Prose join",
      created_at: nowSeconds - 40,
    }),
    // The provider is answering right now: without this lease nothing may
    // read as arriving (CONTRACT rule 7), whatever the turn state says.
    leaseEvent(provider, {
      state: "live",
      leaseSequence: 3,
      created_at: nowSeconds - 5,
    }),
    piece(1, { kind: "user_prompt", content: "Fix the parser." }),
    ...PARAGRAPHS.map((text, index) =>
      piece(index + 2, { kind: "assistant_text", text }),
    ),
  ];
  await installMockRelay(page, seeded);
  await page.goto(`/repos/linked-repo/sessions/${SESSION_REF}`);

  const transcript = page.getByTestId("coding-session-transcript");
  const assistantRows = transcript.locator(
    '[data-testid="coding-session-transcript-row"][data-role="assistant"]',
  );
  await expect(assistantRows).toHaveCount(1);
  const answer = assistantRows.first();
  await expectExactText(answer);
  await expect(
    answer.getByTestId("coding-session-transcript-arriving"),
  ).toHaveText("Writing…");

  mkdirSync(SHOT_DIR, { recursive: true });
  await waitForAnimations(page);
  const arriving = await transcript.screenshot();
  writeFileSync(resolve(SHOT_DIR, "sv36-web-arriving.png"), arriving);

  const result = piece(5, {
    kind: "result",
    subtype: "success",
    result: "completed",
    durationMs: 4200,
    costUsd: null,
    isError: false,
  });
  await page.evaluate((event) => {
    (
      window as unknown as { __pushRelayEvent__: (event: unknown) => void }
    ).__pushRelayEvent__(event);
  }, result);

  await expect(transcript.getByText("Turn result")).toBeVisible();
  await expect(
    page.getByTestId("coding-session-transcript-arriving"),
  ).toHaveCount(0);
  await expect(assistantRows).toHaveCount(1);
  await expectExactText(answer);

  await waitForAnimations(page);
  const settled = await transcript.screenshot();
  writeFileSync(resolve(SHOT_DIR, "sv36-web-settled.png"), settled);

  const hash = (png: Buffer) => createHash("sha256").update(png).digest("hex");
  expect(hash(arriving)).not.toBe(hash(settled));
});
