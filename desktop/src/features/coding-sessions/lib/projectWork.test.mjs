import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  criterionStatusLabel,
  declarationStateLabel,
  nextStep,
  orderedDeclarations,
  owedBy,
  planLabel,
  shortCommit,
} from "./projectWork.ts";
import { decodeProjectWorkResponse } from "@/shared/api/tauriProjectWork.ts";

// The frozen contract's own fixtures. This surface renders exactly this
// shape, so its presentation is exercised against the sequences `buzz-core`
// is verified against rather than against a hand-written double.
const SEQUENCES = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../../../conformance/project-work/fixtures/sequences",
);

function fold(name) {
  return JSON.parse(
    readFileSync(resolve(SEQUENCES, name, "expected-fold.json"), "utf8"),
  );
}

function response(coverage, unreadablePlans = []) {
  return {
    schema: "buzz-project-work-response/v1",
    implementation: "buzz-core",
    coverage,
    unreadablePlans,
    agentsRepoRead: true,
  };
}

test("every frozen sequence renders through this presentation", () => {
  const names = readdirSync(SEQUENCES);
  assert.ok(names.length > 0, "the frozen sequences are readable");
  for (const name of names) {
    const coverage = fold(name);
    assert.equal(coverage.schema, "buzz-project-work-coverage/v1");
    for (const declaration of orderedDeclarations(coverage)) {
      assert.ok(planLabel(declaration).includes(declaration.planRef.path));
      assert.ok(declarationStateLabel(declaration).length > 0);
      for (const criterion of declaration.criteria) {
        assert.ok(criterionStatusLabel(criterion.status).length > 0);
        assert.ok(owedBy(criterion).length > 0);
      }
    }
    // One next step at most, for every sequence in the contract.
    const step = nextStep(response(coverage));
    if (step !== null) assert.ok(step.releasedBy.length > 0);
  }
});

test("the happy path asks for nothing", () => {
  assert.equal(nextStep(response(fold("happy-path"))), null);
});

test("a fork is the first thing asked for, ahead of any open criterion", () => {
  const step = nextStep(response(fold("fork")));
  assert.match(step.text, /Resolve the fork/);
  assert.match(step.releasedBy, /naming all current heads/);
});

test("an unread plan is asked for before an open criterion", () => {
  const step = nextStep(
    response(fold("happy-path"), [
      {
        repository: "30617:aa:agents",
        commit: "ab".repeat(20),
        path: "plans/kettle.md",
        reasonCode: "plan_unreadable",
        reason: "fatal: path does not exist",
      },
    ]),
  );
  assert.match(step.text, /Make plans\/kettle\.md readable/);
  assert.match(step.text, /fatal: path does not exist/);
});

test("mixed artifacts is reported as the fold words it, not as 'not covered'", () => {
  const coverage = fold("mixed-artifacts");
  const head = coverage.declarations[0];
  assert.equal(head.coverageComplete, false);
  assert.equal(head.coverageReasonCode, "mixed_artifacts");
  // Individually covered criteria keep their own status: they are true
  // statements about the commits they name.
  assert.ok(head.criteria.some((criterion) => criterion.status === "covered"));
  assert.equal(nextStep(response(coverage)).text, head.coverageReason);
});

test("an unassigned criterion says nobody is assigned, not 'no evidence'", () => {
  assert.equal(
    owedBy({ criterionId: "x", assignmentRefs: [], evidence: [] }),
    "nobody is assigned this yet",
  );
});

test("the four declaration states each read differently", () => {
  const labels = ["head", "superseded", "stale", "conflict"].map((state) =>
    declarationStateLabel({
      state,
      supersededBy: ["ab".repeat(32)],
      planRef: { repository: "r", commit: "c", path: "p" },
    }),
  );
  assert.equal(new Set(labels).size, 4);
});

test("an answer about another session is refused, never rendered", () => {
  const coverage = fold("happy-path");
  assert.throws(
    () =>
      decodeProjectWorkResponse(response(coverage), {
        sessionRef: "another-session",
        projectRef: coverage.projectRef,
      }),
    /different session or project/,
  );
  assert.doesNotThrow(() =>
    decodeProjectWorkResponse(response(coverage), {
      sessionRef: coverage.sessionRef,
      projectRef: coverage.projectRef,
    }),
  );
});

test("a response this build does not recognise is refused by name", () => {
  assert.throws(
    () =>
      decodeProjectWorkResponse(
        { ...response(fold("happy-path")), implementation: "typescript" },
        { sessionRef: "s", projectRef: "p" },
      ),
    /does not recognise/,
  );
});

test("commits are abbreviated the way the contract abbreviates them", () => {
  assert.equal(shortCommit("ab".repeat(20)), "abababababab…");
  assert.equal(shortCommit("abc"), "abc");
});
