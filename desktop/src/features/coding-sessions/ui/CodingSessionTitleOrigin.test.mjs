import assert from "node:assert/strict";
import { test } from "node:test";

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionTitleAttribution,
  CodingSessionTitleOrigin,
  codingSessionTitleOrigin,
} from "./CodingSessionTitleOrigin.tsx";

const PROVIDER = "D4".repeat(32);
const GENERATED = {
  origin: "generated",
  model: "claude-haiku-4-5",
  signerPubkey: PROVIDER,
};

test("only a generated name has an origin to mark, with its signer lowercased", () => {
  assert.deepEqual(codingSessionTitleOrigin(GENERATED), {
    origin: "generated",
    model: "claude-haiku-4-5",
    signerPubkey: PROVIDER.toLowerCase(),
  });
  assert.equal(
    codingSessionTitleOrigin({ origin: "person", model: null }),
    null,
  );
  // No stated origin is never upgraded into "a model wrote this".
  assert.equal(codingSessionTitleOrigin({}), null);
  assert.equal(codingSessionTitleOrigin(null), null);
  assert.equal(codingSessionTitleOrigin(undefined), null);
  // A blank model is no model, not an empty claim.
  assert.equal(
    codingSessionTitleOrigin({ ...GENERATED, model: "  " }).model,
    null,
  );
});

test("the marker is a muted text-2xs Auto-named with the shared test id", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionTitleOrigin, { name: GENERATED }),
  );
  assert.match(markup, /data-testid="coding-session-title-origin"/);
  assert.match(markup, />Auto-named</);
  assert.match(markup, /text-2xs/);
  assert.match(markup, /text-muted-foreground/);
  assert.doesNotMatch(markup, /text-\[/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionTitleOrigin, {
        name: { origin: "person", model: null, signerPubkey: PROVIDER },
      }),
    ),
    "",
  );
});

function attribution(name) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, enabled: false } },
  });
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(CodingSessionTitleAttribution, {
        name,
        testId: "attr",
      }),
    ),
  );
}

test("the attribution line names the provider and the model, and nothing for a person", () => {
  const markup = attribution(GENERATED);
  assert.match(markup, /data-testid="attr"/);
  assert.match(
    markup,
    /Named automatically from the first message by d4d4d4d4…d4d4 · claude-haiku-4-5\./,
  );
  assert.equal(
    attribution({ origin: "person", model: null, signerPubkey: PROVIDER }),
    "",
  );
});
