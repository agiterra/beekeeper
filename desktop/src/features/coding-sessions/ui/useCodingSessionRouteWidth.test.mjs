import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_ROUTE_COLLAPSED_WIDTH_PX,
  CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX,
  CODING_SESSION_ROUTE_MAX_WIDTH_PX,
  CODING_SESSION_ROUTE_MIN_WIDTH_PX,
  clampCodingSessionRouteWidth,
  parsePersistedCodingSessionRouteCollapsed,
  parsePersistedCodingSessionRouteWidth,
} from "./useCodingSessionRouteWidth.ts";
import { codingSessionRouteFits } from "../lib/codingSessionRouteTypes.ts";

/** Only the pure exports are proved here; the hook is an e2e concern. */

test("the rail's bounds are the ones §L4.6 states", () => {
  assert.equal(CODING_SESSION_ROUTE_MIN_WIDTH_PX, 176);
  assert.equal(CODING_SESSION_ROUTE_MAX_WIDTH_PX, 480);
  assert.equal(CODING_SESSION_ROUTE_DEFAULT_WIDTH_PX, 224);
  assert.equal(CODING_SESSION_ROUTE_COLLAPSED_WIDTH_PX, 40);
});

test("the clamp holds the rail inside its bounds and leaves the stream its floor", () => {
  // A container with room to spare: only the rail's own bounds bite.
  assert.equal(clampCodingSessionRouteWidth(300, 2_000), 300);
  assert.equal(clampCodingSessionRouteWidth(10, 2_000), 176);
  assert.equal(clampCodingSessionRouteWidth(9_000, 2_000), 480);
  // A container that cannot pay for the drag: the 420 px stream floor wins,
  // but never below the rail's own minimum.
  assert.equal(clampCodingSessionRouteWidth(480, 700), 280);
  assert.equal(clampCodingSessionRouteWidth(480, 500), 176);
});

test("a persisted width is parsed strictly or rejected as corrupt", () => {
  assert.equal(parsePersistedCodingSessionRouteWidth("224"), 224);
  assert.equal(parsePersistedCodingSessionRouteWidth("176"), 176);
  assert.equal(parsePersistedCodingSessionRouteWidth("480"), 480);
  for (const raw of [
    null,
    "",
    "175",
    "481",
    "0",
    "-224",
    "224px",
    "2.24e2",
    "224.0",
    " 224",
    "12345",
    "NaN",
  ]) {
    assert.equal(
      parsePersistedCodingSessionRouteWidth(raw),
      null,
      `expected ${JSON.stringify(raw)} to be rejected`,
    );
  }
});

test("an absent or unrecognised collapsed flag means not collapsed", () => {
  assert.equal(parsePersistedCodingSessionRouteCollapsed("1"), true);
  assert.equal(parsePersistedCodingSessionRouteCollapsed("0"), false);
  for (const raw of [null, "", "true", "yes", "01", " 1", "2"]) {
    assert.equal(
      parsePersistedCodingSessionRouteCollapsed(raw),
      false,
      `expected ${JSON.stringify(raw)} to read as not collapsed`,
    );
  }
});

/**
 * L4.6's four widths, against **the gate the app actually runs**.
 *
 * REVIEW-L4 F10: the first version of this proved a declarative
 * `codingSessionMissionFoldOrder` with no runtime consumer — "the four-width
 * table is proved as arithmetic" was true of a function that laid nothing out.
 * That function is deleted. What is asserted here is `codingSessionRouteFits`,
 * the gate `useCodingSessionRoute` calls, over the section widths the app
 * really hands it.
 *
 * Two things the spec's table did not say, and the implementation does:
 *
 * - The Inspector becomes a **sheet** below a 960 px body (`isNarrow`,
 *   `CodingSessionUmbrellaWorkspace`), not on the 420 px floor. So at a 1100
 *   window the Inspector contributes 0 px and the rail correctly **stays** at
 *   224 — the spec's row 1 (`route 40 / stream 750`) assumed an inline
 *   Inspector at a width where there is never one.
 * - The section width the gate measures already has both panels subtracted,
 *   which is why `railShown` is fed back in.
 */
const WIDTH_TABLE = [
  // window, body = window − ~310 of app chrome, whether the Inspector is
  // inline at that body, and whether the expanded rail survives.
  { window: 1_100, body: 790, inspectorInline: false, railFits: true },
  { window: 1_280, body: 970, inspectorInline: true, railFits: false },
  { window: 1_600, body: 1_290, inspectorInline: true, railFits: true },
  { window: 1_920, body: 1_610, inspectorInline: true, railFits: true },
];

for (const row of WIDTH_TABLE) {
  test(`at a ${row.window} px window the real gate folds as the DOM measures`, () => {
    assert.equal(row.body, row.window - 310);
    // `isNarrow` is the Inspector's own gate and it is a body-width gate.
    assert.equal(row.inspectorInline, row.body >= 960);
    const inspector = row.inspectorInline ? 360 : 0;
    const sectionWithRail = row.body - 224 - inspector;
    assert.equal(
      codingSessionRouteFits({
        railShown: true,
        railWidthPx: 224,
        sectionWidthPx: sectionWithRail,
      }),
      row.railFits,
      `window ${row.window}: stream ${sectionWithRail} against the 420 floor`,
    );
    // And the flip is stable: asked again about the layout it produces, the
    // gate gives the same answer rather than oscillating on one pixel.
    const sectionAfter = row.railFits
      ? sectionWithRail
      : row.body - CODING_SESSION_ROUTE_COLLAPSED_WIDTH_PX - inspector;
    assert.equal(
      codingSessionRouteFits({
        railShown: row.railFits,
        railWidthPx: 224,
        sectionWidthPx: sectionAfter,
      }),
      row.railFits,
      `window ${row.window}: the gate is stable on its own outcome`,
    );
  });
}

test("a stored expanded choice survives a fold and a restore", () => {
  // The fold is a layout answer, never a write: `codingSessionRouteFits` takes
  // no stored value and returns none, so the viewer's width and collapse flag
  // are the same bytes before and after a narrow window.
  const stored = { width: 224, collapsed: false };
  assert.equal(
    codingSessionRouteFits({
      railShown: true,
      railWidthPx: stored.width,
      sectionWidthPx: 970 - 224 - 360,
    }),
    false,
  );
  assert.deepEqual(stored, { width: 224, collapsed: false });
  assert.equal(
    codingSessionRouteFits({
      railShown: false,
      railWidthPx: stored.width,
      sectionWidthPx: 1_610 - CODING_SESSION_ROUTE_COLLAPSED_WIDTH_PX - 360,
    }),
    true,
  );
});

test("the viewer's collapse is a second term the width never overwrites", () => {
  // `useCodingSessionRoute` computes `fits = roomForRail && !collapsed`. There
  // is room at 1920 and the viewer said no, so the rail is folded and the
  // stored flag is untouched by the width.
  const roomForRail = codingSessionRouteFits({
    railShown: true,
    railWidthPx: 224,
    sectionWidthPx: 1_610 - 224 - 360,
  });
  assert.equal(roomForRail, true);
  assert.equal(roomForRail && !true, false);
});

test("a closed Inspector gives the stream its space instead of a sheet", () => {
  // Body 790 with the Inspector closed: 790 − 224 = 566, above the floor, so
  // the rail stays. This is the row the spec's table got wrong (F10).
  assert.equal(
    codingSessionRouteFits({
      railShown: true,
      railWidthPx: 224,
      sectionWidthPx: 790 - 224,
    }),
    true,
  );
});
