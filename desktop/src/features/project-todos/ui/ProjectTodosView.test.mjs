import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ProjectTodosView } from "./ProjectTodosView.tsx";
import { todoListSummary } from "./ProjectTodosCard.tsx";

const OWNER = "1".repeat(64);
const AGENT = "5".repeat(64);
const PROJECT = {
  id: `${OWNER}:tank-loop`,
  dtag: "tank-loop",
  owner: OWNER,
  name: "Tank Loop",
  description: "",
  createdAt: 1,
  address: `30621:${OWNER}:tank-loop`,
  repoAddrs: [],
  agentAddrs: [],
  channelIds: [],
  visibility: "private",
  members: [],
  icon: null,
  color: null,
};

const PEOPLE = {
  [OWNER]: { pubkey: OWNER, name: "Andy", avatarUrl: null, isAgent: false },
  [AGENT]: { pubkey: AGENT, name: "Kiln", avatarUrl: null, isAgent: true },
};
const personFor = (pubkey) =>
  PEOPLE[pubkey] ?? {
    pubkey,
    name: pubkey.slice(0, 8),
    avatarUrl: null,
    isAgent: false,
  };

function item(id, overrides = {}) {
  return {
    id,
    listId: "1".repeat(32),
    text: `Item ${id.slice(0, 2)}`,
    done: false,
    rank: "a0",
    assignee: null,
    due: null,
    createdAt: 100,
    createdBy: OWNER,
    updatedAt: 100,
    completedAt: null,
    completedBy: null,
    ...overrides,
  };
}

function list(overrides = {}) {
  return {
    id: "1".repeat(32),
    title: "Launch",
    visibility: "project",
    archived: false,
    pinned: false,
    createdAt: 100,
    createdBy: OWNER,
    updatedAt: 200,
    open: [
      item("aa".repeat(16), { rank: "a0", due: "2000-01-01" }),
      item("bb".repeat(16), { rank: "a1", assignee: AGENT }),
    ],
    completed: [
      item("cc".repeat(16), {
        done: true,
        completedAt: 200,
        completedBy: OWNER,
      }),
    ],
    ...overrides,
  };
}

function read(lists, extra = {}) {
  return {
    events: [],
    digest: {
      schema: "buzz-project-todo-digest/v1",
      project: PROJECT.address,
      ignored: 0,
      lists,
    },
    truncated: false,
    latestByTarget: {},
    ...extra,
  };
}

const NOOP = async () => {};
const MUTATIONS = {
  createList: async () => "x".repeat(32),
  renameList: NOOP,
  setListArchived: NOOP,
  setListPinned: NOOP,
  addItem: async () => "y".repeat(32),
  setText: NOOP,
  setDone: NOOP,
  setAssignee: NOOP,
  setDue: NOOP,
  moveItem: NOOP,
  removeItem: NOOP,
};

function render(props) {
  const client = new QueryClient();
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectTodosView, {
        project: PROJECT,
        state: { kind: "ready", read: read([list()]), refreshing: false },
        access: { kind: "writable" },
        mutations: MUTATIONS,
        personFor,
        onWriteError: () => {},
        ...props,
      }),
    ),
  );
}

test("open items render in rank order with their chips; completed items follow", () => {
  const html = render({});
  const openAt = html.indexOf('data-testid="todo-open-section"');
  const completedAt = html.indexOf('data-testid="todo-completed-section"');
  assert.ok(openAt > -1 && completedAt > openAt, "open before completed");
  const a = html.indexOf("todo-item-aaaaaaaa");
  const b = html.indexOf("todo-item-bbbbbbbb");
  const c = html.indexOf("todo-item-cccccccc");
  assert.ok(a > -1 && b > a && c > b, "a, then b, then the completed c");
  // The overdue chip carries its marker; the agent assignee its glyph.
  assert.match(html, /data-overdue="true"/);
  assert.match(html, /data-testid="todo-assignee-agent"/);
  assert.match(html, /Kiln/);
  // Writable: the add field, the drag handle and remove are present.
  assert.match(html, /data-testid="todo-add-input"/);
  assert.match(html, /data-testid="todo-item-drag-handle"/);
  assert.match(html, /data-testid="todo-item-remove"/);
  assert.doesNotMatch(html, /data-testid="todo-notices"/);
});

test("a viewer sees the list read-only with the reason and no controls", () => {
  const html = render({
    access: { kind: "read-only", reason: "You are a viewer of this project." },
  });
  assert.match(html, /data-testid="todo-notice-read-only"/);
  assert.match(html, /Read-only: You are a viewer/);
  assert.doesNotMatch(html, /data-testid="todo-add-input"/);
  assert.doesNotMatch(html, /data-testid="todo-item-drag-handle"/);
  assert.doesNotMatch(html, /data-testid="todo-item-remove"/);
  assert.doesNotMatch(html, /data-testid="todo-list-new"/);
  assert.match(html, /todo-item-aaaaaaaa/, "the items are still shown");
});

test("truncation and ignored ops are disclosed, never hidden", () => {
  const html = render({
    state: {
      kind: "ready",
      read: read([list()], {
        truncated: true,
        digest: {
          schema: "buzz-project-todo-digest/v1",
          project: PROJECT.address,
          ignored: 2,
          lists: [list()],
        },
      }),
      refreshing: false,
    },
  });
  assert.match(html, /data-testid="todo-notice-truncated"/);
  assert.match(html, /older items may be missing/);
  assert.match(html, /data-testid="todo-notice-ignored"/);
  assert.match(html, /2 changes could not be applied/);
});

test("an archived list is selectable but not editable, and an empty project invites a list", () => {
  const html = render({
    state: {
      kind: "ready",
      read: read([list({ archived: true })]),
      refreshing: false,
    },
  });
  assert.match(html, /todo-item-aaaaaaaa/);
  assert.doesNotMatch(
    html,
    /data-testid="todo-add-input"/,
    "no adding to an archived list",
  );
  assert.match(html, /data-testid="todo-list-archive"/);

  const empty = render({
    state: { kind: "ready", read: read([]), refreshing: false },
  });
  assert.match(empty, /Create a list to get started/);
});

test("the rail marks personal and pinned lists and offers the pin toggle", () => {
  const html = render({
    state: {
      kind: "ready",
      read: read([
        list({ visibility: "personal", pinned: true }),
        list({ id: "2".repeat(32), title: "Shared" }),
      ]),
      refreshing: false,
    },
  });
  assert.match(html, /data-testid="todo-list-personal"/);
  assert.match(html, /data-testid="todo-list-pinned"/);
  assert.match(html, /data-testid="todo-list-pin"/);
  assert.match(html, /aria-label="Unpin from sidebar"/);
});

test("the overview card summarizes progress honestly", () => {
  assert.equal(todoListSummary(list()), "1 of 3 done");
  assert.equal(todoListSummary(list({ open: [], completed: [] })), "empty");
});
