import { expect, type Page } from "@playwright/test";

/**
 * The Mission grid, measured off the live layout.
 *
 * L4.6's acceptance test is arithmetic, not eyesight: the stream must be
 * exactly what the two rails leave, at every width. So this reads the four
 * boxes the browser actually laid out — never a class name, never a constant —
 * and hands the caller numbers it can subtract.
 *
 * `route` is 0 when neither the rail nor the scrubber is mounted (Conversation,
 * or a Mission view before the rail resolves); `inspector` is 0 when the
 * Inspector has folded to a sheet, which is exactly the contribution it makes
 * to the row in that state.
 */
export type CodingSessionMissionGrid = {
  body: number;
  route: number;
  inspector: number;
  stream: number;
  /** The reading box inside the stream. */
  column: number;
  /**
   * The content width its scroller offers it — the stream less the gutter that
   * holds text off the window edge. In Mission the column must be all of this;
   * the 258 px of dead margin at 1920 was a `max-w-3xl` cap *inside* it.
   */
  columnAvailable: number;
};

export async function measureCodingSessionMissionGrid(
  page: Page,
): Promise<CodingSessionMissionGrid> {
  return page.evaluate(() => {
    const width = (selector: string) => {
      const node = document.querySelector(selector);
      return node === null ? 0 : node.getBoundingClientRect().width;
    };
    const workspace = document.querySelector(
      '[data-testid="coding-session-umbrella-workspace"]',
    );
    const section = workspace?.querySelector(
      'section[aria-label="Umbrella session narrative"]',
    );
    const body = section?.parentElement;
    const column = section?.querySelector("[data-coding-session-column]");
    return {
      body:
        body === null || body === undefined
          ? 0
          : body.getBoundingClientRect().width,
      route:
        width('[data-testid="coding-session-route-rail"]') ||
        width('[data-testid="coding-session-route-scrubber"]'),
      inspector: width('[data-testid="coding-session-surface-host"]'),
      stream:
        section === null || section === undefined
          ? 0
          : section.getBoundingClientRect().width,
      column:
        column === null || column === undefined
          ? 0
          : column.getBoundingClientRect().width,
      columnAvailable: (() => {
        const scroller = column?.parentElement;
        if (!scroller) return 0;
        const style = window.getComputedStyle(scroller);
        return (
          scroller.clientWidth -
          Number.parseFloat(style.paddingLeft) -
          Number.parseFloat(style.paddingRight)
        );
      })(),
    };
  });
}

/**
 * The stream is what the rails leave — and nothing else.
 *
 * Two claims, because B2 was two bugs. First, the row adds up: the stream is
 * the body minus both rails, to within a pixel of rounding. Second, the
 * reading box inside it is the *whole* stream: the 258 px of dead margin at
 * 1920 came from a `max-w-3xl` cap that survived every rail the viewer closed.
 */
export async function expectStreamFillsTheRails(
  page: Page,
  label: string,
): Promise<CodingSessionMissionGrid> {
  const grid = await measureCodingSessionMissionGrid(page);
  expect(grid.body, `${label}: the workspace body is laid out`).toBeGreaterThan(
    0,
  );
  expect(
    Math.abs(grid.stream - (grid.body - grid.route - grid.inspector)),
    `${label}: stream ${grid.stream} = body ${grid.body} − route ${grid.route} − inspector ${grid.inspector}`,
  ).toBeLessThanOrEqual(2);
  expect(
    Math.abs(grid.column - grid.columnAvailable),
    `${label}: the reading box takes everything its scroller offers (column ${grid.column} vs available ${grid.columnAvailable}), rather than a centred cap inside it`,
  ).toBeLessThanOrEqual(2);
  return grid;
}

/** The Mission column carries no container cap and no centring. */
export async function expectMissionColumnUncapped(page: Page): Promise<void> {
  const classes = await page
    .locator("[data-coding-session-column-mission]")
    .first()
    .getAttribute("class");
  expect(classes).not.toBeNull();
  expect(classes ?? "").not.toMatch(/\bmx-auto\b/);
  expect(classes ?? "").not.toMatch(/max-w-(3xl|5xl|6xl|7xl)/);
  expect(classes ?? "").toMatch(/max-w-none/);
}

/**
 * Conversation's column is byte-identical to what it always was.
 *
 * I8 freezes that lens, and every item in L4 is Mission-gated. This is the
 * cheap continuous check; the outerHTML diff is the exhaustive one.
 */
export async function expectConversationColumnUnchanged(
  page: Page,
): Promise<void> {
  const classes = await page
    .locator("[data-coding-session-column]")
    .first()
    .getAttribute("class");
  expect(classes ?? "").toMatch(/\bmx-auto\b/);
  expect(classes ?? "").toMatch(/max-w-(3xl|5xl|6xl|7xl)/);
  expect(
    await page.locator("[data-coding-session-column-mission]").count(),
  ).toBe(0);
}
