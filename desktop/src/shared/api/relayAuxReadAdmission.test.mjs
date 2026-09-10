import assert from "node:assert/strict";
import test from "node:test";

const queries = [];
globalThis.window = {
  setTimeout,
  clearTimeout,
  __TAURI_INTERNALS__: {
    invoke: async (command, args) => {
      assert.equal(command, "query_relay_filters");
      queries.push(...args.filters);
      return [];
    },
  },
};
const { RelayClient } = await import("./relayClientFeatureApi.ts");
const { buildChannelStructuralAuxFilter, buildChannelAuxDeletionFilter } =
  await import("./relayChannelFilters.ts");

test("auxiliary reads and deletion closure complete without WebSocket admission, retaining every reference and limit", async () => {
  const client = new RelayClient();
  client.fetchHistory = () =>
    assert.fail("must not wait in the WebSocket read queue");
  const ids = Array.from({ length: 205 }, (_, i) =>
    i.toString(16).padStart(64, "0"),
  );
  for (const [read, builder] of [
    [
      () =>
        client.fetchAuxEventsByReference(
          "channel",
          ids,
          buildChannelStructuralAuxFilter,
        ),
      buildChannelStructuralAuxFilter,
    ],
    [
      () => client.fetchAuxDeletionEventsForAuxEvents("channel", ids),
      buildChannelAuxDeletionFilter,
    ],
  ]) {
    queries.length = 0;
    assert.deepEqual(await read(), []);
    assert.deepEqual(
      queries.flatMap((filter) => filter["#e"]),
      ids,
    );
    for (const filter of queries) {
      assert.ok(filter["#e"].length <= 100);
      assert.deepEqual(filter, builder("channel", filter["#e"]));
    }
  }
});
