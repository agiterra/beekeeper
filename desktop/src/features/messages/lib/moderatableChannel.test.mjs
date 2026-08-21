import assert from "node:assert/strict";
import test from "node:test";

import { isModeratableChannelType } from "./moderatableChannel.ts";

test("a DM is never moderatable", () => {
  assert.equal(isModeratableChannelType("dm"), false);
});

test("ordinary channel types are moderatable", () => {
  for (const channelType of ["stream", "forum"]) {
    assert.equal(
      isModeratableChannelType(channelType),
      true,
      `${channelType} must stay moderatable`,
    );
  }
});

test("an unresolved channel type grants nothing", () => {
  // The regression this exists for: the predicate used to be
  // `channelType !== "dm"`, and `undefined !== "dm"` is `true`. Two live
  // callers pass an unresolved type — the inbox detail pane while the channel
  // record loads, and cold-recovered feed items that carry no type at all —
  // so an unresolved DM read as "safe to moderate" and offered a kind:9005
  // delete the relay will refuse. Unknown must fail closed.
  assert.equal(isModeratableChannelType(undefined), false);
  assert.equal(isModeratableChannelType(null), false);
});

test("a channel type outside the union grants nothing", () => {
  // The value is not narrowed at the IPC boundary, and the relay knows types
  // the desktop union does not (e.g. `workflow`). Anything unrecognized is
  // unknown, and unknown fails closed.
  for (const channelType of ["workflow", "", "DM", "stream "]) {
    assert.equal(
      isModeratableChannelType(channelType),
      false,
      `${JSON.stringify(channelType)} must not be treated as moderatable`,
    );
  }
});
