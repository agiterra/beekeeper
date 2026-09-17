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
  KIND_CODING_SESSION_HANDOVER,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_OBSERVATION,
  KIND_CODING_SESSION_PROVIDER_CATALOG,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
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
  KIND_REPO_STATE,
  KIND_PROJECT_TODO_OP,
  KIND_PULSE_ENTRY,
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
      lease: KIND_CODING_SESSION_LEASE,
      providerCatalog: KIND_CODING_SESSION_PROVIDER_CATALOG,
      metadata: KIND_CODING_SESSION_METADATA,
      lifecycleReceipt: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      transcript: KIND_CODING_SESSION_TRANSCRIPT,
      genesis: KIND_CODING_SESSION_GENESIS,
      goal: KIND_CODING_SESSION_GOAL,
      authorityTransition: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
      teamTransaction: KIND_CODING_SESSION_TEAM_TRANSACTION,
      name: KIND_CODING_SESSION_NAME,
    },
    {
      command: 44220,
      closure: 44230,
      lifecycleCommand: 44221,
      lease: 24223,
      providerCatalog: 44222,
      metadata: 44223,
      lifecycleReceipt: 44224,
      transcript: 44225,
      genesis: 44226,
      goal: 44227,
      authorityTransition: 44228,
      teamTransaction: 44244,
      name: 44229,
    },
  );
  assert.equal(CODING_SESSION_EVENT_KINDS.length, 14);
  assert.equal(
    KIND_CODING_SESSION_LEASE >= 20000 && KIND_CODING_SESSION_LEASE <= 29999,
    true,
    "the session lease must remain in Nostr's ephemeral kind range",
  );
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

test("the kinds Project Pulse missions read match buzz-core", () => {
  // The mission rows are folded in Rust from relay-signed 30618 ref state and
  // 44246 observations. Desktop does not query either kind itself — the native
  // command does — but the integers still have to agree, because a drift here
  // is a wire break no type checker catches: the events simply stop matching
  // and the surface renders a project as quieter than it is.
  //
  // `crates/buzz-core/src/kind.rs` calls 30618 `KIND_GIT_REPO_STATE`; this
  // file has carried the same integer as `KIND_REPO_STATE` since NIP-34
  // landed. One integer, two names — asserted here so the pair cannot drift
  // apart unnoticed, and so nobody adds a third constant for it.
  assert.deepEqual(
    {
      gitRepoState: KIND_REPO_STATE,
      codingSessionObservation: KIND_CODING_SESSION_OBSERVATION,
    },
    { gitRepoState: 30618, codingSessionObservation: 44246 },
  );
  // 30618 is a NIP-33 addressable kind; 44246 is a regular one. A mission fold
  // that treated an addressable ref-state event as append-only history would
  // paint superseded refs beside current ones.
  assert.equal(KIND_REPO_STATE >= 30000 && KIND_REPO_STATE <= 39999, true);
  assert.equal(
    KIND_CODING_SESSION_OBSERVATION >= 40000 &&
      KIND_CODING_SESSION_OBSERVATION <= 49999,
    true,
  );
});

test("the observation kind stays out of the chat timeline and the mock relay set", () => {
  // 44246 is not in `CODING_SESSION_EVENT_KINDS` on purpose (see kinds.ts):
  // that list also drives which kinds the e2e mock relay serves, and Desktop
  // reads observations only through the native mission fold.
  assert.equal(
    CODING_SESSION_EVENT_KINDS.includes(KIND_CODING_SESSION_OBSERVATION),
    false,
  );
  assert.equal(
    CHANNEL_TIMELINE_CONTENT_KINDS.includes(KIND_CODING_SESSION_OBSERVATION),
    false,
  );
  assert.equal(CHANNEL_EVENT_KINDS.includes(KIND_REPO_STATE), false);
});

test("the handover kind is read off the wire and served by the mock relay", () => {
  // 44247 (NIP-CSH) is the checkpoint/continuation record.
  // `useCodingSessionHandover` reads it — a bounded query plus a live REQ — so
  // by this list's own rule it belongs in `CODING_SESSION_EVENT_KINDS`, which
  // is also what the e2e mock relay serves. It stays out of the chat timeline:
  // that set is an allowlist of human-visible message kinds.
  assert.equal(KIND_CODING_SESSION_HANDOVER, 44247);
  assert.equal(
    KIND_CODING_SESSION_HANDOVER,
    KIND_CODING_SESSION_OBSERVATION + 1,
    "44247 is the next kind after the observation, with nothing between",
  );
  assert.equal(
    CODING_SESSION_EVENT_KINDS.includes(KIND_CODING_SESSION_HANDOVER),
    true,
  );
  assert.equal(
    CHANNEL_TIMELINE_CONTENT_KINDS.includes(KIND_CODING_SESSION_HANDOVER),
    false,
  );
});

test("projectScopedKinds_matchBuzzCoreValues", () => {
  // Mirror of crates/buzz-core/src/kind.rs `PROJECT_A_SCOPED_KINDS`: the
  // Pulse entry and the to-do op share one relay gate and one `#a` filter
  // shape, and a drift here is a wire break no type checker catches.
  assert.deepEqual(
    { pulse: KIND_PULSE_ENTRY, todoOp: KIND_PROJECT_TODO_OP },
    { pulse: 44240, todoOp: 44248 },
  );
});
