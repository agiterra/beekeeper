import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionHandoverPanel } from "./CodingSessionHandoverPanel.tsx";
import { CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE } from "../lib/codingSessionHandoverModel.ts";

const B = "bb".repeat(32);
const BODY_B = "2b".repeat(32);
const CLAIM = "c1".repeat(32);
const CHECKPOINT = "0c".repeat(32);
const CONTINUATION = "0f".repeat(32);
const NOW_MS = 2_000_000 * 1_000;

function model(overrides = {}) {
  return {
    claim: { state: "no-claim" },
    claimSince: null,
    claimVoidedAt: null,
    activeBody: null,
    continuation: null,
    latestCheckpoint: null,
    viewerMayContinue: false,
    viewerMayTakeBack: false,
    viewerIsClaimant: false,
    thisExecutionFenced: false,
    fenceReason: null,
    metadataFence: null,
    priorContinuation: null,
    supersededCheckpoints: [],
    claimedBodyReachable: false,
    retired: false,
    retiredAt: null,
    evidenceLinks: [],
    outcomeLabel: null,
    missing: [],
    recovered: [],
    ...overrides,
  };
}

function render(props) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionHandoverPanel, {
      nowMs: NOW_MS,
      ...props,
    }),
  );
}

const ACTIVE = {
  state: "active",
  claimant: B,
  bodyPubkey: BODY_B,
  acceptedEventId: CLAIM,
  seq: 2,
};
const SINCE = 2_000_000 - 3_600;

test("an unclaimed session says so rather than implying a handover", () => {
  const markup = render({ model: model() });
  assert.match(markup, /data-testid="coding-session-handover"/);
  assert.match(markup, /data-testid="coding-session-handover-status"/);
  assert.match(markup, /No handover/);
  assert.doesNotMatch(markup, /coding-session-handover-continue/);
});

test("the status line names the claimant, the body and the age", () => {
  const markup = render({
    model: model({ claim: ACTIVE, claimSince: SINCE, activeBody: BODY_B }),
    resolveName: () => "Bea",
  });
  assert.match(markup, /Active/);
  assert.match(markup, /Bea/);
  assert.match(markup, /1h ago/);
});

test("the continue action says it hands over the whole session", () => {
  const markup = render({
    model: model({ viewerMayContinue: true }),
    onContinue() {},
  });
  assert.match(markup, /data-testid="coding-session-handover-continue"/);
  assert.match(markup, /Continue this session&#x27;s work/);
  assert.match(
    markup,
    /This hands over the whole session: every execution and assignment under it is fenced until you release or someone takes it back\./,
  );
  assert.doesNotMatch(markup, /slice/i);
});

test("a fenced execution names the fence in a word, not only in colour", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      activeBody: BODY_B,
      thisExecutionFenced: true,
      fenceReason: "other-body",
      viewerMayTakeBack: true,
    }),
    onTakeBack() {},
  });
  assert.match(markup, /data-testid="coding-session-handover-fenced"/);
  assert.match(markup, /Fenced/);
  assert.match(markup, /data-testid="coding-session-handover-take-back"/);
  assert.match(markup, /Take this session back/);
  // The tint is present, and so is the word — colour never carries it alone.
  assert.match(markup, /amber-500/);
});

test("a voided claim says the fence stays up until somebody takes over", () => {
  const markup = render({
    model: model({
      claim: {
        state: "voided",
        last: {
          claimant: B,
          bodyPubkey: BODY_B,
          acceptedEventId: CLAIM,
          seq: 2,
        },
        voidedBy: "de".repeat(32),
        seq: 3,
      },
      claimVoidedAt: 2_000_000 - 120,
      thisExecutionFenced: true,
      fenceReason: "voided",
      viewerMayTakeBack: true,
    }),
    onTakeBack() {},
    resolveName: () => "Bea",
  });
  assert.match(markup, /Handover voided/);
  assert.match(markup, /Bea lost standing 2m ago/);
  assert.match(
    markup,
    /Every execution of this session stays fenced until someone with standing takes it over\./,
  );
  assert.match(markup, /Take over this session/);
});

test("the two outcomes are labelled apart and the label explains itself", () => {
  const reconstructed = render({
    model: model({
      claim: ACTIVE,
      outcomeLabel: "Reconstructed",
      recovered: ["wip-ref refs/heads/wip/builder/abc at 9999"],
    }),
  });
  assert.match(reconstructed, /data-testid="coding-session-handover-outcome"/);
  assert.match(reconstructed, /Reconstructed/);
  assert.match(reconstructed, /stayed on its machine/);
  assert.match(reconstructed, /Recovered: wip-ref/);

  const native = render({
    model: model({ claim: ACTIVE, outcomeLabel: "Native continuation" }),
  });
  assert.match(native, /Native continuation/);
  assert.match(native, /resumed where it already ran/);
});

test("a partial preservation is rendered as a missing line", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      missing: [
        CODING_SESSION_HANDOVER_PARTIAL_DISCLOSURE,
        "editor scratch buffer",
      ],
    }),
  });
  assert.match(markup, /data-testid="coding-session-handover-missing"/);
  assert.match(markup, /Missing: Not all uncommitted work was preserved/);
  assert.match(markup, /Missing: editor scratch buffer/);
});

test("evidence links are rendered as buttons a reader can open", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      evidenceLinks: [
        { label: "Claim", eventId: CLAIM },
        { label: "Checkpoint", eventId: CHECKPOINT },
        { label: "Continuation", eventId: CONTINUATION },
      ],
    }),
    onOpenEvidence() {},
  });
  assert.match(markup, /data-testid="coding-session-handover-evidence"/);
  assert.match(markup, /Claim c1c1c1c1…c1c1/);
  assert.match(markup, /Checkpoint /);
  assert.match(markup, /Continuation /);
});

test("a reachable claimed body offers no reconstruction, only the composer", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      activeBody: BODY_B,
      claimedBodyReachable: true,
      viewerMayContinue: false,
    }),
    onContinue() {},
  });
  assert.match(markup, /continue from the composer/);
  assert.doesNotMatch(markup, /coding-session-handover-continue/);
});

test("a retired umbrella renders the deletion and nothing to press", () => {
  const markup = render({
    model: model({ retired: true, retiredAt: 1_700_000_000 }),
    onContinue() {},
    onTakeBack() {},
  });
  assert.match(markup, /data-testid="coding-session-handover-retired"/);
  assert.match(markup, /Deleted/);
  assert.doesNotMatch(markup, /coding-session-handover-continue/);
  assert.doesNotMatch(markup, /coding-session-handover-take-back/);
});

test("a refusal is rendered with its own code rather than swallowed", () => {
  const markup = render({
    model: model({ viewerMayContinue: true }),
    errorMessage: "HANDOVER_FENCED",
    onContinue() {},
  });
  assert.match(markup, /data-testid="coding-session-handover-error"/);
  assert.match(markup, /HANDOVER_FENCED/);
});

test("an action in flight says what it is doing and cannot be pressed twice", () => {
  const markup = render({
    model: model({ viewerMayContinue: true }),
    busy: "continue",
    onContinue() {},
  });
  assert.match(markup, /Taking over this session…/);
  assert.match(markup, /disabled=""/);
});

test("the panel is labelled for assistive technology", () => {
  const markup = render({ model: model() });
  const labelledBy = /aria-labelledby="([^"]+)"/.exec(markup);
  assert.ok(labelledBy, "the section must name its heading");
  assert.match(markup, new RegExp(`id="${labelledBy[1]}"`));
});

test("each fence says the fact that causes it, and never `held elsewhere` for a void", () => {
  const otherBody = render({
    model: model({
      claim: ACTIVE,
      activeBody: BODY_B,
      thisExecutionFenced: true,
      fenceReason: "other-body",
    }),
  });
  assert.match(
    otherBody,
    /is not 2b2b2b2b…2b2b, the body holding this session/,
  );

  const notClaimant = render({
    model: model({
      claim: ACTIVE,
      activeBody: BODY_B,
      thisExecutionFenced: true,
      fenceReason: "not-claimant",
    }),
    resolveName: () => "Bea",
  });
  assert.match(
    notClaimant,
    /Bea holds this session; your turns on this execution will be refused until you take it back/,
  );

  const voided = render({
    model: model({
      claim: {
        state: "voided",
        last: {
          claimant: B,
          bodyPubkey: BODY_B,
          acceptedEventId: CLAIM,
          seq: 2,
        },
        voidedBy: "de".repeat(32),
        seq: 3,
      },
      thisExecutionFenced: true,
      fenceReason: "voided",
    }),
  });
  assert.match(voided, /every execution of this session is frozen/);
  assert.doesNotMatch(
    voided,
    /held elsewhere/,
    "nobody holds a voided session — that is why it is frozen",
  );
});

test("an unknown body renders `Fence unknown`, never silence", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      activeBody: BODY_B,
      thisExecutionFenced: true,
      fenceReason: "unknown",
    }),
    resolveName: () => "Bea",
  });
  assert.match(markup, /Fence unknown/);
  assert.match(markup, /cannot tell whether the execution in front of you/);
});

test("a fence this execution's own provider disclosed says whose word it is", () => {
  const markup = render({
    model: model({
      claim: { state: "no-claim" },
      thisExecutionFenced: true,
      fenceReason: "other-body",
      metadataFence: {
        claimant: B,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM,
      },
    }),
  });
  assert.match(markup, /as this execution&#x27;s own provider reports it/);
});

test("a missing checkout directory blocks the action and says which", () => {
  const markup = render({
    model: model({ viewerMayContinue: true }),
    continueBlockedReason:
      "choose a checkout directory on this computer for the work to land in.",
    workdirField: React.createElement("input", {
      "data-testid": "fake-workdir",
    }),
  });
  assert.match(markup, /data-testid="coding-session-handover-blocked"/);
  assert.match(markup, /choose a checkout directory on this computer/);
  assert.match(
    markup,
    /data-testid="fake-workdir"/,
    "the picker stays on screen, so the prerequisite can be met",
  );
  assert.doesNotMatch(markup, /coding-session-handover-continue/);
});

test("a capped read says the history above may be partial", () => {
  const markup = render({
    model: model({ claim: ACTIVE }),
    capped: ["the authority chain"],
  });
  assert.match(markup, /data-testid="coding-session-handover-capped"/);
  assert.match(markup, /came back at its limit/);
});

test("an earlier claim's continuation stays on screen as history", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      priorContinuation: {
        eventId: "1f".repeat(32),
        author: B,
        createdAt: 1_700_000_000,
        claimRef: "aa".repeat(32),
        mode: "reconstructed",
        target: {
          driver: "acp",
          instanceId: "i",
          sessionId: "s",
          generation: 1,
        },
        checkpointRef: null,
        recovered: [],
        missing: [],
        note: null,
        standing: "superseded",
      },
    }),
    resolveName: () => "Bea",
  });
  assert.match(markup, /data-testid="coding-session-handover-history"/);
  assert.match(markup, /Continued by Bea until/);
  assert.match(markup, /under an earlier claim/);
});

test("a replaced checkpoint is said out loud, with the reason it was replaced", () => {
  const markup = render({
    model: model({
      claim: ACTIVE,
      supersededCheckpoints: [
        {
          eventId: "0e".repeat(32),
          author: B,
          createdAt: 1_700_000_000,
          standing: "superseded",
          supersededBy: CHECKPOINT,
          body: {},
        },
      ],
    }),
  });
  assert.match(markup, /data-testid="coding-session-handover-superseded"/);
  assert.match(
    markup,
    /An earlier checkpoint was replaced by its own author&#x27;s next one/,
  );
  assert.match(markup, /newest statement, not the newest timestamp/);
});
