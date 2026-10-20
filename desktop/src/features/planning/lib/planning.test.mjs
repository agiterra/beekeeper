import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { foldProjectTodos } from "../../project-todos/lib/todoFold.ts";
import { statusOps } from "../../project-todos/lib/todoMutations.ts";
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
  groupIssues,
  issueMatches,
  planningView,
  stepSelection,
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
    c.name.startsWith("item.status and item.done write one state field"),
  );
  const digest = foldProjectTodos(vector.project, vector.events);
  const read = nativePlanningRead(digest.lists, {
    truncated: false,
    ignored: digest.ignored,
  });
  assert.equal(read.issues.length, 3);
  const byTitle = Object.fromEntries(read.issues.map((i) => [i.title, i]));
  assert.equal(byTitle["ticked then restarted"].status.category, "started");
  assert.equal(byTitle["started then ticked"].status.category, "completed");
  assert.equal(byTitle.canceled.status.category, "canceled");
  assert.equal(byTitle.canceled.completedAt, 15);

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
  assert.equal(filtered.issues.length, 3);
  assert.equal(filtered.truncated, true);
  assert.equal(filtered.unreadable, 2);
});

test("a status change crossing the closed boundary publishes item.done first, then item.status", () => {
  const L = "1".repeat(32);
  const I = "a".repeat(32);
  assert.deepEqual(
    statusOps(L, I, "done", false).map((op) => [op.op, op.done ?? op.status]),
    [
      ["item.done", true],
      ["item.status", "done"],
    ],
  );
  assert.deepEqual(
    statusOps(L, I, "in_progress", true).map((op) => [
      op.op,
      op.done ?? op.status,
    ]),
    [
      ["item.done", false],
      ["item.status", "in_progress"],
    ],
  );
  assert.deepEqual(
    statusOps(L, I, "canceled", true).map((op) => op.op),
    ["item.status"],
    "done to canceled stays closed: no item.done",
  );
  assert.deepEqual(
    statusOps(L, I, "backlog", false).map((op) => op.op),
    ["item.status"],
  );
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
    c.name.startsWith("a status tie breaks on event id"),
  );
  const listId = "1".repeat(32);
  const tie = nativeIssueActivity(vector.events, listId, "a".repeat(32));
  assert.deepEqual(
    tie.map((e) => e.summary),
    ["created the issue", "marked it done", "set status to Backlog"],
  );
  const skewed = nativeIssueActivity(vector.events, listId, "c".repeat(32));
  assert.deepEqual(
    skewed.map((e) => e.summary),
    ["set status to Done", "created the issue"],
    "the skewed status op is history even though the fold lets the add win",
  );
});
