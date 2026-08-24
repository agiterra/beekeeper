import assert from "node:assert/strict";
import test from "node:test";

import {
  buildInitialProjectEventTemplates,
  isUnsupportedProjectKindError,
} from "./projectCreation.ts";

const OWNER = "a".repeat(64);
const CHANNEL = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";

test("buildInitialProjectEventTemplates emits a NIP-MP project", () => {
  const templates = buildInitialProjectEventTemplates({
    accessChannelId: CHANNEL,
    cloneUrl: "https://relay.example/git/owner/sprout.git",
    description: "A multi-repository workspace",
    name: "Sprout",
    ownerPubkey: OWNER,
    webUrl: "https://example.com/sprout",
  });

  assert.equal(templates.dtag, "sprout");
  assert.equal(templates.project.kind, 30621);
  assert.equal(templates.repository.kind, 30617);
  assert.deepEqual(templates.project.tags, [
    ["d", "sprout"],
    ["name", "Sprout"],
    ["buzz-channel", CHANNEL],
    ["description", "A multi-repository workspace"],
    ["a", `30617:${OWNER}:sprout`],
  ]);
  assert.equal(templates.project.content, "");
  assert.deepEqual(templates.repository.tags, [
    ["d", "sprout"],
    ["name", "Sprout"],
    ["buzz-channel", CHANNEL],
    ["description", "A multi-repository workspace"],
    ["clone", "https://relay.example/git/owner/sprout.git"],
    ["web", "https://example.com/sprout"],
  ]);
});

test("buildInitialProjectEventTemplates omits buzz-channel when no channel is given", () => {
  // The shape the create/import dialogs now produce: the repository's
  // `project` back-reference is its ACL, so there is no channel to pick and
  // no binding tag to emit.
  const templates = buildInitialProjectEventTemplates({
    name: "Sprout",
    ownerPubkey: OWNER,
    projectRef: `30621:${OWNER}:general`,
  });

  assert.deepEqual(templates.repository.tags, [
    ["d", "sprout"],
    ["name", "Sprout"],
    ["project", `30621:${OWNER}:general`],
  ]);
  assert.deepEqual(templates.project.tags, [
    ["d", "sprout"],
    ["name", "Sprout"],
    ["a", `30617:${OWNER}:sprout`],
  ]);
});

test("buildInitialProjectEventTemplates still rejects a malformed channel when one is given", () => {
  // Optional is not the same as unvalidated: a malformed value resolves
  // `Broken` at the relay, which fails closed for everyone — strictly worse
  // than the no-binding case it would have replaced.
  assert.throws(
    () =>
      buildInitialProjectEventTemplates({
        accessChannelId: "not-a-uuid",
        name: "Sprout",
        ownerPubkey: OWNER,
      }),
    /access channel is invalid/,
  );
});

test("buildInitialProjectEventTemplates rejects names without an identifier", () => {
  assert.throws(
    () =>
      buildInitialProjectEventTemplates({
        accessChannelId: CHANNEL,
        name: "!!!",
        ownerPubkey: OWNER,
      }),
    /letters or numbers/,
  );
});

test("buildInitialProjectEventTemplates enforces the description tag byte limit", () => {
  assert.doesNotThrow(() =>
    buildInitialProjectEventTemplates({
      accessChannelId: CHANNEL,
      description: "🙂".repeat(512),
      name: "Sprout",
      ownerPubkey: OWNER,
    }),
  );
  assert.throws(
    () =>
      buildInitialProjectEventTemplates({
        accessChannelId: CHANNEL,
        description: "🙂".repeat(513),
        name: "Sprout",
        ownerPubkey: OWNER,
      }),
    /2,048 bytes/,
  );
});

test("isUnsupportedProjectKindError recognizes relay kind compatibility failures", () => {
  assert.equal(
    isUnsupportedProjectKindError(
      new Error("restricted: unknown event kind 30621"),
    ),
    true,
  );
  assert.equal(
    isUnsupportedProjectKindError(new Error("mock project event rejection")),
    false,
  );
});
