import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { ChannelScreenHeader } from "../../channels/ui/ChannelScreenHeader.tsx";
import {
  ChannelCodingSessionList,
  ChannelCodingSessionsTrigger,
} from "./ChannelCodingSessionsMenu.tsx";

const entries = [
  {
    session: {
      generationId: "instance:seat:7",
      label: "Session worker-a / generation 7",
      title: "Coding session",
      lastEventAt: "2026-07-30T12:00:00.000Z",
      status: "unknown",
      transcript: [],
      conflictCount: 0,
      commandTarget: null,
    },
    status: { kind: "working", label: "Working" },
  },
  {
    session: {
      generationId: "instance:seat:6",
      label: "Session worker-a / generation 6",
      title: "Coding session",
      lastEventAt: "2026-07-30T11:00:00.000Z",
      status: "unknown",
      transcript: [],
      conflictCount: 0,
      commandTarget: null,
    },
    status: { kind: "idle", label: "Idle" },
  },
];

test("channel session list renders labels, statuses, and both actions", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ChannelCodingSessionList, {
      entries,
      onOpen() {},
      onPopout() {},
    }),
  );

  assert.match(markup, /Session worker-a \/ generation 7/);
  assert.match(markup, /Working/);
  assert.match(markup, /Session worker-a \/ generation 6/);
  assert.match(markup, /Idle/);
  assert.equal(
    (markup.match(/data-testid="channel-coding-session-open"/g) ?? []).length,
    2,
  );
  assert.equal(
    (markup.match(/data-testid="channel-coding-session-popout"/g) ?? []).length,
    2,
  );
  assert.match(markup, /data-generation-id="instance:seat:7"/);
  assert.match(markup, /data-generation-id="instance:seat:6"/);
});

test("channel session list forwards only the clicked exact generation ids", () => {
  const opened = [];
  const poppedOut = [];
  const element = ChannelCodingSessionList({
    entries,
    onOpen: (generationId) => opened.push(generationId),
    onPopout: (generationId) => poppedOut.push(generationId),
  });
  const rows = element.props.children;
  const newestActions = rows[0].props.children[1].props.children;
  const olderActions = rows[1].props.children[1].props.children;

  newestActions[0].props.onClick();
  olderActions[1].props.onClick();

  assert.deepEqual(opened, ["instance:seat:7"]);
  assert.deepEqual(poppedOut, ["instance:seat:6"]);
});

test("an empty channel still explains where sessions will appear", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ChannelCodingSessionList, {
      entries: [],
      onOpen() {},
      onPopout() {},
    }),
  );

  assert.match(markup, /data-testid="channel-coding-sessions-empty"/);
  assert.doesNotMatch(markup, /data-testid="channel-coding-session-entry"/);
});

test("channel session trigger keeps its label inline and compacts to a titled icon", () => {
  const inlineMarkup = renderToStaticMarkup(
    React.createElement(ChannelCodingSessionsTrigger, {
      count: 2,
      variant: "inline",
    }),
  );
  const compactMarkup = renderToStaticMarkup(
    React.createElement(ChannelCodingSessionsTrigger, {
      count: 2,
      variant: "compact",
    }),
  );

  assert.match(inlineMarkup, />Sessions</);
  assert.match(inlineMarkup, />2</);
  assert.doesNotMatch(compactMarkup, />Sessions</);
  assert.match(compactMarkup, /aria-label="Coding sessions \(2\)"/);
  assert.match(compactMarkup, /title="Coding sessions \(2\)"/);
});

test("the trigger renders at zero so a first session is discoverable", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ChannelCodingSessionsTrigger, {
      count: 0,
      variant: "inline",
    }),
  );

  assert.match(markup, /data-testid="channel-coding-sessions-trigger"/);
  assert.match(markup, />Sessions</);
  assert.match(markup, />0</);
});

test("channel transition remounts the Sessions doorway so controlled open state cannot leak", () => {
  // The header now calls hooks (useTerminalPanel), so invoking it as a bare
  // function has no dispatcher. Host the call inside a probe component's
  // render and capture the returned element tree for introspection.
  const renderHeader = (channelId) => {
    let captured = null;
    function Probe() {
      captured = ChannelScreenHeader({
        activeChannel: {
          id: channelId,
          channelType: "stream",
          isMember: true,
          visibility: "open",
          archivedAt: null,
        },
        activeChannelEphemeralDisplay: null,
        activeChannelTitle: "Sessions",
        activeDmAvatarUrl: null,
        activeDmHeaderParticipants: [],
        activeDmPresenceStatus: null,
        onManageChannel() {},
        onToggleMembers() {},
      });
      return null;
    }
    renderToStaticMarkup(React.createElement(Probe));
    return captured;
  };

  const channelAMenu =
    renderHeader("channel-a").props.actions.props.children[0];
  const channelBMenu =
    renderHeader("channel-b").props.actions.props.children[0];

  assert.equal(channelAMenu.key, "channel-a");
  assert.equal(channelBMenu.key, "channel-b");
  assert.notEqual(channelAMenu.key, channelBMenu.key);
});
