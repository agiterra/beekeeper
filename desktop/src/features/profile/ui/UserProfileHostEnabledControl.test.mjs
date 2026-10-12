import assert from "node:assert/strict";
import { test, after } from "node:test";
import { JSDOM } from "jsdom";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
Object.assign(globalThis, {
  window: dom.window,
  document: dom.window.document,
  HTMLElement: dom.window.HTMLElement,
  Node: dom.window.Node,
});
after(() => dom.window.close());

test("reported host enabled state renders without desktop launch wording", async () => {
  const React = await import("react");
  const { render, screen, cleanup } = await import("@testing-library/react");
  const { buildOwnerFields, ProfileFieldRows } = await import(
    "./UserProfilePanelFields.tsx"
  );
  const fields = buildOwnerFields({
    includeOperationalFields: true,
    managedAgent: {
      backend: { type: "local" },
      startOnAppLaunch: false,
      status: "stopped",
      name: "Mira",
      pubkey: "ab".repeat(32),
    },
    ownerDisplayName: null,
    ownerHandle: null,
    ownerProfilePubkey: null,
    ownerPubkey: null,
    presenceLoaded: false,
    presenceStatus: undefined,
    relayAgent: undefined,
  });
  const enabled = fields.find(
    (field) => field.testId === "user-profile-start-on-launch",
  );
  render(
    React.createElement(ProfileFieldRows, {
      fields: [enabled],
      variant: "runtime",
    }),
  );
  assert.ok(screen.getByText("Enabled on host"));
  assert.ok(screen.getByText("No"));
  assert.equal(screen.queryByText("Start on launch"), null);
  cleanup();
});
