import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { codingSessionParticipantAccent } from "../lib/codingSessionParticipantAccent.ts";
import { CodingSessionParticipantBar } from "./CodingSessionParticipantBar.tsx";

test("the Mission roster gives participant identity and status real weight", () => {
  const key = "f0".repeat(32);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: "builder",
      items: [
        {
          executionKey: "lead",
          label: "Helios · Lead",
          secondaryLabel: "Codex · gpt-5.6-sol",
          role: "lead",
          status: { kind: "idle", label: "Idle" },
          disposition: "idle",
          activity: null,
          lastTurnLabel: "last turn 4m ago",
        },
        {
          executionKey: "builder",
          label: "Bob · Builder",
          secondaryLabel: "Claude Code · sonnet",
          role: "builder",
          status: { kind: "working", label: "Working" },
          disposition: "live",
          activity: "implementing the stream foundation",
          lastTurnLabel: "last turn just now",
        },
      ],
      onFocus() {},
    }),
  );
  assert.match(markup, /aria-label="Session participants"/);
  assert.match(markup, /Helios · Lead/);
  assert.match(markup, /Bob · Builder/);
  assert.match(markup, /aria-pressed="true"/);
  assert.match(markup, /coding-session-agent-breathe/);
  assert.doesNotMatch(markup, new RegExp(key));
});

test("an empty Mission roster adds no chrome to Conversation", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionParticipantBar, {
        focusedExecutionKey: null,
        items: [],
        onFocus() {},
      }),
    ),
    "",
  );
});

test("workflow controls lead the roster inside one shared strip", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "lead",
          label: "Helios · Lead",
          secondaryLabel: null,
          role: "lead",
          status: { kind: "idle", label: "Idle" },
          disposition: "idle",
          activity: null,
          lastTurnLabel: "last turn just now",
        },
      ],
      leading: React.createElement("span", null, "Brief · Live · Trace"),
      onFocus() {},
    }),
  );
  assert.ok(markup.indexOf("Brief · Live · Trace") < markup.indexOf("Helios"));
  assert.equal(
    (markup.match(/aria-label="Session participants"/g) ?? []).length,
    1,
  );
});

test("identity accents never consume attention colors", () => {
  const forbidden = /amber|yellow|red|destructive/;
  for (const executionKey of ["lead", "seat-1", "verifier", "builder"]) {
    const accent = codingSessionParticipantAccent(executionKey);
    assert.doesNotMatch(Object.values(accent).join(" "), forbidden);
  }

  const waiting = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "verifier",
          label: "Parallax · Verifier",
          secondaryLabel: "Codex · gpt-5.6-sol",
          role: "verifier",
          status: { kind: "waiting", label: "Waiting" },
          disposition: "waiting for you",
          activity: null,
          lastTurnLabel: "last turn just now",
        },
      ],
      onFocus() {},
    }),
  );
  // Amber remains a state word: waiting is actionable, not an identity hue.
  assert.match(waiting, /bg-amber-500/);
});

test("only No provider answering consumes destructive styling", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "failed",
          label: "Bob · Builder",
          secondaryLabel: null,
          role: "builder",
          status: {
            kind: "unknown",
            label: "Needs attention",
            attention: "failed",
          },
          disposition: "needs attention",
          activity: null,
          lastTurnLabel: "last turn 1m ago",
        },
        {
          executionKey: "unreachable",
          label: "Parallax · Verifier",
          secondaryLabel: null,
          role: "verifier",
          status: {
            kind: "unknown",
            label: "No provider answering",
            attention: "unreachable",
          },
          disposition: "no provider answering",
          activity: null,
          lastTurnLabel: "last turn 2m ago",
        },
      ],
      onFocus() {},
    }),
  );
  const chips = markup.match(/<button[\s\S]*?<\/button>/g) ?? [];
  assert.equal(chips.length, 2);
  assert.doesNotMatch(chips[0], /destructive/);
  assert.match(chips[0], /amber/);
  assert.match(chips[1], /destructive/);
});

const SEAT_ACTOR = "b1".repeat(32);
const OTHER_ACTOR = "c2".repeat(32);

function chipItems() {
  return [
    {
      executionKey: "lead",
      label: "Keystone · Lead",
      secondaryLabel: "Codex · gpt-5.6-sol",
      role: "lead",
      status: { kind: "working", label: "Working" },
      disposition: "live",
      activity: null,
      lastTurnLabel: "last turn just now",
    },
    {
      executionKey: "builder",
      label: "Bob · Builder",
      secondaryLabel: "Claude Code · sonnet",
      role: "builder",
      status: { kind: "idle", label: "Idle" },
      disposition: "idle",
      activity: null,
      lastTurnLabel: "last turn 4m ago",
    },
  ];
}

function authority(kind, executionKey, actorPubkey) {
  return {
    executionKey,
    actorPubkey,
    role: "builder",
    kind,
    grantEventId: kind === "granted" ? "g".repeat(64) : null,
    detail:
      kind === "granted"
        ? "Seat granted"
        : kind === "created-ungranted"
          ? "Seat created, not granted"
          : "Seat authority unknown",
    remedy:
      kind === "created-ungranted"
        ? "bee sessions seat-repair --channel chan --session-ref sess --actor act"
        : null,
  };
}

function wakeDelivery(overrides) {
  return {
    sourceEventId: "e".repeat(64),
    operationType: "report",
    sourceActorPubkey: SEAT_ACTOR,
    leadTargetKey: "lead-target",
    kind: "provider-queued",
    owningCommandId: "cmd-1",
    duplicateRefusedCommandIds: [],
    failures: [],
    reArmCount: 0,
    observedAtMs: 10,
    detail: "Provider wake queued",
    ...overrides,
  };
}

test("U-T3: an ungranted seat is badged on its chip with the remedy in the title", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatAuthorities: [
        authority("granted", "lead", OTHER_ACTOR),
        authority("created-ungranted", "builder", SEAT_ACTOR),
      ],
    }),
  );
  assert.match(markup, /data-testid="coding-session-seat-authority-badge"/);
  assert.match(markup, />ungranted</);
  assert.match(
    markup,
    /title="Seat created, not granted — bee sessions seat-repair --channel chan --session-ref sess --actor act"/,
  );
  assert.equal(
    markup.match(/coding-session-seat-authority-badge/g).length,
    1,
    "a granted seat adds no badge",
  );
});

test("U-T3: the newest non-started delivery lands on its own seat's chip only", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatAuthorities: [
        authority("granted", "lead", OTHER_ACTOR),
        authority("granted", "builder", SEAT_ACTOR),
      ],
      deliveries: [
        wakeDelivery({ kind: "provider-queued", observedAtMs: 10 }),
        wakeDelivery({
          sourceEventId: "f".repeat(64),
          kind: "fallback-queued",
          observedAtMs: 40,
          detail: "Desktop covered for the provider",
        }),
        wakeDelivery({
          sourceEventId: "0".repeat(64),
          sourceActorPubkey: OTHER_ACTOR,
          kind: "provider-started",
          observedAtMs: 90,
          detail: "Provider wake started",
        }),
      ],
    }),
  );
  const badges = markup.match(/coding-session-delivery-badge/g) ?? [];
  assert.equal(badges.length, 1, "one delivery badge, on one chip");
  assert.match(markup, /data-kind="fallback-queued"/);
  assert.match(markup, />fallback</);
  assert.doesNotMatch(markup, /data-kind="provider-started"/);
});

test("U-T3: without a seat-authority projection the chip stays status-only", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      deliveries: [wakeDelivery({})],
    }),
  );
  assert.doesNotMatch(markup, /coding-session-delivery-badge/);
  assert.doesNotMatch(markup, /coding-session-seat-authority-badge/);
});

test("U-T3: the bar draws no band of its own — the header owns the border", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      className: "border-b border-border/60",
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
    }),
  );
  const nav = markup.slice(0, markup.indexOf("</nav>"));
  const navClass = nav.match(/<nav[^>]*class="([^"]*)"/)[1];
  assert.match(navClass, /border-b/, "the finalizer's class is merged in");
  assert.doesNotMatch(
    renderToStaticMarkup(
      React.createElement(CodingSessionParticipantBar, {
        focusedExecutionKey: null,
        items: chipItems(),
        onFocus() {},
      }),
    ).match(/<nav[^>]*class="([^"]*)"/)[1],
    /border-b|bg-background/,
  );
});

test("U-F2: the chip is status-only — the activity phrase lives in the live strip", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: [
        {
          executionKey: "builder",
          label: "Bob · Builder",
          secondaryLabel: "Claude Code · sonnet",
          role: "builder",
          status: { kind: "working", label: "Working" },
          disposition: "live",
          activity: "Verify the signed live activity",
          lastTurnLabel: "last turn just now",
        },
      ],
      onFocus() {},
    }),
  );
  // The W1 word stays — that is the chip's whole job.
  assert.match(markup, />live</);
  assert.match(markup, /Bob · Builder/);
  // The phrase does not: LiveActivityBar is its one home.
  assert.doesNotMatch(markup, /Verify the signed live activity/);
});

const BUNDLED_STAMP = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: "0.1.0",
  sha: "23728227b",
  dirty: false,
};

const PATH_STAMP = {
  path: "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
  source: "path",
  version: "0.1.0",
  sha: "07c470be0",
  dirty: true,
};

test("L12: each seat's chip names the bee that seat is actually running", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatBeeStamps: new Map([
        ["lead", BUNDLED_STAMP],
        ["builder", PATH_STAMP],
      ]),
    }),
  );
  assert.match(markup, /bee 23728227b \(bundled\)/);
  assert.match(
    markup,
    /bee 07c470be0-dirty \(found on PATH: \/Users\/brian\/Projects\/beekeeper\/beekeeper\/target\/debug\)/,
  );
  assert.equal(
    (markup.match(/data-testid="coding-session-seat-bee"/g) ?? []).length,
    2,
    "one bee line per seat that has a stamp",
  );
});

test("L12: an unparsed --version reads unknown on the chip, never blank", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatBeeStamps: new Map([
        [
          "builder",
          { ...BUNDLED_STAMP, version: null, sha: null, dirty: null },
        ],
      ]),
    }),
  );
  assert.match(markup, /bee build unknown/);
  assert.equal(
    (markup.match(/data-testid="coding-session-seat-bee"/g) ?? []).length,
    1,
  );
});

test("L12: a seat whose host published no stamp keeps the chip it had", () => {
  const withoutStamps = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
    }),
  );
  const withNullStamps = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatBeeStamps: new Map([
        ["lead", null],
        ["builder", null],
      ]),
    }),
  );
  assert.doesNotMatch(withoutStamps, /coding-session-seat-bee/);
  assert.equal(
    withNullStamps,
    withoutStamps,
    "a stampless seat's chip is byte-identical to before",
  );
});

test("L12: the bee line shares the badge row rather than adding one of its own", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatAuthorities: [
        authority("granted", "lead", OTHER_ACTOR),
        authority("created-ungranted", "builder", SEAT_ACTOR),
      ],
      seatBeeStamps: new Map([["builder", BUNDLED_STAMP]]),
    }),
  );
  const chips = markup.match(/<button[\s\S]*?<\/button>/g) ?? [];
  const builderChip = chips.find((chip) => chip.includes("Bob · Builder"));
  assert.ok(builderChip.includes("coding-session-seat-authority-badge"));
  assert.ok(builderChip.includes("coding-session-seat-bee"));
  // One badge row on the chip, carrying both facts.
  assert.equal(
    (builderChip.match(/class="mt-1 flex min-w-0 flex-wrap/g) ?? []).length,
    1,
  );
});

const PACK_REF = {
  repo: `30617:${SEAT_ACTOR}:agiterra-packs`,
  sha: "c".repeat(40),
  role: "builder",
  path: "personas/roles/builder",
};

test("LANE-L23: a seat's chip names the pack that seat actually staged", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatPackRefs: new Map([
        ["lead", null],
        ["builder", PACK_REF],
      ]),
    }),
  );
  assert.match(markup, /no pack staged/);
  assert.match(markup, /pack builder@cccccccc/);
  assert.equal(
    (markup.match(/data-testid="coding-session-seat-pack"/g) ?? []).length,
    2,
    "one pack line per seat once the caller plumbs pack data at all",
  );
});

test("LANE-L23: a caller that never plumbs seatPackRefs adds no pack line at all", () => {
  const withoutPackRefs = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
    }),
  );
  assert.doesNotMatch(withoutPackRefs, /coding-session-seat-pack/);
});

test("LANE-L23: the pack line shares the badge row rather than adding one of its own", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionParticipantBar, {
      focusedExecutionKey: null,
      items: chipItems(),
      onFocus() {},
      seatBeeStamps: new Map([["builder", BUNDLED_STAMP]]),
      seatPackRefs: new Map([["builder", PACK_REF]]),
    }),
  );
  const chips = markup.match(/<button[\s\S]*?<\/button>/g) ?? [];
  const builderChip = chips.find((chip) => chip.includes("Bob · Builder"));
  assert.ok(builderChip.includes("coding-session-seat-bee"));
  assert.ok(builderChip.includes("coding-session-seat-pack"));
  assert.equal(
    (builderChip.match(/class="mt-1 flex min-w-0 flex-wrap/g) ?? []).length,
    1,
  );
});
