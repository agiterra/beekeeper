import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_TITLE_MODE_LABELS,
  chooseCodingSessionTitleMode,
  codingSessionHostMismatchLine,
  codingSessionNamingDraftAfterError,
  codingSessionTitleModeApplyOnChoose,
  codingSessionTitleModeReset,
  codingSessionTitleModeDisclosure,
  codingSessionTitleModeSaveInput,
  codingSessionTitleModeUnsaved,
  namerDisclosure,
} from "./codingSessionTitleModeCopy.ts";
import { namingModelConsulted } from "../../../shared/api/tauriCodingSessionNaming.ts";

const base = {
  titleMode: "agent",
  provider: "off",
  baseUrl: "",
  model: "",
  hasApiKey: false,
};

test("the three choices read as D9 names them", () => {
  assert.deepEqual(CODING_SESSION_TITLE_MODE_LABELS, {
    agent: "Generate with the session's agent",
    "my-model": "Use my naming model",
    off: "Off",
  });
});

test("agent mode names the machine, the account, and the env switch", () => {
  const [line, ...rest] = codingSessionTitleModeDisclosure({
    mode: "agent",
    provider: "off",
    baseUrl: "",
  });
  assert.equal(rest.length, 0);
  assert.match(line, /on the computer it runs on/);
  assert.match(line, /same account that already received the message/);
  assert.match(line, /nothing new leaves this computer/);
  assert.match(line, /another computer follows that computer's setting/);
  assert.match(
    line,
    /BEEKEEPER_CSP_AUTO_TITLE=off, or a runtime configured with no title model, titles nothing/,
  );
});

test("my-model mode names the endpoint and says nothing is titled after Start", () => {
  const lines = codingSessionTitleModeDisclosure({
    mode: "my-model",
    provider: "openai-compatible",
    baseUrl: " http://127.0.0.1:11434/v1 ",
  });
  assert.match(lines[0], /sent to http:\/\/127\.0\.0\.1:11434\/v1 every few/);
  assert.equal(
    lines[1],
    "Your model suggests a name in the Name field while you write; nothing is titled after Start.",
  );
  assert.match(lines[2], /agent host titles nothing/);
  assert.match(
    codingSessionTitleModeDisclosure({
      mode: "my-model",
      provider: "anthropic",
      baseUrl: "",
    })[0],
    /api\.anthropic\.com/,
  );
});

test("off mode says nothing is sent and which machine it governs", () => {
  const lines = codingSessionTitleModeDisclosure({
    mode: "off",
    provider: "anthropic",
    baseUrl: "",
  });
  assert.equal(
    lines[0],
    "Nothing is sent anywhere for a title; sessions stay untitled until someone names them.",
  );
  assert.match(lines[1], /this computer's agent host/);
});

test("a namer with no endpoint chosen admits nothing is sent", () => {
  assert.match(namerDisclosure("off", ""), /nothing is sent/);
  assert.match(namerDisclosure("openai-compatible", ""), /the API URL below/);
});

test("an unsaved choice says the stored mode is still in force", () => {
  assert.equal(codingSessionTitleModeUnsaved("agent", "agent"), null);
  assert.match(
    codingSessionTitleModeUnsaved("agent", "off"),
    /still on “Generate with the session's agent”/,
  );
});

test("choosing my-model with no endpoint starts from the local-capable adapter", () => {
  assert.deepEqual(chooseCodingSessionTitleMode(base, "my-model"), {
    ...base,
    titleMode: "my-model",
    provider: "openai-compatible",
  });
  const anthropic = { ...base, provider: "anthropic" };
  assert.equal(
    chooseCodingSessionTitleMode(anthropic, "my-model").provider,
    "anthropic",
  );
  assert.equal(chooseCodingSessionTitleMode(base, "off").provider, "off");
});

test("only my-model saves the endpoint; the others leave it as stored", () => {
  const stored = {
    ...base,
    titleMode: "my-model",
    provider: "anthropic",
    model: "m",
  };
  const draftOff = {
    ...stored,
    titleMode: "off",
    provider: "openai-compatible",
  };
  assert.deepEqual(
    codingSessionTitleModeSaveInput({ draft: draftOff, stored, apiKey: "k" }),
    { titleMode: "off", provider: "anthropic" },
  );
  assert.deepEqual(
    codingSessionTitleModeSaveInput({ draft: stored, stored, apiKey: "" }),
    {
      titleMode: "my-model",
      provider: "anthropic",
      baseUrl: "",
      model: "m",
    },
  );
  assert.equal(
    codingSessionTitleModeSaveInput({ draft: stored, stored, apiKey: "k" })
      .apiKey,
    "k",
  );
});

test("the naming model is consulted only in my-model with an endpoint", () => {
  assert.equal(namingModelConsulted(null), false);
  assert.equal(namingModelConsulted(base), false);
  assert.equal(namingModelConsulted({ ...base, provider: "anthropic" }), false);
  assert.equal(namingModelConsulted({ ...base, titleMode: "my-model" }), false);
  assert.equal(
    namingModelConsulted({
      ...base,
      titleMode: "my-model",
      provider: "anthropic",
    }),
    true,
  );
  assert.equal(
    namingModelConsulted({ ...base, titleMode: "off", provider: "anthropic" }),
    false,
  );
});

test("choosing agent or off applies at once and leaves the endpoint as stored", () => {
  const stored = { ...base, provider: "anthropic", model: "m" };
  const draft = { ...stored, provider: "openai-compatible" };
  assert.deepEqual(
    codingSessionTitleModeApplyOnChoose({ stored, draft, mode: "off" }),
    { titleMode: "off", provider: "anthropic" },
  );
  assert.deepEqual(
    codingSessionTitleModeApplyOnChoose({ stored, draft, mode: "agent" }),
    { titleMode: "agent", provider: "anthropic" },
  );
  assert.equal(
    codingSessionTitleModeApplyOnChoose({ stored: null, draft, mode: "off" }),
    null,
  );
});

test("my-model applies at once only with a stored endpoint the fields still show", () => {
  assert.equal(
    codingSessionTitleModeApplyOnChoose({
      stored: base,
      draft: base,
      mode: "my-model",
    }),
    null,
  );
  const stored = {
    ...base,
    titleMode: "off",
    provider: "anthropic",
    model: "m",
  };
  assert.deepEqual(
    codingSessionTitleModeApplyOnChoose({
      stored,
      draft: stored,
      mode: "my-model",
    }),
    { titleMode: "my-model", provider: "anthropic" },
  );
  assert.equal(
    codingSessionTitleModeApplyOnChoose({
      stored,
      draft: { ...stored, model: "other" },
      mode: "my-model",
    }),
    null,
  );
});

test("reset to default is offered only off the default", () => {
  assert.equal(codingSessionTitleModeReset(null), null);
  assert.equal(codingSessionTitleModeReset(base), null);
  assert.deepEqual(
    codingSessionTitleModeReset({
      ...base,
      titleMode: "my-model",
      provider: "anthropic",
    }),
    { titleMode: "agent", provider: "anthropic" },
  );
});

test("after a failed write the key state, and for a mode change the mode, are the stored ones", () => {
  const draft = {
    ...base,
    titleMode: "my-model",
    provider: "anthropic",
    model: "typed",
    hasApiKey: true,
  };
  const fresh = {
    ...base,
    hasApiKey: false,
    hostModeMismatch: "this computer's agent host is not on agent: x",
  };
  const kept = codingSessionNamingDraftAfterError(draft, fresh, false);
  assert.equal(kept.hasApiKey, false);
  assert.equal(kept.titleMode, "my-model");
  assert.equal(kept.model, "typed");
  assert.equal(kept.hostModeMismatch, fresh.hostModeMismatch);
  const reverted = codingSessionNamingDraftAfterError(draft, fresh, true);
  assert.equal(reverted.titleMode, "agent");
  assert.equal(reverted.model, "typed");
  assert.deepEqual(
    codingSessionNamingDraftAfterError(null, fresh, true),
    fresh,
  );
});

test("a host that was not told stays on screen, naming the saved mode", () => {
  assert.equal(codingSessionHostMismatchLine(null), null);
  assert.equal(codingSessionHostMismatchLine(base), null);
  assert.equal(
    codingSessionHostMismatchLine({ ...base, hostModeMismatch: null }),
    null,
  );
  const line = codingSessionHostMismatchLine({
    ...base,
    titleMode: "off",
    hostModeMismatch: "this computer's agent host is not on off: p is on agent",
  });
  assert.match(
    line,
    /^“Off” is saved on this computer, but this computer's agent host is not on off/,
  );
  assert.match(line, /may title sessions differently/);
});
