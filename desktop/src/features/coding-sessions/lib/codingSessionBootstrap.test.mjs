import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA,
  getCodingSessionPopoutBootstrap,
  parseCodingSessionPopoutBootstrap,
  rememberCodingSessionPopoutBootstrap,
  resetCodingSessionPopoutBootstrapCache,
} from "./codingSessionBootstrap.ts";

const relayEvent = {
  id: "signed-event-1",
  pubkey: "a".repeat(64),
  created_at: 1,
  kind: 9,
  tags: [["h", "channel-1"]],
  content: "{}",
  sig: "b".repeat(128),
};

test("bootstrap cache returns only the exact channel and generation", () => {
  rememberCodingSessionPopoutBootstrap({
    channelId: "channel-1",
    generationId: "generation-2",
    authorityIdentity: "authority-1",
    relayEvents: [relayEvent],
  });

  assert.deepEqual(
    getCodingSessionPopoutBootstrap("channel-1", "generation-2"),
    {
      schema: CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA,
      channelId: "channel-1",
      generationId: "generation-2",
      authorityIdentity: "authority-1",
      relayEvents: [relayEvent],
    },
  );
  assert.equal(
    getCodingSessionPopoutBootstrap("channel-1", "generation-3"),
    null,
  );
});

test("native bootstrap decoder never falls forward to another generation", () => {
  const value = {
    schema: CODING_SESSION_POPOUT_BOOTSTRAP_SCHEMA,
    channelId: "channel-1",
    generationId: "generation-2",
    authorityIdentity: "authority-1",
    relayEvents: [relayEvent],
  };

  assert.equal(
    parseCodingSessionPopoutBootstrap(value, {
      channelId: "channel-1",
      generationId: "generation-3",
    }),
    null,
  );
  assert.deepEqual(
    parseCodingSessionPopoutBootstrap(value, {
      channelId: "channel-1",
      generationId: "generation-2",
    }),
    value,
  );
});

test("a snapshot without an authority identity or events is never remembered", () => {
  resetCodingSessionPopoutBootstrapCache();

  rememberCodingSessionPopoutBootstrap({
    channelId: "channel-1",
    generationId: "no-authority",
    authorityIdentity: null,
    relayEvents: [relayEvent],
  });
  rememberCodingSessionPopoutBootstrap({
    channelId: "channel-1",
    generationId: "no-events",
    authorityIdentity: "authority-1",
    relayEvents: [],
  });

  assert.equal(
    getCodingSessionPopoutBootstrap("channel-1", "no-authority"),
    null,
  );
  assert.equal(getCodingSessionPopoutBootstrap("channel-1", "no-events"), null);
});

test("the cache is bounded to 64 generations, evicting least recent first", () => {
  resetCodingSessionPopoutBootstrapCache();
  const bound = 64;
  const overflow = 5;

  for (let index = 0; index < bound + overflow; index += 1) {
    rememberCodingSessionPopoutBootstrap({
      channelId: "channel-1",
      generationId: `generation-${index}`,
      authorityIdentity: "authority-1",
      relayEvents: [{ ...relayEvent, id: `signed-event-${index}` }],
    });
  }

  for (let index = 0; index < overflow; index += 1) {
    assert.equal(
      getCodingSessionPopoutBootstrap("channel-1", `generation-${index}`),
      null,
      `generation-${index} must have been evicted`,
    );
  }
  assert.ok(
    getCodingSessionPopoutBootstrap("channel-1", `generation-${overflow}`),
    "the oldest surviving generation must still be readable",
  );
  assert.ok(
    getCodingSessionPopoutBootstrap(
      "channel-1",
      `generation-${bound + overflow - 1}`,
    ),
  );
});

test("re-remembering a generation refreshes it instead of aging it out", () => {
  resetCodingSessionPopoutBootstrapCache();
  const remember = (generationId) =>
    rememberCodingSessionPopoutBootstrap({
      channelId: "channel-1",
      generationId,
      authorityIdentity: "authority-1",
      relayEvents: [relayEvent],
    });

  remember("pinned");
  for (let index = 0; index < 64; index += 1) {
    if (index === 32) remember("pinned");
    remember(`generation-${index}`);
  }

  assert.ok(
    getCodingSessionPopoutBootstrap("channel-1", "pinned"),
    "a refreshed generation must outlive the ones inserted before it",
  );
});

test("the cache is cleared on a community boundary", () => {
  rememberCodingSessionPopoutBootstrap({
    channelId: "channel-1",
    generationId: "generation-2",
    authorityIdentity: "authority-1",
    relayEvents: [relayEvent],
  });
  resetCodingSessionPopoutBootstrapCache();

  assert.equal(
    getCodingSessionPopoutBootstrap("channel-1", "generation-2"),
    null,
  );
});
