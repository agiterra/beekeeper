import assert from "node:assert/strict";
import test from "node:test";

import {
  DraftUnconfirmedError,
  PluginPartialWriteError,
  executePluginDrafts,
} from "./pluginDrafts.ts";

const PROJECT = `30621:${"1".repeat(64)}:demo`;

function draft(group, ifRefused, extra = {}) {
  return {
    kind: 44248,
    createdAt: 100,
    tags: [
      ["a", PROJECT],
      ["td-op", group],
    ],
    content: `{"g":"${group}"}`,
    group,
    ifRefused,
    ...extra,
  };
}

/** Publishes in order; refuses the drafts whose content is in `refuse`. */
function relay(refuse = []) {
  const seen = [];
  return {
    seen,
    effects: {
      publish: async (d) => {
        seen.push(d.content);
        if (refuse.includes(d.content)) throw new Error("relay said no");
        return { id: `id-${seen.length}` };
      },
    },
  };
}

test("every draft lands in order and each is reported with its group", async () => {
  const r = relay();
  const run = await executePluginDrafts(
    [draft("issue", "abort"), draft("status", "skip-group")],
    PROJECT,
    r.effects,
  );
  assert.deepEqual(run.published, [
    { eventId: "id-1", group: "issue" },
    { eventId: "id-2", group: "status" },
  ]);
  assert.deepEqual(run.failed, []);
  assert.equal(run.unconfirmed, null);
});

test("an abort refusal before anything landed is the relay's own error, nothing more", async () => {
  const r = relay(['{"g":"issue"}']);
  await assert.rejects(
    executePluginDrafts(
      [draft("issue", "abort"), draft("x", "abort")],
      PROJECT,
      r.effects,
    ),
    (error) =>
      !(error instanceof PluginPartialWriteError) &&
      error.message === "relay said no",
  );
  assert.equal(r.seen.length, 1, "nothing after the refusal is attempted");
});

test("an abort refusal after something landed is partial and says what landed", async () => {
  const done = draft("status", "abort", { content: "done" });
  const status = draft("status", "abort", {
    content: "status",
    partialSummary:
      "Only part of the change was confirmed: the item is marked “done”, but the relay did not confirm the status “canceled”",
  });
  const r = relay(["status"]);
  await assert.rejects(
    executePluginDrafts([done, status], PROJECT, r.effects),
    (error) => {
      assert.ok(error instanceof PluginPartialWriteError);
      assert.match(
        error.message,
        /marked “done”, but the relay did not confirm/,
      );
      assert.match(error.message, /relay said no/);
      assert.deepEqual(error.published, [{ eventId: "id-1", group: "status" }]);
      return true;
    },
  );
});

test("skip-group reports the group, skips the rest of it, and carries on", async () => {
  const drafts = [
    draft("issue", "abort", { content: "add" }),
    draft("status", "skip-group", { content: "done" }),
    draft("status", "skip-group", { content: "status" }),
    draft("priority", "skip-group", { content: "priority" }),
  ];
  const r = relay(["done"]);
  const run = await executePluginDrafts(drafts, PROJECT, r.effects);
  assert.deepEqual(
    r.seen,
    ["add", "done", "priority"],
    "the status op is skipped",
  );
  assert.deepEqual(run.failed, [{ group: "status", message: "relay said no" }]);
  assert.deepEqual(
    run.published.map((p) => p.group),
    ["issue", "priority"],
  );
});

test("a skip-group refusal after part of its group landed uses the partial summary", async () => {
  const drafts = [
    draft("issue", "abort", { content: "add" }),
    draft("status", "skip-group", { content: "done" }),
    draft("status", "skip-group", {
      content: "status",
      partialSummary: "Marked done; status not confirmed",
    }),
  ];
  const run = await executePluginDrafts(
    drafts,
    PROJECT,
    relay(["status"]).effects,
  );
  assert.deepEqual(run.failed, [
    {
      group: "status",
      message: "Marked done; status not confirmed (relay said no)",
    },
  ]);
});

test("nothing is signed when any draft is a foreign kind or outside the project", async () => {
  for (const bad of [
    draft("x", "abort", { kind: 1 }),
    draft("x", "abort", { tags: [["a", `30621:${"2".repeat(64)}:other`]] }),
    draft("x", "abort", {
      tags: [
        ["a", PROJECT],
        ["a", PROJECT],
      ],
    }),
    draft("x", "abort", { tags: [] }),
  ]) {
    const r = relay();
    await assert.rejects(
      executePluginDrafts([draft("ok", "abort"), bad], PROJECT, r.effects),
      /Nothing was saved/,
    );
    assert.equal(r.seen.length, 0, "not even the good draft before it");
  }
});

test("a publish that may still land stops the run and is reported as unconfirmed, never refused", async () => {
  const seen = [];
  const effects = {
    publish: async (d) => {
      seen.push(d.content);
      if (d.content === "add")
        throw new DraftUnconfirmedError("no answer in time");
      return { id: `id-${seen.length}` };
    },
  };
  const run = await executePluginDrafts(
    [
      draft("issue", "abort", { content: "list" }),
      draft("issue", "abort", { content: "add" }),
      draft("status", "skip-group", { content: "done" }),
      draft("priority", "skip-group", { content: "priority" }),
    ],
    PROJECT,
    effects,
  );
  assert.deepEqual(seen, ["list", "add"], "nothing after the uncertain draft");
  assert.equal(run.unconfirmed?.group, "issue");
  assert.match(run.unconfirmed?.message ?? "", /may still land/);
  assert.deepEqual(run.notAttempted, ["status", "priority"]);
  assert.deepEqual(run.failed, []);
});
