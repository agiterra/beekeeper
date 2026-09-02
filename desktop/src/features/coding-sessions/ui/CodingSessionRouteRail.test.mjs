import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  buildCodingSessionRouteTransactions,
  deriveCodingSessionRoute,
} from "../lib/codingSessionRouteModel.ts";
import { buildCodingSessionMissionTransactionRows } from "../lib/codingSessionMissionTransactionRows.ts";
import {
  CodingSessionRouteRail,
  layOutRouteStretchLabels,
} from "./CodingSessionRouteRail.tsx";
import { CodingSessionRouteScrubber } from "./CodingSessionRouteScrubber.tsx";

const DAY = 1_788_400_000;
const at = (h, m, s = 0) => DAY + h * 3_600 + m * 60 + s;
const FOUNDER = "f0".repeat(32);
const LEAD_ACTOR = "1a".repeat(32);
const BUILDER_ACTOR = "2b".repeat(32);
const RUNNER_ACTOR = "3c".repeat(32);
const ASSIGNMENT_ID = "a1".repeat(32);
const REPORT_ID = "b2".repeat(32);
const BLOCKED_ID = "e5".repeat(32);
const LEAD_TARGET = "target:lead";

const ACTORS = new Map([
  [LEAD_ACTOR, { label: "Keystone · Lead", executionKey: "exec-lead" }],
  [BUILDER_ACTOR, { label: "Bob · Builder", executionKey: "exec-builder" }],
  [RUNNER_ACTOR, { label: "Gordan · Runner", executionKey: "exec-runner" }],
]);

const INPUTS = [
  {
    sourceEventId: ASSIGNMENT_ID,
    type: "assignment",
    authorPubkey: LEAD_ACTOR,
    createdAt: at(21, 46),
    counterpartyPubkey: BUILDER_ACTOR,
    parentEventId: null,
    summary: "Lane A — both sides, red-first, gates",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
  },
  {
    sourceEventId: BLOCKED_ID,
    type: "mission.blocked",
    authorPubkey: LEAD_ACTOR,
    createdAt: at(21, 57),
    counterpartyPubkey: null,
    parentEventId: null,
    summary: "Lane B holds until lane A's gate report lands.",
    decision: null,
    requiredAction: "Rule on lane A before lane B starts.",
    fileCount: null,
    testCount: null,
    unseated: false,
  },
  {
    sourceEventId: REPORT_ID,
    type: "report",
    authorPubkey: BUILDER_ACTOR,
    createdAt: at(21, 58),
    counterpartyPubkey: LEAD_ACTOR,
    parentEventId: ASSIGNMENT_ID,
    summary: "Lane A — both sides, gates run",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
  },
];

function fixtureRoute(overrides = {}) {
  const rows = buildCodingSessionMissionTransactionRows({
    transactions: INPUTS,
    resolveActor: (pubkey) =>
      ACTORS.get(pubkey) ?? { label: null, executionKey: null },
    founderPubkey: FOUNDER,
    deliveries: [],
    density: "live",
  });
  return {
    route: deriveCodingSessionRoute({
      founderPubkey: FOUNDER,
      participants: [
        {
          executionKey: "exec-lead",
          label: "Keystone · Lead",
          actorPubkey: LEAD_ACTOR,
          role: "lead",
          targetKey: LEAD_TARGET,
          word: "live",
          live: true,
          firstSignedAt: at(21, 34),
          releasedAt: null,
        },
        {
          executionKey: "exec-builder",
          label: "Bob · Builder",
          actorPubkey: BUILDER_ACTOR,
          role: "builder",
          targetKey: "target:builder",
          word: "idle",
          live: false,
          firstSignedAt: at(21, 39),
          releasedAt: null,
        },
        {
          executionKey: "exec-runner",
          label: "Gordan · Runner",
          actorPubkey: RUNNER_ACTOR,
          role: "runner",
          targetKey: "target:runner",
          word: "idle",
          live: false,
          firstSignedAt: at(22, 2),
          releasedAt: null,
        },
      ],
      transactions: buildCodingSessionRouteTransactions(rows, INPUTS),
      deliveries: [
        {
          sourceEventId: REPORT_ID,
          operationType: "report",
          sourceActorPubkey: BUILDER_ACTOR,
          leadTargetKey: LEAD_TARGET,
          kind: "provider-queued",
          owningCommandId: "cmd-1",
          duplicateRefusedCommandIds: [],
          failures: [],
          reArmCount: 0,
          observedAtMs: at(21, 58) * 1_000,
          detail: "Provider wake queued",
        },
      ],
      seatAuthorities: [
        {
          executionKey: "exec-runner",
          actorPubkey: RUNNER_ACTOR,
          role: "runner",
          kind: "created-ungranted",
          grantEventId: null,
          detail: "Seat created, not granted",
          remedy:
            "bee sessions seat-repair --channel c --session-ref s --actor a",
        },
      ],
      hires: [
        {
          executionKey: "exec-builder",
          hiredByPubkey: LEAD_ACTOR,
          at: at(21, 39),
          sourceEventId: "11".repeat(32),
        },
        {
          executionKey: "exec-runner",
          hiredByPubkey: LEAD_ACTOR,
          at: at(22, 2),
          sourceEventId: "22".repeat(32),
        },
      ],
      visibleAt: [at(21, 46), at(21, 58)],
      nowMs: (at(21, 58) + 260) * 1_000,
      ...overrides,
    }),
    rows,
  };
}

test("the rail is one nav with a screen-reader list of every sign, oldest first", () => {
  const { route } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route }),
  );
  assert.match(markup, /<nav aria-label="Route"/);
  assert.match(markup, /aria-label="Route signs, oldest first"/);
  // Scoped to the sign list: the rail also carries a `Route roads` list whose
  // items say where each road's start came from (F1).
  const signList = markup.slice(
    markup.indexOf('aria-label="Route signs, oldest first"'),
    markup.indexOf('aria-label="Route roads"'),
  );
  const listed = [...signList.matchAll(/<li>([^<]*)/g)].map(
    (match) => match[1],
  );
  assert.equal(listed.length, route.signs.length);
  for (const [index, sign] of route.signs.entries()) {
    assert.ok(
      listed[index].startsWith(sign.title.replaceAll("&", "&amp;")),
      `sr row ${index} is not ${sign.title}`,
    );
  }
});

test("every sign's accessible name is its row's own title and time", () => {
  const { route, rows } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route }),
  );
  const labels = [
    ...markup.matchAll(/aria-label="([^"]*)"[^>]*data-kind="([^"]*)"/g),
  ];
  assert.ok(labels.length >= rows.length);
  for (const row of rows) {
    const expected = `${row.title} · ${row.meta.timeLabel}`;
    assert.ok(
      markup.includes(`aria-label="${expected}"`),
      `no sign labelled ${expected}`,
    );
  }
});

test("a queued wake renders as a dashed stretch carrying its own duration", () => {
  const { route } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route }),
  );
  assert.match(markup, /data-kind="queued"/);
  assert.match(markup, /· 4m 20s ·/);
  assert.match(markup, /stroke-dasharray="2 3"/);
  assert.equal(
    route.stretches.filter((stretch) => stretch.kind === "queued").length,
    1,
  );
});

test("the road heads name each participant, their W1 word, and an ungranted seat", () => {
  const { route } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route }),
  );
  assert.match(markup, /Now · /);
  assert.match(markup, /Keystone · Lead/);
  assert.match(markup, /Gordan · Runner/);
  // The founder is not a seat, so the founder's head carries no W1 word.
  assert.equal(
    [...markup.matchAll(/data-testid="coding-session-route-head"/g)].length,
    4,
  );
  assert.equal(
    [...markup.matchAll(/data-testid="coding-session-route-head-word"/g)]
      .length,
    3,
  );
  assert.match(markup, /Seat created, not granted/);
  assert.match(markup, /bee sessions seat-repair/);
  // R6: the band says where the reader is and how far Now is.
  assert.match(markup, /You are here/);
  assert.match(markup, /4m 20s to Now/);
});

test("the rail uses theme tokens only — no hex colours, no arbitrary text sizes", () => {
  const { route } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route }),
  );
  assert.doesNotMatch(markup, /#[0-9a-fA-F]{3,8}\b/);
  assert.doesNotMatch(markup, /text-\[/);
});

test("the scrubber names its attention signs in words and can be expanded", () => {
  const { route } = fixtureRoute();
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteScrubber, { route }),
  );
  assert.match(markup, /data-testid="coding-session-route-scrubber"/);
  assert.match(markup, /aria-expanded="false"/);
  const label = markup.match(/aria-label="Route — ([^"]*)"/);
  assert.ok(label, "the scrubber must name why its track is not empty");
  assert.match(label[1], /mission blocked/);
  assert.match(label[1], /ungranted/);
  assert.match(label[1], /tap to expand/);
  // Only the attention signs survive the fold — the assignment and the report
  // are detail, and detail is what folding is allowed to hide.
  assert.equal(
    [...markup.matchAll(/data-testid="coding-session-route-scrubber-sign"/g)]
      .length,
    2,
  );
});

// ---------------------------------------------------------------------------
// Fix round 1 (REVIEW-A4)
// ---------------------------------------------------------------------------

/** The same fixture with every create undated — what a legacy session looks like. */
function undatedRoute() {
  const { route } = fixtureRoute();
  return deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: [
      {
        executionKey: "exec-lead",
        label: "Keystone · Lead",
        actorPubkey: LEAD_ACTOR,
        role: "lead",
        targetKey: LEAD_TARGET,
        word: "live",
        live: true,
        firstSignedAt: at(21, 34),
        releasedAt: null,
      },
      {
        executionKey: "exec-builder",
        label: "Bob · Builder",
        actorPubkey: BUILDER_ACTOR,
        role: "builder",
        targetKey: "target:builder",
        word: "idle",
        live: false,
        firstSignedAt: at(21, 39),
        releasedAt: at(21, 59),
      },
    ],
    transactions: [],
    hires: [],
    nowMs: route.nowAt * 1_000,
  });
}

test("F1: an unproven road start wears an open cap and says so; a create's is filled", () => {
  const dated = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, {
      route: fixtureRoute().route,
    }),
  );
  assert.match(dated, /road starts since its create/);
  assert.match(dated, /class="fill-current [^"]*" cx="\d+" cy="\d+" r="3"/);

  const undated = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route: undatedRoute() }),
  );
  assert.match(
    undated,
    /road starts since its first signed sign, not a create/,
  );
  assert.match(
    undated,
    /class="fill-none stroke-current [^"]*" cx="\d+" cy="\d+" r="3"/,
  );
  assert.doesNotMatch(undated, /road starts since its create/);
});

test("F6: the whole rail is one tab stop, and a selected sign can say so", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, {
      route: fixtureRoute().route,
    }),
  );
  const stops = [...markup.matchAll(/tabindex="0"/g)].length;
  assert.equal(stops, 1, `the rail must be one tab stop, found ${stops}`);
  const negative = [...markup.matchAll(/tabindex="-1"/g)].length;
  assert.ok(negative > 3, "every other focusable must be taken out of order");
  // `aria-pressed` exists on the signs and starts false; the rail flips it on
  // the sign it reveals, which a constant `false` could never do.
  assert.match(
    markup,
    /aria-pressed="false"[^>]*data-testid="coding-session-route-sign"/,
  );
});

test("F7: a locally-observed sign is drawn hollow, marked ~, and says it is not signed", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, {
      route: fixtureRoute().route,
    }),
  );
  assert.match(markup, /data-time-source="local"/);
  assert.match(markup, /Provider wake queued[^"]*— local time, not signed/);
  // The visible `~`, in its own element — not the one inside the tooltip.
  assert.match(markup, /class="shrink-0 text-muted-foreground\/70">~<\/span>/);
  // And the dot is hollow, so the mixed axis is visible without reading.
  assert.match(markup, /size-1\.5 shrink-0 rounded-full border border-current/);
  const signed = [...markup.matchAll(/data-time-source="signed"/g)].length;
  assert.ok(signed >= 3, "every 44244 sign stays on the signed axis");

  const undated = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: [
      {
        executionKey: "exec-lead",
        label: "Keystone · Lead",
        actorPubkey: LEAD_ACTOR,
        role: "lead",
        targetKey: LEAD_TARGET,
        word: "live",
        live: true,
        firstSignedAt: at(21, 34),
        releasedAt: null,
      },
    ],
    transactions: [],
    deliveries: [
      {
        sourceEventId: REPORT_ID,
        operationType: "report",
        sourceActorPubkey: BUILDER_ACTOR,
        leadTargetKey: LEAD_TARGET,
        kind: "provider-queued",
        owningCommandId: "cmd-1",
        duplicateRefusedCommandIds: [],
        failures: [],
        reArmCount: 0,
        observedAtMs: null,
        detail: "Provider wake queued",
      },
    ],
    hires: [],
    nowMs: at(22, 0) * 1_000,
  });
  const undatedMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route: undated }),
  );
  assert.match(
    undatedMarkup,
    /data-testid="coding-session-route-head-undated"/,
  );
  assert.match(undatedMarkup, /1 undated/);
});

test("F10: ticks, attention anchors, road-end caps and a clickable `+N earlier`", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, {
      route: fixtureRoute().route,
    }),
  );
  // R4's dotted tick from the road to the sign.
  assert.match(markup, /stroke-dasharray="1 2"/);
  // R1's end caps: the live lead is filled and breathing, an idle seat hollow.
  assert.match(
    markup,
    /fill-current coding-session-agent-breathe[^"]*" cx="\d+" cy="\d+" r="4"/,
  );
  assert.match(
    markup,
    /class="fill-none stroke-current [^"]*" cx="\d+" cy="\d+" r="4"/,
  );

  const released = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, { route: undatedRoute() }),
  );
  // A released seat's road stops with a flat cap, not a ring.
  assert.match(released, /stroke-width="2" x1="\d+" x2="\d+"/);

  // §9.5's marker is a control, not a sentence.
  const many = Array.from({ length: 214 }, (_unused, index) => ({
    sourceEventId: index.toString(16).padStart(64, "0"),
    type: "assignment",
    authorPubkey: LEAD_ACTOR,
    createdAt: at(1, 0) + index,
    counterpartyPubkey: null,
    parentEventId: null,
    summary: "s",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
  }));
  const rows = buildCodingSessionMissionTransactionRows({
    transactions: many,
    resolveActor: (pubkey) =>
      ACTORS.get(pubkey) ?? { label: null, executionKey: null },
    founderPubkey: FOUNDER,
    deliveries: [],
    density: "live",
  });
  const bounded = deriveCodingSessionRoute({
    founderPubkey: FOUNDER,
    participants: [
      {
        executionKey: "exec-lead",
        label: "Keystone · Lead",
        actorPubkey: LEAD_ACTOR,
        role: "lead",
        targetKey: LEAD_TARGET,
        word: "live",
        live: true,
        firstSignedAt: at(1, 0),
        releasedAt: null,
      },
    ],
    transactions: buildCodingSessionRouteTransactions(rows, many),
    hires: [],
    nowMs: (at(1, 0) + 400) * 1_000,
  });
  const boundedMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionRouteRail, {
      onExpandRoad: () => {},
      route: bounded,
    }),
  );
  assert.match(
    boundedMarkup,
    /<button[^>]*data-testid="coding-session-route-earlier"/,
  );
  assert.match(boundedMarkup, /\+14 earlier/);
});

test("F15: two overlapping wakes never stack their duration labels on top of each other", () => {
  const route = {
    stretches: [
      { key: "a", kind: "queued", offsetPx: 48, heightPx: 120, label: "10m" },
      { key: "b", kind: "queued", offsetPx: 72, heightPx: 96, label: "8m" },
      { key: "c", kind: "silence", offsetPx: 0, heightPx: 48, label: "30m" },
    ],
  };
  const laid = layOutRouteStretchLabels(route);
  assert.deepEqual(
    laid.map((stretch) => stretch.label),
    ["30m", "10m", "8m"],
    "labels are laid out top-down",
  );
  for (let index = 1; index < laid.length; index += 1) {
    assert.ok(
      laid[index].labelTopPx - laid[index - 1].labelTopPx >= 14,
      `labels ${index - 1} and ${index} collide at ${laid[index].labelTopPx}`,
    );
  }
  // The measurement itself is never rewritten to make room.
  assert.deepEqual(
    laid.map((stretch) => stretch.label),
    route.stretches
      .slice()
      .sort((left, right) => left.offsetPx - right.offsetPx)
      .map((stretch) => stretch.label),
  );
});
