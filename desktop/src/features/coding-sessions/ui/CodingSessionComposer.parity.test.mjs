/**
 * Session view parity, Wave A lane A4 (SV-17, SV-18, SV-19): the immersive
 * composer's sandbox chip, provider mark and attach icon.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionComposer } from "./CodingSessionComposer.tsx";

const target = {
  driver: "provider-neutral",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 2,
};

function controlContext(overrides = {}) {
  return {
    capabilities: null,
    model: "claude-opus-5",
    providerLabel: "Claude Code",
    runtimeLabel: "Claude Code",
    status: { kind: "idle", label: "Idle" },
    ...overrides,
  };
}

function render(props = {}) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: false,
      channelId: "channel-1",
      channelAccess: { kind: "member" },
      immersive: true,
      isWorking: false,
      target,
      variant: "floating",
      controlContext: controlContext(),
      ...props,
    }),
  );
}

test("SV-19: attach is an icon immediately before send, not a labelled row", () => {
  const markup = render({ canAttachImages: true });
  const actions = markup.slice(
    markup.indexOf('data-testid="coding-session-composer-actions"'),
  );
  const attachAt = actions.indexOf(
    'data-testid="coding-session-composer-attach"',
  );
  const sendAt = actions.indexOf(
    'data-testid="coding-session-composer-primary"',
  );
  assert.ok(attachAt > 0, "attach sits among the composer actions");
  assert.ok(sendAt > attachAt, "attach comes before send");
  assert.doesNotMatch(markup, />Image</);
  // An empty draft spends no row on the attachment strip.
  assert.doesNotMatch(
    markup,
    /data-testid="coding-session-composer-attachments"/,
  );
});

test("SV-19: the placeholder claims only what this composer does", () => {
  const withImages = render({ canAttachImages: true });
  assert.match(
    withImages,
    /placeholder="Ask anything, or paste or drop an image…"/,
  );
  const withoutImages = render({ canAttachImages: false });
  assert.match(withoutImages, /placeholder="Ask anything…"/);
  for (const markup of [withImages, withoutImages]) {
    const placeholder = markup.match(/placeholder="([^"]*)"/)?.[1] ?? "";
    assert.doesNotMatch(placeholder, /[@$/]/);
  }
});

test("SV-19: without the capability the icon is still shown, disabled", () => {
  const markup = render({ canAttachImages: false });
  assert.match(markup, /coding-session-composer-attach"[^>]*disabled/);
});

test("SV-18: the identity chip leads with the provider's mark", () => {
  const claude = render();
  const chip = claude.slice(
    claude.indexOf('data-testid="coding-session-control-identity"'),
  );
  assert.ok(
    chip.indexOf('data-provider-mark="claude"') <
      chip.indexOf("Claude Code · "),
    "the mark comes before the model name",
  );
  const codex = render({
    controlContext: controlContext({
      model: "gpt-5-codex",
      providerLabel: "Codex",
      runtimeLabel: "Codex",
    }),
  });
  // The gallery's neutral terminal glyph: the OpenAI mark is not bundled
  // (desktop/public/harness-logos/CREDITS.md), so none is guessed at.
  assert.match(codex, /data-provider-mark="codex"/);
  assert.doesNotMatch(codex, /data-provider-mark="generic"/);
});

test("SV-17: the deck carries the sandbox chip when the workspace supplies one", () => {
  const without = render();
  assert.doesNotMatch(without, /coding-session-control-sandbox/);
  const full = render({
    controlContext: controlContext({
      sandbox: {
        report: { state: "full-access", boundaryText: "x", isolation: [] },
        local: null,
      },
    }),
  });
  assert.match(full, /data-testid="coding-session-control-sandbox"/);
  assert.match(full, />Full access</);
});
