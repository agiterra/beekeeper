import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  authPreflightPresentation,
  checkedAgoLabel,
  runtimeHasAuthPreflight,
} from "./authPreflightLogic.ts";

const NOW = 1_800_000_000_000;

function verdict(state, overrides = {}) {
  return {
    runtimeId: "claude",
    state,
    checkedAtMs: NOW - 5_000,
    command:
      'claude -p "reply with the single word ok" --max-turns 1 --output-format text',
    remedy:
      state.state === "credential_dead"
        ? "Run `claude auth login` in a terminal, then retry."
        : null,
    cached: false,
    ...overrides,
  };
}

describe("runtimeHasAuthPreflight", () => {
  it("is only claude, installed, with a status read that says signed in or out", () => {
    assert.equal(
      runtimeHasAuthPreflight({
        id: "claude",
        availability: "available",
        authStatus: { status: "logged_in" },
      }),
      true,
    );
    // Signed out is what the catalog reports once the pre-flight found the
    // credential dead — the row must keep showing the verdict then.
    assert.equal(
      runtimeHasAuthPreflight({
        id: "claude",
        availability: "available",
        authStatus: { status: "logged_out" },
      }),
      true,
    );
    assert.equal(
      runtimeHasAuthPreflight({
        id: "claude",
        availability: "adapter_missing",
        authStatus: { status: "unknown" },
      }),
      false,
    );
    assert.equal(
      runtimeHasAuthPreflight({
        id: "claude",
        availability: "available",
        authStatus: { status: "config_invalid", diagnostic: "bad json" },
      }),
      false,
    );
    assert.equal(
      runtimeHasAuthPreflight({
        id: "codex",
        availability: "available",
        authStatus: { status: "logged_in" },
      }),
      false,
    );
  });
});

describe("authPreflightPresentation", () => {
  it("says it is checking while the first verdict is pending", () => {
    const shown = authPreflightPresentation({
      verdict: undefined,
      isFetching: true,
      error: null,
      nowMs: NOW,
    });
    assert.equal(shown.label, "Checking login…");
    assert.equal(shown.tone, "muted");
    assert.equal(shown.checkedLabel, null);
  });

  it("says the check itself failed when the command threw", () => {
    const shown = authPreflightPresentation({
      verdict: undefined,
      isFetching: false,
      error: new Error("runtime claude has no login pre-flight"),
      nowMs: NOW,
    });
    assert.equal(shown.label, "Login check failed");
    assert.equal(shown.tone, "bad");
    assert.equal(shown.detail, "runtime claude has no login pre-flight");
    assert.equal(shown.remedy, null);
  });

  it("verified-live is green with no detail and no remedy", () => {
    const shown = authPreflightPresentation({
      verdict: verdict({ state: "verified_live" }),
      isFetching: false,
      error: null,
      nowMs: NOW,
    });
    assert.deepEqual(shown, {
      label: "Login verified",
      tone: "ok",
      detail: null,
      remedy: null,
      checkedLabel: "checked just now",
    });
  });

  it("credential-dead carries the CLI's sentence and the remedy", () => {
    const shown = authPreflightPresentation({
      verdict: verdict({
        state: "credential_dead",
        sentence:
          "Failed to authenticate: OAuth session expired and could not be refreshed",
      }),
      isFetching: false,
      error: null,
      nowMs: NOW,
    });
    assert.equal(shown.label, "Login expired");
    assert.equal(shown.tone, "bad");
    assert.equal(
      shown.detail,
      "Failed to authenticate: OAuth session expired and could not be refreshed",
    );
    assert.equal(
      shown.remedy,
      "Run `claude auth login` in a terminal, then retry.",
    );
  });

  it("unknown says why, and claims nothing either way", () => {
    const shown = authPreflightPresentation({
      verdict: verdict(
        { state: "unknown", reason: "the check did not finish within 20s" },
        { checkedAtMs: NOW - 3 * 60_000 },
      ),
      isFetching: false,
      error: null,
      nowMs: NOW,
    });
    assert.equal(shown.label, "Login unverified");
    assert.equal(shown.tone, "muted");
    assert.equal(shown.detail, "the check did not finish within 20s");
    assert.equal(shown.remedy, null);
    assert.equal(shown.checkedLabel, "checked 3 min ago");
  });

  it("a stale verdict still shows while a re-check is in flight", () => {
    const shown = authPreflightPresentation({
      verdict: verdict({ state: "verified_live" }),
      isFetching: true,
      error: null,
      nowMs: NOW,
    });
    assert.equal(shown.label, "Login verified");
  });
});

describe("checkedAgoLabel", () => {
  it("rounds down to the unit a person would say", () => {
    assert.equal(checkedAgoLabel(NOW - 59_000, NOW), "checked just now");
    assert.equal(checkedAgoLabel(NOW - 61_000, NOW), "checked 1 min ago");
    assert.equal(checkedAgoLabel(NOW - 59 * 60_000, NOW), "checked 59 min ago");
    assert.equal(checkedAgoLabel(NOW - 125 * 60_000, NOW), "checked 2 h ago");
    // A clock that ran backwards is "just now", never negative.
    assert.equal(checkedAgoLabel(NOW + 10_000, NOW), "checked just now");
  });
});
