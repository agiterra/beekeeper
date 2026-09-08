/**
 * The provenance hook, mounted.
 *
 * Two failures live here and nowhere else, because both are properties of the
 * *cache* rather than of the model: a verdict read from one relay being served
 * for another community's rows, and a cold tab that never re-asks after the
 * proof it was missing finally arrives. Both need a real QueryClient, a real
 * remount and the real observed-event bus, so this file mounts the hook rather
 * than testing a pure function that cannot express either.
 *
 * The relay is never touched: `fetchEvents` is injected through the hook's
 * deps. The events it answers with are really signed.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand.ts";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis.ts";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTrustedIngress.ts";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds.ts";

import { rolePackProvenanceKey } from "./rolePackProvenance.ts";

const CHANNEL = "channel-provenance-1";
const OTHER_CHANNEL = "channel-provenance-2";
const PROJECT_REF = `30621:${"a".repeat(64)}:beekeeper`;
const RELAY_A = "wss://relay-a.example";
const RELAY_B = "wss://relay-b.example";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-mounted-1";

const FOUNDER_SECRET = generateSecretKey();
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER = getPublicKey(PROVIDER_SECRET);

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

function sign(built, secret, createdAt) {
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    secret,
  );
}

const GENESIS = sign(
  buildCodingSessionGenesisEvent({
    channelId: CHANNEL,
    sessionRef: SESSION_REF,
  }),
  FOUNDER_SECRET,
  1_799_999_999,
);

const CREATE = sign(
  buildCodingSessionCreateEvent({
    channelId: CHANNEL,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: GENESIS.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER,
    model: null,
    title: "Advance the Packs tab",
    initialTurn: null,
  }),
  FOUNDER_SECRET,
  1_800_000_000,
);

const RECEIPT = finalizeEvent(
  {
    kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    created_at: 1_800_000_005,
    tags: [
      ["h", CHANNEL],
      ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
      ["csl-command", COMMAND_ID],
      ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
    ],
    content: JSON.stringify({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId: COMMAND_ID,
      status: "created",
      session: TARGET,
      error: null,
    }),
  },
  PROVIDER_SECRET,
);

const REPORT = finalizeEvent(
  {
    kind: KIND_CODING_SESSION_METADATA,
    created_at: 1_800_000_010,
    tags: [
      ["h", CHANNEL],
      ["cs-target", buildCodingSessionTargetKey(TARGET)],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-metadata/v1",
      session: TARGET,
      projectRef: null,
      repoRef: null,
      title: null,
      agentRef: null,
      provider: null,
      runtime: null,
      model: null,
      status: "running",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: false,
        threadSteer: false,
        context: false,
        diff: false,
        plan: false,
      },
      sessionRef: SESSION_REF,
    }),
  },
  PROVIDER_SECRET,
);

const ROW = {
  channelId: CHANNEL,
  targetKey: buildCodingSessionTargetKey(TARGET),
  metadataEventId: REPORT.id,
  signerPubkey: REPORT.pubkey,
  sessionRef: SESSION_REF,
};

const FULL_PROOF = [GENESIS, CREATE, RECEIPT, REPORT];
const NO_LIVE_CLIENT = { subscribeLive: async () => () => {} };

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

/** Answer the four per-kind reads out of one flat event list. */
function fetcherFor(eventsByRelay, relayRef, calls) {
  return async (filter) => {
    calls.push(filter);
    const events = eventsByRelay.get(relayRef.current) ?? [];
    return events.filter((event) => event.kind === filter.kinds[0]);
  };
}

async function mountHook({
  eventsByRelay,
  relayRef,
  calls,
  initialRelayUrl,
  fetchEvents = null,
  getRelaySelf = async () => null,
  authorityLiveClient = NO_LIVE_CLIENT,
}) {
  const React = await import("react");
  const { act, renderHook, waitFor } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { useRolePackProvenance } = await import("./useRolePackProvenance.ts");

  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const wrapper = ({ children }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const mounted = renderHook(
    ({ relayUrl }) =>
      useRolePackProvenance(
        {
          relayUrl,
          projectRef: PROJECT_REF,
          channelIds: [CHANNEL],
          rows: [ROW],
        },
        {
          getRelaySelf,
          authorityLiveClient,
          fetchEvents:
            fetchEvents ?? fetcherFor(eventsByRelay, relayRef, calls),
        },
      ),
    { initialProps: { relayUrl: initialRelayUrl }, wrapper },
  );
  return { act, mounted, queryClient, waitFor };
}

function stateOf(mounted) {
  const disposition = mounted.result.current.result?.dispositions.get(
    rolePackProvenanceKey(ROW),
  );
  return disposition?.state ?? null;
}

test("a verdict read from one relay is never served for another community", async () => {
  const relayRef = { current: RELAY_A };
  const eventsByRelay = new Map([
    [RELAY_A, FULL_PROOF],
    // The next community's relay holds the report and no proof at all.
    [RELAY_B, [REPORT]],
  ]);
  const calls = [];
  const { mounted, waitFor } = await mountHook({
    eventsByRelay,
    relayRef,
    calls,
    initialRelayUrl: RELAY_A,
  });

  try {
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));

    relayRef.current = RELAY_B;
    mounted.rerender({ relayUrl: RELAY_B });
    await waitFor(() => assert.equal(stateOf(mounted), "proof-unavailable"));
    assert.equal(
      mounted.result.current.result.dispositions.get(rolePackProvenanceKey(ROW))
        .reason,
      "No accepted lifecycle proof for this generation.",
    );
  } finally {
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("a null relay leaves the hook disabled rather than answering unattributably", async () => {
  const relayRef = { current: RELAY_A };
  const calls = [];
  const { act, mounted } = await mountHook({
    eventsByRelay: new Map([[RELAY_A, FULL_PROOF]]),
    relayRef,
    calls,
    initialRelayUrl: null,
  });

  try {
    await act(async () => {});
    assert.equal(mounted.result.current.result, null);
    assert.equal(mounted.result.current.isLoading, false);
    assert.equal(mounted.result.current.isFetching, false);
    assert.equal(calls.length, 0, "no read is issued without a relay");
  } finally {
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("proof that arrives after the first read re-asks, without polling", async () => {
  const relayRef = { current: RELAY_A };
  // The cold tab: the report is in hand, its proof is still replaying.
  const eventsByRelay = new Map([[RELAY_A, [REPORT]]]);
  const calls = [];
  const { act, mounted, waitFor } = await mountHook({
    eventsByRelay,
    relayRef,
    calls,
    initialRelayUrl: RELAY_A,
  });
  const { fanOutObservedCodingSessionEvents } = await import(
    "@/features/coding-sessions/lib/codingSessionObservedEvents.ts"
  );

  try {
    await waitFor(() => assert.equal(stateOf(mounted), "proof-unavailable"));
    const afterFirstRead = calls.length;

    // An event in a channel this hook does not watch changes nothing.
    await act(async () => {
      fanOutObservedCodingSessionEvents([
        { ...RECEIPT, tags: [["h", OTHER_CHANNEL]] },
      ]);
    });
    await act(async () => {});
    assert.equal(
      calls.length,
      afterFirstRead,
      "another channel's event issues no read",
    );

    eventsByRelay.set(RELAY_A, FULL_PROOF);
    await act(async () => {
      fanOutObservedCodingSessionEvents([RECEIPT]);
    });
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
    assert.ok(
      calls.length > afterFirstRead,
      "the late receipt invalidated the answer and it was re-read",
    );
  } finally {
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("a positive verdict is withdrawn the moment conflicting proof arrives, and stays withdrawn while the re-read is unconfirmed or incomplete", async () => {
  const relayRef = { current: RELAY_A };
  const eventsByRelay = new Map([[RELAY_A, FULL_PROOF]]);
  const calls = [];
  // A gate the test holds shut: while it is set, every read waits on it and,
  // when told to, fails instead of answering.
  let gate = null;
  const answer = fetcherFor(eventsByRelay, relayRef, calls);
  const fetchEvents = async (filter) => {
    if (gate) {
      await gate.opened;
      if (gate.fail) throw new Error("the relay closed the connection");
    }
    return answer(filter);
  };
  const { act, mounted, waitFor } = await mountHook({
    eventsByRelay,
    relayRef,
    calls,
    initialRelayUrl: RELAY_A,
    fetchEvents,
  });
  const { fanOutObservedCodingSessionEvents } = await import(
    "@/features/coding-sessions/lib/codingSessionObservedEvents.ts"
  );

  try {
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));

    // New lifecycle evidence for this channel: the old verdict must stop
    // being served before the replacement read has answered.
    let open;
    gate = {
      opened: new Promise((resolve) => {
        open = resolve;
      }),
      fail: true,
    };
    await act(async () => {
      fanOutObservedCodingSessionEvents([RECEIPT]);
    });
    await waitFor(() =>
      assert.equal(
        mounted.result.current.result,
        null,
        "the positive label is withdrawn while the re-read is in flight",
      ),
    );
    assert.equal(mounted.result.current.error, null);

    // The re-read fails: the proof reads are incomplete, so the row must not
    // return to a positive.
    await act(async () => {
      open();
    });
    await waitFor(() => assert.equal(stateOf(mounted), "proof-unavailable"));
    const disposition = mounted.result.current.result.dispositions.get(
      rolePackProvenanceKey(ROW),
    );
    assert.match(disposition.reason, /could not be read/);
    assert.match(disposition.reason, /closed the connection/);

    // Once the relay answers again, the verdict is re-earned, not remembered.
    gate = null;
    await act(async () => {
      mounted.result.current.refetch();
    });
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
  } finally {
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("a re-read paused while offline confirms nothing, so the old positive stays withdrawn until it completes", async () => {
  const relayRef = { current: RELAY_A };
  const eventsByRelay = new Map([[RELAY_A, FULL_PROOF]]);
  const calls = [];
  const { act, mounted, waitFor } = await mountHook({
    eventsByRelay,
    relayRef,
    calls,
    initialRelayUrl: RELAY_A,
  });
  const { onlineManager } = await import("@tanstack/react-query");
  const { fanOutObservedCodingSessionEvents } = await import(
    "@/features/coding-sessions/lib/codingSessionObservedEvents.ts"
  );

  try {
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
    const beforePause = calls.length;

    await act(async () => {
      onlineManager.setOnline(false);
      fanOutObservedCodingSessionEvents([RECEIPT]);
    });
    await waitFor(() =>
      assert.equal(
        mounted.result.current.result,
        null,
        "a paused re-read is not a confirmation",
      ),
    );
    assert.equal(calls.length, beforePause, "nothing was read while offline");
    assert.equal(mounted.result.current.isFetching, false);

    await act(async () => {
      onlineManager.setOnline(true);
    });
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
    assert.ok(calls.length > beforePause, "the re-read ran once back online");
  } finally {
    onlineManager.setOnline(true);
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("a manual warm recheck stays fetching until the deferred proof read answers", async () => {
  const relayRef = { current: RELAY_A };
  const eventsByRelay = new Map([[RELAY_A, FULL_PROOF]]);
  const calls = [];
  const answer = fetcherFor(eventsByRelay, relayRef, calls);
  let pending = null;
  let release;
  const { act, mounted, waitFor } = await mountHook({
    eventsByRelay,
    relayRef,
    calls,
    initialRelayUrl: RELAY_A,
    fetchEvents: async (filter) => {
      if (pending) await pending;
      return answer(filter);
    },
  });
  try {
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
    assert.equal(mounted.result.current.isFetching, false);
    pending = new Promise((resolve) => {
      release = resolve;
    });
    await act(async () => {
      mounted.result.current.refetch();
    });
    await waitFor(() => assert.equal(mounted.result.current.isFetching, true));
    assert.equal(
      mounted.result.current.isLoading,
      false,
      "cached data makes this a warm read",
    );
    assert.equal(
      mounted.result.current.result,
      null,
      "old proof stays withdrawn during the read",
    );
    await act(async () => {
      release();
    });
    await waitFor(() => assert.equal(mounted.result.current.isFetching, false));
    assert.equal(stateOf(mounted), "commissioned");
  } finally {
    release?.();
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("leaving Roles stops the evidence scan after its in-flight read settles", async () => {
  let release;
  const pending = new Promise((resolve) => {
    release = resolve;
  });
  const calls = [];
  const { act, mounted, waitFor } = await mountHook({
    initialRelayUrl: RELAY_A,
    fetchEvents: async (filter) => {
      calls.push(filter);
      await pending;
      return FULL_PROOF.filter((event) => event.kind === filter.kinds[0]);
    },
  });
  try {
    await waitFor(() => assert.equal(calls.length, 1));
    mounted.unmount();
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    assert.equal(
      calls.length,
      1,
      "no remaining proof kinds are fetched after unmount",
    );
  } finally {
    release();
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});

test("live operator receipt rechecks proof and a changed relay identity withdraws it", async () => {
  const operatorSecret = generateSecretKey();
  const relaySecret = generateSecretKey();
  let relayKey = getPublicKey(relaySecret);
  const operatorCreate = finalizeEvent(
    {
      kind: CREATE.kind,
      created_at: CREATE.created_at,
      tags: CREATE.tags,
      content: CREATE.content,
    },
    operatorSecret,
  );
  const transition = finalizeEvent(
    {
      kind: 44228,
      created_at: CREATE.created_at - 1,
      tags: [
        ["h", CHANNEL],
        ["csat-v", "csat1-1"],
        ["csat-genesis", GENESIS.id],
      ],
      content: JSON.stringify({
        genesisRef: GENESIS.id,
        prevAccepted: null,
        seq: 1,
        type: "grant-operator",
        granteePubkey: getPublicKey(operatorSecret),
      }),
    },
    FOUNDER_SECRET,
  );
  const receipt = finalizeEvent(
    {
      kind: 40099,
      created_at: CREATE.created_at - 1,
      tags: [["h", CHANNEL]],
      content: JSON.stringify({
        type: "coding_session_authority_transition_accepted",
        genesisRef: GENESIS.id,
        acceptedEventId: transition.id,
        seq: 1,
        transitionType: "grant-operator",
        granteePubkey: getPublicKey(operatorSecret),
      }),
    },
    relaySecret,
  );
  const events = [GENESIS, operatorCreate, RECEIPT, REPORT];
  let onLive;
  const { mounted, waitFor, act } = await mountHook({
    initialRelayUrl: RELAY_A,
    calls: [],
    fetchEvents: async (filter) =>
      events.filter((event) => filter.kinds.includes(event.kind)),
    getRelaySelf: async () => relayKey,
    authorityLiveClient: {
      subscribeLive: async (_filter, callback) => {
        onLive = callback;
        return () => {};
      },
    },
  });
  try {
    await waitFor(() => assert.equal(stateOf(mounted), "proof-unavailable"));
    await act(async () => {
      events.push(transition, receipt);
      onLive(receipt);
    });
    await waitFor(() => assert.equal(stateOf(mounted), "commissioned"));
    relayKey = getPublicKey(generateSecretKey());
    await act(async () => mounted.result.current.refetch());
    await waitFor(() => assert.equal(stateOf(mounted), "proof-unavailable"));
  } finally {
    const { cleanup } = await import("@testing-library/react");
    cleanup();
  }
});
