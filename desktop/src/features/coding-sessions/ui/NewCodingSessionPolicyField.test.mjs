import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { EMPTY_CODING_SESSION_POLICY_DRAFT } from "@/features/coding-sessions/lib/codingSessionPolicy.ts";
import { NewCodingSessionPolicyField } from "./NewCodingSessionPolicyField.tsx";

function render(draft = EMPTY_CODING_SESSION_POLICY_DRAFT) {
  return renderToStaticMarkup(
    React.createElement(NewCodingSessionPolicyField, {
      draft,
      onDraftChange: () => {},
    }),
  );
}

test("L21 finding 39: the launch form carries gates.verifierRequired at all", () => {
  const html = render();
  assert.match(
    html,
    /data-testid="new-coding-session-policy-verifier-required"/,
    "the one policy field the 44244 fold enforces was the one the form could not set",
  );
});

test("L21: the three states are offered, and 'not set' is one of them", () => {
  const html = render();
  // Three-way, because `verifierRequired` is a nullable boolean on the wire
  // and a two-way control would publish `false` for a founder who never
  // touched it — a stated policy nobody chose.
  assert.match(html, /value="unset"/);
  assert.match(html, /value="true"/);
  assert.match(html, /value="false"/);
});

test("L21: the selected state is the draft's, not a default", () => {
  assert.match(
    render({ ...EMPTY_CODING_SESSION_POLICY_DRAFT, verifierRequired: true }),
    /<option value="true" selected="">/,
  );
  assert.match(
    render({ ...EMPTY_CODING_SESSION_POLICY_DRAFT, verifierRequired: false }),
    /<option value="false" selected="">/,
  );
  assert.match(render(), /<option value="unset" selected="">/);
});

test("L21: the copy says what it enforces, and does not claim the push gate", () => {
  const html = render();
  // What `gates.verifierRequired` actually gates is the 44244 fold's
  // completion check (`coding_session_completion_verification.rs:42`) — the
  // mission cannot complete while a settled report has no verifier's ruling.
  // It is NOT read by the verdict-gated push rule
  // (`coding_session_verdict_admission.rs`, `verdict_admission_fold_context`
  // passes `verifier_required: false` and says so), and a label promising
  // otherwise would be a control lying about what it enforces.
  assert.match(html, /complet/i);
  assert.ok(
    !/lands? this mission's work/i.test(html),
    "the switch does not decide who may land a commit; the repository's rule does",
  );
});
