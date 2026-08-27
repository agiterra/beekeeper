import assert from "node:assert/strict";
import { test } from "node:test";
import { classifyCodingSessionEvent } from "./trust.ts";
import {
  CHANNEL_ID,
  corruptSignature,
  metadataEvent,
  newSigner,
  receiptEvent,
  resign,
  transcriptEvent,
} from "./testFixtures.mjs";

const channels = new Set([CHANNEL_ID]);

test("a well-formed, correctly signed metadata event is accepted", () => {
  const provider = newSigner();
  const classified = classifyCodingSessionEvent(
    metadataEvent(provider),
    channels,
  );
  assert.equal(classified.kind, "metadata");
  assert.equal(classified.signerPubkey, provider.pubkey);
  assert.equal(classified.metadata.status, "idle");
});

test("an invalid signature is rejected, not merely counted as malformed", () => {
  const provider = newSigner();
  for (const event of [
    metadataEvent(provider),
    receiptEvent(provider),
    transcriptEvent(provider),
  ]) {
    assert.equal(
      classifyCodingSessionEvent(corruptSignature(event), channels).kind,
      "invalid-signature",
      `kind ${event.kind} must reject a broken signature`,
    );
  }
});

test("an event whose payload was edited after signing is rejected", () => {
  const provider = newSigner();
  const event = metadataEvent(provider);
  const tampered = {
    ...event,
    content: event.content.replace('"idle"', '"running"'),
  };
  assert.equal(
    classifyCodingSessionEvent(tampered, channels).kind,
    "invalid-signature",
  );
});

test("a fact from outside the subscribed channel is not admitted", () => {
  const provider = newSigner();
  const event = metadataEvent(provider, {
    channelId: "99999999-9999-4999-8999-999999999999",
  });
  assert.equal(classifyCodingSessionEvent(event, channels).kind, "malformed");
});

test("a coding-session kind with no version tag is malformed, not ignored", () => {
  const provider = newSigner();
  const event = metadataEvent(provider);
  const stripped = resign(provider, {
    ...event,
    tags: event.tags.filter((tag) => tag[0] !== "csm-v"),
  });
  assert.equal(
    classifyCodingSessionEvent(stripped, channels).kind,
    "malformed",
  );
});

test("a cs-target tag that disagrees with the payload is malformed", () => {
  const provider = newSigner();
  const event = metadataEvent(provider);
  const swapped = resign(provider, {
    ...event,
    tags: event.tags.map((tag) =>
      tag[0] === "cs-target" ? ["cs-target", "coding-session/v1|1:x"] : tag,
    ),
  });
  assert.equal(classifyCodingSessionEvent(swapped, channels).kind, "malformed");
});

test("tags in the wrong order are refused even when the set is right", () => {
  const provider = newSigner();
  const event = metadataEvent(provider);
  const reordered = resign(provider, {
    ...event,
    tags: [...event.tags].reverse(),
  });
  assert.equal(
    classifyCodingSessionEvent(reordered, channels).kind,
    "malformed",
  );
});

test("an unrelated kind is irrelevant rather than malformed", () => {
  const provider = newSigner();
  const event = resign(provider, { ...metadataEvent(provider), kind: 1 });
  assert.equal(classifyCodingSessionEvent(event, channels).kind, "irrelevant");
});
