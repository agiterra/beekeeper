import assert from "node:assert/strict";
import test from "node:test";

import {
  describeCodingSessionHireIdentityModelRefusal,
  describeCodingSessionHireModelRefusal,
  resolveCodingSessionHireModel,
  resolveCodingSessionSeatIdentityModel,
} from "./codingSessionHireModel.ts";

const CATALOG = [
  "default",
  "claude-fable-5[1m]",
  "haiku",
  "opus[1m]",
  "sonnet",
];

test("a model the catalog offers is used exactly as it was asked for", () => {
  for (const offered of CATALOG) {
    assert.deepEqual(resolveCodingSessionHireModel(offered, CATALOG), {
      kind: "offered",
      model: offered,
    });
  }
});

// Brian's ruling, 2026-08-29: "The model picker should not be faked." A
// vendor family name is not an id the catalog publishes, and matching it onto
// one is the host inventing an answer — the seat then runs weights nobody
// named. Item 82 shipped that table and item 93 lane A widened it; both are
// gone. Every one of these used to resolve `{ kind: "translated" }`.
test("a Claude family alias is refused, never matched onto a catalog id", () => {
  for (const requested of [
    "claude-sonnet-5",
    "claude-opus-4-1",
    "claude-haiku-3",
    "claude-sonnet",
    "Claude-Sonnet-5",
    "claude-opus-5",
  ]) {
    assert.deepEqual(
      resolveCodingSessionHireModel(requested, CATALOG),
      { kind: "not-offered", requested, offered: CATALOG },
      `requested ${requested}`,
    );
  }
});

// `opus` and `opus[1m]` are two ids, and the runtime publishes exactly one of
// them. Stripping the window suffix to make them match was the second half of
// the same lie: a hire asking for `opus` got a million-token context it never
// asked for, and the record said `opus[1m]` either way.
test("an id the catalog publishes only with a window suffix is a different id", () => {
  assert.deepEqual(resolveCodingSessionHireModel("claude-fable-5", CATALOG), {
    kind: "not-offered",
    requested: "claude-fable-5",
    offered: CATALOG,
  });
  assert.deepEqual(resolveCodingSessionHireModel("opus", CATALOG), {
    kind: "not-offered",
    requested: "opus",
    offered: CATALOG,
  });
});

test("a model nobody offers is refused with the offered ids", () => {
  const resolution = resolveCodingSessionHireModel("gpt-9", CATALOG);
  assert.equal(resolution.kind, "not-offered");
  const reason = describeCodingSessionHireModelRefusal(
    "claude-primary",
    resolution,
  );
  assert.match(reason, /gpt-9/);
  for (const offered of CATALOG) assert.ok(reason.includes(offered));
});

test("an identity's own unoffered model is refused naming the identity", () => {
  const resolution = resolveCodingSessionHireModel("gpt-5.6-sol", [
    "default",
    "gpt-5.6-terra",
  ]);
  const reason = describeCodingSessionHireIdentityModelRefusal(
    "codex-primary",
    "Banksy",
    resolution,
  );
  assert.match(reason, /Banksy's record says gpt-5\.6-sol/);
  assert.match(reason, /codex-primary/);
  assert.match(reason, /default, gpt-5\.6-terra/);
  assert.match(reason, /Agents screen/);
});

test("an unread catalog refuses nothing — it is not a claim about the model", () => {
  assert.deepEqual(resolveCodingSessionHireModel("claude-sonnet-5", []), {
    kind: "unknown",
    model: "claude-sonnet-5",
  });
});

test("no model asked for is no resolution to make", () => {
  assert.equal(resolveCodingSessionHireModel(null, CATALOG), null);
});

test("there is no resolution kind that substitutes one id for another", () => {
  const kinds = new Set(
    [
      ...CATALOG,
      "claude-sonnet-5",
      "opus",
      "gpt-9",
      "claude-fable-5",
      "Claude-Sonnet-5",
    ].map(
      (requested) => resolveCodingSessionHireModel(requested, CATALOG).kind,
    ),
  );
  assert.deepEqual([...kinds].sort(), ["not-offered", "offered"]);
});

// --- the create path: an identity's record against the same catalog --------

test("seating an identity the runtime offers preselects that exact id", () => {
  assert.deepEqual(
    resolveCodingSessionSeatIdentityModel({
      agentModel: "opus[1m]",
      allowedModels: CATALOG,
      providerInstanceRef: "claude-primary",
      selectionExplicit: false,
    }),
    { model: "opus[1m]", note: null, mustPick: false },
  );
});

test("an identity whose record names an unoffered id preselects nothing and says why", () => {
  const resolved = resolveCodingSessionSeatIdentityModel({
    agentModel: "gpt-5.6-sol",
    allowedModels: CATALOG,
    providerInstanceRef: "codex-primary",
    selectionExplicit: false,
  });
  assert.equal(resolved.model, null);
  assert.equal(resolved.mustPick, true);
  assert.equal(
    resolved.note,
    "This identity's record names gpt-5.6-sol, which codex-primary does not " +
      "offer. Pick a model; the record keeps gpt-5.6-sol until you change it " +
      "on the Agents screen.",
  );
});

// The old table made this record runnable by stripping the window suffix. It
// is a different id, so now it is disclosed and the person picks.
test("a record naming an id the catalog only publishes with a suffix must be picked", () => {
  const resolved = resolveCodingSessionSeatIdentityModel({
    agentModel: "claude-fable-5",
    allowedModels: CATALOG,
    providerInstanceRef: "claude-primary",
    selectionExplicit: false,
  });
  assert.equal(resolved.model, null);
  assert.equal(resolved.mustPick, true);
  assert.match(resolved.note ?? "", /claude-fable-5/);
});

test("the person's pick, an empty record and an unread catalog block nothing", () => {
  for (const input of [
    {
      agentModel: "gpt-5.6-sol",
      allowedModels: CATALOG,
      providerInstanceRef: "claude-primary",
      selectionExplicit: true,
    },
    {
      agentModel: null,
      allowedModels: CATALOG,
      providerInstanceRef: "claude-primary",
      selectionExplicit: false,
    },
    {
      agentModel: "opus[1m]",
      allowedModels: [],
      providerInstanceRef: "claude-primary",
      selectionExplicit: false,
    },
  ]) {
    assert.deepEqual(resolveCodingSessionSeatIdentityModel(input), {
      model: null,
      note: null,
      mustPick: false,
    });
  }
});
