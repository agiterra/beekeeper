import assert from "node:assert/strict";
import test from "node:test";

import {
  DASHBOARD_TABS,
  dashboardTabSearch,
  parseDashboardTab,
  resolveDashboardTab,
} from "./dashboardTabs.ts";

test("parseDashboardTab_defaultsToOverview", () => {
  assert.equal(parseDashboardTab(undefined), "overview");
  assert.equal(parseDashboardTab({}), "overview");
  assert.equal(parseDashboardTab({ tab: "nope" }), "overview");
});

test("parseDashboardTab_readsKnownTabs", () => {
  assert.equal(parseDashboardTab({ tab: "inbox" }), "inbox");
  assert.equal(parseDashboardTab({ tab: "pulse" }), "pulse");
  assert.equal(parseDashboardTab({ tab: "agent-progress" }), "agent-progress");
  assert.equal(parseDashboardTab({ tab: "agents" }), "agents");
});

test("parseDashboardTab_itemWithoutTabImpliesInbox", () => {
  assert.equal(parseDashboardTab({ item: "abc" }), "inbox");
  assert.equal(parseDashboardTab({ item: "" }), "overview");
  assert.equal(parseDashboardTab({ item: "abc", tab: "agents" }), "agents");
});

test("resolveDashboardTab_gatedTabsFallBackToOverview", () => {
  const off = { pulse: false, agentProgress: false };
  const on = { pulse: true, agentProgress: true };
  assert.equal(resolveDashboardTab("pulse", off), "overview");
  assert.equal(resolveDashboardTab("agent-progress", off), "overview");
  assert.equal(resolveDashboardTab("pulse", on), "pulse");
  assert.equal(resolveDashboardTab("agent-progress", on), "agent-progress");
  assert.equal(resolveDashboardTab("inbox", off), "inbox");
  assert.equal(resolveDashboardTab("agents", off), "agents");
});

test("dashboardTabSearch_overviewClearsParam", () => {
  assert.deepEqual(dashboardTabSearch("overview"), {});
  assert.deepEqual(dashboardTabSearch("inbox"), { tab: "inbox" });
});

// §F: no tab strip trigger names "roles" anymore (DASHBOARD_TABS drives the
// strip), but a URL that still names it must parse as "roles" — never fall
// back to the overview — so `app/routes/index.tsx` can redirect it to the
// project it names, per the ruling that this content is entirely
// project-scoped now.
test("parseDashboardTab_rolesStillParsesButHasNoStripEntry", () => {
  assert.equal(parseDashboardTab({ tab: "roles" }), "roles");
  assert.equal(DASHBOARD_TABS.includes("roles"), false);
});
