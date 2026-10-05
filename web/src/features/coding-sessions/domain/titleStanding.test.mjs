/**
 * SV-31 standing back-fill: which held titles the observer asks about, and
 * exactly what it asks the relay for.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { CodingSessionObserverStore } from "./catalog.ts";
import { buildCodingSessionTargetKey } from "./keys.ts";
import {
  CHANNEL_ID,
  createEvent,
  generatedTitleEvent,
  metadataEvent,
  newSigner,
  OTHER_SESSION_REF,
  receiptEvent,
  SESSION_REF,
  target,
} from "./testFixtures.mjs";
import {
  codingSessionTitleStandingFilters,
  codingSessionTitleStandingGaps,
  isTitleStandingPageTruncated,
  TITLE_STANDING_WINDOW_AFTER_SECONDS,
  TITLE_STANDING_WINDOW_BEFORE_SECONDS,
} from "./titleStanding.ts";

const channels = [CHANNEL_ID];
const CREATE_ID = "a".repeat(64);

function factsOf(events) {
  const store = new CodingSessionObserverStore();
  store.ingest(events, channels);
  return store.facts(channels);
}

/** A session the reader shows, but whose create fell off the page. */
function unprovenSession(provider) {
  return [
    receiptEvent(provider, { status: "created" }),
    metadataEvent(provider, { sessionRef: SESSION_REF }),
  ];
}

test("a shown session's unproven title is a gap, carrying its create id", () => {
  const provider = newSigner();
  const title = generatedTitleEvent(provider, {
    createEventId: CREATE_ID,
    created_at: 1_700_000_100,
  });
  const gaps = codingSessionTitleStandingGaps(
    factsOf([...unprovenSession(provider), title]),
  );
  assert.deepEqual(gaps, [
    {
      titleEventId: title.id,
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      signerPubkey: provider.pubkey,
      targetKey: buildCodingSessionTargetKey(target()),
      createEventId: CREATE_ID,
      createdAt: 1_700_000_100,
    },
  ]);
});

test("a title already standing is never asked about", () => {
  const founder = newSigner();
  const provider = newSigner();
  const gaps = codingSessionTitleStandingGaps(
    factsOf([
      createEvent(founder, {
        providerAuthorityPubkey: provider.pubkey,
        sessionRef: SESSION_REF,
      }),
      ...unprovenSession(provider),
      generatedTitleEvent(provider),
    ]),
  );
  assert.deepEqual(gaps, []);
});

test("a title for a session the reader does not show is left alone", () => {
  // Back-filling it would bring an old session back onto the list with a
  // status from the past; the read settles names, not membership.
  const provider = newSigner();
  const gaps = codingSessionTitleStandingGaps(
    factsOf([
      ...unprovenSession(provider),
      generatedTitleEvent(provider, { sessionRef: OTHER_SESSION_REF }),
    ]),
  );
  assert.deepEqual(gaps, []);
});

test("the read is the create by id, then a window around each title", () => {
  const provider = newSigner();
  const gaps = codingSessionTitleStandingGaps(
    factsOf([
      ...unprovenSession(provider),
      generatedTitleEvent(provider, {
        createEventId: CREATE_ID,
        created_at: 1_700_000_100,
      }),
    ]),
  );
  const since = 1_700_000_100 - TITLE_STANDING_WINDOW_BEFORE_SECONDS;
  const until = 1_700_000_100 + TITLE_STANDING_WINDOW_AFTER_SECONDS;
  assert.deepEqual(codingSessionTitleStandingFilters(gaps), [
    { kinds: [44221], "#h": [CHANNEL_ID], ids: [CREATE_ID], limit: 1 },
    {
      kinds: [44223],
      "#h": [CHANNEL_ID],
      authors: [provider.pubkey],
      since,
      until,
      limit: 1000,
    },
    {
      kinds: [44224],
      "#h": [CHANNEL_ID],
      authors: [provider.pubkey],
      since,
      until,
      limit: 1000,
    },
  ]);
});

test("a full by-id page is complete; a full window page is truncated", () => {
  assert.equal(
    isTitleStandingPageTruncated(
      { kinds: [44221], "#h": [CHANNEL_ID], ids: [CREATE_ID], limit: 1 },
      1,
    ),
    false,
  );
  assert.equal(
    isTitleStandingPageTruncated(
      { kinds: [44223], "#h": [CHANNEL_ID], authors: ["x"], limit: 1000 },
      1000,
    ),
    true,
  );
});
