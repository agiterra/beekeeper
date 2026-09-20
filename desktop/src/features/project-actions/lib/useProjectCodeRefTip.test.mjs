import assert from "node:assert/strict";
import test from "node:test";

import {
  codeRepositoryIds,
  loadProjectCodeRefTip,
  readDeliveryRefTip,
} from "./useProjectCodeRefTip.ts";

const OWNER = "3d".repeat(32);
const RELAY = "9a".repeat(32);
const TIP = "e6".repeat(20);

test("the agents repository is never the code repository", () => {
  assert.deepEqual(
    codeRepositoryIds([
      `30617:${OWNER}:pivot-test`,
      `30617:${OWNER}:pivot-test-beekeeper-agents`,
    ]),
    ["pivot-test"],
  );
});

test("HEAD picks the delivery ref, then main, then the only branch", () => {
  const base = { created_at: 10, pubkey: RELAY };
  assert.equal(
    readDeliveryRefTip({
      ...base,
      tags: [
        ["d", "pivot-test"],
        ["HEAD", "ref: refs/heads/trunk"],
        ["refs/heads/trunk", TIP],
        ["refs/heads/main", "ab".repeat(20)],
      ],
    }).refName,
    "refs/heads/trunk",
  );
  assert.equal(
    readDeliveryRefTip({
      ...base,
      tags: [
        ["d", "pivot-test"],
        ["refs/heads/main", TIP],
        ["refs/heads/other", "ab".repeat(20)],
      ],
    }).tip,
    TIP,
  );
  assert.equal(
    readDeliveryRefTip({
      ...base,
      tags: [
        ["d", "pivot-test"],
        ["refs/heads/only", TIP],
      ],
    }).tip,
    TIP,
  );
});

test("an ambiguous ref state offers nothing and says why", () => {
  const answer = readDeliveryRefTip({
    created_at: 10,
    pubkey: RELAY,
    tags: [
      ["d", "pivot-test"],
      ["refs/heads/a", TIP],
      ["refs/heads/b", "ab".repeat(20)],
    ],
  });
  assert.equal(answer.tip, null);
  assert.match(answer.reason, /names 2 branches and no HEAD/);
});

test("no relay-signed ref state is a named absence, never a guess", async () => {
  const answer = await loadProjectCodeRefTip([`30617:${OWNER}:pivot-test`], {
    relaySelf: async () => RELAY,
    fetchEvents: async () => [],
  });
  assert.equal(answer.tip, null);
  assert.match(answer.reason, /published no ref state for pivot-test/);
});

test("a pusher-signed ref state is not a delivery observation", async () => {
  const answer = await loadProjectCodeRefTip([`30617:${OWNER}:pivot-test`], {
    relaySelf: async () => RELAY,
    fetchEvents: async () => [
      {
        created_at: 10,
        pubkey: OWNER,
        tags: [
          ["d", "pivot-test"],
          ["refs/heads/main", TIP],
        ],
      },
    ],
  });
  assert.equal(answer.tip, null);
  assert.match(answer.reason, /published no ref state/);
});

test("the newest relay-signed ref state wins", async () => {
  const answer = await loadProjectCodeRefTip([`30617:${OWNER}:pivot-test`], {
    relaySelf: async () => RELAY,
    fetchEvents: async () => [
      {
        created_at: 10,
        pubkey: RELAY,
        tags: [
          ["d", "pivot-test"],
          ["refs/heads/main", "ab".repeat(20)],
        ],
      },
      {
        created_at: 20,
        pubkey: RELAY,
        tags: [
          ["d", "pivot-test"],
          ["refs/heads/main", TIP],
        ],
      },
    ],
  });
  assert.equal(answer.tip, TIP);
  assert.equal(answer.observedAt, 20);
});

test("a project with no code repository, and a relay with no known key", async () => {
  assert.match(
    (await loadProjectCodeRefTip([], { relaySelf: async () => RELAY })).reason,
    /names no code repository/,
  );
  assert.match(
    (
      await loadProjectCodeRefTip([`30617:${OWNER}:pivot-test`], {
        relaySelf: async () => null,
      })
    ).reason,
    /relay's own key is unknown/,
  );
});
