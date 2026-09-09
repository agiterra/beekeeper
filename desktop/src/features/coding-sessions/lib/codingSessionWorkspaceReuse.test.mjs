import assert from "node:assert/strict";
import test from "node:test";

import {
  WORKSPACE_REUSE_SENTENCES,
  resolveWorkspaceDraftBranch,
  resolveWorkspaceReuse,
} from "./codingSessionWorkspaceReuse.ts";

/**
 * "New session in this workspace" hands the launcher a directory. Every rule
 * below exists because the wrong answer here is not a blank field — it is a
 * session started in somebody else's folder, or an offer to reuse a directory
 * that is gone, or a Mac path handed to an execution running on Windows.
 */

const SESSION = "9f2c1d40-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const OTHER = "1a2b3c4d-5e6f-4071-8293-a4b5c6d7e8f9";

function row(overrides = {}) {
  return {
    key: `${SESSION}/lead`,
    sessionRef: SESSION,
    seatLabel: "lead",
    path: "/Users/x/Code/repo-wt-a",
    branch: "wt-a",
    repoRoot: "/Users/x/Code/repo",
    disposition: "held",
    dirtyFiles: 1,
    reclaimableBytes: null,
    reclaimableLabel: "unknown",
    reclaimableNow: false,
    graceRemainingSecs: null,
    exists: true,
    tipOnRelayKnown: true,
    detail: "Held.",
    ...overrides,
  };
}

const ON_DISK = { exists: true, isDir: true };

test("a recorded tree that is still there is available, with its path and branch", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "available");
  assert.equal(resolution.path, "/Users/x/Code/repo-wt-a");
  assert.equal(resolution.branch, "wt-a");
  assert.equal(resolution.branchSource, "recorded");
  assert.deepEqual(resolution.alsoHere, []);
  assert.match(resolution.sentence, /on this computer/);
  assert.match(resolution.sentence, /wt-a/);
});

test("the head read off disk wins over the creation-time branch, and says so", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: true,
    liveBranch: "wt-a-fixups",
  });

  assert.equal(resolution.branch, "wt-a-fixups");
  assert.equal(resolution.branchSource, "live");

  // A failed branch read keeps the recorded answer rather than blanking it.
  const failed = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: true,
    liveBranch: null,
  });
  assert.equal(failed.branch, "wt-a");
  assert.equal(failed.branchSource, "recorded");
});

test("a row the host no longer sees is missing, and still names the path", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row({ exists: false })],
    validation: null,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "missing");
  assert.equal(resolution.path, "/Users/x/Code/repo-wt-a");
  assert.equal(resolution.sentence, WORKSPACE_REUSE_SENTENCES.missing);
});

test("a validation that failed, or never ran, is missing rather than an unverified offer", () => {
  for (const validation of [
    null,
    { exists: false, isDir: false },
    { exists: true, isDir: false },
  ]) {
    const resolution = resolveWorkspaceReuse({
      sessionRef: SESSION,
      rows: [row()],
      validation,
      executionIsLocal: true,
    });
    assert.equal(
      resolution.availability,
      "missing",
      JSON.stringify(validation),
    );
  }
});

test("no row for this session is unrecorded — never another session's folder", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    // Another session's tree sits right there, verified and on disk. It is
    // still not this session's workspace.
    rows: [row({ sessionRef: OTHER, key: `${OTHER}/lead` })],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "unrecorded");
  assert.equal(resolution.path, null);
  assert.equal(resolution.branch, null);
  assert.equal(resolution.branchSource, null);
  assert.equal(resolution.sentence, WORKSPACE_REUSE_SENTENCES.unrecorded);
});

test("a failed seat read reads as unrecorded with nothing to reuse", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [],
    validation: null,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "unrecorded");
  assert.equal(resolution.path, null);
  assert.deepEqual(resolution.alsoHere, []);
});

test("unrecorded wins over a foreign execution — there is no path to withhold", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [],
    validation: null,
    executionIsLocal: false,
  });

  assert.equal(resolution.availability, "unrecorded");
});

test("an execution on another provider withholds the path entirely", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: false,
    liveBranch: "wt-a",
  });

  assert.equal(resolution.availability, "elsewhere");
  assert.equal(resolution.path, null, "a Mac path is not a Windows checkout");
  assert.equal(resolution.branch, null);
  assert.equal(resolution.branchSource, null);
  assert.deepEqual(resolution.alsoHere, []);
  assert.equal(resolution.sentence, WORKSPACE_REUSE_SENTENCES.elsewhere);
  assert.doesNotMatch(resolution.sentence, /Windows|remote|control/i);
});

test("an unknown execution location never becomes elsewhere, and says it is unknown", () => {
  const available = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: null,
  });
  assert.equal(available.availability, "available");
  assert.equal(available.path, "/Users/x/Code/repo-wt-a");
  assert.match(available.sentence, /not known here/);

  const missing = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row({ exists: false })],
    validation: null,
    executionIsLocal: null,
  });
  assert.equal(missing.availability, "missing");
  assert.match(missing.sentence, /not known here/);

  const unrecorded = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [],
    validation: null,
    executionIsLocal: null,
  });
  assert.equal(unrecorded.availability, "unrecorded");
  assert.match(unrecorded.sentence, /not known here/);
});

test("a known-local execution adds no unknown-location clause", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row()],
    validation: ON_DISK,
    executionIsLocal: true,
  });
  assert.doesNotMatch(resolution.sentence, /not known here/);
});

test("alsoHere lists other sessions at the identical path, from these rows alone", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [
      row(),
      row({ sessionRef: OTHER, key: `${OTHER}/lead`, seatLabel: "lead" }),
      // A second seat of the same other session at the same path counts once.
      row({ sessionRef: OTHER, key: `${OTHER}/builder`, seatLabel: "builder" }),
      // A different directory is not "also here".
      row({
        sessionRef: "c0ffee00-0000-4000-8000-000000000000",
        key: "c0ffee/lead",
        path: "/Users/x/Code/repo-wt-b",
      }),
    ],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.deepEqual(resolution.alsoHere, [OTHER]);
});

test("a session's own second seat is never listed as somebody else here", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row(), row({ key: `${SESSION}/builder`, seatLabel: "builder" })],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.deepEqual(resolution.alsoHere, []);
});

test("multiple recorded workspaces require a choice even when one is gone", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [
      row({
        key: `${SESSION}/gone`,
        path: "/Users/x/Code/gone",
        exists: false,
      }),
      row({ key: `${SESSION}/lead`, path: "/Users/x/Code/repo-wt-a" }),
    ],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "ambiguous");
  assert.equal(resolution.path, null);
  assert.match(resolution.sentence, /multiple recorded workspaces/);
});

test("a row with an empty path is not a workspace", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row({ path: "" })],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.equal(resolution.availability, "unrecorded");
});

test("a row with no recorded branch reports no branch rather than an empty one", () => {
  const resolution = resolveWorkspaceReuse({
    sessionRef: SESSION,
    rows: [row({ branch: "" })],
    validation: ON_DISK,
    executionIsLocal: true,
  });

  assert.equal(resolution.branch, null);
  assert.equal(resolution.branchSource, null);
  assert.equal(
    resolution.sentence,
    "This session's directory is on this computer.",
  );
});

test("no sentence claims anything about another machine", () => {
  for (const sentence of Object.values(WORKSPACE_REUSE_SENTENCES)) {
    assert.doesNotMatch(sentence, /\b(their|remote|host machine|Windows)\b/i);
  }
});

// The draft's branch line. The request that opened the draft carries a branch
// recorded when the worktree was cut; only a read of that exact directory,
// made during this open, can upgrade it to a claim about now.

test("before the read answers, the recorded branch stands with no source claimed", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({ recordedBranch: "wt-a", read: null }),
    { missing: false, branch: "wt-a", branchSource: null },
  );
});

test("a head read off that directory is the live branch", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      read: {
        validation: { exists: true, isDir: true },
        headBranch: "wt-a-fixups",
      },
    }),
    { missing: false, branch: "wt-a-fixups", branchSource: "live" },
  );
});

test("a head that could not be read leaves the recorded branch unattributed", () => {
  // The directory is there; `list_coding_session_worktree_branches` threw or
  // the checkout is on a detached head. Either way the recorded branch is all
  // that is known, and it is not "on disk now".
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      read: { validation: { exists: true, isDir: true }, headBranch: null },
    }),
    { missing: false, branch: "wt-a", branchSource: null },
  );
});

test("a directory that answered it is gone shows no branch at all", () => {
  for (const validation of [
    { exists: false, isDir: false },
    { exists: true, isDir: false },
  ]) {
    assert.deepEqual(
      resolveWorkspaceDraftBranch({
        recordedBranch: "wt-a",
        read: { validation, headBranch: null },
      }),
      { missing: true, branch: null, branchSource: null },
      JSON.stringify(validation),
    );
  }
});

test("a validation that threw is not evidence the directory is gone", () => {
  // `validation: null` means the check itself failed — no host, no answer.
  // Saying "No such directory" from that would be a claim nothing supports.
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      read: { validation: null, headBranch: null },
    }),
    { missing: false, branch: "wt-a", branchSource: null },
  );
});

test("a request with no recorded branch reports none rather than an empty one", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({ recordedBranch: "", read: null }),
    { missing: false, branch: null, branchSource: null },
  );
  assert.deepEqual(
    resolveWorkspaceDraftBranch({ recordedBranch: null, read: null }),
    { missing: false, branch: null, branchSource: null },
  );
});

test("a branch the request recorded at creation says so straight away", () => {
  // Creation-time facts do not go stale, so this needs no read to be true.
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      recordedBranchSource: "recorded",
      read: null,
    }),
    { missing: false, branch: "wt-a", branchSource: "recorded" },
  );
});

test("a live head still outranks a recorded branch once it is read", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      recordedBranchSource: "recorded",
      read: {
        validation: { exists: true, isDir: true },
        headBranch: "wt-a-fixups",
      },
    }),
    { missing: false, branch: "wt-a-fixups", branchSource: "live" },
  );
});

test("a recorded branch survives a head that could not be read", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      recordedBranchSource: "recorded",
      read: { validation: { exists: true, isDir: true }, headBranch: null },
    }),
    { missing: false, branch: "wt-a", branchSource: "recorded" },
  );
});

test("a gone folder claims no provenance either", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: "wt-a",
      recordedBranchSource: "recorded",
      read: { validation: { exists: false, isDir: false }, headBranch: null },
    }),
    { missing: true, branch: null, branchSource: null },
  );
});

test("a provenance with no branch under it claims nothing", () => {
  assert.deepEqual(
    resolveWorkspaceDraftBranch({
      recordedBranch: null,
      recordedBranchSource: "recorded",
      read: null,
    }),
    { missing: false, branch: null, branchSource: null },
  );
});
