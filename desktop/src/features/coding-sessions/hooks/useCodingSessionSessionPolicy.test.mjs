import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCodingSessionPolicyFilters,
  CODING_SESSION_POLICY_HISTORY_LIMIT,
  readCodingSessionSessionPolicy,
} from "./useCodingSessionSessionPolicy.ts";

const CHANNEL = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
const SESSION = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
const GENESIS = "ce5d87ed".repeat(8);
const FOUNDER = "11".repeat(32);
const RELAY = "99".repeat(32);

const SCOPE = {
  channelRef: CHANNEL,
  sessionRef: SESSION,
  genesisRef: GENESIS,
  founderPubkey: FOUNDER,
};

const ENFORCEMENT =
  "a published policy is a stated intention, not an enforced limit: only budget.turns is enforced (at the provider's turn gate); every other field is read and shown, never counted";

function policyEvent() {
  return {
    id: "a5".repeat(32),
    pubkey: FOUNDER,
    created_at: 1_788_359_807,
    kind: 44245,
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
      ["csp-v", "buzz-coding-session-policy/v1"],
      ["csp-genesis", GENESIS],
    ],
    content: "{}",
    sig: "00".repeat(64),
  };
}

test("L2.3: every filter names its kinds, so the relay's p-gate never answers 403", () => {
  const filters = buildCodingSessionPolicyFilters(
    SCOPE,
    CODING_SESSION_POLICY_HISTORY_LIMIT,
    RELAY,
  );
  assert.equal(filters.length, 3);
  for (const filter of filters) {
    assert.ok(Array.isArray(filter.kinds) && filter.kinds.length > 0);
    assert.deepEqual(filter["#h"], [CHANNEL]);
  }
  assert.deepEqual(filters[0].kinds, [44245]);
  assert.deepEqual(filters[0]["#d"], [SESSION]);
  assert.deepEqual(filters[1].kinds, [44228]);
  assert.deepEqual(filters[1]["#csat-genesis"], [GENESIS]);
  assert.deepEqual(filters[2].authors, [RELAY]);
});

test("L2.3: the read hands the native fold the records and the accepted chain", async () => {
  const requests = [];
  const fetched = [];
  const result = await readCodingSessionSessionPolicy({
    scope: SCOPE,
    relayPubkey: RELAY,
    client: {
      fetchEvents: async (filter) => {
        fetched.push(filter.kinds[0]);
        return filter.kinds[0] === 44245 ? [policyEvent()] : [];
      },
    },
    invoke: async (command, args) => {
      requests.push([command, args]);
      return {
        schema: "buzz-coding-session-policy-fold-adapter/v1",
        implementation: "buzz-core",
        selected: null,
        excluded: [],
        enforcement: ENFORCEMENT,
      };
    },
  });
  assert.deepEqual(fetched.sort(), [40099, 44228, 44245]);
  const [command, args] = requests[0];
  assert.equal(command, "fold_coding_session_policies_command");
  assert.equal(
    args.request.schema,
    "buzz-coding-session-policy-fold-request/v1",
  );
  assert.equal(args.request.sessionRef, SESSION);
  assert.equal(args.request.genesisRef, GENESIS);
  assert.equal(args.request.founderPubkey, FOUNDER);
  // An umbrella with no accepted transitions has an empty chain, not a missing
  // one — the founder can always set policy without any grant at all.
  assert.deepEqual(args.request.grants, []);
  assert.equal(args.request.events.length, 1);
  assert.equal(result.enforcement, ENFORCEMENT);
  assert.equal(result.selected, null);
});

test("L2.3: an adapter response that is not the fold's own shape throws", async () => {
  await assert.rejects(
    readCodingSessionSessionPolicy({
      scope: SCOPE,
      relayPubkey: RELAY,
      client: { fetchEvents: async () => [] },
      invoke: async () => ({
        schema: "buzz-coding-session-policy-adapter/v1",
        implementation: "buzz-core",
        selected: null,
        excluded: [],
        enforcement: ENFORCEMENT,
      }),
    }),
    /malformed fold response/,
  );
});
