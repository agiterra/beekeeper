import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionChangesRail } from "./CodingSessionChangesRail.tsx";

test("renders signed changed files, known stats, and inline diff detail", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [
        {
          path: "src/session.ts",
          filename: "session.ts",
          additions: 1,
          deletions: 1,
          editCount: 1,
          diffs: [
            {
              id: "edit-1",
              path: "src/session.ts",
              filename: "session.ts",
              additions: 1,
              deletions: 1,
              lines: [
                { kind: "remove", text: "const state = 'old';" },
                { kind: "add", text: "const state = 'ready';" },
              ],
            },
          ],
        },
      ],
    }),
  );

  assert.match(markup, /Session changes/);
  assert.match(markup, /src\/session\.ts/);
  assert.match(markup, /\+1/);
  assert.match(markup, /-1/);
  assert.match(markup, /const state = &#x27;ready&#x27;;/);
  assert.match(markup, /From signed session activity/);
});

test("does not invent stats for path-only edits", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [
        {
          path: "README.md",
          filename: "README.md",
          additions: null,
          deletions: null,
          editCount: 1,
          diffs: [],
        },
      ],
    }),
  );

  assert.match(markup, /README\.md/);
  assert.doesNotMatch(markup, />\+0</);
  assert.doesNotMatch(markup, />-0</);
});

test("renders an honest empty state", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, { files: [] }),
  );

  assert.match(markup, /No signed changes yet/);
  assert.match(markup, /File edits reported by this execution/);
});
