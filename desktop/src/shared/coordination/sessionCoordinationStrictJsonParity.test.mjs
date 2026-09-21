/**
 * Parity between the Pulse's strict gate and the session decoder — over the
 * **shared** vectors, not over a fixture list of this file's own.
 *
 * `isStrictMetadataContent` (this directory) and `parseBuzzCodingSessionMetadata`
 * (`features/coding-sessions/lib/codingSessionIngressPayloads.ts`) both read
 * kind 44223. They are allowed to disagree in one direction only: the gate may
 * be *more* open than the decoder (finding 34's fix makes `capabilities` and a
 * few routing tokens open sets the decoder still closes), but the gate must
 * never be *stricter* — never refuse a shape the decoder itself accepts. A
 * gate that is stricter than the decoder is exactly finding 34: real signed
 * events the rest of the app can read, quietly excluded from the Pulse.
 *
 * Until lane 216 this test compared the two readers over twelve fixtures it
 * maintained itself, and it therefore did not notice that the gate refused
 * `composeRef` while the decoder accepted it — the forbidden direction, on a
 * key the provider emits for every seat staged from a composed pack. That is
 * the whole failure mode `conformance/README.md` exists to prevent: a reader's
 * tests using the reader's own fixtures. So the fixtures are gone and the
 * shared vectors are loaded instead. Adding a vector there now exercises this
 * parity rule for free.
 *
 * This is still a black-box check — same bytes in, does each accept them —
 * not an import of one decoder's internals into the other.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { parseBuzzCodingSessionMetadata } from "../../features/coding-sessions/lib/codingSessionIngressPayloads.ts";
import { isStrictMetadataContent } from "./sessionCoordinationStrictJson.ts";

/** Every 44223 vector, as `[name, content string]`. */
function sharedVectors() {
  const parsed = JSON.parse(
    readFileSync(
      resolve(
        process.cwd(),
        "../conformance/coding-session-records/44223-metadata/fixtures/vectors.json",
      ),
      "utf8",
    ),
  );
  assert.equal(parsed.kind, 44223);
  return parsed.vectors.map((vector) => [
    vector.name,
    JSON.stringify(vector.content),
  ]);
}

test("the strict gate never rejects a shape the session decoder accepts", () => {
  for (const [name, source] of sharedVectors()) {
    if (parseBuzzCodingSessionMetadata(source) === null) continue;
    assert.equal(
      isStrictMetadataContent(source),
      true,
      `the session decoder accepted vector "${name}" but the strict gate ` +
        `refused it. The gate may be more open than the decoder, never ` +
        `stricter: a Pulse that silently drops a session the rest of the app ` +
        `renders is finding 34. ${source}`,
    );
  }
});

test("the shared vectors actually reach the decoder (the test proves something)", () => {
  // Guards the test above against a silent no-op: if every vector failed to
  // decode, that loop would pass trivially without checking anything.
  const vectors = sharedVectors();
  const decoded = vectors.filter(
    ([, source]) => parseBuzzCodingSessionMetadata(source) !== null,
  ).length;
  assert.ok(
    decoded >= 10,
    `only ${decoded} of ${vectors.length} shared vectors decoded`,
  );
});

test("the gate may be more open than the decoder — the one allowed divergence", () => {
  // Not a shared vector, because it is not a disagreement with buzz-core: an
  // unknown boolean capability is accepted by the gate (open map, finding 34)
  // and refused by the decoder's `decodeCapabilities`, which knows only
  // `promptImage` as an extra key. That is the allowed direction — being more
  // forgiving never drops a real session — and it is asserted here so the
  // allowance stays a decision rather than drifting into the other direction.
  const base = JSON.parse(
    sharedVectors().find(([name]) => name === "base-twelve-key")[1],
  );
  const source = JSON.stringify({
    ...base,
    capabilities: { ...base.capabilities, futureThing: true },
  });
  assert.equal(parseBuzzCodingSessionMetadata(source), null);
  assert.equal(isStrictMetadataContent(source), true);
});
