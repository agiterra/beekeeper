import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionResumeInput } from "./codingSessionResumeSeat.ts";
import { publishSeatedCodingSessionResume } from "./codingSessionSeatedCreate.ts";

const ACTOR = "a".repeat(64);
const PROJECT_REF =
  "30621:bb00000000000000000000000000000000000000000000000000000000000000:beekeeper";

/** A custody seam that records every call, in order. */
function recordingDeps(packSource = { repo: "30617:owner:agiterra-packs" }) {
  const calls = [];
  return {
    calls,
    deps: {
      async stageSeat(input) {
        calls.push(["stageSeat", input]);
        return {
          packStaged: true,
          packRef: { repo: "30617:owner:x", sha: "f".repeat(40) },
        };
      },
      async clearSeat(commandId) {
        calls.push(["clearSeat", commandId]);
      },
      async fetchPackSource(projectRef) {
        calls.push(["fetchPackSource", projectRef]);
        return packSource;
      },
    },
  };
}

test("the resume input carries the execution's projectRef through to the publisher", () => {
  const publish = async () => "published";
  const built = buildCodingSessionResumeInput({
    commandId: "csl-1",
    seat: { actorPubkey: ACTOR, role: "builder", projectRef: PROJECT_REF },
    publish,
    deps: { stageSeat: async () => undefined, clearSeat: async () => {} },
  });

  assert.equal(built.commandId, "csl-1");
  assert.equal(built.actorPubkey, ACTOR);
  assert.equal(built.actorRole, "builder");
  // Finding 85: this was the missing prop. Everything else already flowed.
  assert.equal(built.projectRef, PROJECT_REF);
  assert.equal(built.publish, publish);
});

test("a resume stages from the project's pack source, not this computer's copy", async () => {
  const { calls, deps } = recordingDeps();

  const result = await publishSeatedCodingSessionResume(
    buildCodingSessionResumeInput({
      commandId: "csl-2",
      seat: { actorPubkey: ACTOR, role: "builder", projectRef: PROJECT_REF },
      publish: async () => "resumed",
      deps,
    }),
  );

  assert.equal(result, "resumed");
  // The reader is asked about the execution's own project, and its answer is
  // the `packSource` the host is handed. Before this, no reader was wired in
  // and no project was named, so `packSource` was always null.
  assert.deepEqual(calls[0], ["fetchPackSource", PROJECT_REF]);
  const [, staged] = calls[1];
  assert.equal(staged.commandId, "csl-2");
  assert.equal(staged.agentPubkey, ACTOR);
  assert.equal(staged.role, "builder");
  assert.deepEqual(staged.packSource, { repo: "30617:owner:agiterra-packs" });
});

test("a standalone execution names no project and stages the local copy", async () => {
  const { calls, deps } = recordingDeps();

  await publishSeatedCodingSessionResume(
    buildCodingSessionResumeInput({
      commandId: "csl-3",
      seat: { actorPubkey: ACTOR, role: "lead", projectRef: null },
      publish: async () => "resumed",
      deps,
    }),
  );

  // No project, so no reader call at all — the host is handed `null`, which is
  // a deliberate "stage what you have", not a failed lookup.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["stageSeat"],
  );
  assert.equal(calls[0][1].packSource, null);
});

test("an unseated execution publishes with no staging at all", async () => {
  const { calls, deps } = recordingDeps();

  const result = await publishSeatedCodingSessionResume(
    buildCodingSessionResumeInput({
      commandId: "csl-4",
      seat: { actorPubkey: null, role: null, projectRef: PROJECT_REF },
      publish: async () => "resumed",
      deps,
    }),
  );

  assert.equal(result, "resumed");
  assert.deepEqual(calls, []);
});

test("a reader that fails refuses the resume rather than staging the wrong pack", async () => {
  const { deps } = recordingDeps();
  deps.fetchPackSource = async () => {
    throw new Error("the relay did not answer");
  };

  await assert.rejects(
    publishSeatedCodingSessionResume(
      buildCodingSessionResumeInput({
        commandId: "csl-5",
        seat: { actorPubkey: ACTOR, role: "builder", projectRef: PROJECT_REF },
        publish: async () => "resumed",
        deps,
      }),
    ),
    /Could not read the project's pack source/,
  );
});
