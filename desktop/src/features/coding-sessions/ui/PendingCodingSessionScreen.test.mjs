import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { acpAuthMethodsQueryKey } from "@/features/agents/hooks";
import { PendingCodingSessionScreen } from "./PendingCodingSessionScreen.tsx";

function withQueryClient(element, seedMethodsByRuntime = {}) {
  const client = new QueryClient();
  for (const [runtime, methods] of Object.entries(seedMethodsByRuntime)) {
    client.setQueryData([...acpAuthMethodsQueryKey, runtime], { methods });
  }
  return React.createElement(QueryClientProvider, { client }, element);
}

function makeTransaction(overrides = {}) {
  return {
    schema: "buzz.coding-session-create.v1",
    scopeId: "channel-1",
    input: {
      channelId: "channel-1",
      commandId: "cmd-1",
      projectRef: null,
      repoRef: null,
      sessionRef: "umbrella-1",
      providerInstanceRef: "instance-1",
      providerAuthorityPubkey: "a".repeat(64),
      model: null,
      title: "Fix the flaky test",
      initialTurn: "Please find and fix the flaky test in CI.",
      ...overrides,
    },
    event: { id: "e".repeat(64) },
    publishState: "published",
    createdAt: 1_755_000_000_000,
  };
}

function render(props = {}, seedMethods = {}) {
  return renderToStaticMarkup(
    withQueryClient(
      React.createElement(PendingCodingSessionScreen, {
        transaction: makeTransaction(),
        lifecycle: { state: "pending" },
        lifecycleIsLoading: false,
        lifecycleErrorMessage: null,
        isPublishing: false,
        hostPhase: "idle",
        publishError: null,
        stalled: false,
        failedRuntime: null,
        channelName: "engineering",
        onBack() {},
        retryExact() {},
        startFresh() {},
        beginLoginWatch() {},
        onEditRequest() {},
        ...props,
      }),
      seedMethods,
    ),
  );
}

test("a waiting create renders as a session: echoed message, working badge, muted status", () => {
  const markup = render();
  assert.match(markup, /data-testid="pending-coding-session-screen"/);
  assert.match(markup, /Fix the flaky test/);
  assert.match(markup, /Please find and fix the flaky test in CI\./);
  assert.match(markup, /Session status: Working/);
  assert.match(
    markup,
    /Waiting for the session provider to accept this request/,
  );
  assert.match(markup, /data-testid="pending-coding-session-start-fresh"/);
  assert.match(markup, /data-testid="pending-coding-session-retry"/);
});

test("no initial turn: no echoed bubble, Idle badge", () => {
  const markup = render({
    transaction: makeTransaction({ initialTurn: null, title: null }),
  });
  assert.doesNotMatch(
    markup,
    /data-testid="pending-coding-session-first-message"/,
  );
  assert.match(markup, /Session status: Idle/);
  assert.match(markup, /Coding session/);
});

test("an auth-failed receipt surfaces the login remediation in place", () => {
  const markup = render({
    lifecycle: {
      state: "failed",
      error: { code: "PROVIDER_AUTH_REQUIRED", message: "sign in" },
    },
    failedRuntime: { runtime: "claude-agent-acp", label: "Claude Code" },
  });
  assert.match(markup, /data-testid="new-coding-session-auth-required"/);
  assert.match(markup, /Session status: Status unknown/);
});

test("a workdir failure offers the return-to-form remediation", () => {
  const markup = render({
    lifecycle: {
      state: "failed",
      error: { code: "PROJECT_CWD_UNRESOLVED", message: "no cwd" },
    },
  });
  assert.match(markup, /data-testid="pending-coding-session-fix-workdir"/);
  assert.match(markup, /could not resolve a working directory/);
});

test("a stalled wait escalates to the destructive diagnosis with retry disabled", () => {
  const markup = render({ stalled: true });
  assert.match(markup, /has not accepted this request after\s+30 seconds/);
  const retry = markup.match(
    /<button[^>]*data-testid="pending-coding-session-retry"[^>]*>/,
  );
  assert.ok(retry, "retry button renders");
  assert.match(retry[0], /disabled/);
});

test("conflicting receipts show the conflict copy", () => {
  const markup = render({ lifecycle: { state: "conflict" } });
  assert.match(markup, /Conflicting signed lifecycle receipts/);
  assert.match(markup, /Session status: Status unknown/);
});

test("an unconfirmed publish says so, keeps retry enabled, and shows Status unknown", () => {
  const markup = render({
    transaction: { ...makeTransaction(), publishState: "ambiguous" },
    stalled: true,
  });
  assert.match(markup, /relay never confirmed/);
  assert.doesNotMatch(markup, /provider has not accepted/);
  assert.match(markup, /Session status: Status unknown/);
  const retry = markup.match(
    /<button[^>]*data-testid="pending-coding-session-retry"[^>]*>/,
  );
  assert.ok(retry, "retry button renders");
  // The `disabled` *attribute*, not Tailwind's `disabled:` variant classes.
  assert.doesNotMatch(retry[0], /\sdisabled=""/);
});

/**
 * A seated create must never read like a human one. The pending screen is the
 * first place a person sees the seat that was actually signed — including,
 * when custody could not stage a role pack, that the agent will run on its
 * persona prompt alone.
 */
test("the pending screen names the seat the create actually carries", () => {
  const markup = render({
    transaction: makeTransaction({ actor: "d".repeat(64), role: "builder" }),
    seat: { actorLabel: "Ada", role: "builder", packStaged: true },
  });
  assert.match(markup, /data-testid="pending-coding-session-seat"/);
  assert.match(markup, /Seated: Ada · Builder/);
  assert.doesNotMatch(markup, /no role skills/);
  // The same label rides the header, so a seated execution reads the same way
  // in the pending screen as it does in the workspace it becomes.
  assert.match(markup, /data-testid="coding-session-header-seat"/);
});

test("a seat staged without its pack says so, on the screen it was created from", () => {
  const markup = render({
    transaction: makeTransaction({ actor: "d".repeat(64), role: "builder" }),
    seat: { actorLabel: "Ada", role: "builder", packStaged: false },
  });
  assert.match(
    markup,
    /Seated: Ada · Builder — seated with no role skills: this computer has no role pack behind this persona\./,
  );
});

test("a staging outcome nobody observed is not reported as a missing pack", () => {
  const markup = render({
    transaction: makeTransaction({ actor: "d".repeat(64), role: "builder" }),
    seat: { actorLabel: "Ada", role: "builder", packStaged: null },
  });
  assert.match(markup, /Seated: Ada · Builder/);
  assert.doesNotMatch(markup, /no role skills/);
});

test("an unresolved agent name falls back to the role rather than inventing one", () => {
  const markup = render({
    transaction: makeTransaction({ actor: "d".repeat(64), role: "builder" }),
    seat: { actorLabel: null, role: "builder", packStaged: null },
  });
  assert.match(markup, /Seated: Builder/);
  assert.doesNotMatch(markup, /dddddddd/);
});

test("an unseated create renders exactly as it did before", () => {
  const markup = render();
  assert.doesNotMatch(markup, /data-testid="pending-coding-session-seat"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-header-seat"/);
  assert.doesNotMatch(markup, /Seated:/);
});
