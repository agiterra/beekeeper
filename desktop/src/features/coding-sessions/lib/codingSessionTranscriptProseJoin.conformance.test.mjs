/**
 * The shared prose-join vectors, run through **this** client's projection
 * and transcript model (SV-36 S5).
 *
 * `conformance/transcript-prose-join/fixtures/vectors.json` is written by the
 * real Rust translator (plus hand-written attribution cases) and read by every
 * reader of `assistant_text` (CONTRACT.md § Readers). Loaded from the
 * repository root, relative to `desktop/`, never copied.
 *
 * Each vector's envelopes are handed over the way the trusted ingress store
 * hands them (`codingSessionTrustedIngress.ts`): one stream per (signer,
 * exact target), repeated deliveries of one event kept once, ordered by
 * `eventSeq`, projected with that stream's own generation id and signer.
 * Then `deriveCodingSessionTranscriptModel` joins them, with rule 7's
 * liveness resolved by the same pure function the surfaces call.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { test } from "node:test";

import {
  codingSessionArrivingPieceId,
  isCodingSessionProducerWriting,
} from "./codingSessionProseArriving.ts";
import {
  codingSessionProseLastEventId,
  deriveCodingSessionTranscriptModel,
  isCodingSessionProseArriving,
  joinConsecutiveCodingSessionProse,
} from "./codingSessionTranscriptModel.ts";
import { withoutCodingSessionSubagentEchoedReport } from "./codingSessionSubagents.ts";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection.ts";

const fixture = JSON.parse(
  readFileSync(
    resolve(
      process.cwd(),
      "../conformance/transcript-prose-join/fixtures/vectors.json",
    ),
    "utf8",
  ),
);

function streamKey(signer, target) {
  return JSON.stringify([
    signer,
    target.driver,
    target.instanceId,
    target.sessionId,
    target.generation,
  ]);
}

function sameTarget(left, right) {
  return (
    left.driver === right.driver &&
    left.instanceId === right.instanceId &&
    left.sessionId === right.sessionId &&
    left.generation === right.generation
  );
}

/** One stream per (signer, exact target), as the ingress snapshot yields. */
function streamsOf(vector) {
  const streams = new Map();
  for (const envelope of vector.input) {
    const target = envelope.content.session;
    const key = streamKey(envelope.signer, target);
    const stream = streams.get(key) ?? {
      key,
      signer: envelope.signer,
      target,
      byEventId: new Map(),
    };
    // Rule 0: one event, one piece — the ingress keys records by event id.
    stream.byEventId.set(envelope.eventId, {
      target,
      eventSeq: envelope.content.eventSeq,
      timestamp: envelope.content.timestamp,
      turnId: envelope.content.turnId,
      item: envelope.content.item,
      sourceEventId: envelope.eventId,
    });
    streams.set(key, stream);
  }
  return [...streams.values()]
    .sort((left, right) => (left.key < right.key ? -1 : 1))
    .map((stream) => ({
      ...stream,
      envelopes: [...stream.byEventId.values()].sort(
        (left, right) => left.eventSeq - right.eventSeq,
      ),
    }));
}

function projectStream(stream, generationId = stream.key) {
  return projectCodingSessionTranscript(stream.envelopes, {
    channelId: "vector-channel",
    generationId,
    bridgeSource: { pubkey: stream.signer, label: "vector" },
  });
}

/**
 * The coordination fold's reading of a vector's lease (`provider_reachable`
 * needs the current generation, a `live` unexpired lease and no
 * stopped/disconnected status); the fold knows every generation it holds.
 */
function reachabilityOf(vector, stream, superseded, status) {
  const lease =
    vector.sessionLease.find(
      (entry) =>
        entry.signer === stream.signer &&
        sameTarget(entry.target, stream.target),
    )?.lease ?? null;
  const terminal = status === "stopped" || status === "disconnected";
  return {
    known: true,
    reachable: !terminal && !superseded && lease === "live",
  };
}

function producerWritingOf(vector, stream, streams) {
  const generationSuperseded = streams.some(
    (other) =>
      other.signer === stream.signer &&
      other.target.driver === stream.target.driver &&
      other.target.instanceId === stream.target.instanceId &&
      other.target.sessionId === stream.target.sessionId &&
      other.target.generation > stream.target.generation,
  );
  const status =
    vector.sessionStatus.find(
      (entry) =>
        entry.signer === stream.signer &&
        sameTarget(entry.target, stream.target),
    )?.status ?? null;
  return isCodingSessionProducerWriting({
    reachability: reachabilityOf(vector, stream, generationSuperseded, status),
    status,
    generationSuperseded,
  });
}

function isProse(item) {
  return (
    (item.type === "message" && item.role === "assistant") ||
    item.type === "thought"
  );
}

/** Every prose message the model shows, in reading order, with its stream. */
function modelMessages(vector) {
  const streams = streamsOf(vector);
  const messages = [];
  for (const stream of streams) {
    const model = deriveCodingSessionTranscriptModel(projectStream(stream), {
      isWorking: false,
      producerWriting: producerWritingOf(vector, stream, streams),
    });
    const entries = model.blocks.flatMap((block) =>
      block.kind === "turn" ? block.entries : [block.entry],
    );
    for (const entry of entries) {
      // A Turn result's synthesized answer row is the desktop's reading of
      // the `result` item, not a 44225 prose piece; it has no source event.
      if (entry.kind !== "item" || !isProse(entry.item)) continue;
      if (entry.item.sourceEventId === undefined) continue;
      messages.push({ item: entry.item, stream });
    }
  }
  return messages;
}

test("the fixture is the shared one, and names this reader", () => {
  assert.equal(fixture.schema, "buzz.conformance/transcript-prose-join@1");
  assert.equal(fixture.vectors.length, 15);
  const contract = readFileSync(
    resolve(process.cwd(), "../conformance/transcript-prose-join/CONTRACT.md"),
    "utf8",
  );
  assert.match(contract, /codingSessionTranscriptProseJoin\.conformance/);
});

for (const vector of fixture.vectors) {
  test(`prose-join vector ${vector.name}`, () => {
    const actual = modelMessages(vector);
    assert.equal(
      actual.length,
      vector.expectedMessages.length,
      `${vector.name}: message count`,
    );
    vector.expectedMessages.forEach((expected, index) => {
      const { item, stream } = actual[index];
      const where = `${vector.name} message ${index}`;
      assert.equal(
        item.type,
        expected.kind === "reasoning" ? "thought" : "message",
        `${where} kind`,
      );
      assert.equal(item.sourceEventId, expected.firstEventId, `${where} first`);
      assert.equal(
        codingSessionProseLastEventId(item),
        expected.lastEventId,
        `${where} last`,
      );
      assert.equal(item.text, expected.text, `${where} text, byte for byte`);
      assert.equal(item.turnId ?? null, expected.turnId, `${where} turnId`);
      assert.equal(
        item.parentToolId ?? null,
        expected.parentToolId,
        `${where} parentToolId`,
      );
      assert.equal(stream.signer, expected.signer, `${where} signer`);
      assert.deepEqual(stream.target, expected.target, `${where} target`);
      assert.equal(
        isCodingSessionProseArriving(item),
        expected.arriving,
        `${where} arriving`,
      );
    });
  });
}

test("the vectors exercise both arriving outcomes", () => {
  const all = fixture.vectors.flatMap((vector) => vector.expectedMessages);
  assert.ok(all.some((message) => message.arriving === true));
  assert.ok(all.some((message) => message.arriving === false));
});

// ── Divergence 3: the exact target, not the display generation alone ──────

test("two targets under one generation id never join (exact target)", () => {
  const vector = fixture.vectors.find(
    (candidate) => candidate.name === "two-generations-share-turn-id",
  );
  // Worst case for the old key: both generations projected under ONE
  // display generation id, so `sessionId` no longer tells them apart. Only
  // the target the item was projected from (`sourceTargetKey`) does.
  const items = streamsOf(vector).flatMap((stream) =>
    projectStream(stream, "one-generation-id"),
  );
  const prose = joinConsecutiveCodingSessionProse(
    items.filter((item) => item.type === "message"),
  );
  assert.deepEqual(
    prose.map((item) => item.text),
    vector.expectedMessages.map((message) => message.text),
  );
});

// ── Rule 7, past the vectors ────────────────────────────────────────────────

const LIVE = { known: true, reachable: true };

test("arriving needs positive lease evidence; unknown or unreachable is not", () => {
  const live = (reachability, status = "running", superseded = false) =>
    isCodingSessionProducerWriting({
      reachability,
      status,
      generationSuperseded: superseded,
    });
  assert.equal(live(LIVE), true);
  assert.equal(live(undefined), false, "no coordination read");
  assert.equal(live({ known: false }), false, "generation never proved");
  assert.equal(live({ known: true, reachable: false }), false, "lapsed");
  assert.equal(live(LIVE, "running", true), false, "higher generation");
  assert.equal(live(LIVE, null), true, "no metadata leaves it to the lease");
  // The fold's `terminal` omits `completed`; the rule does not.
  for (const status of ["completed", "stopped", "disconnected"]) {
    assert.equal(live(LIVE, status), false, `${status} ends the target`);
  }
  // A turn's own failure or interruption does not end the session.
  for (const status of ["interrupted", "failed", "idle", "waiting_for_input"]) {
    assert.equal(live(LIVE, status), true, `${status} leaves it to the lease`);
  }
});

const TURN = "turn-1";
const say = (id, text = id, extra = {}) => ({
  id,
  type: "message",
  renderClass: "message",
  role: "assistant",
  title: "Response",
  text,
  timestamp: "2026-10-07T12:00:00.000Z",
  turnId: TURN,
  ...extra,
});
const ask = {
  ...say("prompt", "Go"),
  role: "user",
  title: "Prompt",
};
const status = {
  id: "status",
  type: "lifecycle",
  renderClass: "status",
  title: "Status",
  text: "compacting",
  timestamp: "2026-10-07T12:00:00.000Z",
  turnId: TURN,
};

test("only the turn's last item, when prose and unterminated, is arriving", () => {
  assert.equal(codingSessionArrivingPieceId([ask, say("a")], true), "a");
  assert.equal(codingSessionArrivingPieceId([ask, say("a")], false), null);
  assert.equal(
    codingSessionArrivingPieceId([ask, say("a"), status], true),
    null,
    "a status after the prose ends the message, though the view hides it",
  );
  assert.equal(
    codingSessionArrivingPieceId([say("a", "a", { turnId: undefined })], true),
    null,
    "an item with no turn is never arriving",
  );
  const interrupted = { ...status, id: "stop", title: "Interrupted" };
  assert.equal(
    codingSessionArrivingPieceId([ask, interrupted, say("late")], true),
    null,
  );
});

test("the model marks the joined message, not its first piece alone", () => {
  const model = deriveCodingSessionTranscriptModel(
    [ask, say("p1", "One.\n\n"), say("p2", "Two.\n\n"), say("p3", "Three.")],
    { isWorking: false, producerWriting: true },
  );
  const [turn] = model.blocks;
  const prose = turn.entries.filter(
    (entry) => entry.kind === "item" && entry.item.role === "assistant",
  );
  assert.equal(prose.length, 1);
  assert.equal(prose[0].item.id, "p1", "keyed on the first piece");
  assert.equal(prose[0].item.text, "One.\n\nTwo.\n\nThree.");
  assert.equal(isCodingSessionProseArriving(prose[0].item), true);

  const settled = deriveCodingSessionTranscriptModel(
    [ask, say("p1", "One.\n\n"), say("p2", "Two.")],
    { isWorking: true },
  );
  assert.equal(
    isCodingSessionProseArriving(settled.blocks[0].entries[1].item),
    false,
    "isWorking alone never makes prose arriving",
  );
});

test("a turn a later turn followed is not arriving", () => {
  const model = deriveCodingSessionTranscriptModel(
    [ask, say("a"), { ...ask, id: "prompt-2", turnId: "turn-2" }],
    { isWorking: false, producerWriting: true },
  );
  const first = model.blocks[0];
  assert.equal(first.superseded, true);
  assert.equal(isCodingSessionProseArriving(first.entries[1].item), false);
});

// ── Census: the SV-97 subagent echo drop reads joined pieces ──────────────

test("a subagent answer published in pieces is dropped whole when it echoes the report", () => {
  const child = (id, text) => say(id, text, { parentToolId: "task-1" });
  const tool = {
    id: "read",
    type: "tool",
    title: "Read",
    turnId: TURN,
    parentToolId: "task-1",
  };
  const children = [
    child("look", "Looking."),
    tool,
    child("a1", "Found it.\n\n"),
    child("a2", "It is in `main.rs`."),
  ];
  const report = "Found it.\n\nIt is in `main.rs`.";
  assert.deepEqual(
    withoutCodingSessionSubagentEchoedReport(children, report).map(
      (item) => item.id,
    ),
    ["look", "read"],
    "both pieces of the echoed answer go, nothing before the tool does",
  );
  // The one-piece case still holds, and prose that is not the report stays.
  assert.deepEqual(
    withoutCodingSessionSubagentEchoedReport(
      [child("only", report)],
      report,
    ).map((item) => item.id),
    [],
  );
  assert.equal(
    withoutCodingSessionSubagentEchoedReport(children, "Something else."),
    children,
  );
});
