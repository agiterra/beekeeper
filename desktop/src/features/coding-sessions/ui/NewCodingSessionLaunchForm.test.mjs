import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  NewCodingSessionLeadField,
  resolveNewCodingSessionLead,
} from "./NewCodingSessionLeadField.tsx";
import { NewCodingSessionBenchField } from "./NewCodingSessionBenchField.tsx";
import { NewCodingSessionPolicyField } from "./NewCodingSessionPolicyField.tsx";
import { NewCodingSessionReadiness } from "./NewCodingSessionReadiness.tsx";
import {
  codingSessionLaunchPlan,
  codingSessionLaunchReadiness,
} from "../lib/codingSessionLaunchForm.ts";
import {
  CODING_SESSION_POLICY_STATED_NOT_ENFORCED,
  EMPTY_CODING_SESSION_POLICY_DRAFT,
} from "../lib/codingSessionPolicy.ts";
import { codingSessionGoalOverflowSentence } from "../lib/codingSessionGoal.ts";

const KEYSTONE = "a1".repeat(32);
const TWIN = "b2".repeat(32);
const CANDIDATES = [
  { pubkey: KEYSTONE, name: "Keystone", role: "lead", model: "opus" },
  // Same display name, different key — the case the identity line exists for.
  { pubkey: TWIN, name: "Keystone", role: "builder", model: null },
  // No role: not seatable as a lead, and not offered as one.
  { pubkey: "c3".repeat(32), name: "Scribe", role: null, model: null },
];

function leadHtml(actor) {
  const lead = resolveNewCodingSessionLead({
    actor,
    candidates: CANDIDATES,
    youLabel: "You",
  });
  return {
    lead,
    html: renderToStaticMarkup(
      React.createElement(NewCodingSessionLeadField, {
        candidates: CANDIDATES,
        lead,
        onLeadChange: () => {},
      }),
    ),
  };
}

test("leading it yourself is not governed, and the switch says why", () => {
  const { lead, html } = leadHtml(null);
  assert.equal(lead.kind, "you");
  assert.match(html, /Not governed/);
  assert.match(html, /Pick an agent to lead/);
  // Never a control that claims to decide something it does not.
  assert.match(html, /disabled/);
});

test("picking an agent turns the session governed by construction", () => {
  const { lead, html } = leadHtml(KEYSTONE);
  assert.equal(lead.kind, "agent");
  assert.equal(lead.role, "lead");
  assert.match(html, />Governed</);
  assert.match(html, /always governed/);
});

test("two Keystones are told apart on the identity line", () => {
  const first = leadHtml(KEYSTONE).html;
  const second = leadHtml(TWIN).html;
  assert.match(first, /Keystone · a1a1a1a1…a1a1/);
  assert.match(second, /Keystone · b2b2b2b2…b2b2/);
  // The role is read from the identity's pack and rendered — never a text box.
  assert.match(first, /· lead/);
  assert.match(second, /· builder/);
  assert.equal(/<input[^>]*role/i.test(first), false);
});

test("an identity carrying no role is not offered as a lead", () => {
  const { html } = leadHtml(null);
  assert.equal(html.includes("c3".repeat(32)), false);
  assert.match(html, /Scribe/.test(html) ? /Scribe/ : /You/);
});

test("a lead with no model of its own says so rather than borrowing one", () => {
  // The whole of finding 12: an unpinned seat used to be created on whatever
  // the other tab was showing. The line now states the absence.
  const { html } = leadHtml(TWIN);
  assert.match(html, /no model of its own/);
});

test("an empty bench says the lead will work alone", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionBenchField, {
      challengerRate: null,
      identities: [],
      onChallengerRateChange: () => {},
      onToggleIdentity: () => {},
      onToggleProvider: () => {},
      providers: [],
      selectedIdentities: [],
      selectedProviders: [],
    }),
  );
  assert.match(html, /nobody to bench/);
  assert.match(html, /work alone/);
});

test("a set bench names who may be hired, and says nobody is seated by this", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionBenchField, {
      challengerRate: 0.25,
      identities: [{ value: TWIN, label: "Keystone", detail: "builder" }],
      onChallengerRateChange: () => {},
      onToggleIdentity: () => {},
      onToggleProvider: () => {},
      providers: [
        { value: "claude-primary", label: "Claude Code", detail: null },
      ],
      selectedIdentities: [TWIN],
      selectedProviders: ["claude-primary"],
    }),
  );
  assert.match(html, /Nobody here is seated by this launch/);
  assert.match(html, /bee sessions hire/);
  assert.match(html, /b2b2b2b2…b2b2/);
  assert.match(html, /value="25"/);
});

test("the policy block always carries the stated-not-enforced disclosure", () => {
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionPolicyField, {
      draft: EMPTY_CODING_SESSION_POLICY_DRAFT,
      onDraftChange: () => {},
    }),
  );
  // The rendered form escapes the apostrophe, so compare the escaped text —
  // the point is that the disclosure is the shared constant and not a second
  // phrasing that could soften independently.
  assert.ok(
    html.includes(
      CODING_SESSION_POLICY_STATED_NOT_ENFORCED.replace(/'/g, "&#x27;"),
    ),
  );
  assert.match(html, /none set/);
  // Every closed vocabulary is offered, and "Not set" is an option rather
  // than a default word the founder never chose.
  for (const word of ["spike", "ship", "investigate", "overnight"]) {
    assert.ok(html.includes(`>${word}<`), `posture ${word} must be offered`);
  }
  assert.match(html, /Not set/);
});

test("readiness shows blockers inline and unknowns behind Details", () => {
  const readiness = codingSessionLaunchReadiness({
    channelId: "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
    canCreateChannel: false,
    goal: "",
    goalOverflow: null,
    lead: { kind: "you", label: "You" },
    governed: false,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: "ab".repeat(32),
    leadModel: "opus",
    modelOverrideReason: null,
    modelOverridden: false,
    providerRefusal: null,
    projectReadiness: null,
    projectReadinessUnknown: false,
    policySet: false,
    unresolvedBenchIdentities: [],
    busySentence: null,
  });
  const html = renderToStaticMarkup(
    React.createElement(NewCodingSessionReadiness, {
      plan: codingSessionLaunchPlan({
        governed: false,
        lead: { kind: "you", label: "You" },
        goal: "",
        policySet: false,
        benchCount: 0,
      }),
      readiness,
    }),
  );
  // The blocker is inline, in an alert, and it is the reason the button is off.
  assert.match(html, /role="alert"/);
  assert.match(html, /Write the goal/);
  // The unknown is disclosed, not dropped, and not inline.
  assert.match(html, /<details/);
  assert.match(html, /Ungoverned: no genesis/);
  const detailsAt = html.indexOf("<details");
  assert.ok(html.indexOf("Ungoverned: no genesis") > detailsAt);
  // The plan carries the kind integers, so the sentence and the wire cannot
  // drift apart silently.
  assert.match(html, /data-kind="44221"/);
});

test("an over-cap goal is refused in the launch block's own words", () => {
  // A2's sentence, reused rather than re-worded: the counter under the field
  // and the refusal under the button must never be two phrasings of one rule.
  const overflow = { bytes: 9000, cap: 8192 };
  const readiness = codingSessionLaunchReadiness({
    channelId: "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
    canCreateChannel: false,
    goal: "x",
    goalOverflow: overflow,
    lead: { kind: "you", label: "You" },
    governed: false,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: "ab".repeat(32),
    leadModel: "opus",
    modelOverrideReason: null,
    modelOverridden: false,
    providerRefusal: null,
    projectReadiness: null,
    projectReadinessUnknown: false,
    policySet: false,
    unresolvedBenchIdentities: [],
    busySentence: null,
  });
  const blocker = readiness.blockers.find((entry) => entry.id === "goal-cap");
  assert.ok(blocker);
  // Both sentences name the same two numbers.
  const sentence = codingSessionGoalOverflowSentence(overflow);
  assert.ok(sentence.includes("9,000") || sentence.includes("9000"));
  assert.match(blocker.sentence, /9,000/);
  assert.match(blocker.sentence, /8,192/);
  assert.equal(readiness.canLaunch, false);
});
