import assert from "node:assert/strict";
import test from "node:test";

import {
  agentCommunityAvailability,
  agentCommunityBadgeVariant,
  agentCommunityStatusDetail,
  canonicalRelayUrl,
  findManagedAgentRuntime,
  managedAgentRuntimeKey,
  managedAgentPairAction,
} from "./managedAgentRuntimeStatus.ts";

const runtime = (overrides = {}) => ({
  pubkey: "aa",
  relayUrl: "wss://relay.example",
  localSetup: true,
  lifecycle: "ready",
  pid: 1,
  error: null,
  logPath: null,
  ...overrides,
});

test("projects every backend lifecycle to the four product labels", () => {
  assert.equal(agentCommunityAvailability(runtime()), "Here");
  for (const lifecycle of ["starting", "listening", "waking"]) {
    assert.equal(agentCommunityAvailability(runtime({ lifecycle })), "Waking");
  }
  for (const lifecycle of ["failed", "stopped"]) {
    assert.equal(
      agentCommunityAvailability(runtime({ lifecycle })),
      "Unavailable",
    );
  }
});

test("backend-authoritative local setup takes precedence", () => {
  assert.equal(
    agentCommunityAvailability(
      runtime({ localSetup: false, lifecycle: "ready" }),
    ),
    "Needs setup on this device",
  );
});

test("host ownership discloses missing relay availability without offering local setup", () => {
  const adopted = runtime({
    localSetup: false,
    lifecycle: "host_managed",
    pid: null,
  });
  assert.equal(agentCommunityAvailability(adopted), "Unavailable");
  assert.match(agentCommunityStatusDetail(adopted), /Managed by the host/);
  assert.match(agentCommunityStatusDetail(adopted), /unconfirmed/);
  assert.equal(agentCommunityBadgeVariant(adopted), "secondary");
});

/**
 * The four host-managed states a person must tell apart at a glance, each
 * from a different wire fact: a signed lease, a live heartbeat naming this
 * execution, a live heartbeat that does not, and a heartbeat nobody has
 * heard. Start/Stop follows the same split, and the detail never claims a
 * number the status row does not carry.
 */
const hostManaged = (overrides = {}) =>
  runtime({
    localSetup: false,
    lifecycle: "host_managed",
    pid: null,
    runtimeAvailable: false,
    ...overrides,
  });

test("a signed execution lease reads as working and offers Stop", () => {
  const working = hostManaged({
    runtimeAvailable: true,
    providerPresence: "live",
    execution: "live",
  });
  assert.equal(agentCommunityAvailability(working), "Here");
  assert.equal(agentCommunityBadgeVariant(working), "default");
  assert.match(
    agentCommunityStatusDetail(working),
    /Working.*signed relay lease/,
  );
  assert.equal(managedAgentPairAction(working), "stop");
});

test("a live heartbeat with an idle word reads as idle-but-here, and Stop stays", () => {
  const idle = hostManaged({ providerPresence: "live", execution: "idle" });
  assert.equal(agentCommunityAvailability(idle), "Idle");
  assert.equal(agentCommunityBadgeVariant(idle), "success");
  assert.match(agentCommunityStatusDetail(idle), /Here and idle/);
  assert.match(agentCommunityStatusDetail(idle), /heartbeat is live/);
  assert.equal(managedAgentPairAction(idle), "stop");
});

test("a signed stopped execution under a live heartbeat reads Stopped, never Idle, and offers Start", () => {
  const stopped = hostManaged({
    providerPresence: "live",
    execution: "stopped",
  });
  assert.equal(agentCommunityAvailability(stopped), "Stopped");
  assert.equal(agentCommunityBadgeVariant(stopped), "secondary");
  assert.match(
    agentCommunityStatusDetail(stopped),
    /provider is live; this agent's execution is signed stopped/,
  );
  assert.doesNotMatch(agentCommunityStatusDetail(stopped), /idle/i);
  assert.equal(managedAgentPairAction(stopped), "start");
});

test("a live heartbeat with no execution word is disclosed as not reported, with no Stop on offer", () => {
  for (const execution of [null, undefined]) {
    const unreported = hostManaged({ providerPresence: "live", execution });
    assert.equal(
      agentCommunityAvailability(unreported),
      "Execution not reported",
    );
    assert.equal(agentCommunityBadgeVariant(unreported), "secondary");
    assert.match(
      agentCommunityStatusDetail(unreported),
      /reported its presence but no execution state/,
    );
    assert.doesNotMatch(agentCommunityStatusDetail(unreported), /idle/i);
    assert.equal(managedAgentPairAction(unreported), "start");
  }
});

test("a live heartbeat that does not name this execution reads as not running, never idle", () => {
  const absent = hostManaged({ providerPresence: "live", execution: "absent" });
  assert.equal(agentCommunityAvailability(absent), "Not running");
  assert.equal(agentCommunityBadgeVariant(absent), "secondary");
  assert.match(agentCommunityStatusDetail(absent), /host is up/);
  assert.match(agentCommunityStatusDetail(absent), /Start creates one/);
  assert.equal(managedAgentPairAction(absent), "start");
});

test("an expired or never-seen heartbeat reads as not heard from, never unreachable, without inventing a last-seen clock", () => {
  for (const providerPresence of ["expired", "none"]) {
    const lost = hostManaged({ providerPresence, execution: "disconnected" });
    assert.equal(agentCommunityAvailability(lost), "Provider not heard from");
    assert.equal(agentCommunityBadgeVariant(lost), "warning");
    const detail = agentCommunityStatusDetail(lost);
    assert.match(detail, /Provider not heard from/);
    // No heartbeat within TTL is not a reachability test, so the word
    // "unreachable" never appears; and Start's safety is stated, not assumed.
    assert.doesNotMatch(detail, /unreachable/i);
    assert.match(detail, /does not create a second execution/);
    assert.doesNotMatch(detail, /\d+\s*s(ec)?\b/);
    assert.doesNotMatch(detail, /last seen/i);
    assert.equal(managedAgentPairAction(lost), "start");
  }
  assert.match(
    agentCommunityStatusDetail(hostManaged({ providerPresence: "expired" })),
    /heartbeat expired/,
  );
  assert.match(
    agentCommunityStatusDetail(hostManaged({ providerPresence: "none" })),
    /no presence heartbeat has been seen/,
  );
});

test("a backend that reported no presence stays unconfirmed, not unreachable", () => {
  const silent = hostManaged({ providerPresence: null, execution: null });
  assert.equal(agentCommunityAvailability(silent), "Unavailable");
  assert.match(
    agentCommunityStatusDetail(silent),
    /did not report its presence/,
  );
  assert.equal(managedAgentPairAction(silent), "start");
});

test("badge variants for local runtimes: failed is the only red", () => {
  assert.equal(agentCommunityBadgeVariant(runtime()), "default");
  assert.equal(
    agentCommunityBadgeVariant(runtime({ lifecycle: "waking" })),
    "info",
  );
  assert.equal(
    agentCommunityBadgeVariant(runtime({ lifecycle: "stopped" })),
    "secondary",
  );
  assert.equal(
    agentCommunityBadgeVariant(runtime({ lifecycle: "failed" })),
    "destructive",
  );
  assert.equal(
    agentCommunityBadgeVariant(runtime({ localSetup: false })),
    "secondary",
  );
});

test("unavailable detail distinguishes stopped and failed", () => {
  assert.equal(
    agentCommunityStatusDetail(runtime({ lifecycle: "stopped" })),
    "Stopped by you",
  );
  assert.equal(
    agentCommunityStatusDetail(
      runtime({ lifecycle: "failed", error: "Relay timed out" }),
    ),
    "Relay timed out",
  );
});

test("pair key cannot collide at component boundaries", () => {
  assert.notEqual(
    managedAgentRuntimeKey(runtime({ pubkey: "ab", relayUrl: "c" })),
    managedAgentRuntimeKey(runtime({ pubkey: "a", relayUrl: "bc" })),
  );
});

test("selects one relay without collapsing same-pubkey pairs", () => {
  const runtimes = [
    runtime({ relayUrl: "wss://a.example", lifecycle: "ready" }),
    runtime({ relayUrl: "wss://b.example", lifecycle: "failed" }),
  ];
  assert.equal(
    findManagedAgentRuntime(runtimes, "AA", "wss://b.example")?.lifecycle,
    "failed",
  );
  assert.equal(
    findManagedAgentRuntime(runtimes, "aa", "wss://c.example"),
    undefined,
  );
});

test("canonicalRelayUrl mirrors the backend pair-key normalization", () => {
  // Loopback folding + default-port and trailing-slash stripping — the
  // standard dev setup that previously broke pair matching.
  assert.equal(canonicalRelayUrl("ws://localhost:3000"), "ws://127.0.0.1:3000");
  assert.equal(
    canonicalRelayUrl("WSS://Relay.Example:443/"),
    "wss://relay.example",
  );
  assert.equal(
    canonicalRelayUrl("ws://relay.example:80/"),
    "ws://relay.example",
  );
  assert.equal(
    canonicalRelayUrl("wss://relay.example/path/"),
    "wss://relay.example/path",
  );
  assert.equal(canonicalRelayUrl("ws://[::1]:3000"), "ws://127.0.0.1:3000");
  assert.equal(canonicalRelayUrl("https://relay.example"), null);
  assert.equal(canonicalRelayUrl("not a url"), null);
});

test("matches a stored community URL against canonical backend rows", () => {
  const runtimes = [
    runtime({ relayUrl: "ws://127.0.0.1:3000", lifecycle: "ready" }),
  ];
  assert.equal(
    findManagedAgentRuntime(runtimes, "aa", "ws://localhost:3000")?.lifecycle,
    "ready",
  );
  assert.equal(
    findManagedAgentRuntime(runtimes, "aa", "ws://localhost:3001"),
    undefined,
  );
});

test("backend clean-start refusal stays visible when local prerequisites are absent", () => {
  const blocked = runtime({
    lifecycle: "failed",
    localSetup: false,
    pid: null,
    error: "Previous desktop record retained. Recreate through headless setup.",
  });
  assert.equal(agentCommunityStatusDetail(blocked), blocked.error);
});

test("host ownership uses verified lease availability independently of local setup or pid", () => {
  const host = runtime({
    lifecycle: "host_managed",
    localSetup: false,
    pid: null,
  });
  assert.equal(
    agentCommunityAvailability({ ...host, runtimeAvailable: true }),
    "Here",
  );
  assert.match(
    agentCommunityStatusDetail({ ...host, runtimeAvailable: true }),
    /signed relay lease/,
  );
  assert.equal(
    agentCommunityAvailability({ ...host, runtimeAvailable: false }),
    "Unavailable",
  );
  assert.match(
    agentCommunityStatusDetail({ ...host, runtimeAvailable: false }),
    /No current verified execution lease/,
  );
  for (const runtimeAvailable of [null, undefined]) {
    assert.equal(
      agentCommunityAvailability({ ...host, runtimeAvailable }),
      "Unavailable",
    );
    assert.match(
      agentCommunityStatusDetail({ ...host, runtimeAvailable }),
      /unconfirmed/,
    );
  }
  assert.equal(
    agentCommunityAvailability({ ...host, pid: 123, runtimeAvailable: false }),
    "Unavailable",
  );
});

test("host ownership reads idle, not disconnected, while the provider's presence is live", () => {
  const host = runtime({
    lifecycle: "host_managed",
    localSetup: false,
    pid: null,
    runtimeAvailable: false,
  });
  // Live presence with an idle word and no lease: here and idle.
  const idle = { ...host, providerPresence: "live", execution: "idle" };
  assert.equal(agentCommunityAvailability(idle), "Idle");
  assert.match(
    agentCommunityStatusDetail(idle),
    /Here and idle: the host's presence heartbeat is live/,
  );
  assert.equal(managedAgentPairAction(idle), "stop");
  // Live presence with no execution word at all is not idle: the backend
  // said nothing about the execution, and the pill says exactly that.
  assert.equal(
    agentCommunityAvailability({ ...host, providerPresence: "live" }),
    "Execution not reported",
  );
  assert.equal(
    managedAgentPairAction({ ...host, providerPresence: "live" }),
    "start",
  );
  // A live lease still wins over presence for the detail text.
  assert.match(
    agentCommunityStatusDetail({
      ...host,
      runtimeAvailable: true,
      providerPresence: "live",
    }),
    /signed relay lease/,
  );
  // A live heartbeat that does not name this pair's execution is "absent":
  // not Here, not idle, no Stop on offer — Start is the only honest action.
  const absent = { ...host, providerPresence: "live", execution: "absent" };
  assert.equal(agentCommunityAvailability(absent), "Not running");
  assert.match(
    agentCommunityStatusDetail(absent),
    /Not running: the host is up but reports no execution/,
  );
  assert.equal(managedAgentPairAction(absent), "start");
  // The lease still wins over an absent word (the lease is signed per
  // execution; the heartbeat may predate it).
  assert.equal(
    agentCommunityAvailability({ ...absent, runtimeAvailable: true }),
    "Here",
  );
  // Only the idle word (or a live word ahead of lease verification) keeps
  // the idle reading; stopped and no-word each have their own.
  for (const execution of ["idle", "live"]) {
    assert.equal(
      agentCommunityAvailability({
        ...host,
        providerPresence: "live",
        execution,
      }),
      "Idle",
    );
  }
  assert.equal(
    agentCommunityAvailability({
      ...host,
      providerPresence: "live",
      execution: "stopped",
    }),
    "Stopped",
  );
  // Expired or absent presence is the only thing that reads as not heard from.
  for (const providerPresence of ["expired", "none"]) {
    assert.equal(
      agentCommunityAvailability({ ...host, providerPresence }),
      "Provider not heard from",
    );
    assert.equal(
      managedAgentPairAction({ ...host, providerPresence }),
      "start",
    );
  }
  assert.match(
    agentCommunityStatusDetail({ ...host, providerPresence: "expired" }),
    /Provider not heard from: its last presence heartbeat expired/,
  );
  // No presence report at all is unconfirmed, which is not disconnected.
  for (const providerPresence of [null, undefined]) {
    assert.equal(
      agentCommunityAvailability({ ...host, providerPresence }),
      "Unavailable",
    );
    assert.match(
      agentCommunityStatusDetail({ ...host, providerPresence }),
      /No current verified execution lease/,
    );
    assert.equal(
      managedAgentPairAction({ ...host, providerPresence }),
      "start",
    );
  }
});

test("host pair Start reconciles unavailable execution without claiming termination", () => {
  const host = runtime({
    lifecycle: "host_managed",
    localSetup: false,
    pid: null,
  });
  assert.equal(
    managedAgentPairAction({ ...host, runtimeAvailable: true }),
    "stop",
  );
  for (const runtimeAvailable of [false, null, undefined]) {
    assert.equal(
      managedAgentPairAction({ ...host, runtimeAvailable }),
      "start",
    );
    assert.equal(
      agentCommunityAvailability({ ...host, runtimeAvailable }),
      "Unavailable",
    );
  }
});
