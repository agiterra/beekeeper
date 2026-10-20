import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { NATIVE_CAPABILITIES } from "../lib/nativePlanning.ts";
import {
  DEFAULT_PLANNING_DISPLAY,
  DEFAULT_PLANNING_FILTERS,
} from "../lib/planningView.ts";
import { PlanningIssueDetail } from "./PlanningIssueDetail.tsx";
import { PlanningView } from "./PlanningView.tsx";

const OWNER = "1".repeat(64);
const AGENT = "5".repeat(64);
const LIST = "1".repeat(32);
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

function issue(id, statusId, overrides = {}) {
  return {
    provider: "native",
    key: `${LIST}/${id}`,
    containerId: LIST,
    issueId: id,
    title: `Issue ${id.slice(0, 2)}`,
    description: null,
    status: NATIVE_CAPABILITIES.statuses.find((s) => s.id === statusId),
    priority: NATIVE_CAPABILITIES.priorities.find(
      (p) => p.id === (overrides.priorityId ?? "none"),
    ),
    assignee: null,
    due: null,
    rank: "a0",
    createdAt: 100,
    createdBy: OWNER,
    updatedAt: 100,
    completedAt: null,
    ...overrides,
  };
}

const ISSUES = [
  issue("aa".repeat(16), "in_progress", {
    assignee: AGENT,
    priorityId: "urgent",
  }),
  issue("bb".repeat(16), "triage"),
  issue("cc".repeat(16), "done", { completedAt: 200 }),
];

function readOf(issues, extra = {}) {
  return {
    provider: "native",
    issues,
    containers: [{ id: LIST, title: "Launch" }],
    truncated: false,
    unreadable: 0,
    people: [],
    ...extra,
  };
}

const ACTIONS = {
  open: () => {},
  select: () => {},
  setStatus: () => {},
  setPriority: () => {},
  setAssignee: () => {},
  createIn: () => {},
};

function render(props) {
  const client = new QueryClient();
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(PlanningView, {
        project: PROJECT,
        provider: "native",
        state: { kind: "ready", read: readOf(ISSUES), refreshing: false },
        access: { kind: "writable" },
        capabilities: NATIVE_CAPABILITIES,
        display: DEFAULT_PLANNING_DISPLAY,
        filters: DEFAULT_PLANNING_FILTERS,
        self: OWNER,
        selectedKey: null,
        openKey: null,
        isPending: () => false,
        personFor,
        actions: ACTIONS,
        searchRef: { current: null },
        onDisplayChange: () => {},
        onFiltersChange: () => {},
        onCreate: () => {},
        ...props,
      }),
    ),
  );
}

test("the list groups by status in workflow order and folds away empty groups", () => {
  const html = render({});
  const triage = html.indexOf('data-testid="planning-group-triage"');
  const started = html.indexOf('data-testid="planning-group-in_progress"');
  const done = html.indexOf('data-testid="planning-group-done"');
  assert.ok(triage > -1 && started > triage && done > started);
  assert.doesNotMatch(html, /data-testid="planning-group-backlog"/);
  assert.match(html, /3 of 3 issues/);
  // Writable: the row controls are menus, and quick create is offered.
  assert.match(html, /aria-label="Status: In Progress. Change status"/);
  assert.match(html, /data-testid="planning-new-issue"/);
  assert.match(html, /data-testid="todo-assignee-agent"/);
  assert.match(html, /Kiln/);
  assert.doesNotMatch(html, /data-testid="planning-notices"/);
  // The provider is stated, not implied.
  assert.match(html, /aria-label="Planning provider: Beekeeper"/);
});

test("a viewer reads the same issues with the reason and no write controls", () => {
  const html = render({
    access: {
      kind: "read-only",
      reason: "You are a viewer of this project.",
    },
  });
  assert.match(html, /data-testid="planning-notice-read-only"/);
  assert.match(html, /You are a viewer of this project/);
  assert.doesNotMatch(html, /Change status/);
  assert.doesNotMatch(html, /data-testid="planning-new-issue"/);
  assert.doesNotMatch(html, /data-testid="planning-group-create-/);
  assert.match(html, /data-testid="planning-row-title-aaaaaaaa/);
});

test("a failed refresh keeps the last read on screen and says it may be stale", () => {
  const html = render({
    state: { kind: "error", read: readOf(ISSUES), message: "relay offline" },
  });
  assert.match(html, /data-testid="planning-notice-error"/);
  assert.match(html, /Showing the last read, which may be out of date/);
  assert.match(html, /data-testid="planning-row-title-aaaaaaaa/);
});

test("a failed first read shows the error and no invented empty state", () => {
  const html = render({
    state: { kind: "error", read: null, message: "relay offline" },
  });
  assert.match(html, /Could not read the issues: relay offline/);
  assert.doesNotMatch(html, /planning-empty/);
  assert.doesNotMatch(html, /planning-loading/);
});

test("loading, empty, truncated and unreadable states are each distinct", () => {
  assert.match(render({ state: { kind: "loading" } }), /Reading issues…/);
  const empty = render({
    state: { kind: "ready", read: readOf([]), refreshing: false },
  });
  assert.match(empty, /data-testid="planning-empty-create"/);
  const emptyViewer = render({
    state: { kind: "ready", read: readOf([]), refreshing: false },
    access: { kind: "read-only", reason: "viewer" },
  });
  assert.doesNotMatch(emptyViewer, /planning-empty-create/);
  const odd = render({
    state: {
      kind: "ready",
      read: readOf(ISSUES, { truncated: true, unreadable: 2 }),
      refreshing: true,
    },
  });
  assert.match(odd, /data-testid="planning-notice-truncated"/);
  assert.match(odd, /2 changes could not be applied/);
  assert.match(odd, /refreshing/);
});

test("filters that match nothing say so instead of showing an empty project", () => {
  const html = render({
    filters: { ...DEFAULT_PLANNING_FILTERS, query: "no such words" },
  });
  assert.match(html, /data-testid="planning-no-matches"/);
  assert.match(html, /Clear the filters/);
  assert.match(html, /0 of 3 issues/);
});

test("hiding closed issues drops them from the list only", () => {
  const html = render({
    display: { ...DEFAULT_PLANNING_DISPLAY, showClosed: false },
  });
  assert.doesNotMatch(html, /planning-row-title-cccccccc/);
  assert.match(html, /2 of 3 issues/);
});

test("the detail shows every field, the real history, and says comments are unavailable", () => {
  const client = new QueryClient();
  const html = renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(PlanningIssueDetail, {
        project: PROJECT,
        issue: { ...ISSUES[0], description: "Steps:\n1. open it" },
        containerTitle: "Launch",
        capabilities: NATIVE_CAPABILITIES,
        canEdit: true,
        readOnlyReason: null,
        pendingFields: new Set(["status"]),
        personFor,
        activity: [
          {
            id: "e1",
            actor: OWNER,
            createdAt: 100,
            summary: "created the issue",
          },
          {
            id: "e2",
            actor: OWNER,
            createdAt: 110,
            summary: "assigned",
            subject: AGENT,
          },
        ],
        actions: {
          close: () => {},
          setTitle: async () => {},
          setDescription: async () => {},
          setStatus: () => {},
          setPriority: () => {},
          setAssignee: () => {},
        },
      }),
    ),
  );
  assert.match(html, /value="Issue aa"/);
  assert.match(html, /Steps:\n1. open it/);
  assert.match(html, /In Progress/);
  assert.match(html, /Urgent/);
  assert.match(html, /animate-pulse/, "the in-flight status write shows");
  assert.match(html, /created the issue/);
  assert.match(html, /assigned.{0,80}Kiln/s);
  assert.match(html, /data-testid="planning-detail-no-comments"/);
  assert.doesNotMatch(html, /<textarea/, "no comment box that cannot save");
});
