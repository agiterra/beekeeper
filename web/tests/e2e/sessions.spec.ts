import { expect, test } from "@playwright/test";

const CHANNEL_ID = "11111111-1111-4111-8111-111111111111";
const PUBKEY = "a".repeat(64);

type RelayEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: string[][];
  content: string;
  sig: string;
};

function repoEvent(id: string, tags: string[][]): RelayEvent {
  return {
    id: id.padEnd(64, "0"),
    pubkey: PUBKEY,
    created_at: 1_700_000_000,
    kind: 30617,
    tags,
    content: "",
    sig: "0".repeat(128),
  };
}

/**
 * Serve a fixed event set to every REQ.
 *
 * The relay never answers in a smoke run, so without this the repo page is
 * permanently empty and the session surfaces never mount.
 */
async function installMockRelay(
  page: import("@playwright/test").Page,
  events: RelayEvent[],
  refusal: string | null = null,
) {
  await page.addInitScript(
    ({
      seeded,
      refuseWith,
    }: {
      seeded: RelayEvent[];
      refuseWith: string | null;
    }) => {
      class MockWebSocket {
        static readonly CONNECTING = 0;
        static readonly OPEN = 1;
        static readonly CLOSING = 2;
        static readonly CLOSED = 3;
        readyState = 1;
        private handlers = new Map<string, ((event: unknown) => void)[]>();
        constructor(public url: string) {
          setTimeout(() => this.emit("open", {}), 0);
        }
        addEventListener(type: string, fn: (event: unknown) => void) {
          this.handlers.set(type, [...(this.handlers.get(type) ?? []), fn]);
        }
        removeEventListener() {}
        private emit(type: string, event: unknown) {
          for (const fn of this.handlers.get(type) ?? []) fn(event);
        }
        send(raw: string) {
          const message = JSON.parse(raw) as [
            string,
            string,
            { kinds?: number[] },
          ];
          if (message[0] !== "REQ") return;
          const [, subId, filter] = message;
          // Only the coding-session reads are refused: the repo event itself
          // must still resolve, or the panel never mounts and the test would
          // pass on a blank page.
          const refusable = (filter.kinds ?? []).some(
            (kind) => kind >= 44_220 || kind === 24_223,
          );
          if (refuseWith !== null && refusable) {
            this.emit("message", {
              data: JSON.stringify(["CLOSED", subId, refuseWith]),
            });
            return;
          }
          for (const event of seeded) {
            if (filter.kinds && !filter.kinds.includes(event.kind)) continue;
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
      (window as unknown as { WebSocket: unknown }).WebSocket = MockWebSocket;
    },
    { seeded: events, refuseWith: refusal },
  );
}

test("repo detail exposes a Sessions tab", async ({ page }) => {
  await installMockRelay(page, [
    repoEvent("repo1", [
      ["d", "linked-repo"],
      ["name", "Linked repo"],
      ["buzz-channel", CHANNEL_ID],
    ]),
  ]);
  await page.goto("/repos/linked-repo");
  await expect(
    page.getByRole("heading", { name: "Linked repo" }),
  ).toBeVisible();
  await page.getByTestId("repo-tab-sessions").click();
  // The panel specifically. A disjunction that admitted the error state would
  // stay green with the whole surface dead, which is what it is here to catch;
  // the failure path is its own test below.
  await expect(page.getByTestId("coding-sessions-panel")).toBeVisible();
  await expect(page.getByTestId("coding-sessions-error")).toHaveCount(0);
});

test("a relay that refuses the read says so instead of an empty list", async ({
  page,
}) => {
  await installMockRelay(
    page,
    [
      repoEvent("repo4", [
        ["d", "linked-repo"],
        ["name", "Linked repo"],
        ["buzz-channel", CHANNEL_ID],
      ]),
    ],
    "restricted: not a member of this channel",
  );
  await page.goto("/repos/linked-repo/sessions");
  const error = page.getByTestId("coding-sessions-error");
  await expect(error).toBeVisible();
  await expect(error).toContainText("Could not read coding sessions");
  await expect(error).toContainText("restricted: not a member of this channel");
  await expect(page.getByTestId("coding-sessions-panel")).toHaveCount(0);
});

test("a repo with no channel says so instead of showing an empty list", async ({
  page,
}) => {
  await installMockRelay(page, [
    repoEvent("repo2", [
      ["d", "orphan-repo"],
      ["name", "Orphan repo"],
    ]),
  ]);
  await page.goto("/repos/orphan-repo/sessions");
  await expect(
    page.getByRole("heading", { name: "Coding sessions" }),
  ).toBeVisible();
  await expect(page.getByTestId("coding-sessions-no-channel")).toBeVisible();
  await expect(page.getByText("No session channel")).toBeVisible();
});

test("the session route renders a resolvable state for an unknown ref", async ({
  page,
}) => {
  await installMockRelay(page, [
    repoEvent("repo3", [
      ["d", "linked-repo"],
      ["name", "Linked repo"],
      ["buzz-channel", CHANNEL_ID],
    ]),
  ]);
  await page.goto("/repos/linked-repo/sessions/not-a-session");
  await expect(page.getByText("All coding sessions")).toBeVisible();
  await expect(page.getByTestId("coding-session-not-found")).toBeVisible();
  await expect(page.getByTestId("coding-sessions-error")).toHaveCount(0);
});
