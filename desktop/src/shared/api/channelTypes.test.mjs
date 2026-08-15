import assert from "node:assert/strict";
import test from "node:test";

import { isSessionTransportChannel } from "./channelTypes.ts";

test("only channel_type identifies a session transport — never the name", () => {
  assert.equal(isSessionTransportChannel({ channelType: "transport" }), true);
  for (const channelType of ["stream", "forum", "dm"]) {
    assert.equal(
      isSessionTransportChannel({ channelType }),
      false,
      `${channelType} must not be treated as a transport`,
    );
  }
});
