import assert from "node:assert/strict";
import test from "node:test";
import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import { buildCodingSessionTranscriptGenerationId } from "./codingSessionTranscriptPresentation.ts";
import {
  buildCodingSessionLink,
  codingSessionLinkGenerationId,
  parseCodingSessionLink,
} from "./codingSessionLink.ts";

const link = {
  channelId: "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9",
  providerPubkey: "a".repeat(64),
  targetKey: buildCodingSessionTargetKey({
    driver: "driver",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  }),
};

test("provider target opens without inventing a transcript sequence", () => {
  const url = buildCodingSessionLink(link);
  assert.deepEqual(parseCodingSessionLink(url), link);
  assert.ok(!url.includes("seq="));
  assert.equal(
    codingSessionLinkGenerationId(link),
    buildCodingSessionTranscriptGenerationId(
      link.channelId,
      link.providerPubkey,
      {
        driver: "driver",
        instanceId: "instance",
        sessionId: "session",
        generation: 1,
      },
    ),
  );
  assert.notEqual(
    codingSessionLinkGenerationId(link),
    codingSessionLinkGenerationId({ ...link, providerPubkey: "b".repeat(64) }),
  );
});

test("fact links, ambiguous parameters and unrelated URLs are not session navigation", () => {
  const url = buildCodingSessionLink(link);
  for (const value of [
    `${url}&seq=1`,
    `${url}&channel=${link.channelId}`,
    `${url}&other=x`,
    `${url}#fragment`,
    "https://example.com",
    buildCodingSessionLink({ ...link, channelId: "elsewhere" }),
    buildCodingSessionLink({ ...link, targetKey: `${link.targetKey}\n` }),
  ]) {
    assert.equal(parseCodingSessionLink(value), null, value);
  }
});
