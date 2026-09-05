/**
 * The hire host's grant, under the relay's back-pressure.
 *
 * Live on 2026-08-31 (item 103, finding 3): a team launch published three
 * 44228 authority writes, this host published three more for the seat it had
 * just created, and the fourth came back `rate-limited: quota exceeded; retry
 * in 2s`. The host disclosed "seated, but not granted" and stopped — a working
 * agent left mute over a two-second wait, because a "come back later" was read
 * as a refusal.
 *
 * Everything below the hook is production code. Only the outside world is
 * injected, so what these assert is the sequence a running desktop performs.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

/**
 * Every Tauri command this host invoked, in order.
 *
 * REVIEW-L9 F2: `install_coding_session_seat_hooks` had no live caller, so a
 * hire armed nothing and Pulse showed silence. This records the IPC so the
 * install is a thing a test watches rather than a thing a comment claims.
 */
const ipcCalls = [];
const tauriInternals = {
  invoke(command, args) {
    ipcCalls.push({ command, args });
    if (command === "install_coding_session_seat_hooks") {
      return Promise.resolve({
        wipRef: "refs/heads/wip/builder/ab12cd34",
        hooksDir: `${args.request.worktreePath}/.git/hooks`,
        hooksWritten: ["post-commit", "prepare-commit-msg"],
        configScope: "worktree",
        dispatch: "worktree",
        changed: true,
        signing: "unsigned",
      });
    }
    return Promise.reject(new Error(`unmocked: ${command}`));
  },
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const LEAD_SECRET = generateSecretKey();
const LEAD_PUBKEY = getPublicKey(LEAD_SECRET);
const OPERATOR_SECRET = generateSecretKey();
const OPERATOR_PUBKEY = getPublicKey(OPERATOR_SECRET);
const PROVIDER_PUBKEY = "9".repeat(64);
const ADA_PUBKEY = "d".repeat(64);
const HIRE_CREATED_AT = 1_700_000_000;

/** The relay's own words when the per-pubkey admission budget is spent. */
const QUOTA_EXCEEDED = "rate-limited: quota exceeded; retry in 2s";

const LEAD_TARGET = {
  driver: "claude",
  instanceId: "claude-primary",
  sessionId: "lead-session",
  generation: 1,
};

const UMBRELLA = {
  umbrellaKey: SESSION_REF,
  sessionRef: SESSION_REF,
  genesisRef: GENESIS_REF,
  title: "Agent Teams",
  founderPubkey: OPERATOR_PUBKEY,
  executions: [
    {
      activeGeneration: {
        agentRef: LEAD_PUBKEY,
        role: "lead",
        status: "running",
      },
    },
  ],
};

const CLAUDE_RUNTIME = {
  instanceRef: "claude-primary",
  runtime: "claude",
  driver: "claude",
  label: "Claude Code",
  authState: "ready",
  defaultModel: "default",
  allowedModels: ["default"],
  capabilities: {},
};

const CLAUDE_CATALOG = [
  "default",
  "claude-fable-5[1m]",
  "haiku",
  "opus[1m]",
  "sonnet",
];

async function signedHire() {
  const { buildCodingSessionHireEvent } = await import(
    "../lib/codingSessionHireWire.ts"
  );
  return finalizeEvent(
    {
      created_at: HIRE_CREATED_AT,
      ...buildCodingSessionHireEvent({
        channelId: CHANNEL_ID,
        commandId: "csl-hire-1",
        sessionRef: SESSION_REF,
        genesisRef: GENESIS_REF,
        role: "builder",
        providerInstanceRef: "claude-primary",
        model: null,
        brief: "Take the badge lane. Red test first.",
      }),
    },
    LEAD_SECRET,
  );
}

/**
 * Mount the real hook with the relay answering `grantAnswers` in order.
 *
 * Each entry is either null (the grant landed) or the message the relay
 * answered with. The list is consumed per grant *call*, so a retry reads the
 * next entry.
 */
async function harness({
  grantAnswers = [],
  umbrellas = [UMBRELLA],
  /** The project's 30624 the host's reader answers with, by project ref. */
  packSources = new Map(),
} = {}) {
  ipcCalls.length = 0;
  /** Every custody and pack-source call this host made, in order. */
  const staging = [];
  // A fake wall clock the injected sleep advances, so the grant's 60 s wait
  // budget is measured against the delays this test serves instantly.
  const { act, renderHook } = await import("@testing-library/react");
  const { DEFAULT_CODING_SESSION_HIRE_POLICY } = await import(
    "../lib/codingSessionHirePolicy.ts"
  );
  const { useCodingSessionHire } = await import("./useCodingSessionHire.ts");

  const published = [];
  const grants = [];
  const delays = [];
  const worktreeCalls = [];
  let clockMs = 0;
  let listener = null;

  const deps = {
    subscribe: (receive) => {
      listener = receive;
      return () => {
        listener = null;
      };
    },
    fetchRosterFold: async () => ({
      accepted: new Map([[LEAD_PUBKEY, "operator"]]),
    }),
    createWorktree: async (input) => {
      worktreeCalls.push(input);
      return {
        path: `/tmp/trees/${input.name}`,
        branch: `session/${input.name}`,
      };
    },
    stageCreateHint: async () => {},
    seatDeps: {
      ensureMembership: async () => {},
      stageSeat: async (input) => {
        staging.push(["stageSeat", input]);
        return {
          packStaged: true,
          packRef:
            input.packSource === null
              ? null
              : {
                  repo: input.packSource.repo,
                  sha: "dd935f43".padEnd(40, "0"),
                  role: input.role,
                  path: `${input.packSource.path}/${input.role}`,
                },
        };
      },
      clearSeat: async () => {},
      fetchPackSource: async (projectRef) => {
        staging.push(["fetchPackSource", projectRef]);
        return packSources.get(projectRef) ?? null;
      },
    },
    signer: async (input) =>
      finalizeEvent({ created_at: HIRE_CREATED_AT, ...input }, OPERATOR_SECRET),
    publisher: {
      publishEvent: async (event) => {
        published.push(event);
        return event;
      },
    },
    awaitSeatReceipt: async ({ channelId, commandId }) => ({
      driver: "claude",
      instanceId: "claude-primary",
      sessionId: `${channelId}:${commandId}`,
      generation: 1,
    }),
    ensureOperatorGrant: async (input) => {
      grants.push(input);
      const answer = grantAnswers[grants.length - 1] ?? null;
      if (answer !== null) throw new Error(answer);
    },
    newSeatCommandId: () => "csl-seat-1",
    newTurnCommandId: () => "csc-refusal-1",
    now: () => HIRE_CREATED_AT,
    monotonicNow: () => clockMs,
    sleep: async (milliseconds) => {
      delays.push(milliseconds);
      clockMs += milliseconds;
    },
  };

  const mounted = renderHook(() =>
    useCodingSessionHire({
      agents: [
        {
          pubkey: ADA_PUBKEY,
          name: "Ada",
          homeRole: "builder",
          hasRolePack: true,
          model: "opus[1m]",
        },
      ],
      channelIds: [CHANNEL_ID],
      modelCatalogs: new Map([["claude-primary", CLAUDE_CATALOG]]),
      checkoutForChannel: () => "/Users/brian/Projects/beekeeper",
      deps,
      operatorPubkey: OPERATOR_PUBKEY,
      policy: DEFAULT_CODING_SESSION_HIRE_POLICY,
      providerAuthorityPubkey: PROVIDER_PUBKEY,
      runtimes: [CLAUDE_RUNTIME],
      targetForActor: (channelId, actorPubkey) =>
        channelId === CHANNEL_ID && actorPubkey === LEAD_PUBKEY
          ? LEAD_TARGET
          : null,
      umbrellas,
    }),
  );

  const settle = async () => {
    for (let round = 0; round < 24; round += 1) {
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    }
  };
  await settle();

  return {
    delays,
    deliver: async (event) => {
      assert.ok(listener !== null, "the host never subscribed to 44221 events");
      await act(async () => {
        listener([event]);
      });
      await settle();
    },
    grants,
    ipcCalls,
    outcomes: () => mounted.result.current.outcomes,
    published,
    staging,
    teardown: () => mounted.unmount(),
    worktreeCalls,
  };
}

/** Grant calls made for one grantee, in order. */
function grantsFor(host, granteePubkey) {
  return host.grants.filter((grant) => grant.granteePubkey === granteePubkey);
}

/**
 * Every published disclosure that names a failed grant — the 44220 turn the
 * requesting seat reads and the lane line the person reads, in that order.
 */
function grantFailureLines(host) {
  return host.published
    .map((event) => {
      if (event.kind !== 44220) return event.content;
      return JSON.parse(event.content).action.text;
    })
    .filter((line) => line.includes("seated, but not granted"));
}

test("A2.1: a rate-limited grant is retried, and the seat ends up granted", async () => {
  const host = await harness({
    grantAnswers: [QUOTA_EXCEEDED, QUOTA_EXCEEDED, null, null],
  });
  await host.deliver(await signedHire());

  assert.equal(
    grantsFor(host, PROVIDER_PUBKEY).length,
    3,
    "the provider grant is attempted three times: two rate-limits, then accepted",
  );
  assert.equal(
    grantsFor(host, ADA_PUBKEY).length,
    1,
    "the seat actor's grant runs once, after the provider's landed",
  );
  // The relay published its own hint; this host waits exactly that long
  // rather than substituting a guess of its own.
  assert.deepEqual(host.delays, [2_000, 2_000]);
  assert.deepEqual(grantFailureLines(host), []);

  const [outcome] = host.outcomes();
  assert.equal(outcome.state, "seated");
  assert.equal(outcome.granted, true);
  assert.equal(outcome.detail, null);
  host.teardown();
});

test("A2.1: five rate-limits stop, and the disclosure names the attempts", async () => {
  const host = await harness({
    grantAnswers: [
      QUOTA_EXCEEDED,
      QUOTA_EXCEEDED,
      QUOTA_EXCEEDED,
      QUOTA_EXCEEDED,
      QUOTA_EXCEEDED,
      null,
    ],
  });
  await host.deliver(await signedHire());

  assert.equal(grantsFor(host, PROVIDER_PUBKEY).length, 5, "the ceiling is 5");
  assert.equal(
    grantsFor(host, ADA_PUBKEY).length,
    0,
    "the seat's own grant is never attempted on a provider grant that failed",
  );

  const [outcome] = host.outcomes();
  assert.equal(outcome.state, "seated");
  assert.equal(outcome.granted, false);
  assert.equal(
    outcome.detail,
    `provider wake authority: ${QUOTA_EXCEEDED} (after 5 attempts)`,
  );
  // The disclosure keeps its exact wording and adds the count — a person
  // reading "seated, but not granted" has to be able to tell one unlucky
  // write from a relay that refused this host five times.
  assert.deepEqual(grantFailureLines(host), [
    `seated, but not granted: provider wake authority: ${QUOTA_EXCEEDED} (after 5 attempts) — it cannot report until granted`,
    `Hired a builder — seated, but not granted: provider wake authority: ${QUOTA_EXCEEDED} (after 5 attempts) — it cannot report until granted`,
  ]);
  host.teardown();
});

test("A2.1: a refusal is answered once, with its wording untouched", async () => {
  const host = await harness({
    grantAnswers: ["forbidden: only a session founder may grant"],
  });
  await host.deliver(await signedHire());

  assert.equal(
    grantsFor(host, PROVIDER_PUBKEY).length,
    1,
    "an answer that will not change is never retried",
  );
  assert.deepEqual(host.delays, []);

  const [outcome] = host.outcomes();
  assert.equal(outcome.granted, false);
  assert.equal(
    outcome.detail,
    "provider wake authority: forbidden: only a session founder may grant",
  );
  assert.deepEqual(grantFailureLines(host), [
    "seated, but not granted: provider wake authority: forbidden: only a session founder may grant — it cannot report until granted",
    "Hired a builder — seated, but not granted: provider wake authority: forbidden: only a session founder may grant — it cannot report until granted",
  ]);
  host.teardown();
});

test("A2.1: back-pressure with no hint climbs 2s → 16s and stops", async () => {
  const { codingSessionGrantRetryDelayMs, CODING_SESSION_GRANT_MAX_ATTEMPTS } =
    await import("../lib/codingSessionGrantRetry.ts");
  const noHint = "rate-limited: too many concurrent requests";
  assert.deepEqual(
    [1, 2, 3, 4].map((attempt) =>
      codingSessionGrantRetryDelayMs(noHint, attempt),
    ),
    [2_000, 4_000, 8_000, 16_000],
  );
  // Anything that is not literally `retry in Ns` is the default ladder —
  // never a number parsed out of arbitrary prose.
  assert.equal(
    codingSessionGrantRetryDelayMs("rate-limited: retry in about a minute", 1),
    2_000,
  );
  assert.equal(
    codingSessionGrantRetryDelayMs("rate-limited: retry in 7s", 4),
    7_000,
  );
  assert.equal(CODING_SESSION_GRANT_MAX_ATTEMPTS, 5);
});

test("F3: a refusal carrying a `retry in Ns` hint is still a refusal", async () => {
  const { isCodingSessionGrantRateLimited } = await import(
    "../lib/codingSessionGrantRetry.ts"
  );
  // The relay's four rate-limit answers, by their own shape.
  for (const said of [
    "rate-limited: quota exceeded; retry in 2s",
    "rate-limited: too many concurrent requests",
    "rate-limited: shared admission unavailable",
    "relay rate-limited: retry in 4s",
  ]) {
    assert.equal(isCodingSessionGrantRateLimited(new Error(said)), true, said);
  }
  // Back-pressure is the relay's answer, never any message that happens to
  // suggest trying later. Retrying a refusal five times only delays telling
  // the person what the relay already decided.
  for (const said of [
    "forbidden: not a founder; retry in 5s",
    "invalid: event rejected, retry with a valid signature",
    "The relay accepted the transition, but it answered every confirmation read with back-pressure, so this host could not verify the receipt.",
  ]) {
    assert.equal(isCodingSessionGrantRateLimited(new Error(said)), false, said);
  }

  const host = await harness({
    grantAnswers: ["forbidden: not a founder; retry in 5s"],
  });
  await host.deliver(await signedHire());
  assert.equal(grantsFor(host, PROVIDER_PUBKEY).length, 1);
  assert.deepEqual(host.delays, []);
  host.teardown();
});

test("F4: one grant never waits longer than the 60s budget", async () => {
  const {
    CODING_SESSION_GRANT_WAIT_BUDGET_MS,
    codingSessionGrantRetryDelayMs,
  } = await import("../lib/codingSessionGrantRetry.ts");
  // The relay clamps its own hint at 300 s; four of those would have held a
  // seat's disclosure for twenty minutes while a lead waited on its report.
  assert.equal(
    codingSessionGrantRetryDelayMs("rate-limited: retry in 300s", 1),
    60_000,
  );
  assert.equal(
    codingSessionGrantRetryDelayMs("rate-limited: retry in 300s", 1, 5_000),
    5_000,
  );
  assert.equal(
    codingSessionGrantRetryDelayMs("rate-limited: retry in 300s", 1, -1),
    0,
  );
  assert.equal(CODING_SESSION_GRANT_WAIT_BUDGET_MS, 60_000);

  const host = await harness({
    grantAnswers: Array(6).fill(
      "rate-limited: quota exceeded; retry in 999999s",
    ),
  });
  await host.deliver(await signedHire());

  // One wait of the whole budget, then the second failure has nothing left to
  // wait with — the ceiling stops it, not the attempt count.
  assert.deepEqual(host.delays, [60_000]);
  assert.equal(grantsFor(host, PROVIDER_PUBKEY).length, 2);
  assert.equal(
    host.delays.reduce((total, delay) => total + delay, 0),
    60_000,
  );

  const [outcome] = host.outcomes();
  assert.equal(outcome.granted, false);
  // The disclosure names the ceiling, so a person can tell "we stopped
  // waiting" from "the relay refused five times".
  assert.equal(
    outcome.detail,
    "provider wake authority: rate-limited: quota exceeded; retry in 999999s (after 2 attempts and the 60s retry ceiling)",
  );
  host.teardown();
});

test("F2: a rate-limited confirming read re-reads, it never republishes", async () => {
  const { ensureCodingSessionOperatorGrant } = await import(
    "../lib/codingSessionOperatorGrant.ts"
  );
  const { ensureCodingSessionGrantWithBackoff } = await import(
    "../lib/codingSessionGrantRetry.ts"
  );
  const GRANTEE = "ad".repeat(32);

  // The live shape (item 103 review, ATTACK g): the pre-check read gets
  // through and shows the grantee ungranted, the transition publishes, and
  // the relay then rate-limits the *confirmation* reads. The write has
  // already landed, so re-entering the whole grant would sign a second 44228
  // on a budget that is already spent.
  const publishes = [];
  let phase = "precheck";
  let rateLimitedConfirms = 0;
  const fetchFold = async () => {
    if (phase === "precheck") {
      return { accepted: new Map(), activeSeats: new Map() };
    }
    rateLimitedConfirms += 1;
    if (rateLimitedConfirms <= 2) {
      throw new Error("rate-limited: quota exceeded; retry in 2s");
    }
    return {
      accepted: new Map([[GRANTEE, "operator"]]),
      activeSeats: new Map(),
    };
  };
  const publishTransition = async (input) => {
    publishes.push(input.type);
    phase = "confirm";
    return { id: `evt-${publishes.length}` };
  };

  const failure = await ensureCodingSessionGrantWithBackoff({
    grant: async () => {
      phase = "precheck";
      await ensureCodingSessionOperatorGrant(
        {
          channelId: CHANNEL_ID,
          genesisRef: GENESIS_REF,
          granteePubkey: GRANTEE,
        },
        { fetchFold, publishTransition, wait: async () => {} },
      );
    },
    sleep: async () => {},
    now: () => 0,
  });

  assert.equal(failure, null, "the grant lands once the receipt folds");
  assert.deepEqual(
    publishes,
    ["grant-operator"],
    "one grant, one 44228 — the confirming read is re-read, never republished",
  );
  assert.equal(rateLimitedConfirms, 3, "the confirmation was retried in place");
});

test("F2: a confirmation the relay never lets through is not retryable", async () => {
  const { ensureCodingSessionOperatorGrant } = await import(
    "../lib/codingSessionOperatorGrant.ts"
  );
  const { isCodingSessionGrantRateLimited } = await import(
    "../lib/codingSessionGrantRetry.ts"
  );
  const publishes = [];
  let reads = 0;
  await assert.rejects(
    ensureCodingSessionOperatorGrant(
      {
        channelId: CHANNEL_ID,
        genesisRef: GENESIS_REF,
        granteePubkey: "ad".repeat(32),
      },
      {
        fetchFold: async () => {
          reads += 1;
          if (reads === 1)
            return { accepted: new Map(), activeSeats: new Map() };
          throw new Error("rate-limited: quota exceeded; retry in 2s");
        },
        publishTransition: async (input) => {
          publishes.push(input.type);
          return { id: "evt-1" };
        },
        wait: async () => {},
        receiptPollAttempts: 3,
      },
    ),
    (error) => {
      // The write landed, so this must not read as back-pressure: a caller
      // that retried it would sign a second transition for the same grant.
      assert.equal(isCodingSessionGrantRateLimited(error), false);
      assert.match(
        error.message,
        /answered every confirmation read with back-pressure/,
      );
      return true;
    },
  );
  assert.deepEqual(publishes, ["grant-operator"]);
});

// ── B3.2: the create names the hire it answers, and the requester is named ────

test("B3.2: the seated create carries hireRef = the 44221's own event id", async () => {
  // Before this, a seated create pointed at nothing: the founder's Desktop
  // signed it, so the brief read as the founder's own words and nothing on the
  // wire recorded which lead had asked (batch 1, item 10).
  const host = await harness();
  const hire = await signedHire();
  await host.deliver(hire);
  const create = host.published.find(
    (event) =>
      event.kind === 44221 &&
      JSON.parse(event.content).action.type === "session.create",
  );
  assert.ok(create, "the hire must be answered with a seated create");
  const action = JSON.parse(create.content).action;
  assert.equal(action.hireRef, hire.id);
  assert.equal(action.actor, ADA_PUBKEY);
  // The two keys never cross: a create carries hireRef and never requestedBy.
  assert.equal(Object.hasOwn(action, "requestedBy"), false);
  host.teardown();
});

test("B3.2: a refusal names the requesting seat rather than 'A seat'", async () => {
  // The relay does not compare `requestedBy` with the signer (POLICY.md §5),
  // so this host does — and a hire whose claim holds earns the plain name.
  const { buildCodingSessionHireEvent } = await import(
    "../lib/codingSessionHireWire.ts"
  );
  const host = await harness();
  await host.deliver(
    finalizeEvent(
      {
        created_at: HIRE_CREATED_AT,
        ...buildCodingSessionHireEvent({
          channelId: CHANNEL_ID,
          commandId: "csl-hire-refused-1",
          sessionRef: SESSION_REF,
          genesisRef: GENESIS_REF,
          // A role nothing on this computer can fill: refused, and the
          // refusal is what this test reads.
          role: "archaeologist",
          providerInstanceRef: "claude-primary",
          model: null,
          brief: "Dig.",
          requestedBy: LEAD_PUBKEY,
        }),
      },
      LEAD_SECRET,
    ),
  );
  const lines = host.published
    .filter((event) => event.kind !== 44220)
    .map((event) => event.content)
    .filter((content) => content.includes("asked to hire"));
  assert.ok(lines.length > 0, "the umbrella must be told about the refusal");
  // Named, not "A seat". The lead is not one of this host's managed agents in
  // this fixture, so the canonical short pubkey is the honest name.
  assert.match(lines[0], /^A seat \(/);
  assert.equal(lines[0].includes("A seat asked"), false);
  host.teardown();
});

// ── REVIEW-B3 N2: the store is community-scoped and must be reset with one ────

test("N2: switching communities clears the hire-outcome store", async () => {
  // CLAUDE.md: `resetCommunityState()` is the canonical inventory of
  // community-scoped singletons, and React remounting clears React state only.
  // This store became a *rendered* fact this batch — a hired seat's first turn
  // is attributed from it — so a hire answered in community A could otherwise
  // put a lead's name on a transcript row in community B.
  const {
    publishCodingSessionHireOutcomes,
    readCodingSessionHireOutcomes,
    resetCodingSessionHireOutcomes,
  } = await import("./useCodingSessionHire.ts");
  resetCodingSessionHireOutcomes();
  publishCodingSessionHireOutcomes([
    {
      commandId: "csl-hire-a",
      channelId: CHANNEL_ID,
      sessionRef: SESSION_REF,
      role: "builder",
      state: "seated",
      detail: null,
      seatCommandId: "csl-seat-a",
      granted: true,
      seatActor: ADA_PUBKEY,
      requesterLabel: "Keystone",
      hostPubkey: OPERATOR_PUBKEY,
    },
  ]);
  assert.equal(readCodingSessionHireOutcomes().length, 1);

  // The reset the community switch runs, read from the module that owns the
  // inventory rather than called directly — so deleting the line fails here.
  const source = await import("node:fs").then((fs) =>
    fs.readFileSync(
      new URL("../../communities/useCommunityInit.ts", import.meta.url),
      "utf8",
    ),
  );
  assert.match(
    source,
    /resetCodingSessionHireOutcomes\(\);/,
    "resetCommunityState() must reset the hire-outcome store",
  );
  assert.match(
    source,
    /import \{ resetCodingSessionHireOutcomes \}/,
    "…and import it from the hook that owns it",
  );

  resetCodingSessionHireOutcomes();
  assert.deepEqual(readCodingSessionHireOutcomes(), []);
});

test("finding 60: the worktree create carries the session ref and seat label the host needs to record it", async () => {
  // Live-run finding 60: the host staged every seat worktree into `pending`
  // and never promoted one into its durable `worktrees` record, because the
  // hire path never told `create_coding_session_worktree` which session and
  // seat the tree belonged to — even though, unlike the lead's own worktree
  // at launch, a hire always targets a mission that already exists, so both
  // are known before the tree is even cut.
  const host = await harness();
  const hire = await signedHire();
  await host.deliver(hire);

  assert.equal(host.worktreeCalls.length, 1);
  assert.equal(host.worktreeCalls[0].sessionRef, SESSION_REF);
  assert.equal(host.worktreeCalls[0].seatLabel, "Ada");
  host.teardown();
});

test("F2: a hire installs the wip-share hooks into the seat's own worktree", async () => {
  const host = await harness();
  const hire = await signedHire();
  await host.deliver(hire);

  const installs = host.ipcCalls.filter(
    (call) => call.command === "install_coding_session_seat_hooks",
  );
  assert.equal(
    installs.length,
    1,
    "the hire host installed no seat hooks at all",
  );
  // The worktree it just cut, the seat it just seated, and the hire the seat
  // answers — all three derived here, never read back from agent text.
  assert.equal(
    installs[0].args.request.worktreePath,
    "/tmp/trees/agent-teams-builder-1",
  );
  assert.equal(installs[0].args.request.seatPubkey, ADA_PUBKEY);
  assert.equal(installs[0].args.request.seatRole, "builder");
  assert.equal(installs[0].args.request.assignmentId, hire.id);
  assert.equal(
    installs[0].args.request.branch,
    "session/agent-teams-builder-1",
  );
  // The one thing this host cannot supply is null rather than a guess: a seat
  // signing with the only key file this host can name — the operator's — is a
  // forged attribution.
  assert.equal(installs[0].args.request.keyfilePath, null);

  const [outcome] = host.outcomes();
  assert.equal(outcome.state, "seated");
  assert.equal(outcome.wipShare.state, "shared");
  assert.equal(outcome.wipShare.ref, "refs/heads/wip/builder/ab12cd34");
  assert.match(outcome.wipShare.why, /not signed/);
  host.teardown();
});

test("F2: a seat whose arming failed says so instead of going quiet", async () => {
  const previous = tauriInternals.invoke;
  tauriInternals.invoke = () => Promise.reject(new Error("not a git worktree"));
  const warnings = [];
  const previousWarn = console.warn;
  console.warn = (line) => warnings.push(line);
  try {
    const host = await harness();
    await host.deliver(await signedHire());
    const [outcome] = host.outcomes();
    assert.equal(outcome.state, "seated");
    assert.equal(outcome.wipShare.state, "unarmed");
    assert.match(outcome.wipShare.why, /not a git worktree/);
    assert.ok(
      warnings.some((line) => line.includes("shares nothing")),
      `expected a disclosure among:\n${warnings.join("\n")}`,
    );
    host.teardown();
  } finally {
    tauriInternals.invoke = previous;
    console.warn = previousWarn;
  }
});

/**
 * Finding 84: the hire staged every seat from this computer's copy of the
 * role pack. The umbrella's project names a packs repository (kind:30624),
 * the launch dialog's preview read it, and the hire's staging call never did
 * — so the repository the project pointed at was never staged from.
 */
test("finding 84: a hire stages the seat from the umbrella's project pack source", async () => {
  const OWNER = "3d3b7169".padEnd(64, "0");
  const PROJECT_REF = `30621:${OWNER}:beekeeper`;
  const PACK_SOURCE = {
    repo: `30617:${OWNER}:agiterra-packs`,
    gitRef: "refs/heads/main",
    sha: null,
    path: "personas/roles",
  };
  const host = await harness({
    umbrellas: [
      {
        ...UMBRELLA,
        executions: [
          {
            activeGeneration: {
              agentRef: LEAD_PUBKEY,
              role: "lead",
              status: "running",
              projectRef: PROJECT_REF,
            },
          },
        ],
      },
    ],
    packSources: new Map([[PROJECT_REF, PACK_SOURCE]]),
  });
  await host.deliver(await signedHire());

  // The reader was asked about the umbrella's project, and what it answered
  // is what the staging call was handed — the same 30624 the preview shows.
  assert.deepEqual(
    host.staging.map(([name]) => name),
    ["fetchPackSource", "stageSeat"],
  );
  assert.deepEqual(host.staging[0], ["fetchPackSource", PROJECT_REF]);
  const [, staged] = host.staging[1];
  assert.equal(staged.agentPubkey, ADA_PUBKEY);
  assert.equal(staged.role, "builder");
  assert.deepEqual(staged.packSource, PACK_SOURCE);

  // And the outcome says which repository commit the seat was staged from,
  // so the seat card is not left to guess.
  const [outcome] = host.outcomes();
  assert.equal(outcome.state, "seated");
  assert.deepEqual(outcome.packRef, {
    repo: PACK_SOURCE.repo,
    sha: "dd935f43".padEnd(40, "0"),
    role: "builder",
    path: "personas/roles/builder",
  });
  host.teardown();
});

test("finding 84: a hire into an umbrella with no project stages with no source, as before", async () => {
  const host = await harness();
  await host.deliver(await signedHire());

  assert.deepEqual(
    host.staging.map(([name]) => name),
    ["stageSeat"],
    "no project, so no 30624 to read",
  );
  assert.equal(host.staging[0][1].packSource, null);
  const [outcome] = host.outcomes();
  assert.equal(outcome.state, "seated");
  assert.equal(outcome.packRef, null);
  host.teardown();
});
