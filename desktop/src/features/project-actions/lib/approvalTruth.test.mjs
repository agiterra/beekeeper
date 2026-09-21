import assert from "node:assert/strict";
import test from "node:test";

import { hostStepCommand } from "./actionDefinition.ts";
import {
  buildHostStepApprovalView,
  resolveApprovalAuthority,
} from "./hostStepApproval.ts";
import { fromRawWorkflowRunForTest } from "@/shared/api/tauriWorkflows.ts";
import { matchBoundDefinition } from "./resolveBoundDefinition.ts";
import {
  agentsRepoNote,
  readProjectAgentsRepoWith,
} from "./useProjectAgentsRepo.ts";

const HASH_A = "aa".repeat(32);
const HASH_B = "bb".repeat(32);
const CREATOR = "3d".repeat(32);
const CO_OWNER = "11".repeat(32);
const OUTSIDER = "22".repeat(32);
const COORD = `30621:${CREATOR}:kettle`;
const SHA = "e6".repeat(20);

function definition(command) {
  return {
    steps: [{ id: "verify", action: "run_on_host", command }],
  };
}

function request(overrides = {}) {
  return {
    approvalRef: "ab".repeat(32),
    runId: "11111111-2222-4333-8444-555555555555",
    workflowName: "verify",
    stepId: "verify",
    stepIndex: 1,
    approverSpec: `project-owner:${COORD}`,
    message: null,
    expiresAt: null,
    ...overrides,
  };
}

// ── Finding 1 ─────────────────────────────────────────────────────────────

test("a command is never space-joined, so quoting cannot be misread", () => {
  // `["sh","-c","a b"]` joined with spaces reads as four arguments.
  const command = hostStepCommand(definition(["sh", "-c", "a b"]), "verify");
  assert.deepEqual(command, { form: "argv", argv: ["sh", "-c", "a b"] });
});

test("a string command is kept as a shell line, not mistaken for argv", () => {
  assert.deepEqual(hostStepCommand(definition("just ci"), "verify"), {
    form: "shell",
    text: "just ci",
  });
});

test("A waits, B is published: no command is shown and Approve is unavailable", () => {
  // The interleaving Astra named: the run is bound to A, only B is
  // fetchable, and the card used to show A's hash beside B's command.
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: { kind: "not-current", currentHash: HASH_B },
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.command.value, null);
  assert.match(view.command.reason, /no longer the current one/);
  assert.equal(view.grantAvailable, false);
  assert.match(view.grantBlockedReason, /no longer the current one/);
});

test("A republished after B: the resolved definition matches and Approve returns", () => {
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["just", "ci"] },
    },
    checkout: { state: "commit", sha: SHA },
  });
  assert.deepEqual(view.command.argv, ["just", "ci"]);
  assert.equal(view.grantAvailable, true);
  assert.equal(view.grantBlockedReason, null);
});

test("a failed definition read leaves Approve unavailable, never enabled", () => {
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: { kind: "unread", reason: "relay said 403" },
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.grantAvailable, false);
  assert.match(view.grantBlockedReason, /403/);
});

test("a run with no definition binding can never be granted", () => {
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: null,
    runRead: true,
    definition: { kind: "hash-unknown" },
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.grantAvailable, false);
});

test("a definition that resolves but names no such step blocks Approve", () => {
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: { kind: "resolved", hash: HASH_A, command: null },
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.grantAvailable, false);
  assert.match(view.grantBlockedReason, /no command/);
});

// ── Finding 10 ────────────────────────────────────────────────────────────

test("the run's own checkout is carried, absent and null kept apart", () => {
  const base = {
    id: "r",
    workflow_id: "w",
    status: "waiting_approval",
    current_step: 0,
    execution_trace: [],
    started_at: null,
    completed_at: null,
    error_message: null,
    created_at: 1,
  };
  assert.deepEqual(fromRawWorkflowRunForTest(base).checkout, {
    state: "not-reported",
  });
  assert.deepEqual(
    fromRawWorkflowRunForTest({ ...base, checkout: null }).checkout,
    { state: "working-directory" },
  );
  assert.deepEqual(
    fromRawWorkflowRunForTest({ ...base, checkout: SHA }).checkout,
    { state: "commit", sha: SHA },
  );
});

test("an awaiting-approval run names its commit before any host result", () => {
  const view = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["just", "ci"] },
    },
    checkout: { state: "commit", sha: SHA },
  });
  assert.equal(view.boundCommit.value, SHA);

  const unbound = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["just", "ci"] },
    },
    checkout: { state: "working-directory" },
  });
  assert.equal(unbound.boundCommit.value, null);
  assert.match(unbound.boundCommit.reason, /working directory as found/);
  assert.equal(
    unbound.grantAvailable,
    true,
    "a known unbound run is answerable",
  );

  const unreported = buildHostStepApprovalView({
    request: request(),
    runDefinitionHash: HASH_A,
    runRead: true,
    definition: {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["just", "ci"] },
    },
    checkout: { state: "not-reported" },
  });
  assert.equal(
    unreported.grantAvailable,
    false,
    "an unknown commit blocks Approve",
  );
  assert.match(unreported.boundCommit.reason, /does not report/);
});

// ── Finding 9 ─────────────────────────────────────────────────────────────

test("the approver is the project's creator or a current roster Owner", () => {
  const roster = [{ pubkey: CO_OWNER, role: "owner" }];
  const creator = resolveApprovalAuthority({
    viewerPubkey: CREATOR,
    approverSpec: `project-owner:${COORD}`,
    roster,
    rosterRead: true,
  });
  assert.equal(creator.canApprove, true);

  const coOwner = resolveApprovalAuthority({
    viewerPubkey: CO_OWNER,
    approverSpec: `project-owner:${COORD}`,
    roster,
    rosterRead: true,
  });
  assert.equal(
    coOwner.canApprove,
    true,
    "a roster Owner who is not the creator",
  );

  const other = resolveApprovalAuthority({
    viewerPubkey: OUTSIDER,
    approverSpec: `project-owner:${COORD}`,
    roster,
    rosterRead: true,
  });
  assert.equal(other.canApprove, false);
  assert.match(other.sentence, /project owner/);
});

test("a delegated publisher does not become the approver", () => {
  // Lane 186: the lead publishes; the relay still admits only the project's
  // owners. The publisher's key must never widen or narrow this answer.
  const lead = resolveApprovalAuthority({
    viewerPubkey: OUTSIDER,
    approverSpec: `project-owner:${COORD}`,
    roster: [],
    rosterRead: true,
    // The workflow owner / `p` tag, deliberately accepted and ignored.
    publisherPubkey: OUTSIDER,
  });
  assert.equal(lead.canApprove, false);

  const brian = resolveApprovalAuthority({
    viewerPubkey: CREATOR,
    approverSpec: `project-owner:${COORD}`,
    roster: [],
    rosterRead: true,
    publisherPubkey: OUTSIDER,
  });
  assert.equal(brian.canApprove, true);
});

test("an unread roster never grants and never silently refuses", () => {
  const answer = resolveApprovalAuthority({
    viewerPubkey: CO_OWNER,
    approverSpec: `project-owner:${COORD}`,
    roster: [],
    rosterRead: false,
  });
  assert.equal(answer.canApprove, false);
  assert.match(answer.sentence, /roster/);
});

test("a spec this reader does not recognise offers no Approve", () => {
  for (const spec of [null, "", "any", "deadbeef"]) {
    const answer = resolveApprovalAuthority({
      viewerPubkey: CREATOR,
      approverSpec: spec,
      roster: [],
      rosterRead: true,
    });
    assert.equal(answer.canApprove, false, `spec ${String(spec)}`);
  }
});

// ── Finding 1, at the matcher ─────────────────────────────────────────────

test("the matcher compares the run's binding with the relay's current hash", () => {
  const definition = {
    steps: [{ id: "verify", action: "run_on_host", command: ["just", "ci"] }],
  };
  assert.deepEqual(
    matchBoundDefinition({
      runHash: HASH_A,
      currentHash: HASH_A,
      definition,
      stepId: "verify",
    }),
    {
      kind: "resolved",
      hash: HASH_A,
      command: { form: "argv", argv: ["just", "ci"] },
    },
  );
  assert.deepEqual(
    matchBoundDefinition({
      runHash: HASH_A,
      currentHash: HASH_B,
      definition,
      stepId: "verify",
    }),
    { kind: "not-current", currentHash: HASH_B },
  );
  assert.deepEqual(
    matchBoundDefinition({
      runHash: null,
      currentHash: HASH_A,
      definition,
      stepId: "verify",
    }),
    { kind: "hash-unknown" },
  );
  assert.equal(
    matchBoundDefinition({
      runHash: HASH_A,
      currentHash: null,
      definition: null,
      stepId: "verify",
      readError: "relay said 403",
    }).kind,
    "unread",
  );
  // A hash read that failed is never silently treated as a match.
  assert.equal(
    matchBoundDefinition({
      runHash: HASH_A,
      currentHash: null,
      definition,
      stepId: "verify",
    }).kind,
    "unread",
  );
});

// ── Finding 11 ────────────────────────────────────────────────────────────

test("reading where the agents repository is never writes", async () => {
  const invoked = [];
  const status = await readProjectAgentsRepoWith(
    `30621:${CREATOR}:kettle`,
    async (command) => {
      invoked.push(command);
      return { recorded: true, path: "/tmp/agents" };
    },
  );
  assert.deepEqual(invoked, ["project_agents_repo_status"]);
  assert.deepEqual(status, { recorded: true, path: "/tmp/agents" });
});

test("an unrecorded clone is a stated fact, not a silent preparation", () => {
  assert.match(
    agentsRepoNote({ status: { recorded: false, path: null }, error: null }),
    /Nothing has been changed by opening this tab/,
  );
  assert.match(
    agentsRepoNote({ status: undefined, error: null }),
    /Reading where this computer keeps/,
  );
  assert.match(
    agentsRepoNote({ status: undefined, error: "no such store" }),
    /could not be read: no such store/,
  );
});
