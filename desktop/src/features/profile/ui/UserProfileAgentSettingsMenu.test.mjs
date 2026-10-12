import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";
import React from "react";
import { JSDOM } from "jsdom";
const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() => {
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
});
afterEach(async () => (await import("@testing-library/react")).cleanup());
after(() => dom.window.close());
test("profile host-enabled switch dispatches one explicit action and pending/remote controls cannot dispatch", async () => {
  const { render, screen, fireEvent, waitFor } = await import(
    "@testing-library/react"
  );
  const { UserProfileAgentSettingsMenu } = await import(
    "./UserProfileAgentActions.tsx"
  );
  const agent = {
    pubkey: "ab".repeat(32),
    backend: { type: "local" },
    startOnAppLaunch: false,
  };
  let calls = 0;
  const props = {
    managedAgent: agent,
    isPending: false,
    onToggleAutoStart: () => {
      calls++;
    },
  };
  const view = render(React.createElement(UserProfileAgentSettingsMenu, props));
  fireEvent.keyDown(
    screen.getByRole("button", { name: "Open profile settings" }),
    { key: "ArrowDown" },
  );
  const control = await screen.findByRole("switch", {
    name: "Enabled on host",
  });
  assert.equal(control.getAttribute("aria-checked"), "false");
  assert.equal(screen.queryByText("Auto-start"), null);
  fireEvent.click(control);
  assert.equal(calls, 1);
  view.rerender(
    React.createElement(UserProfileAgentSettingsMenu, {
      ...props,
      isPending: true,
    }),
  );
  await waitFor(() =>
    assert.equal(
      screen.getByRole("switch", { name: "Enabled on host" }).disabled,
      true,
    ),
  );
  fireEvent.click(screen.getByRole("switch", { name: "Enabled on host" }));
  assert.equal(calls, 1);
  view.rerender(
    React.createElement(UserProfileAgentSettingsMenu, {
      ...props,
      managedAgent: { ...agent, backend: { type: "provider" } },
    }),
  );
  await waitFor(() =>
    assert.equal(
      screen.queryByRole("switch", { name: "Enabled on host" }),
      null,
    ),
  );
  assert.equal(calls, 1);
});
