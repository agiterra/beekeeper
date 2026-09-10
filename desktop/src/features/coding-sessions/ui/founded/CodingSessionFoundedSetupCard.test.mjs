/**
 * The setup card from a recorded model: what Solo and Team each show, which
 * blockers sit under Start, and when Discard is offered.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

const tauriInternals = {
  // The Where field's children ask the host about the folder on mount; an
  // unanswered host is what the E2E mock bridge also gives them.
  invoke: () => Promise.reject(new Error("no host in this test")),
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const AGENT = "a".repeat(64);

const TARGET = {
  selectionKey: "local:claude-primary",
  channelId: CHANNEL_ID,
  signerPubkey: "9".repeat(64),
  provider: {
    providerInstanceRef: "claude-primary",
    runtime: "claude-agent-acp",
    defaultModel: "claude-opus-5",
    allowedModels: ["claude-opus-5"],
  },
  availability: { state: "ready", label: "Claude Code", hint: null },
};

const CANDIDATE = {
  pubkey: AGENT,
  name: "Fable",
  role: "lead",
  model: "claude-opus-5",
};

const READY = { blockers: [], unknowns: [], canLaunch: true };

const { EMPTY_CODING_SESSION_POLICY_DRAFT } = await import(
  "../../lib/codingSessionPolicy.ts"
);

/** A recorded setup model; every setter records its call. */
function model(overrides = {}, calls = []) {
  const record =
    (name) =>
    (...args) =>
      calls.push([name, ...args]);
  const text = {
    name: "",
    setName: record("setName"),
    nameError: null,
    nameDirty: false,
    commitName: async () => calls.push(["commitName"]),
    suggestion: null,
    autoNameSentence: null,
    requestSuggestionNow: record("requestSuggestionNow"),
    prompt: "Close ledger item 104.",
    setPrompt: record("setPrompt"),
    promptError: null,
    promptDirty: false,
    promptAttempted: false,
    markPromptAttempted: record("markPromptAttempted"),
    goalOverflow: null,
    goalReader: "resolved",
    persistence: { state: "persisted", message: null },
    onPromptKeyDown: () => {},
    commitPrompt: async () => calls.push(["commitPrompt"]),
    remember: record("remember"),
    flush: async () => ({ ok: true }),
    busySentence: null,
    ...(overrides.text ?? {}),
  };
  return {
    mode: "solo",
    setMode: record("setMode"),
    channelReader: "resolved",
    targets: [TARGET],
    selectedTarget: TARGET,
    selectTarget: record("selectTarget"),
    refreshRuntimeTarget: async () => TARGET,
    modelCatalog: { defaultModel: "claude-opus-5", allowedModels: [] },
    effectiveModel: "claude-opus-5",
    seatedModelNote: null,
    selectModel: record("selectModel"),
    leadModel: "claude-opus-5",
    modelOverridden: false,
    overrideReason: "",
    setOverrideReason: record("setOverrideReason"),
    candidates: [CANDIDATE],
    lead: { kind: "you", label: "You" },
    setLeadActor: record("setLeadActor"),
    governed: false,
    benchIdentityOptions: [],
    benchProviderOptions: [],
    benchIdentities: [],
    benchProviders: [],
    toggleBenchIdentity: record("toggleBenchIdentity"),
    toggleBenchProvider: record("toggleBenchProvider"),
    challengerRate: null,
    setChallengerRate: record("setChallengerRate"),
    policyDraft: EMPTY_CODING_SESSION_POLICY_DRAFT,
    setPolicyDraft: record("setPolicyDraft"),
    policySet: false,
    draftSource: "workdir",
    draft: {
      rememberWorkspace: true,
      repoRef: null,
      workspaceSourcePath: null,
    },
    workspaceReuse: null,
    workdir: "/repo/primary",
    setWorkdir: record("setWorkdir"),
    useWorktree: true,
    setUseWorktree: record("setUseWorktree"),
    worktreeName: "",
    setWorktreeName: record("setWorktreeName"),
    worktreeSource: null,
    setWorktreeSource: record("setWorktreeSource"),
    useRoles: false,
    setUseRoles: record("setUseRoles"),
    launchRoles: [],
    teamReadiness: {
      isLoading: false,
      readError: null,
      readiness: null,
      isLaunchPreflighting: false,
      isPreparing: false,
      isScanning: false,
      prepare: async () => {},
      refresh: async () => {},
    },
    beginLoginWatch: record("beginLoginWatch"),
    submit: async () => ({ ok: true }),
    startFresh: record("startFresh"),
    transaction: null,
    lifecycleState: null,
    failureCode: undefined,
    status: null,
    launch: async () => ({ ok: true }),
    isLaunching: false,
    isPublishing: false,
    steps: [],
    launchResult: null,
    readiness: READY,
    plan: [],
    canLaunch: true,
    interactionLocked: false,
    launchError: null,
    setLaunchError: record("setLaunchError"),
    setupError: null,
    setSetupError: record("setSetupError"),
    setIsPreparing: record("setIsPreparing"),
    ...overrides,
    text,
  };
}

async function mount({
  setup,
  onDiscard = () => {},
  onStart = () => {},
  projectRef = null,
} = {}) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { CodingSessionFoundedSetupCard } = await import(
    "./CodingSessionFoundedSetupCard.tsx"
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const tree = (next) =>
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(CodingSessionFoundedSetupCard, {
        channelId: CHANNEL_ID,
        onDiscard,
        onStart,
        projectRef,
        setup,
        ...next,
      }),
    );
  let view = null;
  await act(async () => {
    view = render(tree({}));
  });
  return {
    has: (testid) =>
      document.querySelector(`[data-testid="${testid}"]`) !== null,
    query: (testid) => document.querySelector(`[data-testid="${testid}"]`),
    text: () => document.body.textContent ?? "",
    rerender: async (next) => {
      await act(async () => {
        view.rerender(tree(next));
      });
    },
    click: async (testid) => {
      const element = document.querySelector(`[data-testid="${testid}"]`);
      assert.ok(element, `${testid} must be rendered`);
      await act(async () => {
        element.click();
      });
    },
  };
}

test("Solo: the switch, Name, Initial prompt, runtime and Where — no lead, bench, policy or roles", async () => {
  const page = await mount({ setup: model(), projectRef: "30621:o:p" });
  assert.equal(page.has("coding-session-founded-setup-card"), true);
  assert.match(page.text(), /Set up this session/);
  assert.equal(page.has("coding-session-founded-mode"), true);
  assert.equal(
    page.query("coding-session-founded-mode-solo").dataset.state,
    "checked",
  );
  assert.equal(page.has("coding-session-founded-name"), true);
  assert.equal(
    page.query("coding-session-founded-name").placeholder,
    "Short name for this session",
  );
  assert.equal(page.has("coding-session-founded-prompt"), true);
  assert.match(page.text(), /Initial prompt/);
  assert.equal(page.has("new-coding-session-lead"), false);
  assert.equal(page.has("new-coding-session-bench"), false);
  assert.equal(page.has("new-coding-session-policy"), false);
  assert.equal(page.has("coding-session-use-roles-toggle"), false);
  assert.equal(page.has("coding-session-founded-where"), true);
  assert.match(page.text(), /Where it runs/);
  assert.equal(page.query("coding-session-founded-start").disabled, false);
  assert.equal(page.has("coding-session-founded-discard"), true);
});

test("Team with no agent picked: lead, bench and policy appear; Start is off with the lead sentence", async () => {
  const calls = [];
  const page = await mount({
    setup: model(
      {
        mode: "team",
        governed: true,
        lead: { kind: "unset" },
        readiness: {
          blockers: [
            {
              id: "lead",
              sentence:
                "Pick an agent to lead this session, or switch to Solo.",
            },
          ],
          unknowns: [],
          canLaunch: false,
        },
      },
      calls,
    ),
    projectRef: "30621:o:p",
  });
  assert.equal(
    page.query("coding-session-founded-mode-team").dataset.state,
    "checked",
  );
  assert.equal(page.has("new-coding-session-lead"), true);
  assert.match(page.text(), /Pick an agent…/);
  assert.match(page.text(), /No lead picked yet/);
  assert.doesNotMatch(page.text(), /Governed/);
  assert.equal(page.has("new-coding-session-bench"), true);
  assert.equal(page.has("new-coding-session-policy"), true);
  assert.equal(page.has("coding-session-use-roles-toggle"), true);
  assert.match(page.text(), /Where the lead runs/);
  assert.equal(page.query("coding-session-founded-start").disabled, true);
  assert.equal(page.has("new-coding-session-blocker-lead"), true);
  assert.match(
    page.query("new-coding-session-blocker-lead").textContent,
    /Pick an agent to lead this session, or switch to Solo\./,
  );
  // Roles are a project thing: without a project the block is not shown.
  await page.rerender({ projectRef: null });
  assert.equal(page.has("coding-session-use-roles-toggle"), false);
});

test("the subtitle does not call the session projectless while the channel is unread", async () => {
  const page = await mount({ setup: model({ channelReader: "loading" }) });
  assert.doesNotMatch(page.text(), /belongs to no project/);
  assert.match(page.text(), /Founded\. Nothing runs until Start\./);
  await page.rerender({ setup: model({ channelReader: "resolved" }) });
  assert.match(page.text(), /belongs to no project/);
  await page.rerender({
    setup: model({ channelReader: "resolved" }),
    projectRef: "30621:o:p",
  });
  assert.match(page.text(), /in its channel's project/);
});

test("clicking a mode writes it through the model; the switch is off while locked", async () => {
  const calls = [];
  const page = await mount({ setup: model({}, calls) });
  await page.click("coding-session-founded-mode-team");
  assert.deepEqual(calls.at(-1), ["setMode", "team"]);
  await page.rerender({
    setup: model({ interactionLocked: true }, calls),
  });
  assert.equal(page.query("coding-session-founded-mode").disabled, true);
});

test("a blank prompt keeps Start pressable; pressing it says the sentence under the field", async () => {
  let started = 0;
  const page = await mount({
    setup: model({
      text: { prompt: "" },
      readiness: {
        blockers: [
          {
            id: "goal",
            sentence: "Please specify the initial prompt to start the session.",
            surface: "attempt",
          },
        ],
        unknowns: [],
        canLaunch: false,
      },
      canLaunch: false,
    }),
    onStart: () => {
      started += 1;
    },
  });
  assert.equal(page.query("coding-session-founded-start").disabled, false);
  assert.equal(page.has("new-coding-session-blocker-goal"), false);
  await page.click("coding-session-founded-start");
  assert.equal(started, 1, "the press reaches the Start hook, which marks it");
  await page.rerender({
    setup: model({
      text: { prompt: "", promptAttempted: true },
      readiness: {
        blockers: [
          {
            id: "goal",
            sentence: "Please specify the initial prompt to start the session.",
            surface: "attempt",
          },
        ],
        unknowns: [],
        canLaunch: false,
      },
      canLaunch: false,
    }),
  });
  assert.match(
    page.query("new-coding-session-blocker-goal").textContent,
    /Please specify the initial prompt to start the session\./,
  );
});

test("field refusals sit under their fields, in the relay's words", async () => {
  const page = await mount({
    setup: model({
      text: {
        name: "two lines",
        nameError:
          "Session name must be one line between 1 and 256 UTF-8 bytes.",
        promptError: "forbidden: not a member",
      },
    }),
  });
  assert.match(
    page.query("coding-session-founded-name-error").textContent,
    /one line between 1 and 256/,
  );
  assert.match(
    page.query("coding-session-founded-prompt-error").textContent,
    /forbidden: not a member/,
  );
});

test("Discard is offered only when the host offers it; a busy Start still has its sentence", async () => {
  const page = await mount({ setup: model(), onDiscard: null });
  assert.equal(page.has("coding-session-founded-discard"), false);
  await page.rerender({
    setup: model({
      interactionLocked: true,
      readiness: {
        blockers: [{ id: "busy", sentence: "Publishing the name…" }],
        unknowns: [],
        canLaunch: false,
      },
      canLaunch: false,
    }),
  });
  assert.equal(page.query("coding-session-founded-start").disabled, true);
  assert.match(
    page.query("new-coding-session-blocker-busy").textContent,
    /Publishing the name…/,
  );
});

const AUTO_NAME_SENTENCE =
  "Left blank, it is named from the initial prompt after Start (claude-haiku-4-5).";

test("a blank Name says what happens to it at Start", async () => {
  const page = await mount({
    setup: model({ text: { name: "", autoNameSentence: AUTO_NAME_SENTENCE } }),
  });
  assert.equal(
    page.query("coding-session-founded-name-auto").textContent,
    AUTO_NAME_SENTENCE,
  );
});

test("a typed Name carries no auto-name sentence", async () => {
  const page = await mount({
    setup: model({
      text: { name: "Typed", autoNameSentence: AUTO_NAME_SENTENCE },
    }),
  });
  assert.equal(page.query("coding-session-founded-name-auto"), null);
});

test("no host to ask means no claim about a blank Name either way", async () => {
  const page = await mount({
    setup: model({ text: { name: "", autoNameSentence: null } }),
  });
  assert.equal(page.query("coding-session-founded-name-auto"), null);
});

test("the prompt comes before the optional Name; Solo shows neither Details nor Launch details", async () => {
  const page = await mount({
    setup: model({
      plan: [{ id: "create", kind: 44221, sentence: "One execution." }],
      readiness: {
        blockers: [],
        unknowns: [{ id: "model", sentence: "No model is settled." }],
        canLaunch: true,
      },
    }),
  });
  const prompt = page.query("coding-session-founded-prompt");
  const name = page.query("coding-session-founded-name");
  assert.ok(
    prompt.compareDocumentPosition(name) & Node.DOCUMENT_POSITION_FOLLOWING,
    "the prompt is above the name",
  );
  assert.match(page.text(), /Name \(optional\)/);
  assert.equal(page.query("new-coding-session-launch-details"), null);
  assert.equal(page.query("new-coding-session-readiness-details"), null);
});

test("Team keeps Details, and shows Launch details once a press was refused", async () => {
  const page = await mount({
    setup: model({
      mode: "team",
      attempted: true,
      plan: [{ id: "create", kind: 44221, sentence: "One seat." }],
      readiness: {
        blockers: [],
        unknowns: [{ id: "policy", sentence: "Policy is advisory." }],
        canLaunch: true,
      },
    }),
  });
  assert.notEqual(page.query("new-coding-session-launch-details"), null);
  assert.notEqual(page.query("new-coding-session-readiness-details"), null);
});

test("Team before any press: no Launch details, and no lead sentence until Start is pressed", async () => {
  const leadBlocker = {
    id: "lead",
    sentence: "Pick an agent to lead this session, or switch to Solo.",
    surface: "attempt",
  };
  const quiet = await mount({
    setup: model({
      mode: "team",
      lead: { kind: "unset" },
      attempted: false,
      plan: [{ id: "create", kind: 44221, sentence: "One seat." }],
      readiness: { blockers: [leadBlocker], unknowns: [], canLaunch: false },
    }),
  });
  assert.equal(quiet.query("new-coding-session-launch-details"), null);
  assert.equal(quiet.query("new-coding-session-blocker-lead"), null);
  assert.equal(
    quiet.query("coding-session-founded-start").disabled,
    false,
    "an on-press blocker leaves Start pressable so the press can say it",
  );
});

test("Team after a refused press: the lead sentence sits under its field and Launch details appear", async () => {
  const leadBlocker = {
    id: "lead",
    sentence: "Pick an agent to lead this session, or switch to Solo.",
    surface: "attempt",
  };
  const page = await mount({
    setup: model({
      mode: "team",
      lead: { kind: "unset" },
      attempted: true,
      plan: [{ id: "create", kind: 44221, sentence: "One seat." }],
      readiness: { blockers: [leadBlocker], unknowns: [], canLaunch: false },
    }),
  });
  assert.equal(
    page.query("new-coding-session-blocker-lead").textContent,
    leadBlocker.sentence,
  );
  assert.notEqual(page.query("new-coding-session-launch-details"), null);
});

test("an unnamed worktree is said under the worktree field, only after a press", async () => {
  const blocker = {
    id: "worktree-name",
    sentence:
      "Give the worktree a name — letters and numbers — or turn the worktree off.",
    surface: "attempt",
  };
  const page = await mount({
    setup: model({
      attempted: true,
      readiness: { blockers: [blocker], unknowns: [], canLaunch: false },
    }),
  });
  assert.equal(
    page.query("new-coding-session-blocker-worktree-name").textContent,
    blocker.sentence,
  );
});
