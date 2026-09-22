import assert from "node:assert/strict";
import { test } from "node:test";

import { planCommitRefusals } from "@/features/agents-repo/lib/agentsRepoPlanSource";

const refuse = async () => ({
  code: "missing_frontmatter",
  path: "frontmatter",
  message: "no fence",
});
const accept = async () => ({ code: null, path: null, message: null });

test("only plan source that the reader refuses stops a commit", async () => {
  const changes = [
    { op: "file.put", path: "plans/kettle.md", text: "flattened" },
    { op: "file.put", path: "roles/lead.md", text: "anything" },
    { op: "file.delete", path: "plans/old.md", text: null },
  ];
  const refused = await planCommitRefusals(changes, refuse);
  assert.equal(refused.length, 1);
  assert.match(refused[0], /plans\/kettle\.md.*missing_frontmatter/);
  assert.deepEqual(await planCommitRefusals(changes, accept), []);
});
