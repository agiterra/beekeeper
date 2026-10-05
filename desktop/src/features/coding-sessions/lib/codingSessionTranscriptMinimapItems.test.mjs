import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_MINIMAP_MIN_ITEMS,
  codingSessionMinimapCurrentIndex,
  codingSessionMinimapHasPersistentGutter,
  codingSessionMinimapHeightStyle,
  codingSessionMinimapHitStripWidth,
  codingSessionMinimapIndexFromPointer,
  codingSessionMinimapInteractiveWidth,
  codingSessionMinimapNavigationInteractive,
  codingSessionMinimapPromptLine,
  codingSessionMinimapReplyPreview,
  codingSessionMinimapTopPercent,
  deriveCodingSessionMinimapItemsFromModel,
  resolveCodingSessionMinimapAuthor,
} from "./codingSessionTranscriptMinimapItems.ts";

const ME = "a".repeat(64);
const OTHER = "b".repeat(64);

function message(id, role, text, extra = {}) {
  return {
    kind: "item",
    item: {
      id,
      type: "message",
      renderClass: "message",
      role,
      title: role,
      text,
      timestamp: "2026-10-04T10:00:00.000Z",
      ...extra,
    },
  };
}

function turn(id, entries, extra = {}) {
  return {
    kind: "turn",
    id,
    entries,
    changedFiles: [],
    diagnostics: [],
    completion: null,
    isWorking: false,
    superseded: false,
    startedAt: null,
    fold: null,
    ...extra,
  };
}

test("one item per turn that opens with a prompt, keyed by its row", () => {
  const model = {
    blocks: [
      turn("t1", [
        message("p1", "user", "  Inspect\n this  ", { operatorPubkey: ME }),
        message("a1", "assistant", "Working"),
        message("a2", "assistant", " Done\t now "),
      ]),
      // A producer turn with no prompt gets no dash, as T3 draws one per
      // user message.
      turn("t2", [message("a3", "assistant", "Autonomous")]),
      {
        kind: "standalone",
        id: "s1",
        entry: message("s1", "assistant", "loose"),
      },
      turn("t3", [message("p2", "user", "Next", { operatorPubkey: OTHER })]),
    ],
    diagnostics: [],
    sessionFacts: [],
  };
  const items = deriveCodingSessionMinimapItemsFromModel(model, ME);
  assert.deepEqual(
    items.map((item) => [item.id, item.key, item.rowIndex]),
    [
      ["p1", "turn:t1", 0],
      ["p2", "turn:t3", 3],
    ],
  );
  // The reply is the turn's last assistant text; the source stays untouched
  // until the preview compacts it.
  assert.equal(items[0].assistantText, " Done\t now ");
  assert.equal(
    codingSessionMinimapReplyPreview(items[0].assistantText),
    "Done now",
  );
  assert.equal(items[1].assistantText, null);
  assert.equal(items[0].authorKind, "you");
  assert.equal(items[1].authorKind, "other");
  assert.equal(items[1].authorPubkey, OTHER);
});

test("the reply is the agent's prose, not the provider's result body", () => {
  const model = {
    blocks: [
      turn("t1", [
        message("p1", "user", "Fix step 5", { operatorPubkey: ME }),
        message("a1", "assistant", "Reply 5: step 5 now backs off."),
        message("r1:assistant-result", "assistant", "Done."),
      ]),
      // The result body is the reply only when the agent wrote no prose.
      turn("t2", [
        message("p2", "user", "Quiet turn", { operatorPubkey: ME }),
        message("r2:assistant-result", "assistant", "Done."),
      ]),
    ],
    diagnostics: [],
    sessionFacts: [],
  };
  const items = deriveCodingSessionMinimapItemsFromModel(model, ME);
  assert.equal(items[0].assistantText, "Reply 5: step 5 now backs off.");
  assert.equal(items[1].assistantText, "Done.");
});

test("the card facts: duration, the first three files, failure, start", () => {
  const files = ["a.ts", "b.ts", "c.ts", "d.ts"].map((filename) => ({
    path: `src/${filename}`,
    filename,
    additions: 1,
    deletions: 0,
    diffs: [],
    editCount: 1,
  }));
  const model = {
    blocks: [
      turn("t1", [message("p1", "user", "Go")], {
        changedFiles: files,
        startedAt: "2026-10-04T10:00:00.000Z",
        completion: {
          durationMs: 61_000,
          costUsd: null,
          costBasis: null,
          outcome: null,
          timestamp: "2026-10-04T10:01:01.000Z",
          state: "failed",
        },
      }),
      turn("t2", [message("p2", "user", "Again")], {
        fold: { durationMs: 5_000 },
      }),
    ],
    diagnostics: [],
    sessionFacts: [],
  };
  const [first, second] = deriveCodingSessionMinimapItemsFromModel(model, ME);
  assert.equal(first.durationMs, 61_000);
  assert.equal(first.changedFileCount, 4);
  assert.deepEqual(first.changedFileNames, ["a.ts", "b.ts", "c.ts"]);
  assert.equal(first.failed, true);
  assert.equal(first.startedAtMs, Date.parse("2026-10-04T10:00:00.000Z"));
  // No completion: the fold's measured span, and the prompt's own time.
  assert.equal(second.durationMs, 5_000);
  assert.equal(second.failed, false);
  assert.equal(second.startedAtMs, Date.parse("2026-10-04T10:00:00.000Z"));
});

test("author kinds never guess the viewer", () => {
  assert.deepEqual(
    resolveCodingSessionMinimapAuthor({ operatorPubkey: ME }, ME),
    {
      kind: "you",
      pubkey: ME,
    },
  );
  assert.equal(
    resolveCodingSessionMinimapAuthor({ operatorPubkey: OTHER }, ME).kind,
    "other",
  );
  // Unstamped is unknown, not "you".
  assert.equal(resolveCodingSessionMinimapAuthor({}, ME).kind, "unrecorded");
  // An automatic wake on the viewer's key is nobody's typing.
  assert.equal(
    resolveCodingSessionMinimapAuthor(
      { operatorPubkey: ME, commandId: "team-wake-v1:x:y" },
      ME,
    ).kind,
    "automatic",
  );
  // An unknown viewer makes every stamp somebody else's.
  assert.equal(
    resolveCodingSessionMinimapAuthor({ operatorPubkey: ME }, null).kind,
    "other",
  );
});

test("the card's prompt line is the first non-empty line, compacted", () => {
  assert.equal(
    codingSessionMinimapPromptLine("\n  Fix   the bug \nthen more"),
    "Fix the bug",
  );
  assert.equal(codingSessionMinimapPromptLine("   "), null);
  assert.equal(codingSessionMinimapPromptLine(null), null);
});

test("T3 geometry: shown from two items, 8 px spacing, capped height", () => {
  assert.equal(CODING_SESSION_MINIMAP_MIN_ITEMS, 2);
  assert.equal(
    codingSessionMinimapHeightStyle(40),
    "min(312px, calc(100vh - 18rem))",
  );
  assert.equal(
    codingSessionMinimapHeightStyle(1),
    "min(1px, calc(100vh - 18rem))",
  );
  assert.equal(codingSessionMinimapTopPercent(0, 5), 0);
  assert.equal(codingSessionMinimapTopPercent(4, 5), 100);
  assert.equal(codingSessionMinimapTopPercent(9, 5), 100);
  assert.equal(codingSessionMinimapTopPercent(0, 1), 0);
  assert.equal(
    codingSessionMinimapIndexFromPointer({
      itemCount: 5,
      railTop: 100,
      railHeight: 40,
      pointerY: 121,
    }),
    2,
  );
  assert.equal(
    codingSessionMinimapIndexFromPointer({
      itemCount: 0,
      railTop: 0,
      railHeight: 40,
      pointerY: 0,
    }),
    null,
  );
});

test("T3's 48 px gutter rule and gutter-capped 40 px hit strip", () => {
  // 1000 wide, 900 content: a 50 px gutter, persistent; strip 38 px.
  assert.equal(codingSessionMinimapHasPersistentGutter(1000, 900), true);
  assert.equal(codingSessionMinimapHitStripWidth(1000, 900), 38);
  // A wide gutter caps at 40.
  assert.equal(codingSessionMinimapHitStripWidth(1400, 800), 40);
  // 40 px of gutter: not persistent, strip 28.
  assert.equal(codingSessionMinimapHasPersistentGutter(980, 900), false);
  assert.equal(codingSessionMinimapHitStripWidth(980, 900), 28);
  // No gutter: the strip goes inert.
  assert.equal(codingSessionMinimapHitStripWidth(900, 900), 0);
  assert.equal(codingSessionMinimapHitStripWidth(0, 900), 0);
  assert.equal(codingSessionMinimapNavigationInteractive(14), true);
  assert.equal(codingSessionMinimapNavigationInteractive(13), false);
  assert.equal(codingSessionMinimapInteractiveWidth(28, false), 28);
  assert.equal(codingSessionMinimapInteractiveWidth(28, true), "22rem");
});

test("the current turn is the first in view, else the last above", () => {
  const itemBounds = [
    { top: 0, height: 100 },
    { top: 100, height: 100 },
    { top: null, height: null },
    { top: 400, height: 100 },
  ];
  assert.equal(
    codingSessionMinimapCurrentIndex({
      scrollTop: 150,
      scrollBottom: 450,
      itemBounds,
    }),
    1,
  );
  assert.equal(
    codingSessionMinimapCurrentIndex({
      scrollTop: 250,
      scrollBottom: 350,
      itemBounds,
    }),
    1,
  );
});
