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

import { encodeStructuredKey } from "../lib/codingSessionKeys.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";

// Item 10 (batch 2026-09-01), rendered rather than unit-asserted.
//
// Brian observed session messages that looked like they came from him. The
// resolver's rules are unit-tested in
// `lib/codingSessionPromptAttribution.test.mjs`; these assert the rules
// actually reach the DOM through the Mission stream, and that Conversation —
// which passes no seat resolver — is untouched.

const PROVIDER_SIGNER = "1958c6c4".repeat(8);
const KEYSTONE_ACTOR = "ede63017".repeat(8);
const BOB_ACTOR = "efefd4e5".repeat(8);
const FOUNDER = "a1b2c3d4".repeat(8);
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";

function itemId(seq) {
  return encodeStructuredKey(
    "coding-session-transcript-item/v1",
    "generation-scope",
    "target-key",
    String(seq),
  );
}

/** A `role: "user"` echo exactly as the provider projects one. */
function prompt(seq, { text, operatorPubkey, commandId, turnId }) {
  return {
    id: itemId(seq),
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Prompt",
    text,
    timestamp: "2026-08-29T06:09:00.000Z",
    turnId,
    ...(operatorPubkey === undefined ? {} : { operatorPubkey }),
    ...(commandId === undefined ? {} : { commandId }),
  };
}

function seat({ sessionId, agentRef, role, transcript }) {
  return {
    generationId: `gen-${sessionId}`,
    label: "claude-agent-acp · generation 1",
    title: "Singularity",
    providerAuthorityPubkey: PROVIDER_SIGNER,
    metadataAuthorityPubkey: PROVIDER_SIGNER,
    lastEventAt: "2026-08-29T06:10:00.000Z",
    status: "completed",
    statusAt: null,
    transcript,
    conflictCount: 0,
    commandTarget: {
      driver: "claude-agent-acp",
      instanceId: "claude-instance",
      sessionId,
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: "claude-cc-1",
    runtime: "claude-agent-acp",
    model: "claude-opus-5",
    agentRef,
    role,
    turnBudget: null,
    capabilities: null,
  };
}

/**
 * Keystone (lead) sends Bob (builder) a turn; the founder's Desktop also
 * published an automatic team wake into Bob's execution, and one historical
 * prompt carries no operator stamp at all.
 */
function umbrellaWithKeystoneSendingBob() {
  const umbrellas = groupCodingSessionCatalog([
    seat({
      sessionId: "11111111-1111-1111-1111-111111111111",
      agentRef: KEYSTONE_ACTOR,
      role: "lead",
      transcript: [
        {
          id: itemId("k-1"),
          type: "message",
          renderClass: "message",
          role: "assistant",
          title: "Response",
          text: "Assigning the dispatch arm.",
          timestamp: "2026-08-29T06:08:00.000Z",
          turnId: "turn-keystone",
        },
      ],
    }),
    seat({
      sessionId: "11111111-1111-1111-1111-111111111112",
      agentRef: BOB_ACTOR,
      role: "builder",
      transcript: [
        prompt("b-1", {
          text: "Extract the dispatch arm into a pure function.",
          operatorPubkey: KEYSTONE_ACTOR,
          commandId: "csl-keystone-assignment",
          turnId: "turn-bob-1",
        }),
        prompt("b-2", {
          text: "A new report is waiting.",
          operatorPubkey: FOUNDER,
          commandId: `team-wake-v1:${"9".repeat(64)}:${"a".repeat(24)}`,
          turnId: "turn-bob-2",
        }),
        prompt("b-3", {
          text: "An old prompt from before attribution existed.",
          turnId: "turn-bob-3",
        }),
      ],
    }),
  ]);
  assert.equal(umbrellas[0].executions.length, 2);
  return umbrellas[0];
}

async function renderInRouter(element) {
  const rootRoute = createRootRoute({ component: () => element });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

/** The seat resolver the workspace builds; `null` for anything not a seat. */
function resolvePromptSeat(pubkey) {
  if (pubkey === KEYSTONE_ACTOR) {
    return { label: "Keystone · Lead", executionKey: "exec-keystone" };
  }
  if (pubkey === BOB_ACTOR) {
    return { label: "Bob · Builder", executionKey: "exec-bob" };
  }
  return null;
}

/** Every prompt caption in render order, as `kind` / text pairs. */
function authorCaptions(markup) {
  return [
    ...markup.matchAll(
      /data-author-kind="([^"]*)"[^>]*data-testid="coding-session-user-message-author"[^>]*>([^<]*)</g,
    ),
  ].map(([, kind, label]) => ({ kind, label }));
}

async function renderMission(umbrella) {
  return renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      currentUserPubkey: FOUNDER,
      laneMessages: [],
      missionDensity: "live",
      onHandoff: () => {},
      resolvePromptSeat,
      umbrella,
    }),
  );
}

test("item 10a: a turn Keystone sent Bob is attributed to Keystone", async () => {
  const markup = await renderMission(umbrellaWithKeystoneSendingBob());
  const captions = authorCaptions(markup);
  const seatCaption = captions.find((caption) => caption.kind === "seat");
  assert.ok(seatCaption, `no seat-attributed prompt in: ${captions.length}`);
  assert.equal(seatCaption.label, "Keystone · Lead");
  // The whole point: it is not the reader, and not a hash.
  assert.doesNotMatch(seatCaption.label, /^You$/);
  assert.doesNotMatch(seatCaption.label, /…/);
});

test("item 10b: the founder-signed team wake is Beekeeper, not You", async () => {
  const markup = await renderMission(umbrellaWithKeystoneSendingBob());
  const captions = authorCaptions(markup);
  const wake = captions.find((caption) => caption.kind === "team-wake");
  assert.ok(wake, "the team-wake prompt must be attributed");
  assert.equal(wake.label, "Beekeeper · team wake");
});

test("item 10c: an unstamped prompt says the operator was not recorded", async () => {
  const markup = await renderMission(umbrellaWithKeystoneSendingBob());
  const captions = authorCaptions(markup);
  const unrecorded = captions.find((caption) => caption.kind === "unrecorded");
  assert.ok(unrecorded, "the unstamped prompt must be attributed");
  assert.equal(unrecorded.label, "Operator not recorded");
});

test("item 10: no prompt in this fixture reads You", async () => {
  // The founder is the viewer here and signed one of these commands, so the
  // old rule would have put "You" on two of the three rows.
  const markup = await renderMission(umbrellaWithKeystoneSendingBob());
  const captions = authorCaptions(markup);
  assert.equal(captions.length, 3, "three prompts, three captions");
  for (const caption of captions) {
    assert.notEqual(caption.label, "You", `"You" on a ${caption.kind} row`);
  }
  assert.deepEqual(captions.map((caption) => caption.kind).sort(), [
    "seat",
    "team-wake",
    "unrecorded",
  ]);
});

test("item 10: Conversation passes no seat resolver, so a seat is not named", async () => {
  // The byte-identity gate: Mission's attribution must not leak into the
  // one-seat lens, which has no second seat to attribute to.
  const markup = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      currentUserPubkey: FOUNDER,
      laneMessages: [],
      onHandoff: () => {},
      resolvePromptSeat,
      umbrella: umbrellaWithKeystoneSendingBob(),
    }),
  );
  const captions = authorCaptions(markup);
  assert.equal(captions.length, 3);
  assert.equal(
    captions.filter((caption) => caption.kind === "seat").length,
    0,
    "Conversation must not resolve seats",
  );
  // The automatic and unstamped rules are the shared function's, so they hold
  // in both lenses — those were wrong for every reader, not just Mission's.
  assert.deepEqual(captions.map((caption) => caption.kind).sort(), [
    "operator",
    "team-wake",
    "unrecorded",
  ]);
});

// ── B3.2 / REVIEW-B3 F3, N1: the hire-host dispatch, rendered ───────────────
//
// F3: the resolver, the label and their unit tests existed and no surface
// called them, so `<requester> · via your Desktop` was unreachable while
// SURFACES §21.7 said it rendered.
//
// N1: the first fix derived "this is a hire dispatch" from the `[From the
// lead] ` **text prefix**, before any signed fact, and took that branch before
// it ever looked at `operatorPubkey` — so a lane message signed by the builder
// seat rendered as `Your Desktop (hire host)`. The derivation is now three
// signed conditions and text is never consulted; the attack is pinned below.

/** The umbrella a hired seat produces: its first turn IS the lead's brief. */
function umbrellaWithHiredSeat() {
  const umbrellas = groupCodingSessionCatalog([
    seat({
      sessionId: "11111111-1111-1111-1111-111111111113",
      agentRef: BOB_ACTOR,
      role: "builder",
      transcript: [
        prompt("h-1", {
          // The prefix the host and the CLI both write. It is *not* what makes
          // this a dispatch — nothing about the words is.
          text: "[From the lead] Take the badge lane. Red test first.",
          operatorPubkey: FOUNDER,
          turnId: "turn-hired-1",
        }),
      ],
    }),
  ]);
  return umbrellas[0];
}

/** The hire record this host would have written for that seat. */
function seatedOutcome(umbrella, overrides = {}) {
  return {
    commandId: "csl-hire-1",
    channelId: CHANNEL_ID,
    sessionRef: umbrella.sessionRef,
    role: "builder",
    state: "seated",
    detail: null,
    seatCommandId: "csl-seat-1",
    granted: true,
    seatActor: BOB_ACTOR,
    requesterLabel: "Keystone",
    hostPubkey: FOUNDER,
    ...overrides,
  };
}

test("F3: a hired seat's first turn is attributed to the lead that asked", async () => {
  const { resetCodingSessionHireOutcomes, publishCodingSessionHireOutcomes } =
    await import("../hooks/useCodingSessionHire.ts");
  const umbrella = umbrellaWithHiredSeat();
  resetCodingSessionHireOutcomes();
  publishCodingSessionHireOutcomes([seatedOutcome(umbrella)]);
  const html = await renderMission(umbrella);
  assert.match(html, /Keystone · via your Desktop/);
  assert.match(html, /data-author-kind="hire-host"/);
  resetCodingSessionHireOutcomes();
});

test("N1: a hire record for another umbrella does not attribute this one", async () => {
  const { resetCodingSessionHireOutcomes, publishCodingSessionHireOutcomes } =
    await import("../hooks/useCodingSessionHire.ts");
  const umbrella = umbrellaWithHiredSeat();
  resetCodingSessionHireOutcomes();
  publishCodingSessionHireOutcomes([
    seatedOutcome(umbrella, {
      sessionRef: "99999999-9999-4999-8999-999999999999",
    }),
  ]);
  const html = await renderMission(umbrella);
  assert.doesNotMatch(html, /Keystone · via your Desktop/);
  assert.doesNotMatch(html, /data-author-kind="hire-host"/);
  resetCodingSessionHireOutcomes();
});

test("N1: with no hire record the prompt belongs to its signer, not to a guess", async () => {
  // The honest fallback is the *signer's* byline. This fixture stamps the
  // founder, who is the reader, so it reads `You` — the prefix in the text
  // buys nothing.
  const { resetCodingSessionHireOutcomes } = await import(
    "../hooks/useCodingSessionHire.ts"
  );
  resetCodingSessionHireOutcomes();
  const html = await renderMission(umbrellaWithHiredSeat());
  assert.match(html, /data-author-kind="you"/);
  assert.doesNotMatch(html, /via your Desktop/);
  assert.doesNotMatch(html, /Your Desktop \(hire host\)/);
});

test("N1: a builder-signed message beginning `[From the lead] ` is the builder's", async () => {
  // The reviewer's attack, permanently. A lane message signed by the builder
  // seat whose text carries the prefix rendered as `Your Desktop (hire host)`
  // — a screen naming the wrong author, on the strength of a string.
  const { resetCodingSessionHireOutcomes, publishCodingSessionHireOutcomes } =
    await import("../hooks/useCodingSessionHire.ts");
  const umbrella = umbrellaWithHiredSeat();
  // Even with a *valid* hire vouch in the store, a lane message is not the
  // initial turn of a seated create and must not be attributed to a lead.
  resetCodingSessionHireOutcomes();
  publishCodingSessionHireOutcomes([seatedOutcome(umbrella)]);
  const html = await renderInRouter(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: CHANNEL_ID,
      currentUserPubkey: FOUNDER,
      laneMessages: [
        {
          id: "lane-1",
          authorPubkey: BOB_ACTOR,
          content: "[From the lead] Rebase the lane and run the gate.",
          timestampMs: 1_756_000_000_000,
        },
      ],
      missionDensity: "live",
      onHandoff: () => {},
      umbrella,
    }),
  );
  // Scoped to the lane row: the *transcript's* first turn in this fixture is a
  // real dispatch and is correctly named, so a whole-document assertion would
  // prove nothing.
  const start = html.indexOf(
    'data-testid="coding-session-umbrella-conversation"',
  );
  assert.ok(start > 0, "the lane message must render");
  const row = html.slice(start, html.indexOf("</div>", start) + 6);
  assert.match(row, /data-author-kind="operator"/);
  assert.match(row, /efefd4e5…d4e5/);
  assert.doesNotMatch(row, /Your Desktop \(hire host\)/);
  assert.doesNotMatch(row, /via your Desktop/);
  resetCodingSessionHireOutcomes();
});
