import assert from "node:assert/strict";
import { test } from "node:test";

import {
  projectSessionIndicator,
  projectSessionIndicatorState,
} from "./projectSessionIndicator.ts";

const entry = (overrides = {}) => ({
  isClosed: false,
  isArchived: false,
  pending: false,
  status: { kind: "idle", label: "Idle" },
  ...overrides,
});

test("closure facts outrank provider metadata: archived over closed over status", () => {
  assert.equal(
    projectSessionIndicatorState(
      entry({
        isClosed: true,
        isArchived: true,
        status: { kind: "working", label: "Working" },
      }),
    ),
    "archived",
  );
  assert.equal(
    projectSessionIndicatorState(
      entry({ isClosed: true, status: { kind: "working", label: "Working" } }),
    ),
    "closed",
  );
});

test("only a reported working status is green; everything else is blue", () => {
  assert.equal(
    projectSessionIndicatorState(
      entry({ status: { kind: "working", label: "Working" } }),
    ),
    "running",
  );
  for (const status of [
    { kind: "idle", label: "Idle" },
    { kind: "ended", label: "Ended" },
    { kind: "unknown", label: "Status unknown" },
    { kind: "unknown", label: "Disconnected", attention: true },
  ]) {
    assert.equal(projectSessionIndicatorState(entry({ status })), "idle");
  }
  assert.equal(
    projectSessionIndicatorState(entry({ pending: true })),
    "starting",
  );
});

test("each state has its colour, its name, and an honest hover", () => {
  const archived = projectSessionIndicator(
    entry({ isClosed: true, isArchived: true }),
  );
  assert.equal(archived.label, "Archived");
  assert.equal(archived.title, "Archived");
  assert.equal(archived.colorClass, "bg-red-500");

  const closed = projectSessionIndicator(entry({ isClosed: true }));
  assert.equal(closed.label, "Closed");
  assert.equal(closed.colorClass, "bg-orange-500");

  const running = projectSessionIndicator(
    entry({ status: { kind: "working", label: "Working" } }),
  );
  assert.equal(running.label, "Running");
  assert.equal(running.colorClass, "bg-emerald-500");
  // The colour cannot carry provenance; the hover does.
  assert.match(
    running.title,
    /^Running — Working is what this session's provider last reported/,
  );
  assert.match(running.title, /not a live lease/);

  const idle = projectSessionIndicator(
    entry({ status: { kind: "unknown", label: "Disconnected" } }),
  );
  assert.equal(idle.label, "Idle");
  assert.equal(idle.colorClass, "bg-blue-500");
  // Idle claims nothing, so its hover explains nothing.
  assert.equal(idle.title, "Idle");

  const starting = projectSessionIndicator(entry({ pending: true }));
  assert.equal(starting.colorClass, "bg-blue-500");
  assert.match(starting.title, /waiting for the session provider/);
});

test("a founded row is Not started: hollow, neutral, and outranked by any closure or a pending Start", () => {
  const foundedStatus = { kind: "founded", label: "Not started" };
  assert.equal(
    projectSessionIndicatorState(
      entry({ founded: true, status: foundedStatus }),
    ),
    "founded",
  );
  // The status kind alone is enough — the flag and the status agree.
  assert.equal(
    projectSessionIndicatorState(entry({ status: foundedStatus })),
    "founded",
  );
  const founded = projectSessionIndicator(
    entry({ founded: true, status: foundedStatus }),
  );
  assert.equal(founded.label, "Not started");
  assert.equal(
    founded.colorClass,
    "bg-transparent ring-1 ring-inset ring-sidebar-foreground/45",
  );
  assert.equal(
    founded.title,
    "Not started — founded, but no provider has been asked to run it yet",
  );

  // Precedence: archived → closed → starting → founded.
  assert.equal(
    projectSessionIndicatorState(
      entry({ founded: true, status: foundedStatus, isClosed: true }),
    ),
    "closed",
  );
  assert.equal(
    projectSessionIndicatorState(
      entry({
        founded: true,
        status: foundedStatus,
        isClosed: true,
        isArchived: true,
      }),
    ),
    "archived",
  );
  assert.equal(
    projectSessionIndicatorState(
      entry({ founded: true, status: foundedStatus, pending: true }),
    ),
    "starting",
  );
});
