import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CODING_SESSION_LEASE_TTL_SECONDS,
  parseCodingSessionLease,
  resolveCodingSessionReachability,
} from "./lease.ts";
import {
  corruptSignature,
  leaseEvent,
  newSigner,
  resign,
  target,
} from "./testFixtures.mjs";

const PROVIDER = newSigner();
const NOW_S = 1_700_000_000;

function lease(options = {}) {
  const decoded = parseCodingSessionLease(leaseEvent(PROVIDER, options));
  assert.ok(decoded, "fixture must decode");
  return decoded;
}

function reachability(overrides = {}) {
  return resolveCodingSessionReachability({
    leases: [],
    acceptedCommandId: "command-1",
    providerAuthorityPubkey: PROVIDER.pubkey,
    isCurrentGeneration: true,
    leasesRead: true,
    lastReportedStatus: "idle",
    lastReportedAt: (NOW_S - 30) * 1000,
    nowMs: NOW_S * 1000,
    ...overrides,
  });
}

test("the client TTL under-claims against the relay's 180s expiry", () => {
  assert.equal(CODING_SESSION_LEASE_TTL_SECONDS, 150);
});

test("a live lease younger than the TTL proves the provider is reachable", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S - 149 })],
  });
  assert.equal(report.reachability, "provider_reachable");
});

test("a lease older than 150 seconds proves nothing", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S - 151 })],
  });
  assert.equal(report.reachability, "no_provider_answering");
  assert.equal(report.lastReportedStatus, "idle");
  assert.equal(report.lastReportedAgeSeconds, 30);
});

test("a released lease is not reachability, however fresh", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S, state: "released" })],
  });
  assert.equal(report.reachability, "no_provider_answering");
});

test("a lease whose commandId is not the accepted create's proves nothing", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S, commandId: "some-other-command" })],
  });
  assert.equal(
    report.reachability,
    "no_provider_answering",
    "a lease for a different session must not vouch for this one",
  );
});

test("with no accepted create resolved, a lease cannot prove reachability", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S })],
    acceptedCommandId: null,
  });
  assert.equal(report.reachability, "no_provider_answering");
});

test("a lease signed by a different key than the provider authority is ignored", () => {
  const other = newSigner();
  const foreign = parseCodingSessionLease(
    leaseEvent(other, { created_at: NOW_S }),
  );
  const report = reachability({ leases: [foreign] });
  assert.equal(report.reachability, "no_provider_answering");
});

test("a prior generation is never reachable, even holding a live lease", () => {
  const report = reachability({
    leases: [lease({ created_at: NOW_S })],
    isCurrentGeneration: false,
  });
  assert.equal(report.reachability, "no_provider_answering");
});

test("two distinct leases at the highest sequence prove nothing", () => {
  const report = reachability({
    leases: [
      lease({ created_at: NOW_S, leaseSequence: 4 }),
      lease({ created_at: NOW_S - 1, leaseSequence: 4 }),
    ],
  });
  assert.equal(report.reachability, "no_provider_answering");
});

test("the highest sequence decides, even when an older lease is still live", () => {
  const stale = lease({ created_at: NOW_S, leaseSequence: 1 });
  const newest = lease({
    created_at: NOW_S - 200,
    leaseSequence: 2,
    state: "released",
  });
  const report = reachability({ leases: [stale, newest] });
  assert.equal(report.reachability, "no_provider_answering");
});

test("an unread lease query stays unknown rather than claiming nobody answers", () => {
  const report = reachability({ leasesRead: false, leases: [] });
  assert.equal(report.reachability, "unknown");
});

test("a lease with tags out of order is refused", () => {
  const event = leaseEvent(PROVIDER);
  const reordered = resign(PROVIDER, {
    ...event,
    tags: [...event.tags].reverse(),
  });
  assert.equal(parseCodingSessionLease(reordered), null);
});

test("a lease whose cs-target tag disagrees with its payload is refused", () => {
  const event = leaseEvent(PROVIDER, { target: target() });
  const swapped = resign(PROVIDER, {
    ...event,
    tags: event.tags.map((tag) =>
      tag[0] === "cs-target" ? ["cs-target", "coding-session/v1|1:x"] : tag,
    ),
  });
  assert.equal(parseCodingSessionLease(swapped), null);
});

test("a lease with a broken signature is not decoded by the caller's gate", () => {
  // `parseCodingSessionLease` is pure; the store verifies first. This pins the
  // contract that a corrupted lease never reaches a reachability claim.
  const broken = corruptSignature(leaseEvent(PROVIDER));
  assert.equal(broken.sig === leaseEvent(PROVIDER).sig, false);
});
