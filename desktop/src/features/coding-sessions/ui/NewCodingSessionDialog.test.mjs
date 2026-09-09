import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { acpAuthMethodsQueryKey } from "@/features/agents/hooks";
import {
  newCodingSessionFormProps,
  NewCodingSessionChannelPicker,
  NewCodingSessionWorkspaceDisclosure,
  NewCodingSessionProjectDestination,
  resolveRefreshedNewCodingSessionTarget,
} from "./NewCodingSessionDialog.tsx";
import {
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "./NewCodingSessionProviderPicker.tsx";
import { describeWorkdirProblem } from "./NewCodingSessionWorkdirField.tsx";
import { WorktreePlanNote } from "./NewCodingSessionWorktreeField.tsx";

/**
 * The Connect button reads the ACP auth-methods query, so anything rendering
 * it needs a QueryClient. Seeding a runtime's methods makes its Connect
 * button appear; an unseeded runtime resolves to no methods and no button —
 * exactly the non-Tauri fallback.
 */
function withQueryClient(element, seedMethodsByRuntime = {}) {
  const client = new QueryClient();
  for (const [runtime, methods] of Object.entries(seedMethodsByRuntime)) {
    client.setQueryData([...acpAuthMethodsQueryKey, runtime], { methods });
  }
  return React.createElement(QueryClientProvider, { client }, element);
}

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

test("click refresh uses newly authenticated runtime models and capabilities", () => {
  const fresh = resolveRefreshedNewCodingSessionTarget({
    catalogs: [],
    channelId: "channel-a",
    localProvider: {
      providerPubkey: "b".repeat(64),
      runtimes: [
        {
          instanceRef: "codex-primary",
          runtime: "codex",
          driver: "codex-acp",
          label: "Codex",
          authState: "ready",
          defaultModel: "default",
          allowedModels: ["default"],
          capabilities: { ...capabilities, context: true, diff: true },
        },
      ],
      modelsByInstanceRef: new Map([
        [
          "codex-primary",
          {
            defaultModel: "gpt-5.6-sol",
            allowedModels: ["gpt-5.6-sol", "gpt-5.6-terra"],
          },
        ],
      ]),
    },
    selectedTargetKey: null,
    selectionExplicit: false,
  });

  assert.equal(fresh.availability.state, "ready");
  assert.equal(fresh.provider.providerInstanceRef, "codex-primary");
  assert.equal(fresh.provider.defaultModel, "gpt-5.6-sol");
  assert.deepEqual(fresh.provider.allowedModels, [
    "gpt-5.6-sol",
    "gpt-5.6-terra",
  ]);
  assert.equal(fresh.provider.capabilities.context, true);
  assert.equal(fresh.provider.capabilities.diff, true);
});

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
  // The catalog's models live in the picker's panel now; the closed trigger
  // states the current one by name, and the list itself is covered by
  // `codingSessionModelPickerModel.test.mjs`.
  assert.match(withModels, />Opus</);
  assert.doesNotMatch(
    withModels,
    /Runtime default \(not named on the record\)/,
  );

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
  // No catalog at all: the trigger says the adapter will choose rather than
  // naming a model nobody selected.
  assert.match(withoutModels, /Runtime default \(not named on the record\)/);
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

  // One control now, so the empty state is a disabled trigger rather than a
  // select holding a fake option.
  assert.match(
    markup,
    /data-testid="coding-session-model-picker"[^>]*disabled/,
  );
  assert.match(markup, /Runtime default \(not named on the record\)/);
});

test("the auth-required state names the exact command that fixes it", () => {
  const markup = renderToStaticMarkup(
    withQueryClient(React.createElement(ProviderLoginNeeded, {})),
  );

  assert.match(markup, /data-testid="new-coding-session-auth-required"/);
  assert.match(markup, /Claude login needed/);
  assert.match(markup, /<code[^>]*>claude<\/code>/);
});

test("the auth-required state adapts to the runtime that failed", () => {
  const codex = renderToStaticMarkup(
    withQueryClient(
      React.createElement(ProviderLoginNeeded, {
        runtime: { runtime: "codex", label: "Codex" },
      }),
    ),
  );
  assert.match(codex, /Codex login needed/);
  assert.match(codex, /<code[^>]*>codex login<\/code>/);
  assert.doesNotMatch(codex, /Claude/);

  // A runtime with no known login command still gets an honest sentence.
  const goose = renderToStaticMarkup(
    withQueryClient(
      React.createElement(ProviderLoginNeeded, {
        runtime: { runtime: "goose", label: "Goose" },
      }),
    ),
  );
  assert.match(goose, /Goose login needed/);
  assert.doesNotMatch(goose, /<code/);
});

test("the auth-required state offers Connect when the adapter advertises a CLI login", () => {
  const markup = renderToStaticMarkup(
    withQueryClient(React.createElement(ProviderLoginNeeded, {}), {
      claude: [
        {
          id: "claude-login",
          name: "Log in with Claude",
          description: null,
          type: "terminal",
          args: [],
          command: [],
          meta: null,
        },
        {
          id: "api-key",
          name: "API key",
          description: null,
          type: "api-key",
          args: [],
          command: [],
          meta: null,
        },
      ],
    }),
  );

  assert.match(
    markup,
    /data-testid="coding-session-runtime-connect-claude-claude-login"/,
  );
  assert.match(markup, /Connect Claude</);
  // The API-key method is filtered out — CLI login is the only offered path.
  assert.doesNotMatch(
    markup,
    /data-testid="coding-session-runtime-connect-claude-api-key"/,
  );
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
    withQueryClient(
      React.createElement(NewCodingSessionProviderPicker, {
        disabled: false,
        model: null,
        onModelChange() {},
        onTargetChange() {},
        selectedTarget: targets[0],
        targets: [...targets, disabledTarget],
      }),
      {
        codex: [
          {
            id: "codex-login",
            name: "Log in with Codex",
            description: null,
            type: "terminal",
            args: [],
            command: [],
            meta: null,
          },
        ],
      },
    ),
  );

  // The disabled `<option>` moved into the picker's rows when provider and
  // model became one control; what a signed-out runtime needs on the *screen*
  // is the remediation, and that is what this asserts. The row-level
  // `ready`/`unavailableNote` rules are covered by
  // `codingSessionModelPickerModel.test.mjs`.
  assert.match(markup, /codex login/);
  // The signed-out runtime's hint row carries its Connect button.
  assert.match(
    markup,
    /data-testid="coding-session-runtime-connect-codex-codex-login"/,
  );
  assert.match(markup, /Connect Codex/);
  // A ready runtime contributes no remediation noise.
  assert.doesNotMatch(markup, /Claude[^<]*\(sign-in needed\)/);
  assert.doesNotMatch(markup, /Connect Claude/);
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

test("the worktree note names the exact directory and branch", () => {
  const markup = renderToStaticMarkup(
    React.createElement(WorktreePlanNote, {
      plan: {
        repoRoot: "/Users/x/Code/beekeeper",
        path: "/Users/x/Code/beekeeper-wt-fix-the-timeout",
        branch: "fix-the-timeout",
        slug: "fix-the-timeout",
        disambiguated: false,
        source: "main",
        problem: null,
      },
    }),
  );

  assert.match(markup, /Creates/);
  assert.match(markup, /beekeeper-wt-fix-the-timeout/);
  assert.match(markup, /on branch/);
  assert.match(markup, /from/);
  assert.match(markup, /main/);
});

test("a worktree without a trunk says it starts from HEAD, not nothing", () => {
  const markup = renderToStaticMarkup(
    React.createElement(WorktreePlanNote, {
      plan: {
        repoRoot: "/Users/x/Code/beekeeper",
        path: "/Users/x/Code/beekeeper-wt-fix-the-timeout",
        branch: "fix-the-timeout",
        slug: "fix-the-timeout",
        disambiguated: false,
        source: null,
        problem: null,
      },
    }),
  );

  assert.match(markup, /from/);
  assert.match(markup, /HEAD/);
});

test("a disambiguated worktree says the name was taken rather than quietly renaming", () => {
  const markup = renderToStaticMarkup(
    React.createElement(WorktreePlanNote, {
      plan: {
        repoRoot: "/Users/x/Code/beekeeper",
        path: "/Users/x/Code/beekeeper-wt-fix-the-timeout-9a3f",
        branch: "fix-the-timeout-9a3f",
        slug: "fix-the-timeout-9a3f",
        disambiguated: true,
        source: "main",
        problem: null,
      },
    }),
  );

  assert.match(markup, /That name was taken/);
  assert.match(markup, /fix-the-timeout-9a3f/);
});

test("a worktree that cannot be planned says why instead of showing a path", () => {
  const markup = renderToStaticMarkup(
    React.createElement(WorktreePlanNote, {
      plan: {
        repoRoot: null,
        path: null,
        branch: null,
        slug: null,
        disambiguated: false,
        source: null,
        problem: "That working directory is not a git checkout.",
      },
    }),
  );

  assert.match(markup, /data-testid="coding-session-worktree-problem"/);
  assert.match(markup, /not a git checkout/);
  assert.doesNotMatch(markup, /Creates/);
});

test("the project destination card never names the transport channel", () => {
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionProjectDestination, {
      projectName: "Buzz Glue",
    }),
  );

  assert.match(markup, /Buzz Glue/);
  assert.match(markup, /visible to\s+project members/);
  // The transport channel is plumbing: members reach sessions through the
  // project, so the card must not mention channels at all — by name or
  // otherwise.
  assert.doesNotMatch(markup, /channel/i);
  assert.doesNotMatch(markup, /#/);
});

// A "New session in this workspace" draft opened from a project session has
// two facts to keep at once, and neither can be re-derived downstream: the
// project's placement and repository binding, which the create signs, and the
// one-off directory the draft reuses. Reusing a folder answers where a session
// runs — never which project it belongs to.

test("a reuse draft keeps the project's placement and repository binding", () => {
  const projectContext = {
    projectId: "p1",
    projectName: "Buzz Glue",
    projectRef: "30621:owner:buzz-glue",
    repoRef: "30617:owner:beekeeper",
    channelId: "c1",
    defaultWorkdir: "/Users/x/Code/repo",
    ensureChannelId: async () => "c1",
  };
  const props = newCodingSessionFormProps({
    channelId: undefined,
    onDone() {},
    projectContext,
    workspaceReuse: { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" },
  });

  assert.equal(props.projectContext.projectRef, "30621:owner:buzz-glue");
  assert.equal(props.projectContext.repoRef, "30617:owner:beekeeper");
  assert.deepEqual(props.workspaceReuse, {
    path: "/Users/x/Code/repo-wt-a",
    branch: "wt-a",
  });
  // The project's canonical checkout is still offered as the fallback it has
  // always been; what changes is that the reuse path outranks it in the form.
  assert.equal(props.projectContext.defaultWorkdir, "/Users/x/Code/repo");
});

test("an ordinary draft carries no workspace at all", () => {
  const props = newCodingSessionFormProps({
    channelId: "c1",
    onDone() {},
    projectContext: null,
    workspaceReuse: null,
  });

  assert.equal(props.workspaceReuse, null);
  assert.equal(props.projectContext, null);
  assert.equal(props.channelId, "c1");
});

test("a reuse draft names the folder and says what reusing it means", () => {
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionWorkspaceDisclosure, {
      workspaceReuse: { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" },
    }),
  );

  assert.match(markup, /data-testid="coding-session-workspace-reuse"/);
  // The absolute path, in full — a base name would not distinguish two
  // checkouts of the same repository.
  assert.match(markup, /\/Users\/x\/Code\/repo-wt-a/);
  assert.match(markup, /wt-a/);
  // Both required sentences, verbatim.
  assert.match(markup, /New conversation; uses these files\./);
  assert.match(markup, /Includes uncommitted changes already in this folder\./);
  // …and nothing that claims the earlier session was continued or taken over.
  assert.doesNotMatch(
    markup,
    /isolated|forked|inherited|continued|taken over/i,
  );
});

test("an ordinary draft discloses no workspace at all", () => {
  const markup = renderToStaticMarkup(
    React.createElement(NewCodingSessionWorkspaceDisclosure, {
      workspaceReuse: null,
    }),
  );

  assert.equal(markup, "");
  assert.doesNotMatch(markup, /coding-session-workspace-reuse/);
  assert.doesNotMatch(markup, /New conversation/);
});
