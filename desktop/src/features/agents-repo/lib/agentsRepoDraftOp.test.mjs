import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  agentsRepoCommitAccess,
  agentsRepoDraftAccess,
} from "@/features/agents-repo/lib/agentsRepoAccess";
import { gitBlobSha } from "@/features/agents-repo/lib/agentsRepoBlobSha";
import { foldAgentsRepoDrafts } from "@/features/agents-repo/lib/agentsRepoDraftFold";
import {
  AGENTS_REPO_DRAFT_OP_KIND,
  MAX_AGENTS_REPO_DRAFT_TEXT_BYTES,
  archiveCounterpart,
  decodeDraftOp,
  draftOpTags,
  draftPathClass,
  draftTextError,
  encodeDraftOpContent,
} from "@/features/agents-repo/lib/agentsRepoDraftOp";
import {
  headConflict,
  nextDraftCreatedAt,
} from "@/features/agents-repo/lib/agentsRepoMutations";
import {
  displayName,
  groupOf,
  groupPaths,
  newPlanPath,
} from "@/features/agents-repo/lib/agentsRepoPaths";
import { KIND_AGENTS_REPO_DRAFT_OP } from "@/shared/constants/kinds";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const folds = JSON.parse(
  readFileSync(
    path.resolve(
      HERE,
      "../../../../../conformance/agents-repo-draft-fold/fixtures/fold-vectors.json",
    ),
    "utf8",
  ),
);
const REPO = `30617:${"a".repeat(64)}:tank-loop-beekeeper-agents`;
const COORD = `30621:${"a".repeat(64)}:tank-loop`;

test("the local kind pin matches the shared constant", () => {
  assert.equal(AGENTS_REPO_DRAFT_OP_KIND, KIND_AGENTS_REPO_DRAFT_OP);
  assert.equal(AGENTS_REPO_DRAFT_OP_KIND, 44249);
});

test("every fold vector folds byte-identically", () => {
  assert.ok(folds.cases.length > 0);
  for (const vector of folds.cases) {
    const digest = foldAgentsRepoDrafts(
      vector.project,
      vector.repo,
      vector.events,
    );
    assert.equal(
      JSON.stringify(digest),
      JSON.stringify(vector.expected),
      vector.name,
    );
  }
});

test("ops round-trip through content and carry one ad-path per named path", () => {
  const put = {
    repo: REPO,
    message: "why",
    content: {
      op: "file.put",
      path: "plans/rpg.md",
      text: "# RPG\n",
      base: null,
      baseCommit: null,
      prev: null,
    },
  };
  const move = {
    repo: REPO,
    message: null,
    content: {
      op: "file.move",
      path: "roles/poker.md",
      to: "roles/archive/poker.md",
      base: "1".repeat(40),
      baseCommit: null,
      prev: null,
    },
  };
  const record = {
    repo: REPO,
    message: null,
    content: {
      op: "commit.record",
      commit: "c".repeat(40),
      paths: ["plans/rpg.md", "roles/poker.md"],
      drafts: ["0".repeat(64)],
    },
  };
  for (const op of [put, move, record]) {
    const content = encodeDraftOpContent(op);
    assert.deepEqual(decodeDraftOp(content, REPO), op);
    assert.ok(content.includes('"message":'), "nullables are emitted");
  }
  assert.deepEqual(
    draftOpTags(COORD, move)
      .filter((t) => t[0] === "ad-path")
      .map((t) => t[1]),
    ["roles/poker.md", "roles/archive/poker.md"],
  );
  assert.equal(
    draftOpTags(COORD, record).filter((t) => t[0] === "ad-path").length,
    2,
  );
});

test("the decoder refuses what the Rust validator refuses", () => {
  const base = {
    schema: "buzz-agents-repo-draft/v1",
    op: "file.put",
    path: "plans/rpg.md",
    text: "x",
    base: null,
    baseCommit: null,
    prev: null,
    message: null,
  };
  assert.ok(decodeDraftOp(JSON.stringify(base), REPO));
  assert.equal(
    decodeDraftOp(JSON.stringify({ ...base, mode: "100755" }), REPO),
    null,
    "unknown key",
  );
  const { prev: _prev, ...missing } = base;
  assert.equal(
    decodeDraftOp(JSON.stringify(missing), REPO),
    null,
    "absent is not null",
  );
  assert.equal(
    decodeDraftOp(JSON.stringify({ ...base, path: "docs/plan.md" }), REPO),
    null,
    "outside the layout",
  );
  assert.equal(
    decodeDraftOp(JSON.stringify({ ...base, base: "abc" }), REPO),
    null,
    "bad sha",
  );
  assert.equal(
    decodeDraftOp(
      JSON.stringify({
        ...base,
        text: "x".repeat(MAX_AGENTS_REPO_DRAFT_TEXT_BYTES + 1),
      }),
      REPO,
    ),
    null,
    "over the cap",
  );
  assert.equal(
    decodeDraftOp(
      JSON.stringify({
        ...base,
        op: "file.delete",
        path: "team.yml",
        text: undefined,
      }),
      REPO,
    ),
    null,
    "root files are put-only",
  );
  const move = {
    schema: "buzz-agents-repo-draft/v1",
    op: "file.move",
    path: "plans/rpg.md",
    to: "plans/old.md",
    base: null,
    baseCommit: null,
    prev: null,
    message: null,
  };
  assert.equal(
    decodeDraftOp(JSON.stringify(move), REPO),
    null,
    "a move goes only to the archive counterpart",
  );
  assert.ok(
    decodeDraftOp(
      JSON.stringify({ ...move, to: "plans/archive/rpg.md" }),
      REPO,
    ),
  );
});

test("the path grammar and the archive counterpart", () => {
  assert.equal(draftPathClass("roles/lead.md").class, "role");
  assert.equal(draftPathClass("roles/archive/lead.md").class, "archived-role");
  assert.equal(
    draftPathClass("roles/lead/skills/marker/SKILL.md").class,
    "role-skill",
  );
  assert.equal(draftPathClass("skills/marker/SKILL.md").class, "shared-skill");
  assert.equal(draftPathClass("plans/rpg.md").class, "plan");
  assert.equal(draftPathClass("team.yml").class, "root-file");
  for (const bad of [
    "",
    "/roles/lead.md",
    "roles/../x",
    "roles/Lead.md",
    "roles/archive.md",
    "docs/x.md",
    "skills/marker",
  ]) {
    assert.equal(draftPathClass(bad).ok, false, bad);
  }
  assert.equal(archiveCounterpart("plans/rpg.md"), "plans/archive/rpg.md");
  assert.equal(archiveCounterpart("roles/archive/lead.md"), "roles/lead.md");
  assert.equal(archiveCounterpart("team.yml"), null);
  assert.equal(draftTextError("a\u0000b") !== null, true);
  assert.equal(draftTextError("a\nb\tc\r\n"), null);
  assert.equal(draftTextError(""), null);
});

test("grouping puts plans first and names files by their stem", () => {
  assert.equal(groupOf("plans/rpg.md"), "plans");
  assert.equal(groupOf("roles/archive/x.md"), "archive");
  assert.equal(groupOf("team.yml"), "team");
  assert.equal(groupOf("weird/file.txt"), "other");
  assert.equal(displayName("plans/rpg.md"), "rpg");
  assert.equal(displayName("skills/marker/SKILL.md"), "marker");
  assert.equal(displayName("team.yml"), "team.yml");
  const groups = groupPaths([
    { path: "team.yml" },
    { path: "plans/b.md" },
    { path: "plans/a.md" },
    { path: "roles/lead.md" },
  ]);
  assert.deepEqual(
    groups.map((g) => g.group),
    ["plans", "roles", "team"],
  );
  assert.deepEqual(
    groups[0].entries.map((e) => e.path),
    ["plans/a.md", "plans/b.md"],
  );
  assert.deepEqual(newPlanPath("My Plan!"), {
    ok: true,
    path: "plans/my-plan.md",
  });
  assert.equal(newPlanPath("archive").ok, false);
  assert.equal(newPlanPath("   ").ok, false);
});

test("a save from a stale head is refused before signing, with the author named", () => {
  const read = {
    digest: {
      paths: [
        {
          path: "plans/rpg.md",
          head: { id: "h".repeat(64), author: "2".repeat(64) },
          superseded: [],
        },
      ],
    },
  };
  const name = (pubkey) => (pubkey.startsWith("2") ? "Alice" : "?");
  assert.equal(headConflict(read, "plans/rpg.md", "h".repeat(64), name), null);
  assert.match(
    headConflict(read, "plans/rpg.md", null, name),
    /Alice saved a newer draft/,
  );
  assert.match(
    headConflict(read, "plans/rpg.md", "x".repeat(64), name),
    /Alice saved a newer draft/,
  );
  assert.match(
    headConflict(read, "plans/other.md", "x".repeat(64), name),
    /committed or withdrawn/,
  );
  assert.equal(headConflict(undefined, "plans/rpg.md", null, name), null);
  assert.equal(nextDraftCreatedAt(100, 50), 101);
  assert.equal(nextDraftCreatedAt(100, 500), 500);
});

test("the blob sha matches git's", async () => {
  assert.equal(
    await gitBlobSha(""),
    "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
  );
  assert.equal(
    await gitBlobSha("hello\n"),
    "ce013625030ba8dba906f756967f9e9ca394464a",
  );
});

test("access: a viewer reads only, a collaborator drafts and commits, a public project is open", () => {
  const self = "1".repeat(64);
  const project = {
    id: "p",
    owner: "9".repeat(64),
    address: COORD,
    visibility: "private",
    members: [],
  };
  const caps = { isOwner: false, isLoading: false };
  assert.equal(
    agentsRepoDraftAccess(
      self,
      project,
      [{ pubkey: self, role: "viewer" }],
      caps,
    ).kind,
    "read-only",
  );
  assert.equal(
    agentsRepoCommitAccess(
      self,
      project,
      [{ pubkey: self, role: "collaborator" }],
      caps,
    ).kind,
    "writable",
  );
  assert.equal(
    agentsRepoDraftAccess(self, { ...project, visibility: "public" }, [], caps)
      .kind,
    "writable",
  );
  assert.equal(agentsRepoDraftAccess(null, project, [], caps).kind, "loading");
  assert.equal(
    agentsRepoDraftAccess(self, null, [], caps).kind,
    "no-coordinate",
  );
});
