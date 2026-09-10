import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionLaunchGoalNotes,
  CodingSessionLaunchSteps,
} from "./NewCodingSessionLaunchNotes.tsx";
import { MAX_CODING_SESSION_GOAL_BYTES } from "../lib/codingSessionGoal.ts";
import { codingSessionCrewLaunchBlock } from "../lib/codingSessionCrew.ts";
import { codingSessionCrewLaunchRuntimeBinding } from "./useCodingSessionCrewLaunch.ts";

// Migrated from `NewCodingSessionCrewTab.test.mjs` when that module was
// deleted (REVIEW-B3 F5). Only the tests whose subjects survive the deletion
// are here; the ten exports that lost their last production caller are gone,
// and so are the tests that were keeping them green.

test("the step list marks the failed step and leaves the untouched ones untouched", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchSteps, {
      steps: [
        {
          id: "genesis",
          label: "Found the session",
          state: "done",
          detail: null,
        },
        {
          id: "create:1",
          label: "Seat Codey as builder",
          state: "failed",
          detail: "ACTOR_UNAVAILABLE",
        },
        {
          id: "first-turn",
          label: "Send the goal and the roster",
          state: "pending",
          detail: null,
        },
      ],
    }),
  );
  assert.match(html, /data-state="failed"/);
  assert.match(html, /ACTOR_UNAVAILABLE/);
  assert.match(html, /data-state="pending"/);
  assert.match(html, /data-testid="crew-step-create:1"/);
});

test("ordinary goals need no encoding explanation; near-limit feedback uses bytes", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchGoalNotes, {
      // Four UTF-8 bytes from two characters: the character count the field
      // itself can enforce is not the number the signer refuses on.
      bytes: 4,
      goalOutcome: null,
      overflow: null,
    }),
  );
  assert.equal(html, "");
  assert.doesNotMatch(html, /text-destructive/);
  const nearLimit = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchGoalNotes, {
      bytes: 3600,
      goalOutcome: null,
      overflow: null,
    }),
  );
  assert.match(nearLimit, /88% of the description limit used/);
  assert.match(nearLimit, /title="3,600 of 4,096 UTF-8 bytes\."/);
});

test("at the cap the counter becomes the launch block's own refusal", () => {
  const overflow = { bytes: 5000, cap: MAX_CODING_SESSION_GOAL_BYTES };
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchGoalNotes, {
      bytes: overflow.bytes,
      goalOutcome: null,
      overflow,
    }),
  );
  assert.match(html, /5,000 UTF-8 bytes and the cap is 4,096/);
  assert.match(html, /shorten it by 904 bytes/);
  assert.match(html, /text-destructive/);
  // The same words the button's own block gives, so fixing one reading can
  // never leave the other saying something different.
  const block = codingSessionCrewLaunchBlock({
    hasTeam: true,
    seatCount: 1,
    hasChannel: true,
    canCreateChannel: true,
    createInFlight: false,
    isLaunching: false,
    goal: "x".repeat(5000),
  });
  assert.match(html, new RegExp(block.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
});

test("a launch whose goal never went out says so at the field", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchGoalNotes, {
      bytes: 12,
      goalOutcome: {
        published: false,
        reason: "rate-limited: quota exceeded; retry in 2s",
      },
      overflow: null,
    }),
  );
  assert.match(
    html,
    /The session was founded, but its goal was not published: rate-limited/,
  );
  assert.match(html, /Set it from the session&#x27;s goal pill\./);
});

test("a launch that published its goal adds no line about it", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionLaunchGoalNotes, {
      bytes: 12,
      goalOutcome: { published: true, reason: null },
      overflow: null,
    }),
  );
  assert.doesNotMatch(html, /was not published/);
});

test("a launch binds to the exact runtime the click-time preflight returned", () => {
  // Migrated with the two components: this was the only coverage of the
  // binding, and it outlives the tab that used to hold it.
  const bound = codingSessionCrewLaunchRuntimeBinding({
    selectionKey: "target",
    channelId: "channel",
    signerPubkey: "d".repeat(64),
    provider: {
      providerInstanceRef: "codex-primary",
      runtime: "codex",
      defaultModel: "gpt-5.6-sol",
      allowedModels: ["gpt-5.6-sol"],
      capabilities: { threadTurnStart: true },
    },
    availability: { state: "ready", label: "Codex", hint: null },
    isLocalProvider: true,
  });
  assert.deepEqual(bound, {
    providerAuthorityPubkey: "d".repeat(64),
    providerInstanceRef: "codex-primary",
  });
  assert.throws(
    () =>
      codingSessionCrewLaunchRuntimeBinding({
        selectionKey: "target",
        channelId: "channel",
        signerPubkey: "not-a-pubkey",
        provider: { providerInstanceRef: "codex-primary" },
      }),
    /No coding-session provider is available/,
  );
});
