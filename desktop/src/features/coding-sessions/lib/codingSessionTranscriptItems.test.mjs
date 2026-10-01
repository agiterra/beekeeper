import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";

import { CodingSessionTranscript } from "../ui/CodingSessionTranscript.tsx";

import {
  buildBaseTranscriptItem,
  buildPairedToolResultItem,
  CODING_SESSION_CONTINUITY_REASONS,
  CODING_SESSION_CONTINUITY_STATUSES,
} from "./codingSessionTranscriptItems.ts";
import {
  CODING_SESSION_BOUNDARY_REASONS,
  CODING_SESSION_BOUNDARY_STATUSES,
  CODING_SESSION_BOUNDARY_TITLE,
} from "./codingSessionBoundaryStatus.ts";

const IDENTITY = {
  id: "item-1",
  sessionId: "session-1",
  targetKey: "target-1",
  channelId: "channel-1",
  timestamp: "2026-08-19T00:00:00.000Z",
};

function statusItem(status, reason) {
  const item = { kind: "status", status };
  if (reason !== undefined) {
    item.reason = reason;
  }
  return item;
}

function render(status, reason) {
  return buildBaseTranscriptItem(statusItem(status, reason), IDENTITY);
}

test("a fresh status with a known reason renders the reason clause", () => {
  const item = render("session_fresh", "no_prior_execution");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("no_prior_execution");
  assert.equal(item.title, "Session continuity");
  assert.equal(item.text, `${base} — ${clause}`);
});

test("a restarted-without-context status with a known reason renders the reason clause", () => {
  const item = render("session_restarted_without_context", "relay_unavailable");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get(
    "session_restarted_without_context",
  );
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("relay_unavailable");
  assert.equal(item.title, "Session continuity");
  assert.equal(item.text, `${base} — ${clause}`);
});

test("the resume-path umbrella slug never claims a first execution", () => {
  const item = render(
    "session_restarted_without_context",
    "no_umbrella_context",
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get(
    "session_restarted_without_context",
  );
  const clause = CODING_SESSION_CONTINUITY_REASONS.get("no_umbrella_context");
  assert.equal(item.text, `${base} — ${clause}`);
  assert.ok(
    !item.text.includes("first execution"),
    "a resumed execution has prior work; the row must not deny it",
  );
});

test("a fresh status with an unknown reason renders the raw slug", () => {
  const item = render(
    "session_fresh",
    "some_future_slug_this_build_has_never_seen",
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(
    item.text,
    `${base} (some_future_slug_this_build_has_never_seen)`,
  );
});

test("a status item with no reason renders exactly as before", () => {
  const withoutReasonField = render("session_fresh", undefined);
  const withExplicitUndefined = buildBaseTranscriptItem(
    { kind: "status", status: "session_fresh" },
    IDENTITY,
  );
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(withoutReasonField.text, base);
  assert.equal(withExplicitUndefined.text, base);
});

test("every reason clause in the map is exercised by at least one known slug", () => {
  for (const [slug, clause] of CODING_SESSION_CONTINUITY_REASONS) {
    const item = render("session_fresh", slug);
    const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
    assert.equal(item.text, `${base} — ${clause}`);
  }
});

test("a non-reason-carrying continuity status ignores a reason field", () => {
  const item = render("session_rehydrated", "no_prior_execution");
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_rehydrated");
  assert.equal(item.text, base);
});

test("a non-string reason is treated as absent", () => {
  const item = render("session_fresh", 42);
  const base = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
  assert.equal(item.text, base);
});

test("an unrecognized status is unaffected by CODING_SESSION_CONTINUITY_REASONS", () => {
  const item = render("some_unrecognized_status", "no_prior_execution");
  assert.equal(item.title, "Status");
  assert.equal(item.text, "some_unrecognized_status");
});

// ── Project execution boundary disclosure ────────────────────────────────────
//
// The host publishes one status item per generation saying what it runs inside
// (`execution_scope::boundary_status_item`). The generic renderer used to show
// only the slug and drop `reason`, so an unenforced session read the same as an
// enforced one to anyone who did not know the slugs.

const ENFORCED = "execution_boundary_enforced";
const NOT_ENFORCED = "execution_boundary_not_enforced";

test("an enforced boundary names what it covers and the backend that enforced it", () => {
  const item = render(ENFORCED, "macos-seatbelt");
  assert.equal(item.type, "lifecycle");
  assert.equal(item.renderClass, "status");
  assert.equal(item.title, CODING_SESSION_BOUNDARY_TITLE);
  assert.equal(
    item.text,
    `${CODING_SESSION_BOUNDARY_STATUSES.get(ENFORCED)} (macOS Seatbelt)`,
  );
  assert.ok(item.text.startsWith("Enforced — "));
  assert.ok(item.text.includes("other projects' files are outside it"));
});

test("an unenforced boundary says the session is not isolated, and why", () => {
  const item = render(NOT_ENFORCED, "no-backend-for-platform");
  assert.equal(item.title, CODING_SESSION_BOUNDARY_TITLE);
  assert.equal(
    item.text,
    "Not enforced — this session is not isolated from other projects' files (this platform has no boundary backend)",
  );
  assert.ok(!item.text.includes("inside this project's boundary"));
});

test("an unknown backend or reason slug renders as the bare slug, never as prose", () => {
  assert.ok(
    render(ENFORCED, "linux-bubblewrap").text.endsWith("(linux-bubblewrap)"),
  );
  const unknown = render(NOT_ENFORCED, "some-future-reason");
  assert.ok(unknown.text.startsWith("Not enforced — "));
  assert.ok(unknown.text.endsWith("(some-future-reason)"));
});

test("a missing or non-string reason still discloses the state", () => {
  for (const reason of [
    undefined,
    "",
    42,
    { backend: "macos-seatbelt" },
    null,
  ]) {
    const item = render(NOT_ENFORCED, reason);
    assert.ok(item.text.startsWith("Not enforced — "), String(reason));
    assert.ok(item.text.endsWith("(no reason given)"), String(reason));
  }
  assert.ok(render(ENFORCED, undefined).text.endsWith("(no reason given)"));
});

test("a reason that is not a slug is never echoed: no path, value or model text", () => {
  const leaks = [
    "/Users/someone/Projects/other-project/plans/plan.md",
    "SYNTHETIC-SENTINEL-NOT-A-SECRET=1",
    "the model said: I read the sibling plan",
    "x".repeat(65),
  ];
  for (const reason of leaks) {
    const item = render(NOT_ENFORCED, reason);
    assert.ok(item.text.endsWith("(unrecognized reason)"), reason);
    assert.ok(!item.text.includes(reason), reason);
  }
  // Fields beside `reason` are never rendered at all.
  const item = buildBaseTranscriptItem(
    {
      kind: "status",
      status: ENFORCED,
      reason: "macos-seatbelt",
      policyDigest: "SYNTHETIC-DIGEST",
      path: "/Users/someone/secret",
      model: "SYNTHETIC-MODEL",
    },
    IDENTITY,
  );
  for (const value of [
    "SYNTHETIC-DIGEST",
    "/Users/someone/secret",
    "SYNTHETIC-MODEL",
  ]) {
    assert.ok(!item.text.includes(value), value);
    assert.ok(!item.title.includes(value), value);
  }
});

test("every boundary status and reason in the maps renders through the transcript", () => {
  for (const [status, text] of CODING_SESSION_BOUNDARY_STATUSES) {
    for (const [slug, name] of CODING_SESSION_BOUNDARY_REASONS) {
      assert.equal(render(status, slug).text, `${text} (${name})`);
    }
  }
});

/** The real transcript component, rendered to markup as its own tests do. */
async function renderTranscriptMarkup(items) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionTranscript, {
        generationId: "generation-1",
        isWorking: false,
        items,
      }),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("the transcript shows the boundary row a reader sees, in both states", async () => {
  const prompt = {
    ...IDENTITY,
    id: "prompt-1",
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Operator",
    text: "Plan the work",
    turnId: "turn-1",
  };
  const unenforced = await renderTranscriptMarkup([
    { ...render(NOT_ENFORCED, "no-backend-for-platform"), turnId: "turn-1" },
    prompt,
  ]);
  assert.match(unenforced, /Project boundary/);
  assert.match(
    unenforced,
    /Not enforced — this session is not isolated from other projects(&#x27;|')? ?files/,
  );
  assert.match(unenforced, /this platform has no boundary backend/);
  assert.doesNotMatch(unenforced, /execution_boundary_not_enforced/);

  const enforced = await renderTranscriptMarkup([
    { ...render(ENFORCED, "macos-seatbelt"), turnId: "turn-1" },
    prompt,
  ]);
  assert.match(enforced, /Enforced — this session and every process it starts/);
  assert.match(enforced, /macOS Seatbelt/);
  assert.doesNotMatch(enforced, /Not enforced/);
});

test("other statuses are untouched by the boundary renderer", () => {
  const future = render("execution_boundary_something_new", "macos-seatbelt");
  assert.equal(future.title, "Status");
  assert.equal(future.text, "execution_boundary_something_new");
  const fresh = render("session_fresh", "no_prior_execution");
  assert.equal(fresh.title, "Session continuity");
});

// ── Per-turn usage on the wire ───────────────────────────────────────────────
//
// The provider now stamps an additive `usage` block on the terminal `result`
// item. This observer must accept it and keep projecting the turn: a reader
// that rejected the item on an unknown field would blank the end of every turn
// the moment the provider started measuring context.

test("a result item carrying a usage block still projects", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1000,
      result: "completed",
      inputTokens: 101200,
      outputTokens: 340,
      usage: {
        inputTokens: 1200,
        outputTokens: 340,
        cacheReadTokens: 96000,
        cacheWriteTokens: 4000,
        toolCalls: 7,
        contextWindow: 1000000,
      },
    },
    IDENTITY,
  );
  assert.equal(item.title, "Turn result");
  assert.equal(item.text, "completed");
  assert.equal(item.unknownKind, undefined);
  // Batch 2 integration: the block reaches the renderer type, so the Audit
  // tab reads a real number instead of `not reported` on every live turn.
  assert.deepEqual(item.usage, {
    inputTokens: 1200,
    outputTokens: 340,
    cacheReadTokens: 96000,
    cacheWriteTokens: 4000,
    toolCalls: 7,
    contextWindow: 1000000,
  });
});

test("a partial usage block carries only the fields the driver reported", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      durationMs: 1000,
      result: "completed",
      usage: {
        outputTokens: 340,
        // Unreadable and unknown fields are dropped rather than reported as
        // numbers: absent must never arrive at the audit as `0`.
        inputTokens: "1200",
        cacheReadTokens: Number.NaN,
        pricingIdentity: "anthropic/claude",
      },
    },
    IDENTITY,
  );
  assert.deepEqual(item.usage, { outputTokens: 340 });
});

test("a usage block with nothing readable in it is null, never an empty object", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      result: "completed",
      usage: { inputTokens: null, outputTokens: "many" },
    },
    IDENTITY,
  );
  assert.equal(item.usage, null);
});

test("a classified failure leads with the sentence and keeps the raw error under it", () => {
  const raw =
    "Agent reported error (code -32603): Failed to authenticate: OAuth session expired and could not be refreshed";
  const sentence =
    "Claude's login has expired on this computer. Run `claude auth login` in a terminal, then send the next turn.";
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "error",
      isError: true,
      durationMs: 2500,
      result: sentence,
      detail: raw,
    },
    IDENTITY,
  );
  assert.equal(item.title, "Turn result");
  assert.equal(item.renderClass, "error");
  assert.equal(item.text, `${sentence}\n\n${raw}`);
});

test("a detail equal to the result, or empty, adds nothing", () => {
  const same = buildBaseTranscriptItem(
    { kind: "result", subtype: "error", result: "boom", detail: "boom" },
    IDENTITY,
  );
  assert.equal(same.text, "boom");
  const blank = buildBaseTranscriptItem(
    { kind: "result", subtype: "error", result: "boom", detail: "   " },
    IDENTITY,
  );
  assert.equal(blank.text, "boom");
});

test("a result item with no usage block projects exactly as before", () => {
  const item = buildBaseTranscriptItem(
    {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1000,
      result: "completed",
    },
    IDENTITY,
  );
  assert.equal(item.title, "Turn result");
  assert.equal(item.text, "completed");
  assert.equal(item.usage, null);
});

test("a paired edit keeps the result's final args, discriminant, and paths", () => {
  const paired = buildPairedToolResultItem(
    {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolId: "edit-1",
        input: {},
      },
    },
    IDENTITY,
    {
      kind: "tool_result",
      toolId: "edit-1",
      toolName: "Edit",
      toolKind: "edit",
      input: { file_path: "desktop/src/App.tsx" },
      edit: { paths: ["desktop/src/App.tsx"] },
      content: "updated",
      isError: false,
    },
    { ...IDENTITY, id: "item-2" },
  );

  assert.equal(paired.toolKind, "edit");
  assert.deepEqual(paired.args, { file_path: "desktop/src/App.tsx" });
  assert.deepEqual(paired.editPaths, ["desktop/src/App.tsx"]);
});

test("a context_window_updated item carrying the driver's occupancy still projects", () => {
  const item = buildBaseTranscriptItem(
    { kind: "context_window_updated", usage: { size: 1000000, used: 137498 } },
    IDENTITY,
  );
  assert.equal(item.unknownKind, undefined);
  assert.ok(
    JSON.stringify(item).includes("137498"),
    `the occupancy the driver reported must survive projection: ${JSON.stringify(item)}`,
  );
});

test("a prompt that references its images inline does not also count them", () => {
  const url = `http://relay/media/${"a".repeat(64)}.png`;

  // Inline: the reader sees the picture where it was written, so restating
  // "1 attachment" underneath would describe what is already on screen.
  const inline = buildBaseTranscriptItem(
    {
      kind: "user_prompt",
      content: `When I do X, I see this:\n\n![image](${url})`,
      attachmentCount: 1,
    },
    IDENTITY,
  );
  assert.doesNotMatch(inline.text, /attachment/);
  assert.match(inline.text, /!\[image\]/);

  // No reference in the prose — an older client, or an upload that never
  // landed. The count is then the only evidence an image was part of the
  // turn, so it stays.
  const countOnly = buildBaseTranscriptItem(
    { kind: "user_prompt", content: "look at this", attachmentCount: 2 },
    IDENTITY,
  );
  assert.match(countOnly.text, /\(2 attachments\)/);
});

test("a prompt that links its pasted file inline does not also count it", () => {
  const sha = "b".repeat(64);

  // A pasted file is referenced by a plain link, not an image one — the reader
  // can follow it, so the count would restate what is already on screen.
  const inline = buildBaseTranscriptItem(
    {
      kind: "user_prompt",
      content: `fix the crash in [pasted-text-1.txt](http://relay/media/${sha}.txt)`,
      attachmentCount: 1,
    },
    IDENTITY,
  );
  assert.doesNotMatch(inline.text, /attachment/);

  // An ordinary link someone typed in their prose is not an attachment
  // reference, so it must not suppress the one piece of evidence there is.
  const unrelated = buildBaseTranscriptItem(
    {
      kind: "user_prompt",
      content: "see [the docs](http://example.com/guide) for context",
      attachmentCount: 1,
    },
    IDENTITY,
  );
  assert.match(unrelated.text, /\(1 attachment\)/);
});

test("a steered prompt carries the flag the renderer marks it with, and an ordinary one does not", () => {
  const steered = buildBaseTranscriptItem(
    {
      kind: "user_prompt",
      content: "Also check the tests",
      steered: true,
      commandId: "csc-steer-1",
    },
    { ...IDENTITY, id: "steered-1" },
  );
  assert.equal(steered.type, "message");
  assert.equal(steered.role, "user");
  assert.equal(steered.title, "Steered prompt");
  assert.equal(steered.steered, true);
  assert.equal(steered.commandId, "csc-steer-1");

  const plain = buildBaseTranscriptItem(
    { kind: "user_prompt", content: "Fix the bug" },
    { ...IDENTITY, id: "plain-1" },
  );
  assert.equal(plain.title, "Prompt");
  assert.equal(
    "steered" in plain,
    false,
    "absent, not false: older projections stay byte-identical",
  );
  // `steered: false` on the wire is the ordinary prompt.
  const explicit = buildBaseTranscriptItem(
    { kind: "user_prompt", content: "Fix the bug", steered: false },
    { ...IDENTITY, id: "plain-2" },
  );
  assert.equal("steered" in explicit, false);
});
