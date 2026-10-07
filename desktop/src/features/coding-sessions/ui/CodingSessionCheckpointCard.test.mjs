import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";

import {
  CodingSessionTranscript,
  createCodingSessionDisclosureStore,
} from "./CodingSessionTranscript.tsx";
import { CodingSessionCheckpointsValueProvider } from "./CodingSessionCheckpointsContext.tsx";
import {
  CODING_SESSION_REMOTE_DIFF_SENTENCE,
  CodingSessionDiffSurface,
} from "./CodingSessionDiffSurface.tsx";
import {
  codingSessionCheckpointScopeKey,
  foldCodingSessionCheckpoints,
} from "../lib/codingSessionCheckpoints.ts";
import {
  checkpointEvent,
  gitFacts,
  payload,
  PROVIDER_PUBKEY,
  scope,
  STRANGER_SECRET,
  TARGET,
  TARGET_KEY,
  TREE_A,
  TREE_B,
  unavailablePayload,
} from "../lib/codingSessionCheckpointFixtures.testFixtures.mjs";

const GENERATION = "generation-1";
const timestamp = "2026-10-07T12:00:00.000Z";
const bridgeSource = {
  label: "provider",
  pubkey: PROVIDER_PUBKEY.slice(0, 12),
};

function readOf(events) {
  return {
    fold: foldCodingSessionCheckpoints(events, scope()),
    scopeByGeneration: new Map([
      [
        GENERATION,
        codingSessionCheckpointScopeKey(PROVIDER_PUBKEY, TARGET_KEY),
      ],
    ]),
    generations: new Map([
      [
        GENERATION,
        {
          generationId: GENERATION,
          targetKey: TARGET_KEY,
          signerPubkey: PROVIDER_PUBKEY,
        },
      ],
    ]),
    capped: false,
    isLoading: false,
    errorMessage: null,
  };
}

function message(id, role, text) {
  return {
    id,
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId: "turn-1",
    bridgeSource,
  };
}

const ITEMS = [
  message("prompt", "user", "Rename the helper with sed"),
  message("answer", "assistant", "Done with sed."),
];

async function renderTurn(read, open = []) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(
        CodingSessionCheckpointsValueProvider,
        { value: read },
        React.createElement(CodingSessionTranscript, {
          disclosureStore: createCodingSessionDisclosureStore(open),
          generationId: GENERATION,
          isWorking: false,
          items: ITEMS,
        }),
      ),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("a shell-only turn shows its git files: N files changed · From git", async () => {
  const read = readOf([
    checkpointEvent(
      payload({
        files: [
          {
            path: "src/a.ts",
            status: "modified",
            from: null,
            additions: 2,
            deletions: 2,
          },
          {
            path: "src/b.ts",
            status: "renamed",
            from: "src/old.ts",
            additions: 0,
            deletions: 0,
          },
        ],
      }),
    ),
  ]);
  const closed = await renderTurn(read);
  assert.match(closed, /data-source="git"/);
  assert.match(closed, /2 files changed/);
  assert.match(closed, /· From git/);
  const opened = await renderTurn(read, ["changed-files:turn-1"]);
  assert.match(opened, /src\/old\.ts → src\/b\.ts/);
  assert.match(opened, /data-status="renamed"/);
});

test("no checkpoint for the turn: nothing invented; foreign checkpoints ignored", async () => {
  const foreign = readOf([
    checkpointEvent(payload(), { secret: STRANGER_SECRET }),
  ]);
  const markup = await renderTurn(foreign);
  assert.doesNotMatch(markup, /coding-session-changed-files/);
  assert.equal(foreign.fold.rejected.foreignSigner, 1);
});

test("not a git repository and a missing baseline say so, never 0 files", async () => {
  const notRepo = await renderTurn(
    readOf([checkpointEvent(unavailablePayload())]),
  );
  assert.match(notRepo, /No checkpoint · not a git repository/);
  assert.match(notRepo, /data-source="unavailable"/);
  const noBase = await renderTurn(
    readOf([
      checkpointEvent(
        payload({ git: gitFacts({ baseTree: null }), files: [] }),
      ),
    ]),
  );
  assert.match(noBase, /Baseline not captured/);
  assert.doesNotMatch(noBase, /0 files/);
});

test("outside-turn and omitted files are on the unopened card", async () => {
  const markup = await renderTurn(
    readOf([
      checkpointEvent(
        payload({
          git: gitFacts({
            outsideTurn: true,
            complete: false,
            omitted: [{ path: "big.bin", reason: "too_large" }],
          }),
        }),
      ),
    ]),
  );
  assert.match(markup, /Files also changed outside a turn/);
  assert.match(markup, /1 file not captured \(too large\)/);
});

function renderDiff(read, answers) {
  const client = new QueryClient();
  const generation = [...read.fold.byScope.values()][0];
  const ctx = {
    umbrella: { sessionRef: "5e55a0e1-0000-4000-8000-000000000001" },
    projectRef: null,
    focusedRecord: null,
  };
  const sessionId = TARGET.sessionId;
  for (const [from, to, answer] of answers) {
    client.setQueryData(
      [
        "coding-session-checkpoint-diff",
        ctx.umbrella.sessionRef,
        sessionId,
        null,
        from,
        to,
      ],
      answer,
    );
  }
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(
        CodingSessionCheckpointsValueProvider,
        { value: read },
        React.createElement(CodingSessionDiffSurface, {
          ctx,
          generation,
          observed: React.createElement("div", null, "observed rail"),
        }),
      ),
    ),
  );
}

test("Diff surface: a local turn diff renders the patch, from git", () => {
  const read = readOf([checkpointEvent(payload())]);
  const markup = renderDiff(read, [
    [
      TREE_A,
      TREE_B,
      {
        state: "local",
        checkout: "seat_worktree",
        files: [
          {
            path: "src/main.rs",
            additions: 1,
            deletions: 1,
            patch: "@@ -1 +1 @@\n-old line\n+new line",
            truncated: false,
          },
        ],
        additions: 1,
        deletions: 1,
        filesNotListed: 0,
      },
    ],
  ]);
  assert.match(markup, /From git · checkpoint 1 of 1/);
  assert.match(markup, /coding-session-diff-local/);
  assert.match(markup, /new line/);
  assert.match(markup, /seat worktree/);
});

test("Diff surface: trees elsewhere list the signed files and say where the diff lives", () => {
  const read = readOf([checkpointEvent(payload())]);
  const markup = renderDiff(read, [[TREE_A, TREE_B, { state: "no_checkout" }]]);
  assert.match(
    markup,
    new RegExp(CODING_SESSION_REMOTE_DIFF_SENTENCE.replace(".", "\\.")),
  );
  assert.match(markup, /coding-session-diff-listed-file/);
  assert.match(markup, /src\/main\.rs/);
  const missing = renderDiff(read, [
    [TREE_A, TREE_B, { state: "objects_missing", missing: [TREE_A] }],
  ]);
  assert.match(missing, /does not hold these checkpoints/);
});

test("Diff surface: a measured no-change is said, never an empty diff", () => {
  const read = readOf([checkpointEvent(payload({ files: [] }))]);
  const markup = renderDiff(read, [
    [
      TREE_A,
      TREE_B,
      {
        state: "local",
        checkout: "project_checkout",
        files: [],
        additions: 0,
        deletions: 0,
        filesNotListed: 0,
      },
    ],
  ]);
  assert.match(markup, /Git found no change between these checkpoints/);
});

test("Diff surface: an unavailable checkpoint gives its reason and no diff request", () => {
  const read = readOf([checkpointEvent(unavailablePayload())]);
  const markup = renderDiff(read, []);
  assert.match(markup, /coding-session-diff-none/);
  assert.match(markup, /not a git repository/);
});
