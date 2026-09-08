import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RolesHeader } from "./RolesHeader.tsx";

const SHA = "9f2e1d0c".padEnd(40, "a");

function props(overrides = {}) {
  return {
    busy: false,
    onInstall: () => {},
    onRecheck: () => {},
    packCount: 2,
    packsSource: {
      origins: ["project"],
      location: "30617:owner:packs",
      sha: SHA,
      shasDiffer: false,
    },
    projectName: "Beekeeper",
    sourceDetail: { checkedAgeSeconds: 300, shippedVersion: null },
    ...overrides,
  };
}

function render(overrides = {}) {
  return renderToStaticMarkup(
    React.createElement(RolesHeader, props(overrides)),
  );
}

test("the header states what this page is and where the instructions came from", () => {
  const html = render();

  assert.match(html, /data-testid="roles-subtitle"/);
  // One line, not the old paragraph — which now lives in Technical details.
  assert.match(
    html,
    />What each role is for, its project participants, and the versions available here or reported by agents\.</,
  );
  assert.doesNotMatch(html, /Each role is a set of instructions an agent/);
  assert.match(html, /data-testid="packs-source-sentence"/);
  assert.match(
    html,
    /Instructions come from the project&#x27;s repository at 9f2e1d0c · checked 5m ago/,
  );
  // The full commit stays in the tooltip; the sentence shows eight characters.
  assert.match(html, new RegExp(`title="${SHA}"`));
});

test("a project with no role instructions gets a sentence, not an empty line", () => {
  const html = render({
    packCount: 0,
    packsSource: {
      origins: [],
      location: null,
      sha: null,
      shasDiffer: false,
    },
  });
  assert.match(html, /No role instructions are available for Beekeeper\./);
});

test("Check again is offered while idle and disabled while a read is in flight", () => {
  const idle = render();
  assert.match(idle, /data-testid="roles-recheck"/);
  assert.match(idle, />Check again</);
  assert.doesNotMatch(idle, /data-testid="roles-recheck"[^>]*disabled/);

  const busy = render({ busy: true });
  assert.match(busy, /data-testid="roles-recheck"[^>]*disabled/);
  assert.match(busy, />Checking…</);
  assert.doesNotMatch(busy, />Check again</);
});

test("Check again re-reads on click, and does nothing while it is disabled", async () => {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<div id='root'></div>");
  const oldWindow = globalThis.window;
  const oldDocument = globalThis.document;
  const oldAct = globalThis.IS_REACT_ACT_ENVIRONMENT;
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { createRoot } = await import("react-dom/client");
  const root = createRoot(document.getElementById("root"));
  let rechecks = 0;
  try {
    await React.act(async () =>
      root.render(
        React.createElement(
          RolesHeader,
          props({ onRecheck: () => (rechecks += 1) }),
        ),
      ),
    );
    const button = () =>
      document.querySelector('[data-testid="roles-recheck"]');
    assert.equal(button().disabled, false);
    await React.act(async () => button().click());
    assert.equal(rechecks, 1);

    await React.act(async () =>
      root.render(
        React.createElement(
          RolesHeader,
          props({ busy: true, onRecheck: () => (rechecks += 1) }),
        ),
      ),
    );
    assert.equal(button().disabled, true);
    assert.equal(button().textContent, "Checking…");
    await React.act(async () => button().click());
    assert.equal(rechecks, 1);
  } finally {
    await React.act(async () => root.unmount());
    dom.window.close();
    globalThis.window = oldWindow;
    globalThis.document = oldDocument;
    globalThis.IS_REACT_ACT_ENVIRONMENT = oldAct;
  }
});

test("Install roles keeps its label and its own testid beside Check again", () => {
  const html = render();
  assert.match(html, /data-testid="project-packs-install"/);
  assert.match(html, />Install roles</);
});

test("the two controls are quiet: a ghost recheck and an outline install, both small", () => {
  const html = render();
  // `size="sm"` on both, so the controls sit under the title rather than
  // competing with it.
  assert.equal((html.match(/h-8 px-3 text-xs/g) ?? []).length, 2);
  // Ghost for the repeatable read, outline for the one that opens a dialog.
  assert.match(
    html,
    /hover:bg-accent hover:text-accent-foreground h-8[^"]*" data-testid="roles-recheck"/,
  );
  assert.match(
    html,
    /border border-input\/40[^"]*" data-testid="project-packs-install"/,
  );
});
