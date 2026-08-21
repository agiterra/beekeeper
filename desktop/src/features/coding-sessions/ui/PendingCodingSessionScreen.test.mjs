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
