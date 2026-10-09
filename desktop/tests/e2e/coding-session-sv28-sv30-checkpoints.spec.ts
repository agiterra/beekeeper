import { createHash } from "node:crypto";

import { expect, test, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  CODING_SESSION_CHECKPOINT_SCHEMA,
  CODING_SESSION_CHECKPOINT_TAG_VERSION,
  codingSessionCheckpointSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionCheckpoints";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_CHECKPOINT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// SV-28 / SV-30 (batch C2): a turn's changed files from its signed git
// checkpoint (kind 44231), the observed-transcript fallback, the no-repo and
// outside-turn disclosures, and the Git-backed Diff surface — one turn, the
// whole session, and a turn whose trees live on another computer. The native
// diff is mocked (`src/testing/e2eBridgeCheckpoints.ts`); the checkpoint
// events are real signed 44231s through the mock relay. Scoped shots, gated
// on distinct hashes.

const SHOTS = "test-results/sv28-sv30-checkpoints";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const TREE_0 = `${"0".repeat(39)}1`;
const TREE_1 = "1".repeat(40);
const TREE_2 = "2".repeat(40);
const TREE_3 = "3".repeat(40);
const COMMIT = "c".repeat(40);
const HEAD = "e".repeat(40);

type Target = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

const REPO_SESSION: Target = {
  driver: "claude-agent-acp",
  instanceId: "b1b2c3d4e5f60728",
  sessionId: "da8d6582-0000-4000-8000-000000000028",
  generation: 1,
};
const PLAIN_SESSION: Target = {
  driver: "codex-acp",
  instanceId: "b1b2c3d4e5f60729",
  sessionId: "da8d6582-0000-4000-8000-000000000029",
  generation: 1,
};

let clock = 1_800_700_000;

function signed(kind: number, content: unknown, tags: string[][]) {
  clock += 1;
  return finalizeEvent(
    {
      kind,
      created_at: clock,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(session: Target, title: string): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    {
      schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title,
      agentRef: null,
      provider: session.driver,
      runtime: session.driver,
      model: "sonnet",
      status: "idle",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        promptImage: true,
        context: false,
        diff: false,
        plan: false,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function transcript(
  session: Target,
  seq: number,
  turnId: string,
  item: unknown,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    {
      schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_700_000_000 + seq * 1_000,
      turnId,
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", buildCodingSessionTargetKey(session)],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

type CheckpointBody = {
  turnId: string;
  fromSeq: number;
  throughSeq: number;
  git: Record<string, unknown> | null;
  files?: unknown[];
  unavailable?: { code: string; sentence: string } | null;
};

function checkpoint(session: Target, body: CheckpointBody): RelayEvent {
  const content = {
    schema: CODING_SESSION_CHECKPOINT_SCHEMA,
    session,
    turnId: body.turnId,
    reason: "turn",
    coverage: { fromSeq: body.fromSeq, throughSeq: body.throughSeq },
    git: body.git,
    files: body.files ?? [],
    filesNotListed: 0,
    restorable: false,
    unavailable: body.unavailable ?? null,
    summary: null,
  };
  return signed(KIND_CODING_SESSION_CHECKPOINT, content, [
    ["csck-v", CODING_SESSION_CHECKPOINT_TAG_VERSION],
    ["cs-target", buildCodingSessionTargetKey(session)],
    ["csck-seq", String(body.throughSeq)],
    [
      "csck-key",
      codingSessionCheckpointSemanticKey(session, "turn", body.throughSeq),
    ],
  ]);
}

function git(baseTree: string | null, tree: string, extra = {}) {
  return {
    head: HEAD,
    branch: "main",
    baseTree,
    tree,
    commit: COMMIT,
    outsideTurn: false,
    complete: true,
    omitted: [],
    omittedNotListed: 0,
    ...extra,
  };
}

const file = (
  path: string,
  additions: number | null,
  deletions: number | null,
  status = "modified",
) => ({ path, status, from: null, additions, deletions });

function result(session: Target, seq: number, turnId: string) {
  return transcript(session, seq, turnId, {
    kind: "result",
    subtype: "success",
    isError: false,
    durationMs: 12_000,
    result: "",
    costUsd: 0.02,
  });
}

/** Three turns in one repository: shell-only, observed-only, outside-turn. */
function repoSessionEvents(): RelayEvent[] {
  const s = REPO_SESSION;
  return [
    metadata(s, "Rename the helper"),
    // Turn 1: edits only through the shell — no transcript fold sees them.
    transcript(s, 1, "turn-1", {
      kind: "user_prompt",
      content: "Rename fetchUser to loadUser with sed, no edit tools.",
    }),
    transcript(s, 2, "turn-1", {
      kind: "tool_call",
      tool: {
        toolName: "Bash",
        toolId: "bash-sed",
        input: {
          command: "sed -i '' s/fetchUser/loadUser/g src/api.ts src/user.ts",
        },
      },
    }),
    transcript(s, 3, "turn-1", {
      kind: "tool_result",
      toolId: "bash-sed",
      toolName: "Bash",
      content: "",
      isError: false,
    }),
    transcript(s, 4, "turn-1", {
      kind: "assistant_text",
      text: "Renamed fetchUser to loadUser in both files.",
    }),
    result(s, 5, "turn-1"),
    checkpoint(s, {
      turnId: "turn-1",
      fromSeq: 1,
      throughSeq: 5,
      git: git(TREE_0, TREE_1),
      files: [file("src/api.ts", 2, 2), file("src/user.ts", 1, 1)],
    }),
    // Turn 2: an Edit tool call, and no checkpoint for it.
    transcript(s, 6, "turn-2", {
      kind: "user_prompt",
      content: "Reset the retry counter on close.",
    }),
    transcript(s, 7, "turn-2", {
      kind: "tool_call",
      tool: {
        toolName: "Edit",
        toolKind: "edit",
        toolId: "edit-1",
        input: {
          file_path: "src/useReconnect.ts",
          old_string: "attempts += 1;",
          new_string: "attempts = 0;",
        },
      },
    }),
    transcript(s, 8, "turn-2", {
      kind: "tool_result",
      toolId: "edit-1",
      toolName: "Edit",
      content: "Edited src/useReconnect.ts",
      isError: false,
    }),
    transcript(s, 9, "turn-2", {
      kind: "assistant_text",
      text: "The retry counter now resets when the socket closes.",
    }),
    result(s, 10, "turn-2"),
    // Turn 3: someone edited between turns, and a large file was left out.
    transcript(s, 11, "turn-3", {
      kind: "user_prompt",
      content: "Regenerate the fixtures.",
    }),
    transcript(s, 12, "turn-3", {
      kind: "assistant_text",
      text: "Fixtures regenerated.",
    }),
    result(s, 13, "turn-3"),
    checkpoint(s, {
      turnId: "turn-3",
      fromSeq: 11,
      throughSeq: 13,
      git: git(TREE_2, TREE_3, {
        outsideTurn: true,
        complete: false,
        omitted: [{ path: "fixtures/dump.bin", reason: "too_large" }],
      }),
      files: [
        file("fixtures/users.json", 40, 12),
        file("fixtures/logo.png", null, null, "added"),
      ],
    }),
  ];
}

/** One turn in a directory that is not a git repository. */
function plainSessionEvents(): RelayEvent[] {
  const s = PLAIN_SESSION;
  return [
    metadata(s, "Notes outside git"),
    transcript(s, 1, "turn-1", {
      kind: "user_prompt",
      content: "Write the notes file with the shell.",
    }),
    transcript(s, 2, "turn-1", {
      kind: "assistant_text",
      text: "Wrote notes.txt.",
    }),
    result(s, 3, "turn-1"),
    checkpoint(s, {
      turnId: "turn-1",
      fromSeq: 1,
      throughSeq: 3,
      git: null,
      unavailable: {
        code: "NOT_A_REPOSITORY",
        sentence:
          "The session's working directory is not inside a git repository.",
      },
    }),
  ];
}

const patch = (from: string, to: string) =>
  [`@@ -1 +1 @@`, `-${from}`, `+${to}`].join("\n");

/** Native diff answers by tree pair: local for turn and session; turn 3 remote. */
const DIFF_ANSWERS = {
  [`${TREE_0}..${TREE_1}`]: {
    state: "local",
    checkout: "seat_worktree",
    diff: {
      files: [
        {
          path: "src/api.ts",
          additions: 2,
          deletions: 2,
          patch: [
            "@@ -3,4 +3,4 @@",
            "-export async function fetchUser(id: string) {",
            "+export async function loadUser(id: string) {",
            "   return get(userPath(id));",
            " }",
            "-export const refresh = () => fetchUser(current());",
            "+export const refresh = () => loadUser(current());",
          ].join("\n"),
          truncated: false,
        },
        {
          path: "src/user.ts",
          additions: 1,
          deletions: 1,
          patch: patch(
            "const user = await fetchUser(id);",
            "const user = await loadUser(id);",
          ),
          truncated: false,
        },
      ],
      additions: 3,
      deletions: 3,
      commit_body: null,
    },
    filesNotListed: 0,
  },
  [`${TREE_0}..${TREE_3}`]: {
    state: "local",
    checkout: "seat_worktree",
    diff: {
      files: [
        {
          path: "fixtures/users.json",
          additions: 40,
          deletions: 12,
          patch: patch('"name": "a"', '"name": "b"'),
          truncated: false,
        },
        {
          path: "src/api.ts",
          additions: 2,
          deletions: 2,
          patch: patch("fetchUser", "loadUser"),
          truncated: false,
        },
        {
          path: "src/useReconnect.ts",
          additions: 1,
          deletions: 1,
          patch: patch("attempts += 1;", "attempts = 0;"),
          truncated: false,
        },
        {
          path: "src/user.ts",
          additions: 1,
          deletions: 1,
          patch: patch("fetchUser", "loadUser"),
          truncated: false,
        },
      ],
      additions: 44,
      deletions: 16,
      commit_body: null,
    },
    filesNotListed: 0,
  },
  [`${TREE_2}..${TREE_3}`]: { state: "objects_missing", missing: [TREE_2] },
};

async function openSession(
  page: Page,
  events: RelayEvent[],
  expectText: string,
): Promise<Locator> {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript((answers) => {
    (
      window as Window & { __BEEKEEPER_E2E_CHECKPOINT_DIFF__?: unknown }
    ).__BEEKEEPER_E2E_CHECKPOINT_DIFF__ = answers;
  }, DIFF_ANSWERS);
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Checkpoint provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ name, signedEvents }) => {
      const seedEvent = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seedEvent) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seedEvent({ channelName: name, event });
    },
    { name: channelName, signedEvents: events },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText(expectText);
  return workspace;
}

async function openDiff(page: Page): Promise<Locator> {
  await page.keyboard.press("ControlOrMeta+Alt+KeyB");
  await expect(
    page.getByTestId("coding-session-surface-launcher"),
  ).toBeVisible();
  await page.keyboard.press("d");
  const panel = page.getByTestId("coding-session-surface-panel-diff");
  await expect(panel).toBeVisible();
  return panel;
}

function shooter(page: Page, hashes: Map<string, string>) {
  return async (name: string, locator: Locator) => {
    await expect(locator).toBeVisible();
    await page.mouse.move(2, 2);
    await waitForAnimations(page);
    const png = await locator.screenshot({ path: `${SHOTS}/${name}.png` });
    hashes.set(name, createHash("sha256").update(png).digest("hex"));
  };
}

function expectDistinct(hashes: Map<string, string>) {
  const seen = new Map<string, string>();
  for (const [name, hash] of hashes) {
    expect(
      seen.get(hash),
      `${name} is byte-identical to ${seen.get(hash)}`,
    ).toBeUndefined();
    seen.set(hash, name);
  }
}

const allHashes = new Map<string, string>();

test.describe.configure({ mode: "serial" });

test("SV-28/SV-30: git-backed turn cards and the Diff surface", async ({
  page,
}) => {
  test.setTimeout(90_000);
  const shoot = shooter(page, allHashes);
  const workspace = await openSession(
    page,
    repoSessionEvents(),
    "Fixtures regenerated.",
  );
  const turns = workspace.getByTestId("coding-session-turn");
  await expect(turns).toHaveCount(3);

  // Turn 1: shell-only edits, listed from git.
  const first = turns.nth(0);
  const firstCard = first.getByTestId("coding-session-changed-files");
  await expect(firstCard).toHaveAttribute("data-source", "git");
  await expect(firstCard).toContainText("2 files changed");
  await expect(firstCard).toContainText("From git");
  await firstCard.locator("summary").click();
  await expect(firstCard).toContainText("src/user.ts");
  await shoot("sv28-git-files", first);

  // Turn 2: no checkpoint, so the transcript's edits, labelled as such.
  const second = turns.nth(1);
  const secondCard = second.getByTestId("coding-session-changed-files");
  await expect(secondCard).toHaveAttribute("data-source", "observed");
  await expect(secondCard).toContainText(
    "Observed in transcript · may be incomplete",
  );
  await shoot("sv28-observed-fallback", second);

  // Turn 3: changed between turns, and a file too large to capture.
  const third = turns.nth(2);
  const thirdCard = third.getByTestId("coding-session-changed-files");
  await expect(thirdCard).toContainText("Files also changed outside a turn");
  await expect(thirdCard).toContainText("1 file not captured (too large)");
  await shoot("sv28-outside-turn", third);

  // Diff: latest checkpoint first — its trees are not on this computer.
  const diff = await openDiff(page);
  const surface = diff.getByTestId("coding-session-diff-surface");
  await expect(surface).toHaveAttribute("data-view", "turn");
  await expect(
    surface.getByTestId("coding-session-diff-provenance"),
  ).toContainText("From git · checkpoint 2 of 2");
  await expect(surface.getByTestId("coding-session-diff-remote")).toContainText(
    "Full diff lives on the computer that ran this turn.",
  );
  await expect(
    surface.getByTestId("coding-session-diff-listed-file"),
  ).toHaveCount(2);
  await shoot("sv30-diff-remote", surface);

  // The first checkpoint's diff is local: the patch itself.
  await surface
    .getByTestId("coding-session-diff-turn")
    .selectOption({ index: 0 });
  await expect(surface.getByTestId("coding-session-diff-local")).toContainText(
    "loadUser",
  );
  await shoot("sv30-diff-turn", surface);

  // The whole session: first baseline to latest tree.
  await surface.getByTestId("coding-session-diff-scope-session").click();
  await expect(
    surface.getByTestId("coding-session-diff-provenance"),
  ).toContainText("whole session");
  await expect(surface.getByTestId("coding-session-diff-file")).toHaveCount(4);
  await shoot("sv30-diff-session", surface);
});

test("SV-28: a turn outside any git repository says so", async ({ page }) => {
  test.setTimeout(60_000);
  const shoot = shooter(page, allHashes);
  const workspace = await openSession(
    page,
    plainSessionEvents(),
    "Wrote notes.txt.",
  );
  const turn = workspace.getByTestId("coding-session-turn").first();
  const card = turn.getByTestId("coding-session-changed-files");
  await expect(card).toHaveAttribute("data-source", "unavailable");
  await expect(card).toContainText("No checkpoint · not a git repository");
  await expect(card).not.toContainText("0 files");
  await shoot("sv28-no-repo", turn);

  expectDistinct(allHashes);
  expect(allHashes.size).toBe(7);
});
