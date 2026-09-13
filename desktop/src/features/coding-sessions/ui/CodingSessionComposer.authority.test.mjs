import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
before(() => {
  Object.assign(globalThis, {
    CustomEvent: dom.window.CustomEvent,
    document: dom.window.document,
    Element: dom.window.Element,
    Event: dom.window.Event,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    Node: dom.window.Node,
    NodeFilter: dom.window.NodeFilter,
    window: dom.window,
  });
});
after(() => dom.window.close());

test("a mounted composer distinguishes unresolved authority from denied access, then enables the resolved founder", async () => {
  const React = await import("react");
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const { resolveCodingSessionUmbrellaComposerAuthority } = await import(
    "../lib/codingSessionUmbrellaComposerModel.ts"
  );
  const founder = "a".repeat(64);
  const viewer = "b".repeat(64);
  const genesisRef = "c".repeat(64);
  const target = {
    driver: "provider-neutral",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  };
  function composer(founderPubkey, currentUserPubkey) {
    const authority = resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey, genesisRef },
      currentUserPubkey,
      acceptedOperators: new Set(),
    });
    return React.createElement(CodingSessionComposer, {
      authorityReason: authority.reason,
      authorityUnresolved: authority.isUnresolved,
      canControl: authority.canPromptExecutions,
      canInterrupt: true,
      channelId: "channel",
      immersive: true,
      channelAccess: { kind: "member" },
      isWorking: false,
      target,
    });
  }
  try {
    let mounted;
    await act(async () => {
      mounted = render(composer(null, founder));
    });
    const editor = () => screen.getByLabelText("Coding-session instruction");
    assert.equal(editor().disabled, true);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /Access unresolved/,
    );
    assert.match(
      screen.getByTestId("coding-session-composer-authority-unresolved")
        .textContent,
      /authority could not be resolved/,
    );
    assert.doesNotMatch(editor().placeholder, /collaborator/);

    await act(async () => {
      mounted.rerender(composer(founder, null));
    });
    assert.equal(editor().disabled, true);
    assert.match(editor().placeholder, /identity is available/);

    await act(async () => {
      mounted.rerender(composer(founder, founder));
    });
    assert.equal(editor().disabled, false);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /Can control/,
    );
    assert.equal(
      screen.queryByTestId("coding-session-composer-authority-unresolved"),
      null,
    );

    await act(async () => {
      mounted.rerender(composer(founder, viewer));
    });
    assert.equal(editor().disabled, true);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /View only/,
    );
    assert.match(editor().placeholder, /collaborator access/);
    assert.equal(
      screen.queryByTestId("coding-session-composer-authority-unresolved"),
      null,
    );
  } finally {
    cleanup();
  }
});

/**
 * Channel write access is its own gate, beside founder authority. The live
 * case: a project owner founded a session in the project's transport channel
 * and has no channel-membership row, which the relay admits
 * (`ingest.rs` transport gate) — the composer used to call that "View only".
 */
test("a mounted founder composer follows the relay's transport write rule, with each refusal named", async () => {
  const React = await import("react");
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const { resolveCodingSessionUmbrellaComposerAuthority } = await import(
    "../lib/codingSessionUmbrellaComposerModel.ts"
  );
  const { resolveCodingSessionChannelAccess } = await import(
    "../lib/codingSessionChannelAccess.ts"
  );
  const creator = "1".repeat(64);
  const founder = "a".repeat(64);
  const collaborator = "c".repeat(64);
  const viewer = "d".repeat(64);
  const invitee = "e".repeat(64);
  const genesisRef = "f".repeat(64);
  const transport = {
    channelType: "transport",
    isMember: false,
    projectRef: `30621:${creator}:tank-loop`,
  };
  const roster = [
    { pubkey: founder, role: "owner" },
    { pubkey: collaborator, role: "collaborator" },
    { pubkey: viewer, role: "viewer" },
  ];
  const target = {
    driver: "provider-neutral",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  };
  // Founder authority is held constant (every identity here is treated as
  // steering-capable) so only channel write access varies.
  function composer(currentUserPubkey, overrides = {}) {
    const authority = resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey: currentUserPubkey, genesisRef },
      currentUserPubkey,
      acceptedOperators: new Set(),
    });
    const channelAccess = resolveCodingSessionChannelAccess({
      channel: transport,
      channelsStatus: "ready",
      currentUserPubkey,
      project: { status: "ready", found: true, truncated: false },
      roster: { status: "ready", members: roster },
      ...overrides,
    });
    return React.createElement(CodingSessionComposer, {
      authorityReason: authority.reason,
      authorityUnresolved: authority.isUnresolved,
      canControl: authority.canPromptExecutions,
      canInterrupt: true,
      channelAccess,
      channelId: "channel",
      immersive: true,
      isWorking: false,
      target,
    });
  }
  const editor = () => screen.getByLabelText("Coding-session instruction");
  const deck = () => screen.getByTestId("coding-session-control-deck");
  try {
    let mounted;
    // Project owner founder, no membership row: the relay admits, so do we.
    await act(async () => {
      mounted = render(composer(founder));
    });
    assert.equal(editor().disabled, false);
    assert.match(deck().textContent, /Can control/);
    assert.doesNotMatch(deck().textContent, /View only/);

    // Collaborator: admitted.
    await act(async () => mounted.rerender(composer(collaborator)));
    assert.equal(editor().disabled, false);
    assert.match(deck().textContent, /Can control/);

    // Project viewer: refused, and told why in project terms.
    await act(async () => mounted.rerender(composer(viewer)));
    assert.equal(editor().disabled, true);
    assert.equal(
      editor().placeholder,
      "You can read this project session, but your project role (Viewer) does not allow steering.",
    );
    assert.match(deck().textContent, /Read only/);
    assert.doesNotMatch(editor().placeholder, /Join this channel/);

    // Per-session viewer invitee: reads the transport, not on the roster.
    await act(async () => mounted.rerender(composer(invitee)));
    assert.equal(editor().disabled, true);
    assert.match(
      editor().placeholder,
      /^You can read this project session, but/,
    );
    assert.doesNotMatch(editor().placeholder, /Join this channel/);

    // Roster still loading: checking, never "join", never "view only".
    await act(async () =>
      mounted.rerender(
        composer(founder, { roster: { status: "loading", members: [] } }),
      ),
    );
    // The founder here is a roster owner, not the creator, so the roster
    // is what decides — and it has not answered.
    assert.equal(editor().disabled, true);
    assert.equal(editor().placeholder, "Checking your access to this session…");
    assert.match(deck().textContent, /Checking access/);
    assert.doesNotMatch(deck().textContent, /View only/);

    // Ordinary channel without membership: the join copy stays.
    await act(async () =>
      mounted.rerender(
        composer(founder, {
          channel: { channelType: "stream", isMember: false, projectRef: null },
        }),
      ),
    );
    assert.equal(editor().disabled, true);
    assert.equal(editor().placeholder, "Join this channel to send a message.");
    assert.match(deck().textContent, /View only/);

    // Explicit member of that channel: admitted.
    await act(async () =>
      mounted.rerender(
        composer(viewer, {
          channel: { channelType: "stream", isMember: true, projectRef: null },
        }),
      ),
    );
    assert.equal(editor().disabled, false);
  } finally {
    cleanup();
  }
});
