import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionSeatBeeLine } from "./CodingSessionSeatBeeLine.tsx";

const BUNDLED = {
  path: "/Applications/Beekeeper.app/Contents/MacOS/bee",
  source: "bundled",
  version: "0.1.0",
  sha: "23728227b",
  dirty: false,
};

test("the seat's build is named in words the reader can copy", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatBeeLine, { stamp: BUNDLED }),
  );
  assert.match(markup, /data-testid="coding-session-seat-bee"/);
  assert.match(markup, /bee 23728227b \(bundled\)/);
  // The full reading stays reachable when the chip truncates it.
  assert.match(markup, /title="bee 23728227b \(bundled\)"/);
  assert.match(markup, /text-2xs/);
  assert.match(markup, /text-muted-foreground/);
});

test("a PATH build says where it was found, and that it is dirty", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatBeeLine, {
      stamp: {
        path: "/Users/brian/Projects/beekeeper/beekeeper/target/debug/bee",
        source: "path",
        version: "0.1.0",
        sha: "07c470be0",
        dirty: true,
      },
    }),
  );
  assert.match(
    markup,
    /bee 07c470be0-dirty \(found on PATH: \/Users\/brian\/Projects\/beekeeper\/beekeeper\/target\/debug\)/,
  );
});

test("an unparsed --version reads unknown, never blank", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatBeeLine, {
      stamp: { ...BUNDLED, version: null, sha: null, dirty: null },
    }),
  );
  assert.match(markup, /bee build unknown/);
});

test("a seat whose host published no stamp renders nothing at all", () => {
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionSeatBeeLine, { stamp: null }),
    ),
    "",
  );
});

test("the line never carries state in colour or a frozen pixel size", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSeatBeeLine, { stamp: BUNDLED }),
  );
  assert.doesNotMatch(markup, /#[0-9a-fA-F]{3,8}\b/);
  assert.doesNotMatch(markup, /text-\[/);
});
