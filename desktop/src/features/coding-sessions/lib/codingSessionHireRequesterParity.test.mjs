import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  hasStrictLifecycleCommandJson,
  hasStrictLifecycleCommandValues,
} from "../../../shared/coordination/sessionCoordinationStrictJson.ts";
import {
  CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL,
  CODING_SESSION_REQUESTED_BY_ON_CREATE_REFUSAL,
  describeCrossedCodingSessionLifecycleKey,
} from "./codingSessionLifecycleCommand.ts";
import {
  classifyCodingSessionHireEvent,
  codingSessionHireRequesterStanding,
} from "./codingSessionHireWire.ts";

/**
 * The conformance file `buzz-core` writes and asserts against its own decoder
 * (`coding_session_hire_requester_tests.rs`, `the_shared_fixture_decodes_
 * exactly_as_labelled`). Reading the *same file* is the only thing that makes
 * "the TypeScript decoder accepts what Rust accepts" a fact rather than two
 * decoders that happen to have been written on the same afternoon.
 */
const FIXTURE = JSON.parse(
  readFileSync(
    resolve(
      dirname(fileURLToPath(import.meta.url)),
      "../../../../../crates/buzz-core/testdata/coding_session_hire_requester/vectors.json",
    ),
    "utf8",
  ),
);

const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SECRET = generateSecretKey();
const SIGNER = getPublicKey(SECRET);

function signedHire(content) {
  return finalizeEvent(
    {
      kind: 44221,
      created_at: 1_800_000_000,
      tags: [
        ["h", CHANNEL],
        ["csl-v", "csl1-1"],
        ["csl-command", content.commandId],
      ],
      content: JSON.stringify(content),
    },
    SECRET,
  );
}

const vectors = FIXTURE.vectors;
const hireVectors = vectors.filter(
  (vector) => vector.content.action.type === "session.hire",
);
const createVectors = vectors.filter(
  (vector) => vector.content.action.type === "session.create",
);

test("the shared fixture is the one buzz-core asserts against", () => {
  assert.equal(
    FIXTURE.schema,
    "buzz-coding-session-hire-requester-conformance/v1",
  );
  assert.equal(vectors.length, 13);
  assert.equal(hireVectors.length, 8);
  assert.equal(createVectors.length, 5);
});

test("every hire vector classifies exactly as the fixture labels it", () => {
  for (const vector of hireVectors) {
    const classified = classifyCodingSessionHireEvent(
      signedHire(vector.content),
      new Set([CHANNEL]),
    );
    if (vector.valid) {
      assert.equal(
        classified.kind,
        "hire",
        `${vector.name} must decode: ${JSON.stringify(classified)}`,
      );
      const claimed = vector.content.action.requestedBy;
      assert.equal(
        classified.action.requestedBy,
        claimed === undefined ? undefined : claimed,
        `${vector.name} must carry the requester it was signed with`,
      );
    } else {
      assert.equal(
        classified.kind,
        "malformed",
        `${vector.name} must be refused (${vector.why})`,
      );
    }
  }
});

test("every create vector passes or fails the strict lifecycle decoder as labelled", () => {
  for (const vector of createVectors) {
    const source = JSON.stringify(vector.content);
    const accepted =
      hasStrictLifecycleCommandJson(source, vector.content) &&
      hasStrictLifecycleCommandValues(vector.content);
    assert.equal(
      accepted,
      vector.valid,
      `${vector.name} — ${vector.why}: decoder said ${accepted}`,
    );
  }
});

test("the four accepted hire shapes and the twenty-four create shapes are enumerated, not listed", () => {
  // Both axes are independent, so the accepted set is a product. Building it
  // here from the axes is what proves the decoder enumerates rather than
  // accepting a set somebody typed out and could mistype.
  const base = {
    type: "session.hire",
    sessionRef: FIXTURE.sessionRef,
    genesisRef: FIXTURE.genesisRef,
    role: "builder",
    providerInstanceRef: "claude-primary",
    model: null,
    brief: "Rebase the lane and run the gate.",
  };
  const routing = { class: "code", risk: "low" };
  let accepted = 0;
  for (const withRequester of [false, true]) {
    for (const withRouting of [false, true]) {
      const action = {
        ...base,
        ...(withRequester ? { requestedBy: FIXTURE.requestedBy } : {}),
        ...(withRouting ? { routing } : {}),
      };
      const classified = classifyCodingSessionHireEvent(
        signedHire({
          schema: "buzz-coding-session-lifecycle-command/v1",
          commandId: "hire-1",
          action,
        }),
        new Set([CHANNEL]),
      );
      // A routing request the router rejects is a *routing* refusal, not a
      // key-set one; either way the key set itself must have been accepted.
      if (classified.kind === "malformed") {
        assert.notEqual(
          classified.failingKey,
          "action",
          `the ${withRequester ? "attributed" : "plain"} ${
            withRouting ? "routed" : "unrouted"
          } key set must be accepted`,
        );
      }
      accepted += 1;
    }
  }
  assert.equal(accepted, 4);

  const createBase = {
    type: "session.create",
    projectRef: null,
    repoRef: null,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: "ab".repeat(32),
    model: null,
    title: null,
    initialTurn: null,
  };
  const bases = [
    createBase,
    { ...createBase, sessionRef: FIXTURE.sessionRef },
    {
      ...createBase,
      sessionRef: FIXTURE.sessionRef,
      genesisRef: FIXTURE.genesisRef,
    },
  ];
  const shapes = [];
  for (const form of bases) {
    for (const seated of [false, true]) {
      for (const attributed of [false, true]) {
        for (const routed of [false, true]) {
          shapes.push({
            ...form,
            ...(seated ? { actor: "cd".repeat(32), role: "builder" } : {}),
            ...(attributed ? { hireRef: FIXTURE.hireRef } : {}),
            ...(routed
              ? {
                  routing: {
                    class: "code",
                    tier: "standard",
                    risk: "low",
                    chosen: {
                      providerInstanceRef: "claude-primary",
                      model: "sonnet",
                      vendor: "anthropic",
                    },
                    runnerUp: null,
                    reason: "only candidate",
                    reviewRequired: false,
                    reviewReasons: [],
                    challengerSample: false,
                    override: null,
                    registryVersion: "1",
                    catalogRevision: null,
                  },
                }
              : {}),
          });
        }
      }
    }
  }
  assert.equal(shapes.length, 24);
  for (const action of shapes) {
    const content = {
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId: "create-1",
      action,
    };
    assert.equal(
      hasStrictLifecycleCommandJson(JSON.stringify(content), content),
      true,
      `the create shape ${Object.keys(action).join(",")} must be accepted`,
    );
  }
});

test("the two keys never cross, and the refusal is the sentence the fixture quotes", () => {
  // The fixture's own note names these two sentences as the ones a TypeScript
  // decoder should carry, verbatim. If either drifts, the note is wrong about
  // this codebase and a lead reading the refusal cannot act on it.
  assert.ok(
    FIXTURE.note.includes(CODING_SESSION_REQUESTED_BY_ON_CREATE_REFUSAL),
  );
  assert.ok(FIXTURE.note.includes(CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL));

  const crossedHire = vectors.find(
    (vector) => vector.name === "hire-carrying-the-creates-hireRef",
  );
  const classified = classifyCodingSessionHireEvent(
    signedHire(crossedHire.content),
    new Set([CHANNEL]),
  );
  assert.equal(classified.kind, "malformed");
  assert.equal(classified.reason, CODING_SESSION_HIRE_REF_ON_HIRE_REFUSAL);
  assert.equal(
    classified.reason.includes("missing or unsupported fields"),
    false,
  );

  const crossedCreate = vectors.find(
    (vector) => vector.name === "create-carrying-the-hires-requestedBy",
  );
  assert.equal(
    describeCrossedCodingSessionLifecycleKey(crossedCreate.content.action),
    CODING_SESSION_REQUESTED_BY_ON_CREATE_REFUSAL,
  );
});

test("a requester is attributed only when it equals the hire's own signer", () => {
  // POLICY.md §5: the relay does not compare these, so this host does. Three
  // answers — an unclaimed hire is unknown, never mismatched, and a hire that
  // names somebody other than its signer is disclosed rather than dropped.
  const attributed = vectors.find(
    (vector) => vector.name === "hire-with-a-requester",
  );
  const forged = classifyCodingSessionHireEvent(
    signedHire(attributed.content),
    new Set([CHANNEL]),
  );
  assert.equal(forged.kind, "hire");
  assert.deepEqual(codingSessionHireRequesterStanding(forged), {
    kind: "disputed",
    claimedPubkey: FIXTURE.requestedBy,
    signerPubkey: SIGNER,
  });

  const honest = classifyCodingSessionHireEvent(
    signedHire({
      ...attributed.content,
      action: { ...attributed.content.action, requestedBy: SIGNER },
    }),
    new Set([CHANNEL]),
  );
  assert.deepEqual(codingSessionHireRequesterStanding(honest), {
    kind: "attributed",
    requesterPubkey: SIGNER,
  });

  const unclaimed = classifyCodingSessionHireEvent(
    signedHire(
      vectors.find(
        (vector) => vector.name === "hire-without-a-requester-stays-valid",
      ).content,
    ),
    new Set([CHANNEL]),
  );
  assert.deepEqual(codingSessionHireRequesterStanding(unclaimed), {
    kind: "unclaimed",
    requesterPubkey: SIGNER,
  });
});
