import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionGoalEvent } from "@/features/coding-sessions/lib/codingSessionGoal";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";

/**
 * Width behaviour of the coding-session workspace.
 *
 * The reading measure is deliberately fixed (48rem) — a wide display gets
 * gutters, exactly as the reference implementation does. What must never
 * happen is content that does not fit the measure being *cut off*: a wide
 * code block, table, or tool payload has to scroll inside its own block, with
 * a visible affordance, and nothing may escape the column and get clipped by
 * the workspace's `overflow-hidden` or by the paint containment that
 * `content-visibility: auto` puts on every turn.
 */

const SHOTS = "../test-results/session-width";
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);
const BASE_CREATED_AT = 1_800_000_000;
const BASE_TIMESTAMP_MS = 1_800_000_000_000;
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-founded-session";

/** 214 characters on one line — far past 48rem at any zoom. */
const WIDE_CODE_LINE =
  'const summary = await collectSessionDiagnostics({ relayUrl, channelId, sessionRef, includeTranscript: true, includeToolCalls: true, redactSecrets: false, label: "reconnect-recovery-verification-run" });';
/** A single unbroken token; `whitespace-pre-wrap` alone will not break it. */
const UNBROKEN_TOKEN = `~/Projects/buzz/desktop/src/features/coding-sessions/ui/${"CodingSessionTranscriptPartsAndFoldsAndCompletionFooters".repeat(3)}.tsx`;
const WIDE_TOOL_OUTPUT = [
  "Use it to find files. Use it to find files. Use it to find files. Use it to find files. Use it to find files. Use it to find files. Use it to find files.",
  UNBROKEN_TOKEN,
].join("\n");

function signed(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

function genesisEvent(): RelayEvent {
  const built = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  return signed(
    built.kind,
    BASE_CREATED_AT - 2,
    built.tags,
    built.content,
    FOUNDER_SECRET,
  );
}

function createAndReceiptEvents(genesisRef: string): RelayEvent[] {
  const built = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: "Fix the reconnect bug",
    initialTurn: null,
  });
  return [
    signed(
      built.kind,
      BASE_CREATED_AT - 1,
      built.tags,
      built.content,
      FOUNDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
      PROVIDER_SECRET,
    ),
  ];
}

function goalEvent(): RelayEvent {
  const built = buildCodingSessionGoalEvent({
    channelId: CHANNEL_ID,
    content: "Make authority visible at every decision point",
    sessionRef: SESSION_REF,
  });
  return signed(
    built.kind,
    BASE_CREATED_AT + 10,
    built.tags,
    built.content,
    FOUNDER_SECRET,
  );
}

function metadataEvent(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    BASE_CREATED_AT,
    [
      ["h", CHANNEL_ID],
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", TARGET_KEY],
      ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
    ],
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session: TARGET,
      projectRef: null,
      repoRef: null,
      title: "Fix the reconnect bug",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "running",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: true,
      },
      sessionRef: SESSION_REF,
    }),
    PROVIDER_SECRET,
  );
}

function transcriptEvent(eventSeq: number, item: unknown): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    BASE_CREATED_AT + eventSeq,
    [
      ["h", CHANNEL_ID],
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", TARGET_KEY],
      ["cst-seq", String(eventSeq)],
      ["cst-key", codingSessionTranscriptSemanticKey(TARGET, eventSeq)],
    ],
    JSON.stringify({
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session: TARGET,
      eventSeq,
      timestamp: BASE_TIMESTAMP_MS + eventSeq * 1_000,
      turnId: "turn-1",
      item,
    }),
    PROVIDER_SECRET,
  );
}

/** Prose, a too-wide code fence, a too-wide table, and too-wide tool output. */
function seededEvents(): RelayEvent[] {
  const genesis = genesisEvent();
  return [
    genesis,
    ...createAndReceiptEvents(genesis.id),
    goalEvent(),
    metadataEvent(),
    transcriptEvent(1, {
      kind: "user_prompt",
      content: [
        "Collect the reconnect diagnostics, summarise what the harness reports, and post them to GitHub when the Claude Code status check finishes. Keep the transcript readable while you do it — the last run produced tool output that ran off the side of the screen and I could not tell whether it had been cut off or was simply long.",
        "",
        `The failing path was ${UNBROKEN_TOKEN}`,
      ].join("\n"),
    }),
    transcriptEvent(2, {
      kind: "assistant_text",
      text: [
        "Here is the diagnostic collector as it stands. The first statement is deliberately long, so it is the honest test of what a wide line does inside the reading column: it should scroll inside this block, and the block should say so.",
        "",
        "```ts",
        WIDE_CODE_LINE,
        "if (!summary.ok) throw new Error(summary.reason);",
        "```",
        "",
        "Prose after the fence must still sit on the same measure as the prose before it, and must never be cut mid-word.",
      ].join("\n"),
    }),
    transcriptEvent(3, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "tool-1",
        input: {
          command: `rg --files-with-matches reconnect ${UNBROKEN_TOKEN}`,
        },
      },
    }),
    transcriptEvent(4, {
      kind: "tool_result",
      toolId: "tool-1",
      toolName: "Bash",
      content: WIDE_TOOL_OUTPUT,
      isError: false,
    }),
    transcriptEvent(5, {
      kind: "assistant_text",
      text: [
        "The harness reports the following. This table is wider than the column on purpose:",
        "",
        "| Check | Command | Duration | Exit | Notes |",
        "| --- | --- | --- | --- | --- |",
        "| Claude Code status | `pnpm exec claude-code status --verbose --json` | 12.4s | 0 | Use it to find files, then post them to GitHub |",
        "| Reconnect unit | `cargo test -p buzz-relay reconnect -- --nocapture` | 41.9s | 0 | Bounded retry state verified end to end |",
        "| Typecheck | `pnpm exec tsc --noEmit --pretty false` | 28.1s | 0 | No new diagnostics in the coding-session feature |",
        "",
        "That is everything the run produced.",
      ].join("\n"),
    }),
    // Left unresolved on purpose: the executing branch renders its own `pre`
    // for args and result, a different code path from the settled tool panel.
    transcriptEvent(6, {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "tool-2",
        input: { command: `gh pr comment --body-file ${UNBROKEN_TOKEN}` },
      },
    }),
  ];
}

type WidthAudit = {
  affordance: {
    codeBlocks: number;
    codeBlocksOverflowing: number;
    tablesOverflowing: number;
  };
  edges: Record<string, { left: number; right: number } | null>;
  escapees: string[];
  measure: { column: number; rootFontSize: number };
  pageScrollsSideways: boolean;
  scrollersWithHiddenContent: string[];
  turnPaintEscapees: string[];
};

/**
 * Everything the eye is about to be asked about, measured.
 *
 * `escapees` is the core defect: any box inside the transcript whose border
 * box lies outside the reading column is, given the workspace's
 * `overflow-hidden`, painted only up to the edge and then cut.
 * `turnPaintEscapees` is the same question against the tighter boundary that
 * `content-visibility: auto` imposes on each turn (it implies `contain:
 * paint`, which clips with no scrollbar and no ellipsis).
 */
async function audit(page: Page): Promise<WidthAudit> {
  return await page.evaluate(() => {
    const describe = (el: Element) => {
      const testid = el.getAttribute("data-testid");
      const cls = (el.getAttribute("class") ?? "").slice(0, 70);
      return `${el.tagName.toLowerCase()}${testid ? `[${testid}]` : ""} .${cls}`;
    };
    const transcript = document.querySelector(
      '[data-testid="coding-session-transcript"]',
    ) as HTMLElement | null;
    if (!transcript) throw new Error("transcript missing");
    const column = transcript.closest(
      "[data-coding-session-column]",
    ) as HTMLElement | null;
    if (!column) throw new Error("reading column missing");
    const columnRect = column.getBoundingClientRect();

    /**
     * The nearest ancestor at or above `el` (stopping at `boundary`) that owns
     * horizontal overflow. If one exists, a child painting past the boundary is
     * *scrollable*, not cut off — that is the fixed behaviour, not the bug.
     */
    const overflowOwner = (el: Element, boundary: Element) => {
      let node: Element | null = el.parentElement;
      while (node && node !== boundary) {
        const overflowX = getComputedStyle(node).overflowX;
        if (overflowX !== "visible") return { node, overflowX };
        node = node.parentElement;
      }
      return null;
    };
    // Visually-hidden text and deliberate single-line ellipsis are not clips.
    const excused = (el: Element) =>
      el.classList.contains("sr-only") ||
      getComputedStyle(el).textOverflow === "ellipsis";

    const escapees: string[] = [];
    const scrollersWithHiddenContent: string[] = [];
    for (const el of Array.from(transcript.querySelectorAll("*"))) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) continue;
      if (excused(el)) continue;
      const escapes =
        rect.right > columnRect.right + 2 || rect.left < columnRect.left - 2;
      const owner = overflowOwner(el, column);
      if (escapes && (!owner || !/auto|scroll/.test(owner.overflowX))) {
        escapees.push(
          `${describe(el)} → left ${Math.round(rect.left - columnRect.left)} right +${Math.round(rect.right - columnRect.right)}` +
            (owner
              ? ` [clipped by ${describe(owner.node)} overflow-x:${owner.overflowX}]`
              : " [no clipping ancestor]"),
        );
      }
      const style = getComputedStyle(el);
      const scrollable = /auto|scroll/.test(style.overflowX);
      const ownsHidden = el.scrollWidth - el.clientWidth > 1;
      if (ownsHidden && !scrollable && !owner) {
        scrollersWithHiddenContent.push(
          `${describe(el)} (overflow-x:${style.overflowX}, hidden ${el.scrollWidth - el.clientWidth}px)`,
        );
      }
    }

    // Paint containment boundary: each turn clips its own overflow silently.
    const turnPaintEscapees: string[] = [];
    for (const turn of Array.from(
      document.querySelectorAll(
        '[data-testid="coding-session-turn"],[data-testid="coding-session-standalone"]',
      ),
    )) {
      const turnRect = turn.getBoundingClientRect();
      if (turnRect.width === 0) continue;
      for (const el of Array.from(turn.querySelectorAll("*"))) {
        const rect = el.getBoundingClientRect();
        if (rect.width === 0 && rect.height === 0) continue;
        if (excused(el)) continue;
        const owner = overflowOwner(el, turn);
        if (owner && /auto|scroll/.test(owner.overflowX)) continue;
        if (rect.right > turnRect.right + 2 || rect.left < turnRect.left - 2) {
          turnPaintEscapees.push(
            `${describe(el)} → +${Math.round(rect.right - turnRect.right)}px past its turn`,
          );
        }
      }
    }

    const rectOf = (selector: string) => {
      const el = document.querySelector(selector);
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return {
        left: Math.round(r.left * 100) / 100,
        right: Math.round(r.right * 100) / 100,
      };
    };

    const scroller = transcript.closest(
      ".overflow-y-auto",
    ) as HTMLElement | null;
    const codeBlocks = Array.from(
      document.querySelectorAll("[data-code-block]"),
    );
    const tables = Array.from(document.querySelectorAll("[data-table-block]"));

    return {
      affordance: {
        codeBlocks: codeBlocks.length,
        codeBlocksOverflowing: codeBlocks.filter(
          (b) => b.getAttribute("data-overflow") === "true",
        ).length,
        tablesOverflowing: tables.filter(
          (b) => b.getAttribute("data-overflow") === "true",
        ).length,
      },
      edges: {
        column: {
          left: Math.round(columnRect.left * 100) / 100,
          right: Math.round(columnRect.right * 100) / 100,
        },
        composer: rectOf('[data-testid="coding-session-composer"]'),
        goalPill: rectOf('[data-testid="coding-session-goal-workspace"]'),
        header: rectOf('[data-testid="coding-session-header"]'),
      },
      escapees,
      measure: {
        column: Math.round(columnRect.width),
        rootFontSize: Number.parseFloat(
          getComputedStyle(document.documentElement).fontSize,
        ),
      },
      pageScrollsSideways: scroller
        ? scroller.scrollWidth - scroller.clientWidth > 1
        : document.body.scrollWidth - document.body.clientWidth > 1,
      scrollersWithHiddenContent,
      turnPaintEscapees,
    };
  });
}

async function openSession(page: Page) {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [{ pubkey: FOUNDER_PUBKEY, displayName: "Alice Rivera" }],
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: seededEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("coding-session-transcript")).toBeVisible();
  await expect(page.getByTestId("coding-session-goal-workspace")).toBeVisible();
  await expect(page.getByTestId("coding-session-composer")).toBeVisible();
  return workspace;
}

/**
 * 1100 is the width where the alignment question has teeth. Above ~1130 the
 * expanded measure starts binding. It catches both edge alignment and the
 * responsive rule: 72rem while no side surface is open, 48rem while one is.
 */
const WIDTHS = [1100, 1280, 1920, 2560, 3440] as const;

for (const width of WIDTHS) {
  test(`coding session transcript contains its wide content at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    const workspace = await openSession(page);

    // Bring the wide markdown into view; the fences and table sit above the
    // fold at 900px and `content-visibility` will not have painted them.
    await page
      .getByTestId("coding-session-transcript")
      .scrollIntoViewIfNeeded();
    await expect(page.locator("[data-code-block]").first()).toBeVisible();
    await expect(page.locator("[data-table-block]").first()).toBeVisible();
    await waitForAnimations(page);

    const result = await audit(page);
    await page.evaluate((payload) => {
      (window as unknown as { __WIDTH_AUDIT__?: unknown }).__WIDTH_AUDIT__ =
        payload;
    }, result);

    await workspace.screenshot({ path: `${SHOTS}/${width}-workspace.png` });

    const columnClip = await page.evaluate(() => {
      const transcript = document.querySelector(
        '[data-testid="coding-session-transcript"]',
      ) as HTMLElement;
      const column = transcript.closest(
        "[data-coding-session-column]",
      ) as HTMLElement;
      const r = column.getBoundingClientRect();
      return {
        x: Math.max(0, Math.floor(r.left - 48)),
        y: 0,
        width: Math.min(
          window.innerWidth - Math.max(0, Math.floor(r.left - 48)),
          Math.ceil(r.width + 96),
        ),
        height: Math.min(900, window.innerHeight),
      };
    });
    await page.screenshot({
      clip: columnClip,
      path: `${SHOTS}/${width}-column.png`,
    });

    const codeBlock = page.locator("[data-code-block]").first();
    await codeBlock.screenshot({ path: `${SHOTS}/${width}-codeblock.png` });
    const table = page.locator("[data-table-block]").first();
    await table.screenshot({ path: `${SHOTS}/${width}-table.png` });

    // eslint-disable-next-line no-console
    console.log(`AUDIT ${width} ${JSON.stringify(result)}`);

    expect(result.escapees, "no box may escape the reading column").toEqual([]);
    expect(
      result.turnPaintEscapees,
      "no box may escape its turn's paint-containment box",
    ).toEqual([]);
    expect(
      result.scrollersWithHiddenContent,
      "content wider than its box must live in a scrollable box",
    ).toEqual([]);
    expect(result.pageScrollsSideways).toBe(false);
    // With no side surface open the transcript earns the wider 72rem reading
    // measure; the viewport remains the binding constraint at smaller widths.
    expect(result.measure.column).toBeLessThanOrEqual(1152);
    if (width >= 1920) expect(result.measure.column).toBe(1152);
    // The goal pill, the transcript and the composer are one column. Whichever
    // constraint binds, it must bind identically for all three.
    expect(result.edges.composer).toEqual(result.edges.column);
    expect(result.edges.goalPill).toEqual(result.edges.column);
    expect(result.affordance.codeBlocksOverflowing).toBeGreaterThan(0);
    // Tables may fit naturally once the clear workspace expands to 72rem.
  });
}

/**
 * Session padding, as the settings panel offers it.
 *
 * The measure is typographic and stays where it is; what this setting spends
 * is the margin between the measure and the window edge. The failure worth
 * guarding is not the number — it is one surface taking the new gutter while
 * another keeps the old one, which is how the transcript and the composer
 * drifted apart the first time. So each choice is checked on every surface at
 * once, at a viewport past the `sm` breakpoint where the wider step applies.
 */
const GUTTERS = [
  { choice: "full", px: 32 },
  { choice: "light", px: 16 },
  { choice: "none", px: 10 },
] as const;

for (const { choice, px } of GUTTERS) {
  test(`the ${choice} gutter is ${px}px on every session surface at once`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    // Before installMockBridge: the preference reads localStorage when its
    // module loads, and the bridge is what triggers the React mount.
    await page.addInitScript((stored) => {
      window.localStorage.setItem("buzz.codingSessions.gutter", stored);
    }, choice);
    const workspace = await openSession(page);
    await waitForAnimations(page);

    // The narrative measure is 48rem or, with no side surface open, 72rem —
    // and the goal pill wears the measure classes itself rather than wrapping
    // in a CodingSessionColumn. Find the measure box by either signature so
    // this reads the gutter, not one particular layout.
    const MEASURE = "[data-coding-session-column],.max-w-3xl,.max-w-6xl";
    const gutters = await page.evaluate((measure) => {
      const pad = (el: Element | null | undefined) => {
        if (!el) return null;
        const style = getComputedStyle(el);
        return {
          left: Number.parseFloat(style.paddingLeft),
          right: Number.parseFloat(style.paddingRight),
        };
      };
      const box = (selector: string) =>
        document.querySelector(selector)?.closest(measure) ?? null;
      // The gutter hangs on the ancestor of the measure box, never on it.
      const outer = (selector: string) => pad(box(selector)?.parentElement);
      const edges = (selector: string) => {
        const el = box(selector);
        if (!el) return null;
        const rect = el.getBoundingClientRect();
        return {
          left: Math.round(rect.left * 100) / 100,
          right: Math.round(rect.right * 100) / 100,
        };
      };
      return {
        composer: outer('[data-testid="coding-session-composer"]'),
        goalPill: outer('[data-testid="coding-session-goal-workspace"]'),
        transcript: outer('[data-testid="coding-session-transcript"]'),
        founderLine: pad(
          document.querySelector('[data-testid="coding-session-founded-by"]'),
        ),
        edges: {
          composer: edges('[data-testid="coding-session-composer"]'),
          goalPill: edges('[data-testid="coding-session-goal-workspace"]'),
          transcript: edges('[data-testid="coding-session-transcript"]'),
        },
      };
    }, MEASURE);

    const expected = { left: px, right: px };
    expect(gutters.transcript).toEqual(expected);
    expect(gutters.composer).toEqual(expected);
    expect(gutters.goalPill).toEqual(expected);
    // Null when this fixture's genesis did not resolve; a present line must
    // still share the gutter rather than keeping a hardcoded copy of Full.
    if (gutters.founderLine) expect(gutters.founderLine).toEqual(expected);

    // Whatever the measure resolves to, all three surfaces land on it.
    expect(gutters.edges.composer).toEqual(gutters.edges.transcript);
    expect(gutters.edges.goalPill).toEqual(gutters.edges.transcript);
    const result = await audit(page);
    expect(result.escapees).toEqual([]);
    expect(result.pageScrollsSideways).toBe(false);

    // Where the choice is actually visible. While the session is wider than
    // its measure the gutter is slack — the column is centred either way.
    // Narrow the window and the margin is what sets the edges, so the
    // transcript widens as the gutter shrinks.
    await page.setViewportSize({ width: 820, height: 900 });
    await waitForAnimations(page);
    const narrow = await page.evaluate((measure) => {
      const transcript = document.querySelector(
        '[data-testid="coding-session-transcript"]',
      ) as HTMLElement;
      const column = transcript.closest(measure) as HTMLElement;
      const scroller = column.parentElement as HTMLElement;
      return {
        column: Math.round(column.getBoundingClientRect().width),
        available: Math.round(scroller.getBoundingClientRect().width),
      };
    }, MEASURE);
    expect(narrow.column).toBe(narrow.available - 2 * px);
    // eslint-disable-next-line no-console
    console.log(`GUTTER ${choice} ${JSON.stringify({ ...gutters, narrow })}`);
    await workspace.screenshot({ path: `${SHOTS}/gutter-${choice}.png` });
  });
}

test("the wide code line scrolls inside its own block", async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await openSession(page);
  const pre = page.locator("[data-code-block] > pre").first();
  await expect(pre).toBeVisible();

  const before = await pre.evaluate((el) => ({
    clientWidth: el.clientWidth,
    scrollLeft: el.scrollLeft,
    scrollWidth: el.scrollWidth,
  }));
  expect(before.scrollWidth).toBeGreaterThan(before.clientWidth + 1);

  await pre.evaluate((el) => {
    el.scrollLeft = el.scrollWidth;
  });
  await waitForAnimations(page);
  const after = await pre.evaluate((el) => ({
    overflowFlag: el.parentElement?.getAttribute("data-overflow"),
    scrollLeft: el.scrollLeft,
  }));
  expect(after.scrollLeft).toBeGreaterThan(0);
  // The edge fade must retract once the last column is genuinely on screen,
  // or it is lying about there being more.
  expect(after.overflowFlag).toBe("false");
  await page
    .locator("[data-code-block]")
    .first()
    .screenshot({ path: `${SHOTS}/1920-codeblock-scrolled-right.png` });

  // And the per-block wrap toggle is the escape hatch for anyone who would
  // rather read it all at once.
  await page.getByTestId("code-block-wrap-toggle").first().click();
  await waitForAnimations(page);
  const wrapped = await pre.evaluate((el) => ({
    scrollWidth: el.scrollWidth,
    clientWidth: el.clientWidth,
  }));
  expect(wrapped.scrollWidth).toBeLessThanOrEqual(wrapped.clientWidth + 1);
  await page
    .locator("[data-code-block]")
    .first()
    .screenshot({ path: `${SHOTS}/1920-codeblock-wrapped.png` });
});

test("opening a side surface returns the transcript to its 48rem measure", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await openSession(page);
  await page.getByTestId("coding-session-surface-toggle-changes").click();
  await waitForAnimations(page);

  const withSurface = await audit(page);
  expect(withSurface.measure.column).toBe(768);
  expect(withSurface.edges.composer).toEqual(withSurface.edges.column);
  expect(withSurface.edges.goalPill).toEqual(withSurface.edges.column);
});

test("the measure scales with root font size, so Cmd+ widens the column", async ({
  page,
}) => {
  await page.setViewportSize({ width: 3440, height: 900 });
  const workspace = await openSession(page);
  const base = await audit(page);
  expect(base.measure.rootFontSize).toBe(16);
  expect(base.measure.column).toBe(1152);

  await page.evaluate(() => {
    document.documentElement.style.fontSize = "24px";
  });
  await waitForAnimations(page);
  const zoomed = await audit(page);
  expect(zoomed.measure.rootFontSize).toBe(24);
  expect(zoomed.measure.column).toBe(1728); // 72rem × 24px
  expect(zoomed.escapees).toEqual([]);
  await workspace.screenshot({ path: `${SHOTS}/3440-zoomed-24px.png` });
  // eslint-disable-next-line no-console
  console.log(`AUDIT zoom ${JSON.stringify(zoomed)}`);
});

test("expanded tool output wraps inside the column instead of being cut", async ({
  page,
}) => {
  await page.setViewportSize({ width: 2560, height: 1200 });
  const workspace = await openSession(page);

  // Collapsed, a tool row is a single truncated line — honest, but it hides
  // the payload that used to run off the side. Open every one of them.
  const opened = await page.evaluate(() => {
    const transcript = document.querySelector(
      '[data-testid="coding-session-transcript"]',
    ) as HTMLElement;
    const details = Array.from(transcript.querySelectorAll("details"));
    for (const el of details) el.open = true;
    return details.length;
  });
  expect(opened).toBeGreaterThan(0);
  await expect(
    page.getByText("Use it to find files", { exact: false }).first(),
  ).toBeVisible();
  await waitForAnimations(page);

  const result = await audit(page);
  // eslint-disable-next-line no-console
  console.log(`AUDIT tools ${JSON.stringify(result)}`);
  await workspace.screenshot({ path: `${SHOTS}/2560-tools-expanded.png` });

  expect(result.escapees).toEqual([]);
  expect(result.turnPaintEscapees).toEqual([]);
  expect(result.scrollersWithHiddenContent).toEqual([]);
  expect(result.pageScrollsSideways).toBe(false);

  // A 300-character unbroken path must break, not overflow: `whitespace-pre-wrap`
  // alone would leave it one unbreakable line.
  const unbroken = await page
    .getByText("CodingSessionTranscriptPartsAndFolds", { exact: false })
    .first()
    .evaluate((el) => {
      const box = el.getBoundingClientRect();
      const column = el.closest("[data-coding-session-column]") as HTMLElement;
      return {
        overspill: Math.round(box.right - column.getBoundingClientRect().right),
        lines: Math.round(
          box.height / Number.parseFloat(getComputedStyle(el).lineHeight),
        ),
      };
    });
  expect(unbroken.overspill).toBeLessThanOrEqual(0);
  expect(unbroken.lines).toBeGreaterThan(1);
});
