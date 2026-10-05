import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { foldProjectPulseDigest } from "@/features/project-pulse/lib/pulseFold";
import {
  pulseNameOrigins,
  resetProjectPulseNameOrigins,
} from "@/features/project-pulse/lib/pulseNameOrigins";
import { PulseSessionCard } from "@/features/project-pulse/ui/PulseSessionCard";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const corpus = JSON.parse(
  readFileSync(
    path.join(
      HERE,
      "../../../../../conformance/project-pulse-fold/fixtures/fold-vectors.json",
    ),
    "utf8",
  ),
);
const BASE = corpus.vectors.find(
  (vector) => vector.name === "idle-hours-old-with-live-authorized-lease",
).input;
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = "a1".repeat(32);
const PROVIDER = "d4".repeat(32);
const TARGET = "coding-session/v1|3:acp6:inst-16:sess-11:1";
const GENESIS_ID = "9e".repeat(32);

// The same founding shape `sessionCoordinationNames.test.mjs` uses: the
// corpus session, its create naming a founder genesis.
function events(extra) {
  const base = BASE.events.map((event) => {
    if (event.kind !== 44221) return event;
    const content = JSON.parse(event.content);
    content.action.genesisRef = GENESIS_ID;
    return { ...event, content: JSON.stringify(content) };
  });
  const genesis = {
    id: GENESIS_ID,
    pubkey: FOUNDER,
    created_at: 1_785_589_000,
    kind: 44226,
    tags: [
      ["h", CHANNEL],
      ["csg-v", "csg1-1"],
      ["csg-session", SESSION],
    ],
    content: JSON.stringify({ sessionRef: SESSION, v: 1 }),
  };
  return [...base, genesis, ...extra];
}

const generatedTitle = {
  id: "03".repeat(32),
  pubkey: PROVIDER,
  created_at: 1_785_590_500,
  kind: 44252,
  tags: [
    ["h", CHANNEL],
    ["d", SESSION],
    ["cstl-v", "cstl1-1"],
    ["cs-target", TARGET],
  ],
  content: JSON.stringify({
    schema: "buzz-coding-session-title/v1",
    title: "Login redirect fix",
    model: "claude-haiku-4-5",
    basis: "first-message",
    sourceCommand: null,
    createEventId: "ca".repeat(32),
  }),
};

function digest(extra) {
  return foldProjectPulseDigest({
    project: BASE.project,
    now: BASE.now,
    events: events(extra),
  });
}

test("the digest keeps its pinned keys; the origin travels beside it", () => {
  const folded = digest([generatedTitle]);
  const [session] = folded.sessions;
  assert.equal(session.name, "Login redirect fix");
  assert.ok(!("nameOriginsBySession" in folded));
  assert.deepEqual(pulseNameOrigins(folded).get(session.sessionKey), {
    origin: "generated",
    model: "claude-haiku-4-5",
    signerPubkey: PROVIDER,
  });
});

test("a digest this fold did not produce has no origins, so no marker", () => {
  const copy = structuredClone(digest([generatedTitle]));
  assert.equal(pulseNameOrigins(copy).size, 0);
  assert.equal(pulseNameOrigins(null).size, 0);
});

test("a community reset forgets every recorded origin", () => {
  const folded = digest([generatedTitle]);
  resetProjectPulseNameOrigins();
  assert.equal(pulseNameOrigins(folded).size, 0);
});

test("the Pulse card marks a generated title Auto-named, and not without an origin", () => {
  const folded = digest([generatedTitle]);
  const [session] = folded.sessions;
  const render = (nameOrigin) =>
    renderToStaticMarkup(
      React.createElement(PulseSessionCard, {
        session,
        nowSeconds: BASE.now,
        nameOrigin,
      }),
    );
  assert.match(
    render(pulseNameOrigins(folded).get(session.sessionKey)),
    /data-testid="pulse-session-title-origin"[^>]*>Auto-named/,
  );
  assert.doesNotMatch(render(null), /Auto-named/);
});
