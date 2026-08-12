import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionIngressAuthorityIdentity,
  resolveCodingSessionIngressAuthority,
} from "./codingSessionIngressAuthority.ts";

const PUBKEY_A = "a".repeat(64);
const PUBKEY_B = "b".repeat(64);

test("an absent, empty, or non-array trust list is fail-closed", () => {
  for (const entries of [undefined, null, [], "not-an-array", 42, {}]) {
    const authority = resolveCodingSessionIngressAuthority(entries);
    assert.equal(authority.state, "invalid");
    assert.match(authority.errorMessage, /no allowed provider pubkeys/);
  }
});

test("error strings are provider-neutral and never name a donor product", () => {
  const empty = resolveCodingSessionIngressAuthority([]);
  const malformed = resolveCodingSessionIngressAuthority([{ pubkey: "nope" }]);
  for (const authority of [empty, malformed]) {
    assert.equal(authority.state, "invalid");
    assert.ok(!/hive/i.test(authority.errorMessage), authority.errorMessage);
    assert.ok(!/bridge/i.test(authority.errorMessage), authority.errorMessage);
  }
});

test("a valid list resolves to normalized pubkeys with a lookup index", () => {
  const authority = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A.toUpperCase(), label: "  This computer  " },
    { pubkey: PUBKEY_B, label: "Second provider" },
  ]);
  assert.equal(authority.state, "valid");
  assert.deepEqual(authority.allowed, [
    { pubkey: PUBKEY_A, label: "This computer" },
    { pubkey: PUBKEY_B, label: "Second provider" },
  ]);
  assert.deepEqual(authority.byPubkey.get(PUBKEY_A), {
    pubkey: PUBKEY_A,
    label: "This computer",
  });
  assert.equal(authority.byPubkey.get("c".repeat(64)), undefined);
});

test("one bad entry rejects the whole list rather than admitting the rest", () => {
  for (const bad of [
    null,
    "string-entry",
    ["array-entry"],
    { pubkey: 42 },
    { pubkey: "" },
    { pubkey: `${PUBKEY_A}f` },
    { pubkey: PUBKEY_A.slice(0, 63) },
    { pubkey: `${"g".repeat(64)}` },
  ]) {
    const authority = resolveCodingSessionIngressAuthority([
      { pubkey: PUBKEY_B, label: "Good" },
      bad,
    ]);
    assert.equal(
      authority.state,
      "invalid",
      `entry ${JSON.stringify(bad)} must invalidate the list`,
    );
    assert.match(authority.errorMessage, /config is invalid/);
  }
});

test("a duplicate pubkey invalidates the list instead of silently collapsing", () => {
  const authority = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A, label: "One" },
    { pubkey: PUBKEY_A.toUpperCase(), label: "Two" },
  ]);
  assert.equal(authority.state, "invalid");
});

test("labels are bounded, control-stripped, and default when unusable", () => {
  const authority = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A, label: `bad${String.fromCharCode(7)}label` },
    { pubkey: PUBKEY_B, label: "x".repeat(500) },
  ]);
  assert.equal(authority.state, "valid");
  assert.equal(authority.allowed[0].label, "bad label");
  assert.equal(authority.allowed[1].label.length, 80);

  const defaults = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A },
    { pubkey: PUBKEY_B, label: "   " },
  ]);
  assert.equal(defaults.state, "valid");
  assert.deepEqual(
    defaults.allowed.map((entry) => entry.label),
    ["Provider", "Provider"],
  );
});

test("authority identity is order-independent and changes when the list changes", () => {
  const forward = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A, label: "One" },
    { pubkey: PUBKEY_B, label: "Two" },
  ]);
  const reversed = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_B, label: "Renamed" },
    { pubkey: PUBKEY_A, label: "Also renamed" },
  ]);
  assert.equal(
    buildCodingSessionIngressAuthorityIdentity(forward),
    buildCodingSessionIngressAuthorityIdentity(reversed),
  );

  const narrowed = resolveCodingSessionIngressAuthority([
    { pubkey: PUBKEY_A, label: "One" },
  ]);
  assert.notEqual(
    buildCodingSessionIngressAuthorityIdentity(forward),
    buildCodingSessionIngressAuthorityIdentity(narrowed),
  );
  assert.match(
    buildCodingSessionIngressAuthorityIdentity(
      resolveCodingSessionIngressAuthority([]),
    ),
    /^invalid:/,
  );
});
