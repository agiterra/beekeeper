import assert from "node:assert/strict";
import test from "node:test";

import {
  DRAFT_FUTURE_SECS,
  DRAFT_PAST_SECS,
  DraftUnconfirmedError,
  PluginPartialWriteError,
  executePluginDrafts,
  sealDraft,
} from "./pluginDrafts.ts";

const PROJECT = `30621:${"1".repeat(64)}:demo`;
const ELSEWHERE = `30621:${"2".repeat(64)}:other`;
const LIST = "abcdefabcdefabcdefabcdefabcdefab";
const ITEM = "1234567890abcdef1234567890abcdef";
const MEMBER = "b".repeat(64);
const SCHEMA = "buzz-project-todo/v1";
/** The fixed clock every test seals against. */
const NOW = 1_800_000_000;
const now = () => NOW;

const listTags = (op) => [
  ["a", PROJECT],
  ["td-v", "td1-1"],
  ["td-op", op],
  ["td-list", LIST],
  ["td-vis", "project"],
];

/**
 * A valid `list.title` draft whose title is `marker` (default: the group),
 * so a test can tell drafts apart by what was published.
 */
function draft(group, ifRefused, extra = {}) {
  const { content: marker = group, ...rest } = extra;
  return {
    kind: 44248,
    createdAt: NOW,
    tags: listTags("list.title"),
    content: JSON.stringify({
      schema: SCHEMA,
      op: "list.title",
      listId: LIST,
      title: marker,
    }),
    group,
    ifRefused,
    ...rest,
  };
}

/** An `item.assignee` draft as a relay returns it. */
function assignDraft(assignee, extraTags = []) {
  return {
    kind: 44248,
    createdAt: NOW,
    tags: [...listTags("item.assignee"), ["td-item", ITEM], ...extraTags],
    content: JSON.stringify({
      schema: SCHEMA,
      op: "item.assignee",
      listId: LIST,
      itemId: ITEM,
      assignee,
    }),
    group: "assignee",
    ifRefused: "abort",
  };
}

const markerOf = (d) => JSON.parse(d.content).title;

/** Publishes in order; refuses the drafts whose marker is in `refuse`. */
function relay(refuse = []) {
  const seen = [];
  return {
    seen,
    effects: {
      now,
      publish: async (d) => {
        seen.push(markerOf(d));
        if (refuse.includes(markerOf(d))) throw new Error("relay said no");
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
  const r = relay(["issue"]);
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

test("nothing is signed when any draft is a foreign kind, scope, route or shape", async () => {
  const withTags = (tags) => draft("x", "abort", { tags });
  const base = listTags("list.title");
  for (const bad of [
    draft("x", "abort", { kind: 1 }),
    withTags([...base.slice(1), ["a", ELSEWHERE]]),
    withTags([...base, ["a", PROJECT]]),
    withTags([...base, ["a", ELSEWHERE]]),
    withTags([["a", PROJECT, "wss://elsewhere"], ...base.slice(1)]),
    withTags([...base, ["p", MEMBER]]),
    withTags([...base, ["e", "e".repeat(64)]]),
    withTags([...base, ["h", "a-channel"]]),
    withTags([...base, ["client", "plugin"]]),
    // Tags that disagree with the content: another list, a missing op tag.
    withTags([...base.slice(0, 3), ["td-list", "f".repeat(32)], base[4]]),
    withTags(base.filter((t) => t[0] !== "td-op")),
    // Content that is not a to-do op, or a malformed assignee.
    { ...draft("x", "abort"), content: 42 },
    { ...draft("x", "abort"), content: '{"g":"x"}' },
    assignDraft("not-hex"),
    // Caps: 64 proposed tags become 65 with the scope; a value over 1 KiB.
    withTags(Array.from({ length: 64 }, () => ["td-v", "td1-1"])),
    withTags([...base.slice(0, 4), ["td-vis", "x".repeat(2000)]]),
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
    now,
    publish: async (d) => {
      seen.push(markerOf(d));
      if (markerOf(d) === "add")
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

test("a draft is signed with this app's own scope, whether or not the plugin proposed one", () => {
  const proposed = assignDraft(MEMBER);
  const sealed = sealDraft(proposed, PROJECT, NOW);
  assert.deepEqual(sealed.tags[0], ["a", PROJECT]);
  assert.equal(sealed.tags.filter((t) => t[0] === "a").length, 1);
  const bare = { ...proposed, tags: proposed.tags.slice(1) };
  assert.deepEqual(sealDraft(bare, PROJECT, NOW).tags, sealed.tags);
  assert.equal(
    sealed.content,
    proposed.content,
    "content is signed as drafted",
  );
});

test("a legitimate assignment and an unassignment are both signed", async () => {
  const r = { seen: [] };
  const run = await executePluginDrafts(
    [assignDraft(MEMBER), assignDraft(null)],
    PROJECT,
    {
      now,
      publish: async (d) => {
        r.seen.push(JSON.parse(d.content).assignee);
        return { id: `id-${r.seen.length}` };
      },
    },
  );
  assert.deepEqual(r.seen, [MEMBER, null]);
  assert.equal(run.published.length, 2);
});

test("a draft is signed only when its stamp sits in the window ingest still accepts", () => {
  const at = (createdAt) =>
    sealDraft({ ...draft("x", "abort"), createdAt }, PROJECT, NOW);
  for (const edge of [NOW - DRAFT_PAST_SECS, NOW, NOW + DRAFT_FUTURE_SECS]) {
    assert.equal(at(edge).createdAt, edge, "never re-stamped");
  }
  for (const stamp of [
    NOW - DRAFT_PAST_SECS - 1,
    NOW - 1800,
    NOW + DRAFT_FUTURE_SECS + 1,
    NOW + 86_400,
    0,
  ]) {
    assert.throws(
      () => at(stamp),
      /outside .* of this app's clock/,
      String(stamp),
    );
  }
  // Missing or malformed is refused, never read as zero or as now.
  const { createdAt: _, ...missing } = draft("x", "abort");
  assert.throws(
    () => sealDraft(missing, PROJECT, NOW),
    /whole-second createdAt/,
  );
  for (const stamp of [String(NOW), NOW + 0.5, -1, Number.NaN, null]) {
    assert.throws(() => at(stamp), /whole-second createdAt/, String(stamp));
  }
});

test("a valid first draft and a stale second publish nothing at all", async () => {
  const r = relay();
  await assert.rejects(
    executePluginDrafts(
      [
        draft("first", "abort"),
        draft("second", "abort", { createdAt: NOW - 1800 }),
      ],
      PROJECT,
      r.effects,
    ),
    /Nothing was saved/,
  );
  assert.equal(r.seen.length, 0, "zero publish calls, not one");
  // The run's clock is the effects' clock, not the system's: the same pair
  // judged an hour later refuses the first draft too.
  const later = relay();
  await assert.rejects(
    executePluginDrafts([draft("first", "abort")], PROJECT, {
      ...later.effects,
      now: () => NOW + 3600,
    }),
    /outside/,
  );
  assert.equal(later.seen.length, 0);
});
