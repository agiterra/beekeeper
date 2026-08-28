import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionHireModelNotice,
  describeCodingSessionHireModelRefusal,
  resolveCodingSessionHireModel,
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

test("a Claude family alias is translated onto the id the catalog offers", () => {
  for (const [requested, model] of [
    ["claude-sonnet-5", "sonnet"],
    ["claude-opus-4-1", "opus[1m]"],
    ["claude-haiku-3", "haiku"],
    ["claude-sonnet", "sonnet"],
    ["Claude-Sonnet-5", "sonnet"],
  ]) {
    assert.deepEqual(
      resolveCodingSessionHireModel(requested, CATALOG),
      { kind: "translated", model, requested },
      `requested ${requested}`,
    );
  }
});

test("an alias whose family the catalog does not offer is refused, not guessed", () => {
  const narrow = ["default", "sonnet"];
  const resolution = resolveCodingSessionHireModel("claude-opus-4-1", narrow);
  assert.deepEqual(resolution, {
    kind: "not-offered",
    requested: "claude-opus-4-1",
    offered: narrow,
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

test("an unread catalog refuses nothing — it is not a claim about the model", () => {
  assert.deepEqual(resolveCodingSessionHireModel("claude-sonnet-5", []), {
    kind: "unknown",
    model: "claude-sonnet-5",
  });
});

test("no model asked for is no resolution to make", () => {
  assert.equal(resolveCodingSessionHireModel(null, CATALOG), null);
});

test("a translation is disclosed in the seat's notice, naming both ids", () => {
  const resolution = resolveCodingSessionHireModel("claude-sonnet-5", CATALOG);
  const notice = codingSessionHireModelNotice("claude-primary", resolution);
  assert.ok(notice !== null);
  assert.match(notice, /claude-sonnet-5/);
  assert.match(notice, /sonnet/);
  assert.match(notice, /claude-primary/);
});

test("an exact match discloses nothing — there is nothing to disclose", () => {
  assert.equal(
    codingSessionHireModelNotice(
      "claude-primary",
      resolveCodingSessionHireModel("sonnet", CATALOG),
    ),
    null,
  );
  assert.equal(codingSessionHireModelNotice("claude-primary", null), null);
});
