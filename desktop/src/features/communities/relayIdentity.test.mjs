/**
 * Unit tests for the relay-identity decision matrix.
 *
 * A false-positive `rekey` wipes every relay-scoped cache the user has, so the
 * bar for producing one is exhaustively pinned here: only a resolved,
 * well-formed observation that differs from a well-formed stored key may do it.
 * Everything else — no stored key (the upgrade path for every existing
 * community), an unreachable relay, a relay with no `self`, malformed hex —
 * must leave user data alone.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  decideRelayIdentityAction,
  normalizeRelayPubkey,
} from "./relayIdentity.ts";

const KEY_A = "a".repeat(64);
const KEY_B = "b".repeat(64);

const resolved = (pubkey) => ({ status: "resolved", pubkey });
const unresolved = { status: "unresolved" };

// ---------------------------------------------------------------------------
// (a) No stored pubkey → adopt, no wipe. This is the upgrade path.
// ---------------------------------------------------------------------------

test("decideRelayIdentityAction_no_stored_pubkey_adopts_without_wiping", () => {
  assert.deepEqual(
    decideRelayIdentityAction({ observation: resolved(KEY_A) }),
    { kind: "adopt", relayPubkey: KEY_A },
  );
});

test("decideRelayIdentityAction_undefined_and_empty_stored_pubkey_both_adopt", () => {
  for (const storedRelayPubkey of [undefined, null, "", "   "]) {
    assert.deepEqual(
      decideRelayIdentityAction({
        storedRelayPubkey,
        observation: resolved(KEY_A),
      }),
      { kind: "adopt", relayPubkey: KEY_A },
      `stored=${JSON.stringify(storedRelayPubkey)}`,
    );
  }
});

test("decideRelayIdentityAction_corrupt_stored_pubkey_adopts_never_rekeys", () => {
  // Garbage in the persisted record is "unknown", not proof the relay changed.
  for (const storedRelayPubkey of [
    "not-hex",
    "abc",
    `${KEY_A}ff`,
    "0".repeat(63),
  ]) {
    assert.deepEqual(
      decideRelayIdentityAction({
        storedRelayPubkey,
        observation: resolved(KEY_A),
      }),
      { kind: "adopt", relayPubkey: KEY_A },
      `stored=${storedRelayPubkey}`,
    );
  }
});

// ---------------------------------------------------------------------------
// (b) stored === observed → no-op
// ---------------------------------------------------------------------------

test("decideRelayIdentityAction_matching_pubkey_is_a_noop", () => {
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: KEY_A,
      observation: resolved(KEY_A),
    }),
    { kind: "none", reason: "match" },
  );
});

test("decideRelayIdentityAction_match_ignores_case_and_whitespace", () => {
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: ` ${KEY_A.toUpperCase()} `,
      observation: resolved(KEY_A.toUpperCase()),
    }),
    { kind: "none", reason: "match" },
  );
});

// ---------------------------------------------------------------------------
// (c) stored !== observed → wipe
// ---------------------------------------------------------------------------

test("decideRelayIdentityAction_differing_pubkey_rekeys", () => {
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: KEY_A,
      observation: resolved(KEY_B),
    }),
    { kind: "rekey", relayPubkey: KEY_B, previousRelayPubkey: KEY_A },
  );
});

test("decideRelayIdentityAction_rekey_normalizes_both_sides", () => {
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: KEY_A.toUpperCase(),
      observation: resolved(` ${KEY_B.toUpperCase()} `),
    }),
    { kind: "rekey", relayPubkey: KEY_B, previousRelayPubkey: KEY_A },
  );
});

// ---------------------------------------------------------------------------
// (d) Observed missing / errored / malformed → no-op, even with a stored key
// ---------------------------------------------------------------------------

test("decideRelayIdentityAction_unresolved_observation_is_a_noop", () => {
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: KEY_A,
      observation: unresolved,
    }),
    { kind: "none", reason: "unresolved" },
  );
});

test("decideRelayIdentityAction_relay_advertising_no_self_is_a_noop", () => {
  // A relay that stops advertising `self` must neither wipe nor clear the
  // stored key — absence is not a different identity.
  assert.deepEqual(
    decideRelayIdentityAction({
      storedRelayPubkey: KEY_A,
      observation: resolved(null),
    }),
    { kind: "none", reason: "unadvertised" },
  );
});

test("decideRelayIdentityAction_malformed_observation_is_a_noop", () => {
  for (const pubkey of ["nope", "", `${KEY_B}00`, "z".repeat(64)]) {
    assert.deepEqual(
      decideRelayIdentityAction({
        storedRelayPubkey: KEY_A,
        observation: resolved(pubkey),
      }),
      { kind: "none", reason: "invalid-observed" },
      `observed=${pubkey}`,
    );
  }
});

test("decideRelayIdentityAction_unresolved_with_no_stored_pubkey_does_not_adopt", () => {
  assert.deepEqual(decideRelayIdentityAction({ observation: unresolved }), {
    kind: "none",
    reason: "unresolved",
  });
});

// ---------------------------------------------------------------------------
// normalizeRelayPubkey
// ---------------------------------------------------------------------------

test("normalizeRelayPubkey_accepts_only_64_char_hex", () => {
  assert.equal(normalizeRelayPubkey(KEY_A.toUpperCase()), KEY_A);
  assert.equal(normalizeRelayPubkey(`  ${KEY_A}\n`), KEY_A);
  assert.equal(normalizeRelayPubkey(undefined), null);
  assert.equal(normalizeRelayPubkey(null), null);
  assert.equal(normalizeRelayPubkey(123), null);
  assert.equal(normalizeRelayPubkey(KEY_A.slice(1)), null);
  assert.equal(normalizeRelayPubkey(`${KEY_A}a`), null);
  assert.equal(normalizeRelayPubkey("g".repeat(64)), null);
});
