import assert from "node:assert/strict";
import test from "node:test";

import * as copy from "./codingSessionWorkspaceReuseCopy.ts";

/**
 * The fixtures are literal resolutions, written out here rather than produced
 * by `resolveWorkspaceReuse`. Copy is asserted against the shape it is
 * promised, so a change in the resolver's internals cannot quietly re-word a
 * menu, and this file compiles on its own.
 */
const AVAILABLE = {
  availability: "available",
  path: "/Users/brian/Projects/beekeeper/repo-wt-a",
  branch: "work/contextual-sessions-fable",
  branchSource: "recorded",
  alsoHere: [],
  sentence:
    "This session's directory is on this computer, on work/contextual-sessions-fable.",
};

const AVAILABLE_LIVE_SHARED = {
  ...AVAILABLE,
  branch: "main",
  branchSource: "live",
  alsoHere: ["session-b", "session-c"],
};

const MISSING = {
  availability: "missing",
  path: "C:\\Users\\brian\\code\\repo-wt-a",
  branch: "main",
  branchSource: "recorded",
  alsoHere: [],
  sentence: "No such directory on this computer.",
};

const UNRECORDED = {
  availability: "unrecorded",
  path: null,
  branch: null,
  branchSource: null,
  alsoHere: [],
  sentence: "This computer recorded no directory for this session.",
};

const ELSEWHERE = {
  availability: "elsewhere",
  path: null,
  branch: null,
  branchSource: null,
  alsoHere: [],
  sentence:
    "This session's execution runs on a provider this computer does not hold.",
};

const RESOLUTIONS = [
  AVAILABLE,
  AVAILABLE_LIVE_SHARED,
  MISSING,
  UNRECORDED,
  ELSEWHERE,
];

/**
 * Every string the module can produce, from every export, over every
 * resolution. `coveredExports` is asserted against the module's real export
 * list below, so a new sentence added later cannot slip past the vocabulary
 * table by being unreachable from this collector.
 */
function everySentence() {
  const out = [];
  const coveredExports = new Set([
    "NEW_SESSION_IN_WORKSPACE_LABEL",
    "WORKSPACE_REUSE_CONVERSATION_SENTENCE",
    "WORKSPACE_REUSE_UNCOMMITTED_SENTENCE",
    "WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE",
    "WORKSPACE_REUSE_UNRESOLVED_DETAIL",
    "workspaceDirectoryName",
    "newSessionInWorkspaceMenuDetail",
    "workspaceReuseBranchLine",
    "workspaceReuseAlsoHereLine",
    "workspaceReuseDraftSentences",
    "workspaceReuseUnavailableLines",
  ]);
  for (const value of Object.values(copy)) {
    if (typeof value === "string") out.push(value);
  }
  out.push(copy.newSessionInWorkspaceMenuDetail(null));
  out.push(...copy.workspaceReuseDraftSentences());
  for (const resolution of RESOLUTIONS) {
    out.push(copy.newSessionInWorkspaceMenuDetail(resolution));
    out.push(copy.workspaceReuseBranchLine(resolution));
    out.push(...copy.workspaceReuseUnavailableLines(resolution));
    const alsoHere = copy.workspaceReuseAlsoHereLine(resolution.alsoHere);
    if (alsoHere !== null) out.push(alsoHere);
    if (resolution.path !== null) {
      out.push(copy.workspaceDirectoryName(resolution.path));
    }
  }
  out.push(copy.workspaceReuseAlsoHereLine(["one"]));
  out.push(copy.workspaceReuseAlsoHereLine(["one", "two", "three"]));
  out.push(
    copy.workspaceReuseBranchLine({ branch: "main", branchSource: null }),
  );
  return { coveredExports, sentences: out };
}

test("no export escapes the vocabulary table", () => {
  const { coveredExports } = everySentence();
  const actual = Object.keys(copy).sort();
  assert.deepEqual(
    actual,
    [...coveredExports].sort(),
    "a new export must be added to the vocabulary collector, not just shipped",
  );
});

test("the vocabulary: nothing this module says claims another operation", () => {
  // Each of these names a real, different operation — copying a worktree,
  // forking a conversation, migrating checkpoints, taking a session over.
  // Saying any of them here would describe work this action does not do.
  const forbidden = [
    /isolat/i,
    /\bfork/i,
    /inherit/i,
    /continu/i,
    /resum/i,
    /taken over/i,
    /takeover/i,
    /take over/i,
  ];
  const { sentences } = everySentence();
  assert.ok(sentences.length > 20, "the table must actually cover the module");
  for (const sentence of sentences) {
    assert.equal(typeof sentence, "string");
    for (const word of forbidden) {
      assert.doesNotMatch(sentence, word, `forbidden word in: ${sentence}`);
    }
  }
});

test("an unavailable state speaks about this computer, never another", () => {
  // "No such directory" alone would read as a fact about the world. The
  // resolver's sentences are scoped to this machine and the copy keeps them
  // that way; nothing here may assert what another machine holds.
  for (const resolution of [MISSING, UNRECORDED, ELSEWHERE]) {
    const [why, next] = copy.workspaceReuseUnavailableLines(resolution);
    assert.equal(why, resolution.sentence);
    assert.match(why, /this computer/i);
    assert.equal(next, copy.WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE);
    assert.match(next, /this computer/i);
    // The unavailable menu line is the resolver's sentence, unsoftened.
    assert.equal(copy.newSessionInWorkspaceMenuDetail(resolution), why);
  }
  // The foreign case says where execution runs, and claims nothing about
  // what that provider holds or would allow.
  assert.doesNotMatch(ELSEWHERE.sentence, /available|cannot run|refus/i);
});

test("one label, and the draft's two required sentences, verbatim", () => {
  assert.equal(
    copy.NEW_SESSION_IN_WORKSPACE_LABEL,
    "New session in this workspace",
  );
  assert.deepEqual(copy.workspaceReuseDraftSentences(), [
    "New conversation; uses these files.",
    "Includes uncommitted changes already in this folder.",
  ]);
});

test("the menu's line carries the shared-directory count, freshly read", () => {
  // It lives in the menu, not in the draft: the draft's request is restored
  // from sessionStorage after a reload, and a count carried there would be
  // re-shown long after it stopped being true. The menu re-reads on open.
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail(AVAILABLE_LIVE_SHARED),
    "repo-wt-a · main — 2 other sessions on this computer recorded a tree in this exact folder.",
  );
  // One other session, and the singular.
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail({
      ...AVAILABLE,
      alsoHere: ["session-b"],
    }),
    "repo-wt-a · work/contextual-sessions-fable — 1 other session on this computer recorded a tree in this exact folder.",
  );
  // A directory this computer cannot find still recorded what it recorded:
  // the count is about records, and it says so, so it survives "missing".
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail({ ...MISSING, alsoHere: ["b"] }),
    "No such directory on this computer. — 1 other session on this computer recorded a tree in this exact folder.",
  );
  // Nothing read, nothing said — never "no other sessions".
  assert.doesNotMatch(
    copy.newSessionInWorkspaceMenuDetail(AVAILABLE),
    /other session/,
  );
});

test("the menu's available line names the folder and the branch", () => {
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail(AVAILABLE),
    "repo-wt-a · work/contextual-sessions-fable",
  );
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail({
      ...AVAILABLE,
      branch: null,
      branchSource: null,
    }),
    "repo-wt-a",
  );
  // Before the read there is no answer, and none is invented.
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail(null),
    copy.WORKSPACE_REUSE_UNRESOLVED_DETAIL,
  );
  assert.doesNotMatch(copy.WORKSPACE_REUSE_UNRESOLVED_DETAIL, /will|is on/i);
  // An "available" that somehow carries no path is not treated as available.
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail({
      ...AVAILABLE,
      path: null,
      sentence: "No such directory on this computer.",
    }),
    "No such directory on this computer.",
  );
});

test("the branch line says which fact it is", () => {
  assert.equal(
    copy.workspaceReuseBranchLine(AVAILABLE),
    "work/contextual-sessions-fable · recorded at creation",
  );
  assert.equal(
    copy.workspaceReuseBranchLine(AVAILABLE_LIVE_SHARED),
    "main · on disk now",
  );
  assert.equal(
    copy.workspaceReuseBranchLine({ branch: "main", branchSource: null }),
    "main · source not known here",
  );
  assert.equal(
    copy.workspaceReuseBranchLine(UNRECORDED),
    "No branch recorded for this folder on this computer.",
  );
  assert.equal(
    copy.workspaceReuseBranchLine({ branch: "", branchSource: "recorded" }),
    "No branch recorded for this folder on this computer.",
  );
});

test("the shared-directory line counts rows in hand, and nothing else", () => {
  // Empty renders nothing: "no other sessions" would be a claim about
  // sessions this computer never recorded.
  assert.equal(copy.workspaceReuseAlsoHereLine([]), null);
  assert.equal(
    copy.workspaceReuseAlsoHereLine(["a"]),
    "1 other session on this computer recorded a tree in this exact folder.",
  );
  assert.equal(
    copy.workspaceReuseAlsoHereLine(["a", "b"]),
    "2 other sessions on this computer recorded a tree in this exact folder.",
  );
  assert.doesNotMatch(
    copy.workspaceReuseAlsoHereLine(["a", "b"]),
    /running|active|open now/i,
  );
});

test("the folder name comes off either kind of path", () => {
  assert.equal(
    copy.workspaceDirectoryName("/Users/brian/Projects/beekeeper/repo-wt-a"),
    "repo-wt-a",
  );
  assert.equal(
    copy.workspaceDirectoryName("C:\\Users\\brian\\code\\repo-wt-a"),
    "repo-wt-a",
  );
  assert.equal(
    copy.workspaceDirectoryName("/Users/brian/Projects/repo-wt-a/"),
    "repo-wt-a",
  );
  assert.equal(copy.workspaceDirectoryName("repo"), "repo");
  assert.equal(copy.workspaceDirectoryName(""), "");
});

test("a failed read outranks the resolution it produced", () => {
  // No rows resolves to "unrecorded". Over a read that threw, that sentence
  // would be a fact nobody established, so the read's own line wins.
  const error =
    "This computer could not read its record of this session's directories.";
  assert.equal(copy.newSessionInWorkspaceMenuDetail(UNRECORDED, error), error);
  assert.equal(copy.newSessionInWorkspaceMenuDetail(AVAILABLE, error), error);
  assert.deepEqual(copy.workspaceReuseUnavailableLines(UNRECORDED, error), [
    error,
    copy.WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE,
  ]);
  // Absent, nothing changes.
  assert.equal(
    copy.newSessionInWorkspaceMenuDetail(UNRECORDED, null),
    UNRECORDED.sentence,
  );
});
