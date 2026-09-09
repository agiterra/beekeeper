import assert from "node:assert/strict";
import test from "node:test";

import { resolveNewCodingSessionDialogRoute } from "./NewCodingSessionDialogHost.tsx";

/**
 * Which dialog a request opens is a placement decision, not a cosmetic one.
 *
 * The project wrapper is where `projectRef` (the coordinate the create signs
 * as the session's placement) and `repoRef` (its repository binding) are
 * resolved. A workspace request that skipped it because its directory was
 * already chosen would create a session that runs in the right folder and
 * belongs to nothing — which is why the workspace travels *through* the
 * wrapper rather than around it.
 */

const WORKSPACE = { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" };

test("a workspace request inside a project keeps the project's dialog", () => {
  assert.deepEqual(
    resolveNewCodingSessionDialogRoute({
      kind: "workspace",
      channelId: "c1",
      projectId: "30621:owner:p",
      sessionRef: "s1",
      workspace: WORKSPACE,
    }),
    {
      kind: "project",
      projectId: "30621:owner:p",
      sourceRepoRef: null,
      // …and the directory arrives there rather than being dropped at the
      // fork.
      workspaceReuse: WORKSPACE,
    },
  );
});

test("a workspace request with no project opens the plain dialog with the directory", () => {
  assert.deepEqual(
    resolveNewCodingSessionDialogRoute({
      kind: "workspace",
      channelId: "c1",
      projectId: null,
      sessionRef: "s1",
      workspace: WORKSPACE,
    }),
    { kind: "channel", channelId: "c1", workspaceReuse: WORKSPACE },
  );
});

test("a workspace request with an empty project id is not a project request", () => {
  const route = resolveNewCodingSessionDialogRoute({
    kind: "workspace",
    channelId: null,
    projectId: "",
    sessionRef: "s1",
    workspace: WORKSPACE,
  });

  assert.equal(route.kind, "channel");
  assert.equal(route.channelId, undefined);
  assert.deepEqual(route.workspaceReuse, WORKSPACE);
});

test("an ordinary project request carries no workspace", () => {
  assert.deepEqual(
    resolveNewCodingSessionDialogRoute({
      kind: "project",
      projectId: "30621:owner:p",
    }),
    {
      kind: "project",
      projectId: "30621:owner:p",
      sourceRepoRef: null,
      workspaceReuse: null,
    },
  );
});

test("an ordinary channel request carries no workspace", () => {
  assert.deepEqual(
    resolveNewCodingSessionDialogRoute({ kind: "channel", channelId: "c1" }),
    { kind: "channel", channelId: "c1", workspaceReuse: null },
  );
  assert.deepEqual(
    resolveNewCodingSessionDialogRoute({ kind: "channel", channelId: null }),
    { kind: "channel", channelId: undefined, workspaceReuse: null },
  );
});
