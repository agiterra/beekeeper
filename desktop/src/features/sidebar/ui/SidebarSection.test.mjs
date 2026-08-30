import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
  formatWorkingTooltip,
  resolveSidebarUnreadIndicators,
} from "./SidebarSection.tsx";

function summary(agentNames, agentCount = agentNames.length) {
  return {
    channelId: "chan-1",
    anchorAt: 0,
    agentCount,
    agentPubkeys: Array.from(
      { length: agentCount },
      (_, index) => `agent-${index}-pubkey`,
    ),
    agentNames,
  };
}

describe("formatWorkingTooltip", () => {
  it("names one known agent", () => {
    assert.equal(formatWorkingTooltip(summary(["Ned"])), "Ned working");
  });

  it("names one known agent and counts one additional agent", () => {
    assert.equal(
      formatWorkingTooltip(summary(["Ned", "Bart"])),
      "Ned and 1 agent working",
    );
  });

  it("names one known agent and counts multiple additional agents", () => {
    assert.equal(
      formatWorkingTooltip(summary(["Ned", "Bart", "Carl"])),
      "Ned and 2 agents working",
    );
  });

  it("uses a singular count when all agents are unknown", () => {
    assert.equal(formatWorkingTooltip(summary([], 1)), "1 agent working");
  });

  it("uses a plural count when all agents are unknown", () => {
    assert.equal(formatWorkingTooltip(summary([], 3)), "3 agents working");
  });

  it("counts unknown agents with the named lead", () => {
    assert.equal(
      formatWorkingTooltip(summary(["Ned"], 3)),
      "Ned and 2 agents working",
    );
  });
});

describe("resolveSidebarUnreadIndicators", () => {
  const resolve = (overrides) =>
    resolveSidebarUnreadIndicators({
      channelType: "channel",
      hasThreadUnread: false,
      unreadTotal: 0,
      ...overrides,
    });

  it("shows the count when a channel has unread messages", () => {
    assert.deepEqual(resolve({ unreadTotal: 4 }), {
      showCount: true,
      showDot: false,
    });
  });

  it("shows nothing when a channel is fully read", () => {
    assert.deepEqual(resolve({}), { showCount: false, showDot: false });
  });

  it("shows the dot alone for thread activity with no counted message", () => {
    assert.deepEqual(resolve({ hasThreadUnread: true }), {
      showCount: false,
      showDot: true,
    });
  });

  it("keeps the dot beside the count so thread activity stays visible", () => {
    assert.deepEqual(resolve({ hasThreadUnread: true, unreadTotal: 2 }), {
      showCount: true,
      showDot: true,
    });
  });

  it("leaves the count to the DM row's own badge", () => {
    assert.deepEqual(resolve({ channelType: "dm", unreadTotal: 4 }), {
      showCount: false,
      showDot: false,
    });
  });

  it("counts forum channels like any other channel", () => {
    assert.deepEqual(resolve({ channelType: "forum", unreadTotal: 1 }), {
      showCount: true,
      showDot: false,
    });
  });
});
