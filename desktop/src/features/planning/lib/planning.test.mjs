import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { foldProjectTodos } from "../../project-todos/lib/todoFold.ts";
import {
  TodoPartialWriteError,
  publishStatusOps,
  statusOps,
} from "../../project-todos/lib/todoMutations.ts";
import { createNativeIssue } from "./nativeCreate.ts";
import { nativeIssueActivity } from "./nativeActivity.ts";
import { NATIVE_CAPABILITIES, nativePlanningRead } from "./nativePlanning.ts";
import {
  parsePlanningDisplay,
  planningDisplayKey,
} from "./planningDisplayStorage.ts";
import {
  LINEAR_PLANNING_PROVIDER,
  PLANNING_PROVIDERS,
  resolvePlanningProvider,
} from "./planningProviders.ts";
import {
  DEFAULT_PLANNING_DISPLAY,
  DEFAULT_PLANNING_FILTERS,
  defaultCreateIds,
  groupIssues,
  issueMatches,
  personPubkey,
  planningView,
  stepSelection,
  viewerPersonId,
} from "./planningView.ts";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS = JSON.parse(
  readFileSync(
    path.join(
      HERE,
      "../../../../../conformance/project-todo-fold/fixtures/fold-vectors.json",
    ),
    "utf8",
  ),
);

const ME = "1".repeat(64);
const BOB = "2".repeat(64);

function issue(id, overrides = {}) {
  const status = NATIVE_CAPABILITIES.statuses.find(
    (s) => s.id === (overrides.statusId ?? "todo"),
  );
  const priority = NATIVE_CAPABILITIES.priorities.find(
    (p) => p.id === (overrides.priorityId ?? "none"),
  );
  return {
    provider: "native",
    key: `${"1".repeat(32)}/${id}`,
    containerId: "1".repeat(32),
    issueId: id,
    title: `Issue ${id}`,
    description: null,
    status,
    priority,
    assignee: null,
    due: null,
    rank: "a0",
    createdAt: 1,
    createdBy: ME,
    updatedAt: 1,
    completedAt: null,
    ...overrides,
  };
}

test("native planning reads the fold: project lists only, archived lists dropped, statuses mapped to categories", () => {
  const vector = VECTORS.cases.find((c) =>
    c.name.startsWith("item.done alone decides done"),
  );
  const digest = foldProjectTodos(vector.project, vector.events);
  const read = nativePlanningRead(digest.lists, {
    truncated: false,
    ignored: digest.ignored,
  });
  assert.equal(read.issues.length, 5);
  const byTitle = Object.fromEntries(read.issues.map((i) => [i.title, i]));
  assert.equal(byTitle["restarted as a pair"].status.category, "started");
  assert.equal(byTitle["started then ticked"].status.category, "completed");
  assert.equal(
    byTitle["ticked, then a stale restart"].status.category,
    "completed",
    "a status that disagrees with a later-or-equal item.done is not applied",
  );
  assert.equal(
    byTitle["canceled without item.done"].status.category,
    "unstarted",
  );
  assert.equal(byTitle["canceled as a pair"].status.category, "canceled");
  assert.equal(byTitle["canceled as a pair"].completedAt, 16);

  const lists = [
    { ...digest.lists[0] },
    {
      ...digest.lists[0],
      id: "2".repeat(32),
      visibility: "personal",
    },
    { ...digest.lists[0], id: "3".repeat(32), archived: true },
  ];
  const filtered = nativePlanningRead(lists, { truncated: true, ignored: 2 });
  assert.deepEqual(
    filtered.containers.map((c) => c.id),
    [digest.lists[0].id],
    "a personal or archived list is not a planning container",
  );
  assert.equal(filtered.issues.length, 5);
  assert.equal(filtered.truncated, true);
  assert.equal(filtered.unreadable, 2);
});

test("a status write is always item.done with the status's closed-ness, then item.status", () => {
  const L = "1".repeat(32);
  const I = "a".repeat(32);
  const pairs = (status) =>
    statusOps(L, I, status).map((op) => [op.op, op.done ?? op.status]);
  // Never conditional on this client's read: that read can be stale.
  assert.deepEqual(pairs("done"), [
    ["item.done", true],
    ["item.status", "done"],
  ]);
  assert.deepEqual(pairs("canceled"), [
    ["item.done", true],
    ["item.status", "canceled"],
  ]);
  assert.deepEqual(pairs("in_progress"), [
    ["item.done", false],
    ["item.status", "in_progress"],
  ]);
  assert.deepEqual(pairs("backlog"), [
    ["item.done", false],
    ["item.status", "backlog"],
  ]);
});

test("an unconfirmed item.status is a partial write that says how the item is marked", async () => {
  const L = "1".repeat(32);
  const I = "a".repeat(32);
  const sent = [];
  const refuseStatus = async (op) => {
    sent.push(op.op);
    if (op.op === "item.status") {
      throw new Error('invalid: unknown project todo op "item.status"');
    }
  };
  await assert.rejects(
    publishStatusOps(refuseStatus, L, I, "canceled"),
    (error) => {
      assert.ok(error instanceof TodoPartialWriteError);
      assert.deepEqual(error.saved, ["item.done"]);
      assert.equal(error.refused, "item.status");
      assert.match(error.message, /the item is marked “done”/);
      assert.match(error.message, /did not confirm the status “canceled”/);
      assert.match(error.message, /unknown project todo op/);
      return true;
    },
  );
  assert.deepEqual(sent, ["item.done", "item.status"]);

  // Reopening into a started status: the saved half reads Todo.
  await assert.rejects(
    publishStatusOps(refuseStatus, L, I, "in_progress"),
    /is marked “todo”/,
  );

  // The first op refused: nothing saved, the relay's own error, no second op.
  const firstOnly = [];
  const refuseAll = async (op) => {
    firstOnly.push(op.op);
    throw new Error("relay down");
  };
  await assert.rejects(publishStatusOps(refuseAll, L, I, "done"), (error) => {
    assert.ok(!(error instanceof TodoPartialWriteError));
    assert.equal(error.message, "relay down");
    return true;
  });
  assert.deepEqual(firstOnly, ["item.done"]);
});

function fakeWrites(refuse = {}) {
  const calls = [];
  const step =
    (name, result) =>
    async (...args) => {
      calls.push([name, ...args]);
      if (refuse[name]) throw new Error(refuse[name]);
      return result;
    };
  return {
    calls,
    writes: {
      createList: step("createList", "9".repeat(32)),
      addItem: step("addItem", "8".repeat(32)),
      setAssignee: step("setAssignee"),
      setStatus: step("setStatus"),
      setPriority: step("setPriority"),
      setDescription: step("setDescription"),
    },
  };
}

const CREATE_INPUT = {
  title: "Ship it",
  description: "why",
  statusId: "in_progress",
  priorityId: "high",
  assignee: "f".repeat(64),
  containerId: "1".repeat(32),
};

test("create adds the bare item first, then each field; a refused assignee is reported, not thrown", async () => {
  const { calls, writes } = fakeWrites({ setAssignee: "assignee refused" });
  const result = await createNativeIssue(writes, CREATE_INPUT);
  assert.equal(result.itemId, "8".repeat(32));
  assert.deepEqual(result.failed, [
    { field: "assignee", message: "assignee refused" },
  ]);
  assert.deepEqual(
    calls.map((c) => c[0]),
    ["addItem", "setAssignee", "setStatus", "setPriority", "setDescription"],
  );
  assert.deepEqual(
    calls[0],
    ["addItem", "1".repeat(32), "Ship it"],
    "bare add",
  );
  assert.equal(
    calls.filter((c) => c[0] === "addItem").length,
    1,
    "one item, so the caller closes the draft instead of retrying a duplicate",
  );
});

test("create rejects only while nothing exists, and makes the default list when none is named", async () => {
  const refused = fakeWrites({ addItem: "add refused" });
  await assert.rejects(
    createNativeIssue(refused.writes, CREATE_INPUT),
    /add refused/,
  );
  assert.deepEqual(
    refused.calls.map((c) => c[0]),
    ["addItem"],
  );

  const fresh = fakeWrites();
  const result = await createNativeIssue(fresh.writes, {
    ...CREATE_INPUT,
    containerId: null,
    assignee: null,
    statusId: "todo",
    priorityId: "none",
    description: null,
  });
  assert.equal(result.listId, "9".repeat(32));
  assert.deepEqual(
    fresh.calls.map((c) => c[0]),
    ["createList", "addItem"],
  );
  assert.deepEqual(fresh.calls[0], ["createList", "Issues", "project"]);
});

test("scopes and filters narrow the same records; my issues needs a known self", () => {
  const mine = issue("a".repeat(32), { assignee: ME, statusId: "in_progress" });
  const triage = issue("b".repeat(32), { statusId: "triage" });
  const backlog = issue("c".repeat(32), {
    statusId: "backlog",
    priorityId: "urgent",
    description: "Needle in here",
  });
  const f = (patch) => ({ ...DEFAULT_PLANNING_FILTERS, ...patch });
  assert.ok(issueMatches(mine, f({ scope: "mine" }), ME));
  assert.ok(!issueMatches(mine, f({ scope: "mine" }), null));
  assert.ok(!issueMatches(triage, f({ scope: "mine" }), ME));
  assert.ok(issueMatches(triage, f({ scope: "triage" }), ME));
  assert.ok(issueMatches(mine, f({ scope: "active" }), ME));
  assert.ok(!issueMatches(backlog, f({ scope: "active" }), ME));
  assert.ok(issueMatches(backlog, f({ priorityIds: ["urgent"] }), ME));
  assert.ok(!issueMatches(mine, f({ priorityIds: ["urgent"] }), ME));
  assert.ok(issueMatches(backlog, f({ query: "needle" }), ME));
  assert.ok(!issueMatches(mine, f({ query: "needle" }), ME));
  assert.ok(issueMatches(triage, f({ assignee: { kind: "unassigned" } }), ME));
  assert.ok(!issueMatches(mine, f({ assignee: { kind: "unassigned" } }), ME));
  assert.ok(
    issueMatches(mine, f({ assignee: { kind: "person", pubkey: ME } }), BOB),
  );
});

test("list and board group the same issues; status groups keep empty columns in workflow order", () => {
  const issues = [
    issue("a".repeat(32), { statusId: "in_progress", rank: "a1" }),
    issue("b".repeat(32), { statusId: "in_progress", rank: "a0" }),
    issue("c".repeat(32), { statusId: "done", completedAt: 5 }),
  ];
  const groups = groupIssues(
    issues,
    { groupBy: "status", orderBy: "manual" },
    NATIVE_CAPABILITIES,
    [],
    () => "",
  );
  assert.deepEqual(
    groups.map((g) => [g.id, g.issues.length]),
    [
      ["triage", 0],
      ["backlog", 0],
      ["todo", 0],
      ["in_progress", 2],
      ["done", 1],
      ["canceled", 0],
    ],
  );
  assert.deepEqual(
    groups[3].issues.map((i) => i.issueId.slice(0, 1)),
    ["b", "a"],
    "manual order follows rank",
  );

  const list = planningView(
    issues,
    DEFAULT_PLANNING_FILTERS,
    { ...DEFAULT_PLANNING_DISPLAY, layout: "list", showClosed: false },
    NATIVE_CAPABILITIES,
    [],
    ME,
    () => "",
  );
  assert.equal(list.matched, 2, "hidden closed issues leave the list");
  const board = planningView(
    issues,
    DEFAULT_PLANNING_FILTERS,
    { ...DEFAULT_PLANNING_DISPLAY, layout: "board", showClosed: false },
    NATIVE_CAPABILITIES,
    [],
    ME,
    () => "",
  );
  assert.equal(board.matched, 3, "the board keeps its done column");
});

test("assignee groups sort people by name and end with No assignee", () => {
  const issues = [
    issue("a".repeat(32), { assignee: BOB }),
    issue("b".repeat(32), { assignee: ME }),
    issue("c".repeat(32)),
  ];
  const names = { [ME]: "Zed", [BOB]: "Ann" };
  const groups = groupIssues(
    issues,
    { groupBy: "assignee", orderBy: "manual" },
    NATIVE_CAPABILITIES,
    [],
    (pk) => names[pk],
  );
  assert.deepEqual(
    groups.map((g) => g.label),
    ["Ann", "Zed", "No assignee"],
  );
  assert.equal(groups[2].assignee, null);
});

test("keyboard steps clamp at the ends and start from the first or last row", () => {
  const order = ["a", "b", "c"];
  assert.equal(stepSelection(order, null, 1), "a");
  assert.equal(stepSelection(order, null, -1), "c");
  assert.equal(stepSelection(order, "b", 1), "c");
  assert.equal(stepSelection(order, "c", 1), "c");
  assert.equal(stepSelection(order, "a", -1), "a");
  assert.equal(stepSelection(order, "gone", 1), "a");
  assert.equal(stepSelection([], "a", 1), null);
});

test("display options parse field by field and are keyed per person and project", () => {
  assert.deepEqual(parsePlanningDisplay(null), DEFAULT_PLANNING_DISPLAY);
  assert.deepEqual(parsePlanningDisplay("{not json"), DEFAULT_PLANNING_DISPLAY);
  assert.deepEqual(
    parsePlanningDisplay(
      JSON.stringify({ layout: "board", groupBy: "bogus", showClosed: false }),
    ),
    { ...DEFAULT_PLANNING_DISPLAY, layout: "board", showClosed: false },
  );
  assert.notEqual(
    planningDisplayKey(ME, "30621:x:a"),
    planningDisplayKey(BOB, "30621:x:a"),
  );
});

test("Linear is listed but unavailable, and never resolves as a project's provider", () => {
  assert.deepEqual(
    PLANNING_PROVIDERS.map((p) => p.id),
    ["native", "linear"],
  );
  assert.equal(LINEAR_PLANNING_PROVIDER.availability.kind, "unavailable");
  assert.match(LINEAR_PLANNING_PROVIDER.availability.reason, /cannot connect/);
  assert.equal(resolvePlanningProvider(), "native");
});

test("activity lists every decodable op on the issue in order, including a losing write", () => {
  const vector = VECTORS.cases.find((c) =>
    c.name.startsWith("a status ties item.done on created_at"),
  );
  const listId = "1".repeat(32);
  const tie = nativeIssueActivity(vector.events, listId, "a".repeat(32));
  assert.deepEqual(
    tie.map((e) => e.summary),
    ["created the issue", "set status to Canceled", "marked it done"],
  );
  const skewed = nativeIssueActivity(vector.events, listId, "d".repeat(32));
  assert.deepEqual(
    skewed.map((e) => e.summary),
    ["set status to Backlog", "created the issue"],
    "the skewed status op is history even though the fold lets the add win",
  );
});

test("person ids are the provider's: native ids are pubkeys, a provider's own ids resolve only through a known link", () => {
  const native = { personIds: "beekeeper-pubkey" };
  const pk = "f".repeat(64);
  assert.equal(personPubkey(native, null, pk), pk);
  assert.equal(viewerPersonId(native, null, pk), pk);
  assert.equal(viewerPersonId(native, null, null), null);

  const provider = { personIds: "provider" };
  const read = {
    people: [
      { id: "lin-user-1", pubkey: pk, name: "Ada" },
      { id: "lin-user-2", pubkey: null, name: "Unlinked" },
    ],
  };
  assert.equal(personPubkey(provider, read, "lin-user-1"), pk);
  assert.equal(personPubkey(provider, read, "lin-user-2"), null);
  assert.equal(personPubkey(provider, read, pk), null, "never a guess");
  assert.equal(viewerPersonId(provider, read, pk), "lin-user-1");
  assert.equal(viewerPersonId(provider, read, "e".repeat(64)), null);
});

test("create defaults come from the provider's vocabulary, not native ids", () => {
  assert.deepEqual(defaultCreateIds(NATIVE_CAPABILITIES), {
    statusId: "todo",
    priorityId: "none",
  });
  assert.deepEqual(
    defaultCreateIds({
      statuses: [
        { id: "uuid-backlog", label: "Backlog", category: "backlog" },
        { id: "uuid-todo", label: "Todo", category: "unstarted" },
      ],
      priorities: [
        { id: "1", label: "Urgent", level: "urgent", order: 0 },
        { id: "0", label: "No priority", level: "none", order: 4 },
      ],
    }),
    { statusId: "uuid-todo", priorityId: "0" },
  );
  for (const priority of NATIVE_CAPABILITIES.priorities) {
    assert.equal(priority.level, priority.id, "native ids and levels coincide");
  }
});
