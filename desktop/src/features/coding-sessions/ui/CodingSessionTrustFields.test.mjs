import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { LOCAL_PROVIDER_TRUST_LABEL } from "../lib/codingSessionTrust.ts";
import { CodingSessionTrustFields } from "./CodingSessionTrustFields.tsx";

const KEY_A = "a".repeat(64);
const KEY_B = "b".repeat(64);

function render(props) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionTrustFields, {
      onChange() {},
      ...props,
    }),
  );
}

test("an empty allowlist says so instead of rendering a phantom row", () => {
  const markup = render({ entries: [] });
  assert.match(markup, /data-testid="coding-session-trust-empty"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-trust-pubkey"/);
});

test("each trusted key renders an editable key and name pair", () => {
  const markup = render({
    entries: [
      { pubkey: KEY_A, label: "Build box" },
      { pubkey: KEY_B, label: "" },
    ],
  });
  assert.equal(
    markup.match(/data-testid="coding-session-trust-pubkey"/g).length,
    2,
  );
  assert.equal(
    markup.match(/data-testid="coding-session-trust-remove"/g).length,
    2,
  );
  assert.match(markup, new RegExp(`value="${KEY_A}"`));
  assert.match(markup, /value="Build box"/);
});

test("the explainer names the auto-added local entry and what removing it costs", () => {
  const markup = render({ entries: [] });
  assert.match(markup, /data-testid="coding-session-trust-explainer"/);
  assert.match(markup, /added automatically for this computer/);
  assert.match(markup, /provisioned again/);
});

test("the local provider row is flagged without being made unremovable", () => {
  const markup = render({
    entries: [{ pubkey: KEY_A, label: LOCAL_PROVIDER_TRUST_LABEL }],
  });
  assert.match(markup, /data-testid="coding-session-trust-local-note"/);
  assert.match(markup, /disables session ingest/);
  // Revoking trust in your own provider stays possible.
  assert.doesNotMatch(
    markup,
    /data-testid="coding-session-trust-remove"[^>]*disabled/,
  );
});

test("a malformed key renders its own inline error rather than waiting for the backend", () => {
  const markup = render({ entries: [{ pubkey: "abc", label: "" }] });
  assert.match(markup, /data-testid="coding-session-trust-error"/);
  assert.match(markup, /64 hexadecimal/);
});

test("a duplicate key is reported on the later row only", () => {
  const markup = render({
    entries: [
      { pubkey: KEY_A, label: "one" },
      { pubkey: KEY_A, label: "two" },
    ],
  });
  assert.equal(
    markup.match(/data-testid="coding-session-trust-error"/g).length,
    1,
  );
  assert.match(markup, /already trusted/);
});

test("a saving round-trip disables the inputs instead of dropping them", () => {
  const markup = render({
    disabled: true,
    entries: [{ pubkey: KEY_A, label: "Build box" }],
  });
  assert.match(
    markup,
    /data-testid="coding-session-trust-pubkey"[^>]*disabled/,
  );
  assert.match(markup, /data-testid="coding-session-trust-add"[^>]*disabled/);
});
