import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionSurfaceHost } from "../CodingSessionSurfaceHost.tsx";
import {
  CodingSessionSurfaceLauncher,
  codingSessionSurfaceForShortcut,
} from "../CodingSessionSurfaceLauncher.tsx";
import { CODING_SESSION_BUILTIN_SURFACES } from "./codingSessionBuiltinSurfaces.ts";
import {
  CODING_SESSION_RESERVED_SURFACE_LETTERS,
  codingSessionE2eExtraSurfaceDefinition,
  createCodingSessionSurfaceRegistry,
  resolveCodingSessionSurfaces,
} from "./codingSessionSurfaceRegistry.ts";

// ---------------------------------------------------------------------------
// A ctx with nobody in it: every data-dependent surface is unavailable.
// ---------------------------------------------------------------------------

function emptyCtx(overrides = {}) {
  return {
    layout: "single",
    channelId: "channel-1",
    communityScope: "wss://relay.test",
    sessionKey: "session-1",
    umbrella: {
      umbrellaKey: "session-1",
      sessionRef: "session-1",
      title: "Session",
      executions: [],
      founderPubkey: null,
      genesisRef: null,
      genesisResolution: "legacy",
      status: "idle",
      lastEventAt: "",
      conflictCount: 0,
      foreignAttachmentCount: 0,
    },
    focusedExecution: null,
    focusedRecord: null,
    executions: [],
    transcript: [],
    transcriptModel: null,
    observedChanges: { files: [], unreportedEditCount: 0 },
    subagents: { rows: [] },
    taskModel: null,
    isLocalProvider: false,
    projectRef: null,
    repoRef: null,
    project: null,
    genesisRef: null,
    founderPubkey: null,
    currentUserPubkey: null,
    sessionClosed: false,
    lens: "conversation",
    activeSurfaceId: null,
    panelState: {
      rightOpen: true,
      tabs: [],
      active: null,
      expanded: false,
      bottomOpen: false,
    },
    panels: {},
    observations: { state: "not-read", reason: "no genesis" },
    openRulings: null,
    tree: {
      state: "resolved",
      available: false,
      source: null,
      label: "no working tree",
      reason: "No working tree for this session is recorded on this computer.",
      refusal: "notRecorded",
      query: {
        sessionId: null,
        channelId: "channel-1",
        projectRef: null,
        isLocalProvider: false,
      },
    },
    minimapSlotRef: { current: null },
    resolveActorName: () => null,
    resolveReachability: () => ({ known: false }),
    mission: null,
    extensions: {},
    ...overrides,
  };
}

// The SV-38 acceptance surface: this file defines it; nothing else is edited.
const memory = (overrides = {}) =>
  codingSessionE2eExtraSurfaceDefinition({
    id: "memory",
    label: "Memory",
    shortcut: "Y",
    badgeCount: 2,
    unavailableReason: "Not built",
    panelText: "Memory panel content",
    ...overrides,
  });

function launcherMarkup(registry, ctx = emptyCtx()) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceLauncher, {
      ctx,
      onOpen() {},
      surfaces: resolveCodingSessionSurfaces(registry.forLens(ctx.lens), ctx),
    }),
  );
}

// ---------------------------------------------------------------------------
// DB2: the built-ins and their letters.
// ---------------------------------------------------------------------------

test("every built-in is listed with its DB2 letter, in launcher order", () => {
  const registry = createCodingSessionSurfaceRegistry(
    CODING_SESSION_BUILTIN_SURFACES,
  );
  const letters = (lens) =>
    registry
      .forLens(lens)
      .map((definition) => definition.shortcut)
      .join("");
  assert.equal(letters("conversation"), "ADTFPLEUBM");
  // Mission adds Inspector, Context and Audit, first in its list, and keeps
  // every Conversation surface, Agents included (brief §3, DB2).
  assert.equal(letters("mission"), "ICXADTFPLEUBM");
  const markup = launcherMarkup(registry);
  for (const letter of "ADTFPLEUBM") {
    assert.match(markup, new RegExp(`<kbd[^>]*>${letter}</kbd>`));
  }
  assert.match(markup, />Open a surface</);
});

test("Y is reserved for Memory and no built-in takes it", () => {
  assert.equal(CODING_SESSION_RESERVED_SURFACE_LETTERS.Y, "memory");
  assert.ok(
    CODING_SESSION_BUILTIN_SURFACES.every(
      (definition) => definition.shortcut !== "Y",
    ),
  );
  assert.throws(
    () =>
      createCodingSessionSurfaceRegistry([
        codingSessionE2eExtraSurfaceDefinition({
          id: "notes",
          label: "Notes",
          shortcut: "Y",
        }),
      ]),
    /reserved for "memory"/,
  );
});

test("a duplicate id or letter throws at registration", () => {
  assert.throws(
    () =>
      createCodingSessionSurfaceRegistry([
        ...CODING_SESSION_BUILTIN_SURFACES,
        memory({ id: "diff", shortcut: "Y" }),
      ]),
    /registered twice/,
  );
  assert.throws(
    () =>
      createCodingSessionSurfaceRegistry([
        ...CODING_SESSION_BUILTIN_SURFACES,
        memory({ shortcut: "D" }),
      ]),
    /Letter D is taken by "diff"/,
  );
  assert.throws(
    () => createCodingSessionSurfaceRegistry([memory({ shortcut: "yy" })]),
    /one capital letter/,
  );
});

// ---------------------------------------------------------------------------
// SV-23 mechanics: dimmed, never absent, with the reason in the DOM.
// ---------------------------------------------------------------------------

test("Browser (without a session) and Device are dimmed with their reasons, never absent", () => {
  const registry = createCodingSessionSurfaceRegistry(
    CODING_SESSION_BUILTIN_SURFACES,
  );
  for (const lens of ["conversation", "mission"]) {
    // C4: the Browser is live; with no session it says what to do instead.
    const markup = launcherMarkup(registry, emptyCtx({ lens, channelId: "" }));
    for (const [id, reason] of [
      ["browser", "Open a session to use the Browser."],
      ["device", "Arrives with device support."],
    ]) {
      const row = markup.match(
        new RegExp(
          `<button[^>]*data-testid="coding-session-surface-launcher-row-${id}"[^>]*>[\\s\\S]*?</button>`,
        ),
      );
      assert.ok(row, `${id} row is in the DOM in ${lens}`);
      assert.match(row[0], /aria-disabled="true"/);
      assert.match(row[0], /data-available="false"/);
      assert.match(markup, new RegExp(reason.replace(".", "\\.")));
    }
  }
});

test("every dimmed built-in row carries a reason sentence", () => {
  const ctx = emptyCtx();
  for (const definition of CODING_SESSION_BUILTIN_SURFACES) {
    const availability = definition.availability(ctx);
    if (!availability.available) {
      assert.match(availability.reason, /^[A-Z].+\.$/, definition.id);
    }
  }
});

// ---------------------------------------------------------------------------
// SV-38: one file adds a surface — row, dimmed tooltip, key, badge and tab.
// ---------------------------------------------------------------------------

test("a fake Memory surface shows its launcher row, letter, badge and reason", () => {
  const registry = createCodingSessionSurfaceRegistry([
    ...CODING_SESSION_BUILTIN_SURFACES,
    memory(),
  ]);
  assert.equal(registry.get("memory")?.shortcut, "Y");
  const markup = launcherMarkup(registry);
  assert.match(
    markup,
    /data-testid="coding-session-surface-launcher-row-memory"/,
  );
  assert.match(markup, />Memory</);
  assert.match(markup, /<kbd[^>]*>Y<\/kbd>/);
  // Unavailable: dimmed, with the reason one hover (or one screen reader
  // description) away.
  const row = markup.match(
    /<button[^>]*aria-disabled="true"[^>]*data-testid="coding-session-surface-launcher-row-memory"[^>]*>/,
  );
  assert.ok(row, "the Memory row is dimmed with aria-disabled");
  assert.match(row[0], /aria-disabled="true"/);
  assert.match(row[0], /aria-describedby="[^"]+"/);
  assert.match(markup, /<span class="sr-only" id="[^"]+">Not built<\/span>/);
  // The badge slot sits on the icon and draws the activity count.
  assert.match(
    markup,
    /data-testid="coding-session-surface-badge-slot-memory"[\s\S]*?>2</,
  );
  // An unavailable surface's letter opens nothing.
  const surfaces = resolveCodingSessionSurfaces(
    registry.forLens("conversation"),
    emptyCtx(),
  );
  assert.equal(codingSessionSurfaceForShortcut(surfaces, { key: "y" }), null);
});

test("an available Memory surface opens from Y and becomes a tab", () => {
  const registry = createCodingSessionSurfaceRegistry([
    ...CODING_SESSION_BUILTIN_SURFACES,
    memory({ unavailableReason: undefined }),
  ]);
  const ctx = emptyCtx();
  const surfaces = resolveCodingSessionSurfaces(
    registry.forLens("conversation"),
    ctx,
  );
  assert.equal(
    codingSessionSurfaceForShortcut(surfaces, { key: "y" })?.id,
    "memory",
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionSurfaceHost, {
      ctx,
      layout: "inline",
      panels: {
        state: {
          rightOpen: true,
          tabs: ["memory"],
          active: "memory",
          expanded: false,
          bottomOpen: false,
        },
        actions: {},
      },
      surfaces,
      widthContainerRef: { current: null },
    }),
  );
  assert.match(markup, /data-testid="coding-session-surface-tab-memory"/);
  assert.match(markup, /role="tab"/);
  assert.match(markup, /Memory panel content/);
  assert.match(markup, /data-testid="coding-session-surface-tab-close-memory"/);
});
