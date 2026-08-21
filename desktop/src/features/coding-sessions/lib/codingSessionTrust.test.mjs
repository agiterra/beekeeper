import assert from "node:assert/strict";
import test from "node:test";

import {
  LOCAL_PROVIDER_TRUST_LABEL,
  MAX_TRUST_LABEL_BYTES,
  isLocalProviderTrustEntry,
  isLowercaseHexPubkey,
  normalizeTrustLabel,
  normalizeTrustPubkey,
  trustEntriesFromRows,
  trustEntriesSignature,
  trustRowsFromEntries,
  utf8ByteLength,
  validateTrustRows,
} from "./codingSessionTrust.ts";

const KEY_A = "a".repeat(64);
const KEY_B = "b".repeat(64);
const NUL = String.fromCharCode(0);

function row(id, pubkey, label = "") {
  return { id, pubkey, label };
}

test("normalization matches the backend: trim + lowercase pubkeys, trim labels", () => {
  assert.equal(normalizeTrustPubkey(`  ${KEY_A.toUpperCase()} `), KEY_A);
  assert.equal(normalizeTrustLabel("  Build box  "), "Build box");
  assert.ok(isLowercaseHexPubkey(KEY_A));
  assert.ok(!isLowercaseHexPubkey(KEY_A.toUpperCase()));
  assert.ok(!isLowercaseHexPubkey("abc"));
  assert.ok(!isLowercaseHexPubkey("g".repeat(64)));
});

test("uppercase and padded input still normalizes into the persisted shape", () => {
  const entries = trustEntriesFromRows([
    row("1", ` ${KEY_A.toUpperCase()} `, " Build box "),
    row("2", KEY_B, "Laptop"),
  ]);
  assert.deepEqual(entries, [
    { pubkey: KEY_A, label: "Build box" },
    { pubkey: KEY_B, label: "Laptop" },
  ]);
});

test("blank rows are dropped rather than persisted or rejected", () => {
  const rows = [row("1", "   ", "   "), row("2", KEY_A, "Laptop")];
  assert.deepEqual(trustEntriesFromRows(rows), [
    { pubkey: KEY_A, label: "Laptop" },
  ]);
  assert.equal(validateTrustRows(rows).isValid, true);
});

test("a name with no key is an error, because the name is what disappears", () => {
  const result = validateTrustRows([row("1", "", "Build box")]);
  assert.equal(result.isValid, false);
  assert.match(result.rowErrors["1"], /public key/i);
});

test("malformed, duplicate, and oversized entries are each rejected on their own row", () => {
  const short = validateTrustRows([row("1", "abc")]);
  assert.equal(short.isValid, false);
  assert.match(short.rowErrors["1"], /64 hexadecimal/);

  const nonHex = validateTrustRows([row("1", "g".repeat(64))]);
  assert.equal(nonHex.isValid, false);
  assert.match(nonHex.rowErrors["1"], /64 hexadecimal/);

  // Case-insensitive duplicate: both normalize to the same trusted key, and
  // the backend rejects the whole config for it.
  const duplicate = validateTrustRows([
    row("1", KEY_A, "one"),
    row("2", KEY_A.toUpperCase(), "two"),
  ]);
  assert.equal(duplicate.isValid, false);
  assert.equal(duplicate.rowErrors["1"], undefined);
  assert.match(duplicate.rowErrors["2"], /already trusted/);

  const nul = validateTrustRows([row("1", KEY_A, `bad${NUL}label`)]);
  assert.equal(nul.isValid, false);
  assert.match(nul.rowErrors["1"], /NUL/);
});

test("the label cap is the boundary the Rust constant names, in bytes not characters", () => {
  const atLimit = "x".repeat(MAX_TRUST_LABEL_BYTES);
  assert.equal(validateTrustRows([row("1", KEY_A, atLimit)]).isValid, true);

  const overLimit = "x".repeat(MAX_TRUST_LABEL_BYTES + 1);
  const over = validateTrustRows([row("1", KEY_A, overLimit)]);
  assert.equal(over.isValid, false);
  assert.match(over.rowErrors["1"], /80 bytes/);

  // 40 two-byte characters are 80 bytes — legal — but 41 are not, which a
  // character-count check would wave through.
  const multibyte = "é".repeat(40);
  assert.equal(utf8ByteLength(multibyte), MAX_TRUST_LABEL_BYTES);
  assert.equal(validateTrustRows([row("1", KEY_A, multibyte)]).isValid, true);
  assert.equal(
    validateTrustRows([row("1", KEY_A, "é".repeat(41))]).isValid,
    false,
  );
});

test("a clean list validates and produces no row errors", () => {
  const result = validateTrustRows([
    row("1", KEY_A, LOCAL_PROVIDER_TRUST_LABEL),
    row("2", KEY_B, ""),
  ]);
  assert.deepEqual(result.rowErrors, {});
  assert.equal(result.isValid, true);
});

test("the local provider row is recognized by the label provisioning writes", () => {
  assert.ok(isLocalProviderTrustEntry({ label: LOCAL_PROVIDER_TRUST_LABEL }));
  assert.ok(
    isLocalProviderTrustEntry({ label: ` ${LOCAL_PROVIDER_TRUST_LABEL} ` }),
  );
  assert.ok(!isLocalProviderTrustEntry({ label: "Build box" }));
  assert.ok(!isLocalProviderTrustEntry({ label: "" }));
});

test("seeded rows carry stable positional ids that new rows can never collide with", () => {
  const rows = trustRowsFromEntries([
    { pubkey: KEY_A, label: "one" },
    { pubkey: KEY_B, label: "two" },
  ]);
  assert.deepEqual(
    rows.map((entry) => entry.id),
    ["seeded-0", "seeded-1"],
  );
  assert.deepEqual(
    rows.map((entry) => entry.pubkey),
    [KEY_A, KEY_B],
  );
});

test("the resync signature ignores normalization but not content", () => {
  const canonical = trustEntriesSignature([{ pubkey: KEY_A, label: "Laptop" }]);
  assert.equal(
    trustEntriesSignature([{ pubkey: KEY_A.toUpperCase(), label: " Laptop " }]),
    canonical,
  );
  assert.notEqual(
    trustEntriesSignature([{ pubkey: KEY_A, label: "Desktop" }]),
    canonical,
  );
  assert.notEqual(
    trustEntriesSignature([
      { pubkey: KEY_A, label: "Laptop" },
      { pubkey: KEY_B, label: "" },
    ]),
    canonical,
  );
});
