import assert from "node:assert/strict";
import { test } from "node:test";

import { APP_SOFTWARE_URL } from "../relayBuildDrift.ts";
import {
  aboutBuildState,
  aboutBuildTooltip,
  aboutGitLine,
  shortSha,
  UNKNOWN,
} from "./aboutBuildRowCopy.ts";

const APP_SHA = "f723824d68183a1a2c4c7925673853694825f973";
const RELAY_SHA = "42dd921d831c483e6e16111491b39947b4cf1f86";

const input = ({
  appCommit = APP_SHA,
  appCount = 3332,
  appDirty = null,
  relayCommit = RELAY_SHA,
  relayCount = 3340,
  relayBuildTime = "2026-09-05T12:00:00Z",
  software = APP_SOFTWARE_URL,
} = {}) => ({
  input: {
    app: { commit: appCommit, commitCount: appCount, sourceDirty: appDirty },
    relay: { commit: relayCommit, commitCount: relayCount, software },
  },
  relayBuildTime,
});

test("the same commit reads as one build, whatever the counts say", () => {
  const { input: same } = input({ relayCommit: APP_SHA, relayCount: 1 });
  assert.equal(aboutBuildState(same).label, "same build");
});

test("a distance names the method it was measured by", () => {
  assert.equal(
    aboutBuildState(input({ appCount: 3332, relayCount: 3340 }).input).label,
    "app behind by 8 commits (by commit count)",
  );
  assert.equal(
    aboutBuildState(input({ appCount: 3341, relayCount: 3340 }).input).label,
    "app ahead by 1 commit (by commit count)",
  );
});

/**
 * The case that is live today: hive answers `unknown` on every build because
 * its deployer predates the `BEEKEEPER_SOURCE_SHA` fix (2ba548c9e). A bare
 * "unknown" sends the reader to the relay; naming the cause does not.
 */
test("a relay that stamps no build says why", () => {
  const { input: unstamped } = input({ relayCommit: null, relayCount: null });
  assert.equal(
    aboutBuildState(unstamped).label,
    "relay unknown (deployer does not stamp builds)",
  );
});

test("an app that cannot name its own commit says so, and never guesses", () => {
  const { input: unstamped } = input({ appCommit: null, appCount: null });
  assert.equal(aboutBuildState(unstamped).label, "app build commit unknown");
  assert.equal(shortSha(null), UNKNOWN);
  assert.equal(shortSha(undefined), UNKNOWN);
  assert.equal(shortSha(""), UNKNOWN);
});

test("a modified tree stops the comparison rather than reporting a distance", () => {
  const { input: dirty } = input({ appDirty: true });
  assert.equal(aboutBuildState(dirty).label, "app built from a modified tree");
});

test("equal counts with different commits are not a match", () => {
  const { input: divergent } = input({ appCount: 3340, relayCount: 3340 });
  assert.equal(
    aboutBuildState(divergent).label,
    "different builds at the same commit count",
  );
});

test("the row truncates to eight hex, the tooltip carries the whole name", () => {
  assert.equal(shortSha(APP_SHA), "f723824d");
  assert.equal(shortSha(APP_SHA).length, 8);
  const { input: pair, relayBuildTime } = input();
  const tooltip = aboutBuildTooltip(pair, relayBuildTime);
  assert.match(tooltip, new RegExp(APP_SHA));
  assert.match(tooltip, new RegExp(RELAY_SHA));
  assert.match(tooltip, /count 3332/);
  assert.match(tooltip, /built 2026-09-05T12:00:00Z/);
});

test("every absence in the tooltip is disclosed, never blank", () => {
  const { input: nothing } = input({
    appCommit: null,
    appCount: null,
    relayCommit: null,
    relayCount: null,
  });
  const tooltip = aboutBuildTooltip(nothing, null);
  assert.match(tooltip, /App {4}unknown/);
  assert.match(tooltip, /Relay {2}unknown/);
  assert.match(tooltip, /count null/);
  assert.match(tooltip, /built unknown/);
});

test("a dirty app build is disclosed in the tooltip too", () => {
  const { input: dirty, relayBuildTime } = input({ appDirty: true });
  assert.match(
    aboutBuildTooltip(dirty, relayBuildTime),
    /built from a modified tree/,
  );
});

test("a capable git is stated plainly, with no warning", () => {
  assert.deepEqual(
    aboutGitLine({
      path: "/opt/homebrew/bin/git",
      version: "2.55.0",
      meetsMinimum: true,
      minimum: "2.46",
    }),
    { text: "git 2.55.0 at /opt/homebrew/bin/git", warning: false },
  );
});

test("an old git names itself, the requirement, and warns", () => {
  const line = aboutGitLine({
    path: "/usr/bin/git",
    version: "2.39.5",
    meetsMinimum: false,
    minimum: "2.46",
  });
  assert.equal(line.warning, true);
  assert.match(line.text, /^git 2\.39\.5 at \/usr\/bin\/git/);
  assert.match(line.text, /needs git 2\.46 or newer$/);
});

test("no git at all is an absence, never a version", () => {
  const line = aboutGitLine({
    path: null,
    version: null,
    meetsMinimum: false,
    minimum: "2.46",
  });
  assert.equal(line.warning, true);
  assert.equal(line.text, "no git found — the relay needs git 2.46 or newer");
});

test("an unanswered probe renders nothing rather than a guess", () => {
  assert.equal(aboutGitLine(null), null);
});
