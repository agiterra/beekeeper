import assert from "node:assert/strict";
import test from "node:test";
import { QueryClient } from "@tanstack/react-query";
import { projectAcceptedMessage } from "./acceptedMessageCache.ts";
import { channelWindowKey, threadRepliesKey } from "./messageQueryKeys.ts";
import { emptyChannelWindowStore } from "./channelWindowStore.ts";

const event = (id, tags = []) => ({
  id,
  kind: 9,
  pubkey: "a".repeat(64),
  created_at: 20,
  content: id,
  tags,
  sig: "",
});
test("accepted nested reply appears without a live echo and preserves siblings and other channels", () => {
  const client = new QueryClient();
  const key = threadRepliesKey("channel", "root");
  const other = threadRepliesKey("other", "root");
  client.setQueryData(key, [event("sibling")]);
  client.setQueryData(other, [event("other-channel")]);
  client.setQueryData(channelWindowKey("channel"), {
    ...emptyChannelWindowStore(),
    liveOverlay: [event("pending"), event("concurrent")],
  });
  const reply = event("accepted", [
    ["h", "channel"],
    ["e", "root", "", "root"],
    ["e", "parent", "", "reply"],
  ]);
  projectAcceptedMessage(client, "channel", "pending", reply);
  assert.deepEqual(
    client
      .getQueryData(key)
      .map((value) => value.id)
      .sort(),
    ["accepted", "sibling"],
  );
  assert.equal(
    client.getQueryData(key).find((value) => value.id === "accepted").localKey,
    "pending",
  );
  assert.deepEqual(
    client.getQueryData(other).map((value) => value.id),
    ["other-channel"],
  );
  assert.deepEqual(
    client
      .getQueryData(channelWindowKey("channel"))
      .liveOverlay.map((value) => value.id)
      .sort(),
    ["accepted", "concurrent"],
  );
  projectAcceptedMessage(client, "channel", "pending", reply);
  assert.equal(
    client.getQueryData(key).filter((value) => value.id === "accepted").length,
    1,
  );
  client.clear();
});
test("accepted channel message does not manufacture a thread", () => {
  const client = new QueryClient();
  projectAcceptedMessage(
    client,
    "channel",
    "pending",
    event("root", [["h", "channel"]]),
  );
  assert.equal(
    client.getQueriesData({ queryKey: ["thread-replies"] }).length,
    0,
  );
  client.clear();
});
