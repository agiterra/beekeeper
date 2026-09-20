import assert from "node:assert/strict";
import test from "node:test";

import {
  loadProjectActions,
  projectActionChannelIds,
} from "./useProjectActions.ts";

const OWNER = "3d".repeat(32);
const ADDRESS = `30621:${OWNER}:pivot-test`;
const TRANSPORT = "11111111-2222-4333-8444-555555555555";

const project = { address: ADDRESS, channelIds: [] };

test("a session transport channel that names the project is in the set", () => {
  // Ledger 171(a): this is the channel `bee actions publish --channel` named
  // for Pivot Test, and the one `useChannelsQuery()` hides by default.
  assert.deepEqual(
    projectActionChannelIds(project, [
      { id: TRANSPORT, isMember: true, projectRef: ADDRESS },
      { id: "other", isMember: true, projectRef: null },
    ]),
    [TRANSPORT],
  );
});

test("a declared channel counts even without a project back-reference", () => {
  assert.deepEqual(
    projectActionChannelIds({ address: ADDRESS, channelIds: ["declared"] }, [
      { id: "declared", isMember: true, projectRef: null },
    ]),
    ["declared"],
  );
});

test("a channel this reader is not in is not asked about", () => {
  assert.deepEqual(
    projectActionChannelIds(project, [
      { id: TRANSPORT, isMember: false, projectRef: ADDRESS },
    ]),
    [],
  );
  assert.deepEqual(projectActionChannelIds(null, []), []);
});

test("an empty channel set refuses rather than answering 'no actions'", async () => {
  // The 171(a) regression itself: `[]` here rendered identically to a read
  // that found nothing published.
  await assert.rejects(
    () => loadProjectActions(ADDRESS, []),
    /no channel of this project is readable from here/,
  );
});
