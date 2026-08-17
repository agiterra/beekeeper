import assert from "node:assert/strict";
import test from "node:test";

import {
  CHANNEL_EVENT_KINDS,
  CHANNEL_MESSAGE_EVENT_KINDS,
  CHANNEL_TIMELINE_CONTENT_KINDS,
  CODING_SESSION_EVENT_KINDS,
  isConversationalUnreadKind,
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_COMMAND,
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_PROVIDER_CATALOG,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_STREAM_MESSAGE,
  KIND_STREAM_MESSAGE_V2,
  KIND_STREAM_MESSAGE_DIFF,
  KIND_SYSTEM_MESSAGE,
  KIND_JOB_REQUEST,
  KIND_JOB_ACCEPTED,
  KIND_JOB_PROGRESS,
  KIND_JOB_RESULT,
  KIND_JOB_CANCEL,
  KIND_JOB_ERROR,
  KIND_HUDDLE_STARTED,
  KIND_HUDDLE_PARTICIPANT_JOINED,
  KIND_HUDDLE_PARTICIPANT_LEFT,
  KIND_HUDDLE_ENDED,
} from "./kinds.ts";

test("isConversationalUnreadKind_streamMessage_counts", () => {
  assert.equal(isConversationalUnreadKind(KIND_STREAM_MESSAGE), true);
});

test("isConversationalUnreadKind_streamMessageV2_counts", () => {
  // 40002 is a real message edit/v2 — must stay counted.
  assert.equal(isConversationalUnreadKind(KIND_STREAM_MESSAGE_V2), true);
});

test("isConversationalUnreadKind_streamMessageDiff_counts", () => {
  // 40008 is a real message diff — must stay counted.
  assert.equal(isConversationalUnreadKind(KIND_STREAM_MESSAGE_DIFF), true);
});

test("isConversationalUnreadKind_systemMessage_excluded", () => {
  // 40099 channel_created / member_joined rows must not inflate the pill.
  assert.equal(isConversationalUnreadKind(KIND_SYSTEM_MESSAGE), false);
});

test("isConversationalUnreadKind_allJobKinds_excluded", () => {
  for (const kind of [
    KIND_JOB_REQUEST,
    KIND_JOB_ACCEPTED,
    KIND_JOB_PROGRESS,
    KIND_JOB_RESULT,
    KIND_JOB_CANCEL,
    KIND_JOB_ERROR,
  ]) {
    assert.equal(isConversationalUnreadKind(kind), false, `kind ${kind}`);
  }
});

test("isConversationalUnreadKind_huddleLifecycle_excluded", () => {
  for (const kind of [
    KIND_HUDDLE_STARTED,
    KIND_HUDDLE_PARTICIPANT_JOINED,
    KIND_HUDDLE_PARTICIPANT_LEFT,
    KIND_HUDDLE_ENDED,
  ]) {
    assert.equal(isConversationalUnreadKind(kind), false, `kind ${kind}`);
  }
});

test("isConversationalUnreadKind_undefinedKind_countsAsConversational", () => {
  // Optimistic/pending rows whose kind has not populated must not be dropped.
  assert.equal(isConversationalUnreadKind(undefined), true);
});

test("isConversationalUnreadKind_unknownKind_countsAsConversational", () => {
  // An exclude-list, not an include-list: anything not explicitly excluded
  // (e.g. a future conversational kind) is kept.
  assert.equal(isConversationalUnreadKind(12345), true);
});

test("codingSessionKinds_matchBuzzCoreValues", () => {
  // Mirror of crates/buzz-core/src/kind.rs. A drift here is a wire break that
  // no type checker catches — the events simply stop matching.
  assert.deepEqual(
    {
      command: KIND_CODING_SESSION_COMMAND,
      closure: KIND_CODING_SESSION_CLOSURE,
      lifecycleCommand: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      providerCatalog: KIND_CODING_SESSION_PROVIDER_CATALOG,
      metadata: KIND_CODING_SESSION_METADATA,
      lifecycleReceipt: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      transcript: KIND_CODING_SESSION_TRANSCRIPT,
      genesis: KIND_CODING_SESSION_GENESIS,
      goal: KIND_CODING_SESSION_GOAL,
      authorityTransition: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      name: KIND_CODING_SESSION_NAME,
    },
    {
      command: 44220,
      closure: 44230,
      lifecycleCommand: 44221,
      providerCatalog: 44222,
      metadata: 44223,
      lifecycleReceipt: 44224,
      transcript: 44225,
      genesis: 44226,
      goal: 44227,
      authorityTransition: 44228,
      name: 44229,
    },
  );
  assert.equal(CODING_SESSION_EVENT_KINDS.length, 11);
});

test("codingSessionKinds_neverEnterTheChatTimeline", () => {
  // Coding sessions render in their own workspace, never as chat rows. Adding
  // any of these to the timeline set would interleave raw transcript items and
  // turn commands into the message list — and, via CHANNEL_EVENT_KINDS, into
  // the live subscription and unread tallies too.
  //
  // This is a regression guard, not a preference: the coding-session consumer
  // is built on the assumption that 442xx events reach it only through its own
  // trusted-ingress path.
  for (const kind of CODING_SESSION_EVENT_KINDS) {
    assert.equal(
      CHANNEL_TIMELINE_CONTENT_KINDS.includes(kind),
      false,
      `kind ${kind} must not be a timeline content kind`,
    );
    assert.equal(
      CHANNEL_MESSAGE_EVENT_KINDS.includes(kind),
      false,
      `kind ${kind} must not be a message kind`,
    );
    assert.equal(
      CHANNEL_EVENT_KINDS.includes(kind),
      false,
      `kind ${kind} must not be a channel event kind`,
    );
  }
});
