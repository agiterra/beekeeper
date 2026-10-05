import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_TERMINAL_DEFAULT_HEIGHT,
  CODING_SESSION_TERMINAL_MAX_PER_GROUP,
  CODING_SESSION_TERMINAL_MIN_HEIGHT,
  EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
  activateCodingSessionTerminal,
  addCodingSessionTerminal,
  clampCodingSessionTerminalHeight,
  closeCodingSessionTerminal,
  codingSessionForegroundFold,
  codingSessionForegroundUnknown,
  codingSessionRunningCommandsLabel,
  codingSessionSharedTerminalsTruncatedLine,
  codingSessionTreeElsewhereClause,
  codingSessionTreeProviderName,
  codingSessionSharedTerminalLabel,
  codingSessionSharedTerminalsFor,
  codingSessionShellsFor,
  codingSessionTerminalGroupLabel,
  codingSessionTerminalHeaderLine,
  codingSessionTerminalHeightStorageKey,
  codingSessionTerminalLabels,
  codingSessionTerminalMaxHeight,
  codingSessionTerminalSplitLimitReached,
  countCodingSessionRunningCommands,
  nextCodingSessionTerminalTitle,
  parseStoredCodingSessionTerminalHeight,
  parseStoredCodingSessionTerminalLayout,
  reconcileCodingSessionTerminalLayout,
  splitCodingSessionTerminal,
  visibleCodingSessionTerminals,
} from "./codingSessionTerminalModel.ts";

test("height: T3's 280 default, 180 minimum, 75% maximum", () => {
  assert.equal(CODING_SESSION_TERMINAL_DEFAULT_HEIGHT, 280);
  assert.equal(CODING_SESSION_TERMINAL_MIN_HEIGHT, 180);
  assert.equal(codingSessionTerminalMaxHeight(900), 675);
  assert.equal(clampCodingSessionTerminalHeight(100, 900), 180);
  assert.equal(clampCodingSessionTerminalHeight(5000, 900), 675);
  assert.equal(clampCodingSessionTerminalHeight(300.6, 900), 301);
  assert.equal(clampCodingSessionTerminalHeight(Number.NaN, 900), 280);
  // A window too short for 75% to clear the minimum still allows 180.
  assert.equal(codingSessionTerminalMaxHeight(200), 180);
});

test("height storage is per community and falls back to the default", () => {
  assert.equal(
    codingSessionTerminalHeightStorageKey("wss://hive.example"),
    "beekeeper:session-terminal-height:v1:wss://hive.example",
  );
  assert.equal(parseStoredCodingSessionTerminalHeight(null), 280);
  assert.equal(parseStoredCodingSessionTerminalHeight("junk"), 280);
  assert.equal(parseStoredCodingSessionTerminalHeight("412"), 412);
});

test("new makes a group of one; split joins the active group, up to four", () => {
  let layout = addCodingSessionTerminal(
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
    "a",
  );
  layout = splitCodingSessionTerminal(layout, "b");
  assert.deepEqual(visibleCodingSessionTerminals(layout), {
    ids: ["a", "b"],
    direction: "horizontal",
  });
  layout = splitCodingSessionTerminal(layout, "c", "vertical");
  layout = splitCodingSessionTerminal(layout, "d");
  assert.equal(layout.groups[0].terminalIds.length, 4);
  assert.equal(CODING_SESSION_TERMINAL_MAX_PER_GROUP, 4);
  assert.ok(codingSessionTerminalSplitLimitReached(layout));
  const refused = splitCodingSessionTerminal(layout, "e");
  assert.equal(refused, layout, "a fifth split is refused");
  layout = addCodingSessionTerminal(layout, "e");
  assert.equal(layout.groups.length, 2);
  assert.equal(layout.activeTerminalId, "e");
  assert.deepEqual(visibleCodingSessionTerminals(layout).ids, ["e"]);
  assert.equal(
    codingSessionTerminalGroupLabel(layout.groups[0]),
    "Side by side",
  );
  assert.equal(codingSessionTerminalGroupLabel(layout.groups[1]), "Single");
});

test("a split lands just after the active terminal and keeps its direction", () => {
  let layout = addCodingSessionTerminal(
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
    "a",
  );
  layout = splitCodingSessionTerminal(layout, "b", "vertical");
  layout = activateCodingSessionTerminal(layout, "a");
  layout = splitCodingSessionTerminal(layout, "c", "vertical");
  assert.deepEqual(layout.groups[0].terminalIds, ["a", "c", "b"]);
  assert.equal(codingSessionTerminalGroupLabel(layout.groups[0]), "Stacked");
});

test("close hands focus to the neighbour and empties to the empty layout", () => {
  let layout = addCodingSessionTerminal(
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
    "a",
  );
  layout = splitCodingSessionTerminal(layout, "b");
  layout = addCodingSessionTerminal(layout, "c");
  layout = activateCodingSessionTerminal(layout, "b");
  layout = closeCodingSessionTerminal(layout, "b");
  assert.equal(layout.activeTerminalId, "c");
  assert.deepEqual(layout.groups[0].terminalIds, ["a"]);
  layout = closeCodingSessionTerminal(layout, "a");
  layout = closeCodingSessionTerminal(layout, "c");
  assert.equal(layout, EMPTY_CODING_SESSION_TERMINAL_LAYOUT);
});

test("reconcile drops shells that ended and adopts ones it never saw", () => {
  let layout = addCodingSessionTerminal(
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
    "a",
  );
  layout = splitCodingSessionTerminal(layout, "b");
  const next = reconcileCodingSessionTerminalLayout(layout, ["b", "z"]);
  assert.deepEqual(next.terminalIds, ["b", "z"]);
  assert.deepEqual(
    next.groups.map((group) => group.terminalIds),
    [["b"], ["z"]],
  );
  assert.equal(next.activeTerminalId, "b");
  assert.ok(next.groups.some((group) => group.id === next.activeGroupId));
  const empty = reconcileCodingSessionTerminalLayout(layout, []);
  assert.equal(empty.activeTerminalId, "");
  assert.deepEqual(empty.groups, []);
});

test("a stored layout survives a round trip and junk reads as empty", () => {
  let layout = addCodingSessionTerminal(
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
    "a",
  );
  layout = splitCodingSessionTerminal(layout, "b", "vertical");
  assert.deepEqual(
    parseStoredCodingSessionTerminalLayout(JSON.stringify(layout)),
    layout,
  );
  assert.deepEqual(
    parseStoredCodingSessionTerminalLayout("{nope"),
    EMPTY_CODING_SESSION_TERMINAL_LAYOUT,
  );
});

test("only this session's shells, oldest first", () => {
  const shells = [
    {
      sessionId: "late",
      title: "Terminal 2",
      createdAt: 20,
      running: true,
      restorable: false,
      codingSession: { sessionRef: "s1", channelId: "c1" },
    },
    {
      sessionId: "plain",
      title: "zsh",
      createdAt: 5,
      running: true,
      restorable: false,
      codingSession: null,
    },
    {
      sessionId: "other",
      title: "Terminal 1",
      createdAt: 6,
      running: true,
      restorable: false,
      codingSession: { sessionRef: "s2", channelId: "c1" },
    },
    {
      sessionId: "early",
      title: "Terminal 1",
      createdAt: 10,
      running: true,
      restorable: false,
      codingSession: { sessionRef: "s1", channelId: "c1" },
    },
  ];
  assert.deepEqual(
    codingSessionShellsFor(shells, { sessionKey: "s1", channelId: "c1" }).map(
      (shell) => shell.sessionId,
    ),
    ["early", "late"],
  );
  assert.equal(
    nextCodingSessionTerminalTitle(shells.slice(0, 1)),
    "Terminal 3",
  );
  assert.deepEqual(
    [...codingSessionTerminalLabels(["x", "y"]).values()],
    ["Terminal 1", "Terminal 2"],
  );
});

test("teammates' terminals: this session's only, never this computer's own", () => {
  const terminals = [
    {
      sessionId: "t1",
      ownerPubkey: "b",
      title: "T",
      projectRef: "p",
      sessionRef: "s1",
    },
    {
      sessionId: "t2",
      ownerPubkey: "b",
      title: "T",
      projectRef: "p",
      sessionRef: "s2",
    },
    {
      sessionId: "t3",
      ownerPubkey: "b",
      title: "T",
      projectRef: "p",
      sessionRef: null,
    },
    {
      sessionId: "mine",
      ownerPubkey: "me",
      title: "T",
      projectRef: "p",
      sessionRef: "s1",
    },
  ];
  assert.deepEqual(
    codingSessionSharedTerminalsFor(terminals, "s1", new Set(["mine"])).map(
      (terminal) => terminal.sessionId,
    ),
    ["t1"],
  );
});

test("labels say whose computer and how live, never which host", () => {
  assert.equal(
    codingSessionTerminalHeaderLine("this session's worktree"),
    "Your shell · not sandboxed · this session's worktree",
  );
  assert.equal(
    codingSessionSharedTerminalLabel({
      ownerName: "Brian",
      isSelf: false,
      status: "live",
    }),
    "Brian's computer · live",
  );
  assert.equal(
    codingSessionSharedTerminalLabel({
      ownerName: "Brian",
      isSelf: true,
      status: "stalled",
    }),
    "Your other computer · stalled",
  );
});

test("the badge counts commands running now, on this session's shells only", () => {
  const ids = new Set(["a", "b", "c"]);
  const reads = [
    { sessionId: "a", runningCommand: true },
    { sessionId: "b", runningCommand: false },
    { sessionId: "c", runningCommand: null },
    { sessionId: "elsewhere", runningCommand: true },
  ];
  assert.equal(countCodingSessionRunningCommands(reads, ids), 1);
  assert.equal(
    countCodingSessionRunningCommands(
      [{ sessionId: "a", runningCommand: false }],
      ids,
    ),
    0,
    "at the prompt the badge clears",
  );
  assert.equal(
    codingSessionRunningCommandsLabel(1),
    "1 command running on this computer",
  );
  assert.equal(
    codingSessionRunningCommandsLabel(2),
    "2 commands running on this computer",
  );
});

test("a failed or stale foreground read is unknown, never the last answer", () => {
  const base = {
    isError: false,
    dataUpdatedAt: 10_000,
    now: 11_000,
    pollMs: 1_500,
  };
  assert.equal(codingSessionForegroundUnknown(base), false);
  assert.equal(
    codingSessionForegroundUnknown({ ...base, isError: true }),
    true,
  );
  assert.equal(
    codingSessionForegroundUnknown({ ...base, now: 10_000 + 3_001 }),
    true,
  );
  // No read has landed yet: loading, not unknown.
  assert.equal(
    codingSessionForegroundUnknown({ ...base, dataUpdatedAt: 0, now: 99_999 }),
    false,
  );
});

test("the foreground fold empties every set when the read is unknown", () => {
  const reads = [
    { sessionId: "a", runningCommand: true },
    { sessionId: "b", runningCommand: false },
    { sessionId: "c", runningCommand: null },
    { sessionId: "z", runningCommand: true },
  ];
  const known = codingSessionForegroundFold({
    reads,
    liveIds: ["a", "b", "c"],
    unknown: false,
  });
  assert.deepEqual([...known.runningIds], ["a"]);
  assert.deepEqual([...known.idleIds], ["b"]);
  assert.equal(known.runningCount, 1);
  const unknown = codingSessionForegroundFold({
    reads,
    liveIds: ["a", "b", "c"],
    unknown: true,
  });
  assert.equal(unknown.runningIds.size, 0);
  assert.equal(unknown.idleIds.size, 0);
  assert.equal(unknown.runningCount, 0);
});

test("the elsewhere clause names a provider, never a computer's owner", () => {
  assert.equal(
    codingSessionTreeElsewhereClause("Andy"),
    "The working tree is on another computer (Andy's provider runs this session)",
  );
  assert.equal(
    codingSessionTreeElsewhereClause("  "),
    "The working tree is on another computer",
  );
  const names = { p: "Andy", s: "Signer" };
  const resolveActorName = (k) => names[k] ?? null;
  assert.equal(
    codingSessionTreeProviderName({
      focusedRecord: { providerAuthorityPubkey: "p" },
      focusedExecution: { signerPubkey: "s" },
      resolveActorName,
    }),
    "Andy",
  );
  assert.equal(
    codingSessionTreeProviderName({
      focusedRecord: null,
      focusedExecution: { signerPubkey: "x" },
      resolveActorName,
    }),
    null,
  );
  assert.match(codingSessionSharedTerminalsTruncatedLine(100), /newest 100/);
});
