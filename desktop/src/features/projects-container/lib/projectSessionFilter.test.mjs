import assert from "node:assert/strict";
import { test } from "node:test";

import {
  DEFAULT_PROJECT_SESSION_FILTER,
  filterProjectSessions,
  parseProjectSessionFilter,
  projectSessionDateRangeLabel,
  projectSessionFilterLabel,
  projectHiddenArtifactsNote,
  projectSessionFounders,
  projectSessionUnattributedNote,
  resolveProjectSessionDateRange,
} from "./projectSessionFilter.ts";
import { parseProjectSessionFilterState } from "./projectSessionFilterStorage.ts";

const ME = "aa".repeat(32);
const ALICE = "bb".repeat(32);
const BOB = "cc".repeat(32);

// A fixed "now": Wednesday 2026-08-26 15:00 local time.
const NOW = new Date(2026, 7, 26, 15, 0, 0);
const at = (date) => new Date(date).toISOString();

const entry = (founderPubkey, overrides = {}) => ({
  founderPubkey,
  isClosed: false,
  isArchived: false,
  session: { lastEventAt: at(NOW) },
  generationId: `gen-${founderPubkey ?? "none"}-${Math.random()}`,
  ...overrides,
});

const members = (mode, pubkeys = []) =>
  mode === "custom" ? { mode, pubkeys } : { mode };
const filter = (overrides = {}) => ({
  ...DEFAULT_PROJECT_SESSION_FILTER,
  ...overrides,
});

test("the default filter is My sessions, closed shown, archived hidden, any time, pins shown", () => {
  assert.deepEqual(DEFAULT_PROJECT_SESSION_FILTER, {
    members: { mode: "mine" },
    showClosed: true,
    showArchived: false,
    range: { kind: "any" },
    // Somebody pinned it for the project to see: a sidebar that hides what a
    // teammate pinned until you find a checkbox is the wrong way round.
    showPinnedArtifacts: true,
  });
});

test("mine keeps only sessions I founded, case-insensitively", () => {
  const mine = entry(ME.toUpperCase());
  const theirs = entry(ALICE);
  const result = filterProjectSessions([mine, theirs], filter(), ME, NOW);
  assert.deepEqual(result.shown, [mine]);
  assert.equal(result.hiddenUnattributed, 0);
  assert.equal(result.hiddenByState, 0);
});

test("mine keeps a pending row — it is the local user's own create", () => {
  const pending = entry(null, { pending: true, session: { lastEventAt: "" } });
  const result = filterProjectSessions(
    [pending],
    filter({ range: { kind: "today" } }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [pending]);
  assert.equal(result.hiddenUnattributed, 0);
});

test("mine with no identity shows nothing attributable and counts the rest", () => {
  const result = filterProjectSessions(
    [entry(ALICE), entry(null)],
    filter(),
    undefined,
    NOW,
  );
  assert.deepEqual(result.shown, []);
  assert.equal(result.hiddenUnattributed, 1);
});

test("all shows every founder, and hides nothing on that axis", () => {
  const entries = [entry(ME), entry(ALICE), entry(null)];
  const result = filterProjectSessions(
    entries,
    filter({ members: members("all") }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, entries);
  assert.equal(result.hiddenUnattributed, 0);
});

test("custom keeps the ticked founders and reports unattributed sessions", () => {
  const alice = entry(ALICE);
  const bob = entry(BOB);
  const legacy = entry(null);
  const result = filterProjectSessions(
    [alice, bob, legacy, entry(ME)],
    filter({ members: members("custom", [ALICE.toUpperCase(), BOB]) }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [alice, bob]);
  assert.equal(result.hiddenUnattributed, 1);
});

test("custom does not smuggle pending rows past an unticked viewer", () => {
  const pending = entry(null, { pending: true });
  const result = filterProjectSessions(
    [pending],
    filter({ members: members("custom", [ALICE]) }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, []);
  assert.equal(result.hiddenUnattributed, 1);
});

test("closed and archived are gated by their boxes; archived needs both", () => {
  const open = entry(ME);
  const closed = entry(ME, { isClosed: true });
  const archived = entry(ME, { isClosed: true, isArchived: true });
  const all = [open, closed, archived];

  let result = filterProjectSessions(all, filter(), ME, NOW);
  assert.deepEqual(result.shown, [open, closed]);
  assert.equal(result.hiddenByState, 1);

  result = filterProjectSessions(all, filter({ showClosed: false }), ME, NOW);
  assert.deepEqual(result.shown, [open]);
  assert.equal(result.hiddenByState, 2);

  result = filterProjectSessions(
    all,
    filter({ showClosed: true, showArchived: true }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, all);

  // Archived implies closed: ticking archived alone reveals nothing extra.
  result = filterProjectSessions(
    all,
    filter({ showClosed: false, showArchived: true }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [open]);
});

test("date ranges resolve against local calendar days, weeks starting Monday", () => {
  const day = (d) => new Date(2026, 7, d).getTime();
  assert.deepEqual(resolveProjectSessionDateRange({ kind: "any" }, NOW), {
    start: null,
    end: null,
  });
  assert.deepEqual(resolveProjectSessionDateRange({ kind: "today" }, NOW), {
    start: day(26),
    end: day(27),
  });
  assert.deepEqual(resolveProjectSessionDateRange({ kind: "yesterday" }, NOW), {
    start: day(25),
    end: day(26),
  });
  // 2026-08-26 is a Wednesday; the week is Mon 24 → Mon 31.
  assert.deepEqual(resolveProjectSessionDateRange({ kind: "week" }, NOW), {
    start: day(24),
    end: day(31),
  });
  assert.deepEqual(resolveProjectSessionDateRange({ kind: "month" }, NOW), {
    start: day(1),
    end: new Date(2026, 8, 1).getTime(),
  });
  assert.deepEqual(
    resolveProjectSessionDateRange(
      { kind: "custom", from: "2026-08-10", to: "2026-08-12" },
      NOW,
    ),
    { start: day(10), end: day(13) },
  );
  assert.deepEqual(
    resolveProjectSessionDateRange(
      { kind: "custom", from: null, to: "2026-08-12" },
      NOW,
    ),
    { start: null, end: day(13) },
  );
});

test("the date range hides sessions whose last activity falls outside it", () => {
  const today = entry(ME, { session: { lastEventAt: at(NOW) } });
  const yesterday = entry(ME, {
    session: { lastEventAt: at(new Date(2026, 7, 25, 9)) },
  });
  const lastMonth = entry(ME, {
    session: { lastEventAt: at(new Date(2026, 6, 30, 9)) },
  });
  const undated = entry(ME, { session: { lastEventAt: "not a date" } });
  const all = [today, yesterday, lastMonth, undated];

  let result = filterProjectSessions(
    all,
    filter({ range: { kind: "today" } }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [today]);
  assert.equal(result.hiddenByState, 3);

  result = filterProjectSessions(
    all,
    filter({ range: { kind: "yesterday" } }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [yesterday]);

  result = filterProjectSessions(
    all,
    filter({ range: { kind: "month" } }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, [today, yesterday]);

  result = filterProjectSessions(
    all,
    filter({ range: { kind: "any" } }),
    ME,
    NOW,
  );
  assert.deepEqual(result.shown, all);
});

test("projectSessionFounders lists distinct known founders in first-seen order", () => {
  assert.deepEqual(
    projectSessionFounders([
      entry(BOB),
      entry(null),
      entry(ALICE.toUpperCase()),
      entry(BOB),
    ]),
    [BOB, ALICE],
  );
});

test("labels name the member mode, the custom set size, and a non-default range", () => {
  assert.equal(projectSessionFilterLabel(filter()), "My sessions");
  assert.equal(
    projectSessionFilterLabel(filter({ members: members("all") })),
    "All sessions",
  );
  assert.equal(
    projectSessionFilterLabel(
      filter({
        members: members("custom", [ALICE, BOB]),
        range: { kind: "week" },
      }),
    ),
    "Custom · 2 · This week",
  );
  assert.equal(projectSessionDateRangeLabel({ kind: "any" }), "Any time");
  assert.equal(
    projectSessionDateRangeLabel({
      kind: "custom",
      from: "2026-08-01",
      to: null,
    }),
    "2026-08-01 – …",
  );
  assert.equal(
    projectSessionDateRangeLabel({ kind: "custom", from: null, to: null }),
    "Custom",
  );
});

test("the unattributed note is absent at zero and grammatical otherwise", () => {
  assert.equal(projectSessionUnattributedNote(0), null);
  assert.match(
    projectSessionUnattributedNote(1),
    /^1 session without .* is hidden/,
  );
  assert.match(
    projectSessionUnattributedNote(3),
    /^3 sessions without .* are hidden/,
  );
});

test("parseProjectSessionFilter normalises junk and upgrades the first stored shape", () => {
  assert.deepEqual(
    parseProjectSessionFilter(null),
    DEFAULT_PROJECT_SESSION_FILTER,
  );
  assert.deepEqual(
    parseProjectSessionFilter("all"),
    DEFAULT_PROJECT_SESSION_FILTER,
  );
  // The first release stored the bare member filter.
  assert.deepEqual(parseProjectSessionFilter({ mode: "all" }), {
    ...DEFAULT_PROJECT_SESSION_FILTER,
    members: { mode: "all" },
  });
  assert.deepEqual(
    parseProjectSessionFilter({
      members: {
        mode: "custom",
        pubkeys: [ALICE.toUpperCase(), 7, ALICE, BOB],
      },
      showClosed: false,
      showArchived: true,
      range: { kind: "custom", from: "2026-08-01", to: "nope" },
    }),
    {
      members: { mode: "custom", pubkeys: [ALICE, BOB] },
      showClosed: false,
      showArchived: true,
      range: { kind: "custom", from: "2026-08-01", to: null },
      // Stored before the axis existed: it reads as the default rather than
      // undefined, so an older build's state does not hide what a teammate
      // pinned.
      showPinnedArtifacts: true,
    },
  );
  assert.deepEqual(
    parseProjectSessionFilter({
      members: { mode: "everything" },
      showClosed: "yes",
      range: { kind: "fortnight" },
    }),
    DEFAULT_PROJECT_SESSION_FILTER,
  );
});

test("a stored blob parses per project and survives corruption", () => {
  assert.deepEqual(parseProjectSessionFilterState(null), {});
  assert.deepEqual(parseProjectSessionFilterState("not json"), {});
  assert.deepEqual(parseProjectSessionFilterState("[1,2]"), {});
  assert.deepEqual(
    parseProjectSessionFilterState(
      JSON.stringify({
        "owner:alpha": { mode: "all" },
        "owner:beta": {
          members: { mode: "custom", pubkeys: [BOB] },
          showClosed: true,
          showArchived: true,
          range: { kind: "week" },
        },
        "owner:gamma": { mode: "bogus" },
      }),
    ),
    {
      "owner:alpha": {
        ...DEFAULT_PROJECT_SESSION_FILTER,
        members: { mode: "all" },
      },
      "owner:beta": {
        members: { mode: "custom", pubkeys: [BOB] },
        showClosed: true,
        showArchived: true,
        range: { kind: "week" },
        showPinnedArtifacts: true,
      },
      "owner:gamma": DEFAULT_PROJECT_SESSION_FILTER,
    },
  );
});

test("hiding pinned artifacts says how many and how to get them back", () => {
  // The rule this module states: nothing is hidden silently. A pin is
  // shared, so a row the sidebar drops without a word is a row somebody
  // else put there and nobody can find.
  assert.equal(projectHiddenArtifactsNote(0), null);
  assert.equal(projectHiddenArtifactsNote(-1), null);
  const one = projectHiddenArtifactsNote(1);
  assert.match(one, /^1 pinned artifact is hidden/);
  assert.match(one, /Show pinned artifacts/);
  const many = projectHiddenArtifactsNote(3);
  assert.match(many, /^3 pinned artifacts are hidden/);
  assert.match(many, /to see them\.$/);
});
