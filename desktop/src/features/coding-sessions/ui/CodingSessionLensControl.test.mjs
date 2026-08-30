import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionLensControl } from "./CodingSessionLensControl.tsx";

test("the lens control is an explicit accessible two-way choice", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionLensControl, {
      lens: "conversation",
      onChange() {},
    }),
  );
  assert.match(markup, /^<fieldset /);
  assert.match(markup, /aria-label="Session lens"/);
  assert.match(markup, /aria-label="Conversation lens" aria-pressed="true"/);
  assert.match(markup, /aria-label="Mission lens" aria-pressed="false"/);
});
