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

  assert.match(markup, /src\/session\.ts/);
  assert.match(markup, /\+1/);
  assert.match(markup, /-1/);
  assert.match(markup, /const state = &#x27;ready&#x27;;/);
  // The footer names the honest provenance of this surface: transcript
  // observation, not a complete workspace diff.
  assert.match(markup, /Observed in transcript activity/);
  // Content only: the host owns the panel landmark and tab chrome.
  assert.doesNotMatch(markup, /<aside/);
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

  assert.match(markup, /No observed changes yet/);
  assert.match(markup, /File edits observed in this session/);
});

test("says how many edits were observed when none named a file", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [],
      unreportedEditCount: 16,
    }),
  );

  assert.match(markup, /16 edits observed · files not reported/);
  assert.match(
    markup,
    /This session&#x27;s provider published the edits without their file paths, so they cannot be listed here\./,
  );
  assert.doesNotMatch(markup, /No observed changes yet/);
});

test("uses the singular for one unnamed edit", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [],
      unreportedEditCount: 1,
    }),
  );

  assert.match(markup, /1 edit observed · files not reported/);
});

test("keeps the empty state when nothing was observed at all", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [],
      unreportedEditCount: 0,
    }),
  );

  assert.match(markup, /No observed changes yet/);
});

test("is labelled Diff and says it is observed edits, not a git diff (SV-24)", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionChangesRail, {
      files: [],
      unreportedEditCount: 2,
    }),
  );
  assert.match(markup, />Diff</);
  assert.match(markup, /observed edits · not a git diff/);
  // The unreported remainder is still disclosed in the body.
  assert.match(markup, /2 edits observed/);
});
