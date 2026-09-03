import assert from "node:assert/strict";
import test from "node:test";

import { inferCodingSessionMissionLandRepo } from "./codingSessionMissionLandRepoInference.ts";

const OWNER = "6c".repeat(32);
const REPO_A = `30617:${OWNER}:a`;
const REPO_B = `30617:${OWNER}:b`;

test("L20.2: a project's one repository is inferred", () => {
  assert.deepEqual(inferCodingSessionMissionLandRepo([REPO_A]), {
    kind: "inferred",
    repoRef: REPO_A,
  });
});

test("L20.2: two or more repositories infer nothing, but disclose the count", () => {
  assert.deepEqual(inferCodingSessionMissionLandRepo([REPO_A, REPO_B]), {
    kind: "multiple",
    count: 2,
  });
});

test("L20.2: a project with no repositories infers none", () => {
  assert.deepEqual(inferCodingSessionMissionLandRepo([]), { kind: "none" });
});
