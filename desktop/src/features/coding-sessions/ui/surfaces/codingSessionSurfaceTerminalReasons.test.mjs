/**
 * SV-23 (reasons), Terminal: lane B2's `codingSessionSurfaceReasons.test.mjs`
 * leaves Terminal to lane B4. A Terminal made unavailable because the tree is
 * on another computer says so on the Files surface's clause — "The working
 * tree is on another computer ({provider}'s provider runs this session)" —
 * never naming whose computer it is from a provider label (fix round 1
 * decision; the brief's §3 "{owner}'s computer" wording is retired), and its
 * panel shows that same sentence (with no "New terminal") rather than an
 * empty frame. A shared read cut off at its page size says so.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CODING_SESSION_BUILTIN_SURFACES } from "./codingSessionBuiltinSurfaces.ts";

const PROVIDER = "b".repeat(64);
const CREATOR = "a".repeat(64);

function terminal() {
  const found = CODING_SESSION_BUILTIN_SURFACES.find(
    (d) => d.id === "terminal",
  );
  assert.ok(found, "no built-in Terminal surface");
  return found;
}

function elsewhereCtx(names) {
  return {
    sessionKey: "session-1",
    focusedExecution: { operatorPubkey: CREATOR, signerPubkey: PROVIDER },
    focusedRecord: { providerAuthorityPubkey: PROVIDER },
    resolveActorName: (pubkey) => names[pubkey] ?? null,
    tree: {
      state: "resolved",
      available: false,
      source: null,
      label: "no working tree",
      reason: "The working tree is on another computer.",
      refusal: "notLocal",
    },
    extensions: {
      terminal: {
        shells: [],
        shellsLoading: false,
        runningIds: new Set(),
        idleIds: new Set(),
        runningCount: 0,
        shared: [],
        foregroundUnknown: false,
        sharedState: "ready",
        sharedTruncated: false,
        myPubkey: null,
      },
    },
  };
}

test("the reason names the provider that runs the session, not a computer owner", () => {
  const ctx = elsewhereCtx({ [CREATOR]: "Brian", [PROVIDER]: "Andy" });
  assert.deepEqual(terminal().availability(ctx), {
    available: false,
    reason:
      "The working tree is on another computer (Andy's provider runs this session), and no terminal there is shared.",
  });
});

test("an unresolved provider names nobody", () => {
  const ctx = elsewhereCtx({});
  assert.equal(
    terminal().availability(ctx).reason,
    "The working tree is on another computer, and no terminal there is shared.",
  );
});

test("a teammate's shared terminal opens the surface even without the tree", () => {
  const ctx = elsewhereCtx({ [PROVIDER]: "Andy" });
  ctx.extensions.terminal.shared = [{ sessionId: "s", ownerPubkey: PROVIDER }];
  assert.deepEqual(terminal().availability(ctx), { available: true });
});

test("the unavailable panel shows the same reason and no New terminal", () => {
  const ctx = elsewhereCtx({ [PROVIDER]: "Andy" });
  const { Panel } = terminal();
  const markup = renderToStaticMarkup(React.createElement(Panel, { ctx }));
  assert.match(markup, /data-testid="coding-session-terminal-unavailable"/);
  assert.ok(
    markup.includes(
      "The working tree is on another computer (Andy&#x27;s provider runs this session), and no terminal there is shared.",
    ),
    markup,
  );
  assert.doesNotMatch(markup, /New terminal/);
});

test("a shared read cut off at its page size does not claim none is shared", () => {
  const ctx = elsewhereCtx({ [PROVIDER]: "Andy" });
  ctx.extensions.terminal.sharedTruncated = true;
  const reason = terminal().availability(ctx).reason;
  assert.equal(
    reason,
    "The working tree is on another computer (Andy's provider runs this session), and no terminal shared there was found among the project's newest 100.",
  );
  assert.doesNotMatch(reason, /no terminal there is shared/);
});
