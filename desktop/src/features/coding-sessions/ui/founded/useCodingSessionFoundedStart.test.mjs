/**
 * Pressing Start on a founded session.
 *
 * The umbrella exists; neither branch founds anything. Both flush the two
 * text fields first and stop on a refusal. Solo is the durable create joined
 * to the umbrella by its refs, with no title and the prompt as the first
 * turn; Team is the crew launch against the existing umbrella. The worktree
 * is cut before either signs; `rememberWorkspace` and `repoRef` are the
 * click-time draft's; the drafts are forgotten on success only.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";
const FOUNDER = "ab12cd34".repeat(8);
const GENESIS_REF = "f".repeat(64);
const PROVIDER_PUBKEY = "9".repeat(64);
const AGENT = "a".repeat(64);
const PROMPT = "Close ledger item 104.";
const REPO_REF = "30617:owner:beekeeper";

const TARGET = {
  selectionKey: "local:claude-primary",
  channelId: CHANNEL_ID,
  signerPubkey: PROVIDER_PUBKEY,
  provider: {
    providerInstanceRef: "claude-primary",
    runtime: "claude-agent-acp",
    defaultModel: "claude-opus-5",
    allowedModels: ["claude-opus-5"],
  },
  availability: { state: "ready", label: "Claude Code", hint: null },
};

const YOU = { kind: "you", label: "You" };
const FABLE = {
  kind: "agent",
  actor: AGENT,
  label: "Fable",
  role: "lead",
  model: "claude-opus-5",
};

/** Mount the hook over a recorded setup model and press Start once. */
async function press({
  setup: setupOverrides = {},
  prompt = PROMPT,
  name = "",
  flushOutcome = { ok: true },
  submitOutcome = { ok: true },
  launchOutcome = null,
  presses = 1,
} = {}) {
  const { act, renderHook } = await import("@testing-library/react");
  const { useCodingSessionFoundedStart } = await import(
    "./useCodingSessionFoundedStart.ts"
  );
  const calls = [];
  const errors = [];
  const setup = {
    canLaunch: true,
    readiness: { blockers: [], unknowns: [], canLaunch: true },
    markAttempted: () => calls.push(["attempted"]),
    candidates: [
      { pubkey: AGENT, name: "Fable", role: "lead", model: "claude-opus-5" },
    ],
    draft: {
      rememberWorkspace: true,
      repoRef: REPO_REF,
      workspaceSourcePath: null,
    },
    lead: YOU,
    leadModel: "claude-opus-5",
    launch: async (input, fresh) => {
      calls.push(["launch", input, fresh]);
      return (
        launchOutcome ?? {
          ok: true,
          channelId: CHANNEL_ID,
          sessionRef: input.existingUmbrella?.sessionRef ?? null,
          genesisRef: input.existingUmbrella?.genesisRef ?? null,
          seats: [
            {
              seat: input.seats[0],
              target: {
                driver: "claude-agent-acp",
                instanceId: "claude-primary",
                sessionId: "sess-1",
                generation: 1,
              },
            },
          ],
          hireableSeats: [],
          seatsWithoutRolePack: [],
          leadWorkdir: null,
          policyEventId: null,
          failedStep: null,
          failureReason: null,
          steps: [],
        }
      );
    },
    mode: "solo",
    policySet: false,
    refreshRuntimeTarget: async () => {
      calls.push(["refresh"]);
      return TARGET;
    },
    selectedTarget: TARGET,
    setIsPreparing: () => {},
    setLaunchError: (message) => errors.push(["launch", message]),
    setSetupError: (message) => errors.push(["setup", message]),
    submit: async (input) => {
      calls.push(["submit", input]);
      return submitOutcome;
    },
    text: {
      name,
      prompt,
      flush: async () => {
        calls.push(["flush"]);
        return flushOutcome;
      },
      markPromptAttempted: () => calls.push(["promptAttempted"]),
      remember: () => calls.push(["remember"]),
    },
    useWorktree: true,
    workdir: "/repo/primary",
    worktreeName: "fresh-work",
    worktreeSource: "main",
    ...setupOverrides,
  };
  const deps = {
    createWorktree: async (input) => {
      calls.push(["worktree", input]);
      return {
        path: `/tmp/trees/${input.name}`,
        branch: input.name,
        repoRoot: input.workdir,
      };
    },
    clearFoundedDraft: (sessionRef) => calls.push(["clearDraft", sessionRef]),
    autoName: async (input) => {
      calls.push(["autoName", input]);
      return { kind: "published", name: "Generated" };
    },
    autoGoal: async (input) => {
      calls.push(["autoGoal", input]);
      return { kind: "published", goal: "One line." };
    },
  };
  const navigations = [];
  const mounted = renderHook(() =>
    useCodingSessionFoundedStart({
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: GENESIS_REF,
      founderPubkey: FOUNDER,
      projectRef: "30621:owner:beekeeper",
      setup,
      goCodingSession: (...args) => navigations.push(args),
      deps,
    }),
  );
  await act(async () => {
    for (let index = 0; index < presses; index += 1) mounted.result.current();
    // Let the async work settle.
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  mounted.unmount();
  return { calls, errors, navigations, names: calls.map(([name]) => name) };
}

test("Solo: flush first, then the worktree, then one create joined to the umbrella with no title", async () => {
  const run = await press();
  assert.deepEqual(run.names, [
    "flush",
    "worktree",
    "submit",
    "remember",
    "clearDraft",
    "autoName",
    "autoGoal",
  ]);
  const [, worktree] = run.calls[1];
  assert.deepEqual(worktree, {
    workdir: "/repo/primary",
    name: "fresh-work",
    source: "main",
    sessionRef: SESSION_REF,
  });
  const [, submitted] = run.calls[2];
  assert.equal(submitted.sessionRef, SESSION_REF);
  assert.equal(submitted.genesisRef, GENESIS_REF);
  assert.equal(submitted.title, null, "the 44229 was just flushed");
  assert.equal(submitted.initialTurn, PROMPT);
  assert.equal(submitted.workdir, "/tmp/trees/fresh-work");
  assert.equal(submitted.rememberWorkdir, "/repo/primary");
  assert.equal(submitted.rememberWorkspace, true);
  assert.equal(submitted.projectRef, "30621:owner:beekeeper");
  assert.equal(submitted.repoRef, REPO_REF, "the click-time repoRef, not null");
  assert.equal(submitted.seat, null);
  assert.equal(submitted.model, "claude-opus-5");
  assert.deepEqual(run.calls[4], ["clearDraft", SESSION_REF]);
  assert.deepEqual(run.errors, [
    ["launch", null],
    ["setup", null],
  ]);
});

test("Solo passes the draft's rememberWorkspace and repoRef; the repoRef is nulled when the workdir left the reused path", async () => {
  const reused = await press({
    setup: {
      draft: {
        rememberWorkspace: false,
        repoRef: REPO_REF,
        workspaceSourcePath: "/repo/primary",
      },
      useWorktree: false,
    },
  });
  const [, kept] = reused.calls.find(([name]) => name === "submit");
  assert.equal(kept.rememberWorkspace, false);
  assert.equal(
    kept.repoRef,
    REPO_REF,
    "the Where field still names the folder",
  );

  const moved = await press({
    setup: {
      draft: {
        rememberWorkspace: false,
        repoRef: REPO_REF,
        workspaceSourcePath: "/repo/primary",
      },
      useWorktree: false,
      workdir: "/somewhere/else",
    },
  });
  const [, nulled] = moved.calls.find(([name]) => name === "submit");
  assert.equal(nulled.repoRef, null, "never a guess about a different folder");
  assert.equal(nulled.rememberWorkspace, false);
});

test("a refused flush starts nothing and says why", async () => {
  const run = await press({
    flushOutcome: { ok: false, field: "prompt", reason: "goal refused" },
  });
  assert.deepEqual(run.names, ["flush"]);
  assert.deepEqual(run.errors.at(-1), ["launch", "goal refused"]);
  assert.deepEqual(run.navigations, []);
});

test("a blank prompt is the on-press blocker: nothing is flushed, nothing is signed", async () => {
  const run = await press({ prompt: "   " });
  assert.deepEqual(run.names, ["promptAttempted", "attempted"]);
  assert.deepEqual(run.errors, []);
});

test("a refused create keeps the drafts", async () => {
  const run = await press({
    submitOutcome: { ok: false, message: "rate-limited: quota exceeded" },
  });
  assert.deepEqual(run.names, ["flush", "worktree", "submit"]);
});

test("Team: flush, then the launch against the existing umbrella with the prompt as the goal; the launch cuts the worktree itself", async () => {
  const run = await press({
    setup: { mode: "team", lead: FABLE, policySet: true },
  });
  assert.deepEqual(run.names, [
    "flush",
    "refresh",
    "launch",
    "remember",
    "clearDraft",
    "autoName",
  ]);
  const [, input, fresh] = run.calls[2];
  assert.deepEqual(input.existingUmbrella, {
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
  });
  assert.equal(input.channelId, CHANNEL_ID);
  assert.equal(input.goal, PROMPT);
  assert.equal(input.seats.length, 1);
  assert.equal(input.seats[0].actor, AGENT);
  assert.equal(input.seats[0].role, "lead");
  assert.equal(input.seats[0].model, "claude-opus-5");
  assert.equal(input.primaryPersonaId, AGENT);
  assert.equal(input.policySet, true);
  assert.equal(input.workdir, "/repo/primary");
  assert.deepEqual(input.leadWorktree, { name: "fresh-work", source: "main" });
  assert.equal(input.repoRef, REPO_REF, "the click-time repoRef, not null");
  assert.equal(
    fresh,
    TARGET,
    "the click-time runtime is what the launch binds",
  );
  assert.equal(run.navigations.length, 1);
  assert.equal(run.navigations[0][0], CHANNEL_ID);
  assert.deepEqual(run.navigations[0][2], { replace: true });
});

test("Team with no agent picked starts nothing, and says so", async () => {
  const run = await press({
    setup: { mode: "team", lead: { kind: "unset" } },
  });
  assert.deepEqual(run.names, ["flush"]);
  assert.match(run.errors.at(-1)[1], /Pick an agent to lead this session/);
});

test("a failed launch keeps the drafts and says why", async () => {
  const run = await press({
    setup: { mode: "team", lead: FABLE },
    launchOutcome: {
      ok: false,
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      genesisRef: GENESIS_REF,
      seats: [],
      hireableSeats: [],
      seatsWithoutRolePack: [],
      leadWorkdir: null,
      policyEventId: null,
      failedStep: "create:0",
      failureReason: "The lead seat was not created.",
      steps: [],
    },
  });
  assert.deepEqual(run.names, ["flush", "refresh", "launch"]);
  assert.deepEqual(run.errors.at(-1), [
    "launch",
    "The lead seat was not created.",
  ]);
  assert.deepEqual(run.navigations, []);
});

test("a second press in the same window is a no-op (finding 51)", async () => {
  const run = await press({ presses: 2 });
  assert.equal(run.names.filter((name) => name === "submit").length, 1);
  assert.equal(run.names.filter((name) => name === "flush").length, 1);
});

test("nothing is pressed while readiness blocks", async () => {
  const run = await press({ setup: { canLaunch: false } });
  assert.deepEqual(run.names, []);
  assert.deepEqual(run.errors, []);
});

test("a session started with a blank Name is named from its first message after the create is accepted", async () => {
  const { calls, names } = await press({ name: "" });
  const at = names.indexOf("autoName");
  assert.ok(at > names.indexOf("submit"), "the namer runs after the create");
  assert.ok(
    at > names.indexOf("clearDraft"),
    "and after the drafts are forgotten",
  );
  assert.deepEqual(calls[at][1], {
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    founderPubkey: FOUNDER,
    firstMessage: PROMPT,
  });
});

test("a typed Name is never second-guessed by the namer", async () => {
  const { names } = await press({ name: "Typed by hand" });
  assert.equal(names.includes("submit"), true);
  assert.equal(names.includes("autoName"), false);
});

test("a refused create names nothing", async () => {
  const { names } = await press({ name: "", submitOutcome: { ok: false } });
  assert.equal(names.includes("autoName"), false);
});

test("Team: a blank Name is named after the launch too", async () => {
  const { names } = await press({
    name: "",
    setup: { mode: "team", lead: FABLE, policySet: true },
  });
  assert.ok(names.indexOf("autoName") > names.indexOf("launch"));
});

test("Team with no lead: the press marks the attempt, says it under the field, and signs nothing", async () => {
  const { names } = await press({
    setup: {
      mode: "team",
      lead: { kind: "unset" },
      canLaunch: true,
      readiness: {
        blockers: [
          { id: "lead", sentence: "Pick an agent…", surface: "attempt" },
        ],
        unknowns: [],
        canLaunch: false,
      },
    },
  });
  assert.deepEqual(names, ["attempted"]);
});

test("an unnamed worktree with the toggle on is refused on press, before any cut", async () => {
  const { names } = await press({
    setup: {
      worktreeName: "",
      readiness: {
        blockers: [
          {
            id: "worktree-name",
            sentence: "Give the worktree a name…",
            surface: "attempt",
          },
        ],
        unknowns: [],
        canLaunch: false,
      },
    },
  });
  assert.deepEqual(names, ["attempted"]);
  assert.equal(names.includes("worktree"), false);
});

test("Solo summarizes the goal after the create; Team leaves the lead's mission as written", async () => {
  const solo = await press({ name: "Typed" });
  assert.ok(solo.names.indexOf("autoGoal") > solo.names.indexOf("submit"));
  assert.equal(
    solo.names.includes("autoName"),
    false,
    "a typed name is not renamed",
  );
  const team = await press({
    name: "",
    setup: { mode: "team", lead: FABLE, policySet: true },
  });
  assert.equal(team.names.includes("autoGoal"), false);
  assert.equal(team.names.includes("autoName"), true);
});
