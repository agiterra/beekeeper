import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  NewCodingSessionChannelPicker,
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "./NewCodingSessionScreen.tsx";
import { describeWorkdirProblem } from "./NewCodingSessionWorkdirField.tsx";

const capabilities = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: true,
  context: false,
  diff: false,
  plan: true,
};

const targets = [
  {
    selectionKey: "claude-target",
    channelId: "channel-a",
    signerPubkey: "a".repeat(64),
    provider: {
      providerInstanceRef: "0123456789abcdef",
      driver: "claude-agent-acp",
      runtime: "claude-agent-acp",
      defaultModel: "sonnet",
      allowedModels: ["sonnet", "opus"],
      capabilities,
    },
  },
];

test("the channel picker lists member channels and says where the transcript lands", () => {
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionChannelPicker, {
      channels: [
        { id: "channel-a", name: "engineering" },
        { id: "channel-b", name: "random" },
      ],
      disabled: false,
      onChange() {},
      value: "channel-b",
    }),
  );

  assert.match(markup, /data-testid="new-coding-session-channel"/);
  assert.match(markup, /#engineering/);
  assert.match(markup, /#random/);
  assert.match(markup, /visible to this channel&#x27;s members/);
});

test("the provider picker offers the catalog's models, and defers when it has none", () => {
  const withModels = renderToStaticMarkup(
    React.createElement(NewCodingSessionProviderPicker, {
      disabled: false,
      model: "opus",
      onModelChange() {},
      onTargetChange() {},
      selectedTarget: targets[0],
      targets,
    }),
  );
  assert.match(withModels, />sonnet</);
  assert.match(withModels, />opus</);
  assert.doesNotMatch(withModels, /Provider default/);

  const uncatalogued = {
    ...targets[0],
    provider: { ...targets[0].provider, allowedModels: [] },
  };
  const withoutModels = renderToStaticMarkup(
    React.createElement(NewCodingSessionProviderPicker, {
      disabled: false,
      model: null,
      onModelChange() {},
      onTargetChange() {},
      selectedTarget: uncatalogued,
      targets: [uncatalogued],
    }),
  );
  assert.match(withoutModels, /Provider default/);
});

test("an empty provider list disables selection rather than pretending to offer one", () => {
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionProviderPicker, {
      disabled: false,
      model: null,
      onModelChange() {},
      onTargetChange() {},
      selectedTarget: null,
      targets: [],
    }),
  );

  assert.match(markup, /No provider available/);
  assert.match(
    markup,
    /data-testid="new-coding-session-provider"[^>]*disabled/,
  );
});

test("the auth-required state names the exact command that fixes it", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ProviderLoginNeeded, {}),
  );

  assert.match(markup, /data-testid="new-coding-session-auth-required"/);
  assert.match(markup, /Claude login needed/);
  assert.match(markup, /<code[^>]*>claude<\/code>/);
});

test("the auth-required state adapts to the runtime that failed", () => {
  const codex = renderToStaticMarkup(
    React.createElement(ProviderLoginNeeded, {
      runtime: { runtime: "codex", label: "Codex" },
    }),
  );
  assert.match(codex, /Codex login needed/);
  assert.match(codex, /<code[^>]*>codex login<\/code>/);
  assert.doesNotMatch(codex, /Claude/);

  // A runtime with no known login command still gets an honest sentence.
  const goose = renderToStaticMarkup(
    React.createElement(ProviderLoginNeeded, {
      runtime: { runtime: "goose", label: "Goose" },
    }),
  );
  assert.match(goose, /Goose login needed/);
  assert.doesNotMatch(goose, /<code/);
});

test("a runtime that is not ready renders disabled with an honest hint", () => {
  const disabledTarget = {
    selectionKey: "codex-target",
    channelId: "channel-a",
    signerPubkey: "a".repeat(64),
    provider: {
      providerInstanceRef: "codex-primary",
      driver: "codex-acp",
      runtime: "codex",
      defaultModel: "default",
      allowedModels: ["default"],
      capabilities,
    },
    availability: {
      state: "needs_auth",
      label: "Codex",
      hint: "Codex is not signed in on this computer. Run `codex login` in a terminal, complete the login, then try again.",
    },
  };
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionProviderPicker, {
      disabled: false,
      model: null,
      onModelChange() {},
      onTargetChange() {},
      selectedTarget: targets[0],
      targets: [...targets, disabledTarget],
    }),
  );

  assert.match(markup, /<option disabled[^>]*>Codex[^<]*\(sign-in needed\)</);
  assert.match(markup, /codex login/);
  // The ready target stays selectable with no suffix ceremony.
  assert.match(markup, /value="claude-target"/);
  assert.doesNotMatch(markup, /Claude[^<]*\(sign-in needed\)/);
});

test("the working-directory field names the exact reason a path will not work", () => {
  assert.equal(describeWorkdirProblem("", null), null);
  assert.equal(
    describeWorkdirProblem("/src/buzz", null),
    null,
    "no validation yet is not a problem to report",
  );
  assert.match(
    describeWorkdirProblem("src/buzz", {
      exists: true,
      isDir: true,
      isAbsolute: false,
    }),
    /absolute path/,
  );
  assert.match(
    describeWorkdirProblem("/src/gone", {
      exists: false,
      isDir: false,
      isAbsolute: true,
    }),
    /No such directory/,
  );
  assert.match(
    describeWorkdirProblem("/src/file.txt", {
      exists: true,
      isDir: false,
      isAbsolute: true,
    }),
    /a file, not a directory/,
  );
  assert.equal(
    describeWorkdirProblem("/src/buzz", {
      exists: true,
      isDir: true,
      isAbsolute: true,
    }),
    null,
  );
});
