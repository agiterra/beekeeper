/**
 * The session People surface, mounted for real.
 *
 * The roster fold and the identity are seeded straight into the query cache
 * rather than faked at the relay: both queries are plain React Query entries
 * with long staleness, so a seeded entry is what the component reads and no
 * WebSocket is involved. Profiles come back through the mocked native
 * `get_users_batch`, because their *timing* is under test — the no-flicker
 * gate is a claim about the moment a real profile result lands, and a seeded
 * cache entry would arrive already settled.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
// React Query schedules five-minute garbage-collection timers for every query
// it builds. They outlive the assertions and, unreferenced by nothing, would
// hold this file's process open for five minutes after its last test — which
// is five minutes added to `pnpm test`. Unreference anything long; nothing
// here ever waits that long on purpose.
const realSetTimeout = globalThis.setTimeout;
globalThis.setTimeout = (callback, delay, ...rest) => {
  const handle = realSetTimeout(callback, delay, ...rest);
  if (delay > 10_000 && typeof handle?.unref === "function") handle.unref();
  return handle;
};

let answers = {};
let clients = [];

before(() => {
  // JSDOM ships no `matchMedia`; the theme provider asks it for the system
  // colour scheme on mount.
  dom.window.matchMedia = (query) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  });
  Object.assign(globalThis, {
    matchMedia: dom.window.matchMedia,
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    Node: dom.window.Node,
    NodeFilter: dom.window.NodeFilter,
    DocumentFragment: dom.window.DocumentFragment,
    HTMLButtonElement: dom.window.HTMLButtonElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    DOMRect: dom.window.DOMRect,
    ResizeObserver:
      dom.window.ResizeObserver ??
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    requestAnimationFrame: (callback) => setTimeout(() => callback(0), 0),
    cancelAnimationFrame: (handle) => clearTimeout(handle),
    MutationObserver: dom.window.MutationObserver,
    CustomEvent: dom.window.CustomEvent,
    Event: dom.window.Event,
    KeyboardEvent: dom.window.KeyboardEvent,
    MouseEvent: dom.window.MouseEvent,
    PointerEvent: dom.window.PointerEvent ?? dom.window.MouseEvent,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    localStorage: dom.window.localStorage,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.__TAURI_INTERNALS__ = {
    invoke: async (command, args) => {
      const answer = answers[command];
      if (typeof answer === "function") return answer(args);
      if (answer !== undefined) return answer;
      if (command === "get_relay_http_url") return "https://hive.example";
      if (command === "get_media_proxy_port") return 3001;
      if (command === "search_users") return { users: [], next_cursor: null };
      if (command === "get_users_batch") return { profiles: {}, missing: [] };
      return null;
    },
  };
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  for (const client of clients) client.clear();
  clients = [];
  answers = {};
  localStorage.clear();
});

after(() => dom.window.close());

const CHANNEL_ID = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS = "d".repeat(64);
const FOUNDER = "1a".repeat(32);
const PROVIDER = "2b".repeat(32);
const SEAT = "3c".repeat(32);
const AGENT = "4d".repeat(32);
const STRANGER = "5e".repeat(32);
const NAMED = "6f".repeat(32);

/** The exact fold shape `useCodingSessionRoster` caches before `select`. */
function fold({ accepted = [], activeSeats = [], pending = [] } = {}) {
  return {
    accepted: new Map(accepted),
    activeSeats: new Map(activeSeats),
    acceptedHead: { eventId: "e".repeat(64), seq: accepted.length },
    pending,
  };
}

const FULL_FOLD = fold({
  accepted: [
    [PROVIDER, "operator"],
    [SEAT, "operator"],
    [AGENT, "operator"],
    [STRANGER, "viewer"],
    [NAMED, "operator"],
  ],
  activeSeats: [[SEAT, "builder"]],
});

async function mount({
  foldData = FULL_FOLD,
  viewer = FOUNDER,
  profiles = undefined,
  providerAuthorityLabels = new Map([[PROVIDER, "codex"]]),
  knownAgents = [AGENT],
  founderPubkey = FOUNDER,
} = {}) {
  localStorage.setItem(
    "buzz-communities",
    JSON.stringify([
      {
        id: "community-a",
        name: "Hive",
        relayUrl: "wss://hive.example",
        addedAt: "2026-09-09T00:00:00Z",
      },
    ]),
  );
  localStorage.setItem("buzz-active-community-id", "community-a");
  const React = (await import("react")).default;
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { act, render } = await import("@testing-library/react");
  const { CommunitiesProvider } = await import(
    "@/features/communities/useCommunities"
  );
  const { ThemeProvider } = await import("@/shared/theme/ThemeProvider");
  const { codingSessionRosterQueryKey } = await import(
    "../lib/codingSessionRoster.ts"
  );
  const { CodingSessionPeoplePopover } = await import(
    "./CodingSessionPeoplePopover.tsx"
  );

  const { KnownAgentPubkeysProvider } = await import(
    "@/features/agents/useKnownAgentPubkeys"
  );
  const { managedAgentsQueryKey, relayAgentsQueryKey } = await import(
    "@/features/agents/hooks"
  );

  answers.get_users_batch =
    profiles === undefined
      ? // Never resolves: profiles have not settled, so the gate must hold.
        () => new Promise(() => {})
      : () => ({ profiles, missing: [] });

  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  clients.push(client);
  client.setQueryData(["identity"], { pubkey: viewer, npub: null });
  client.setQueryData(
    codingSessionRosterQueryKey(CHANNEL_ID, GENESIS),
    foldData,
  );
  // The known-agent baseline is the real provider over its real source
  // queries; seeding them is how a managed or relay-registered agent becomes
  // known without a second query observer anywhere.
  client.setQueryData(
    managedAgentsQueryKey,
    knownAgents.map((pubkey) => ({ pubkey, status: "stopped" })),
  );
  client.setQueryData(relayAgentsQueryKey, []);

  const utils = render(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(
        ThemeProvider,
        null,
        React.createElement(
          CommunitiesProvider,
          null,
          React.createElement(
            KnownAgentPubkeysProvider,
            null,
            React.createElement(CodingSessionPeoplePopover, {
              channelId: CHANNEL_ID,
              founderPubkey,
              genesisRef: GENESIS,
              onOpenChange: () => {},
              open: true,
              providerAuthorityLabels,
            }),
          ),
        ),
      ),
    ),
  );

  // Let the profile batch resolve (or stay pending, when the gate is under
  // test) and React flush the result before anything is asserted.
  for (let tick = 0; tick < 5; tick += 1) {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
  return utils;
}

function rowText(pubkey) {
  const row = document.querySelector(
    `[data-testid="coding-session-people-row-${pubkey}"]`,
  );
  assert.ok(row, `no row for ${pubkey}`);
  return row.textContent ?? "";
}

/**
 * Classes that would clip the evidence sentence to one line. JSDOM computes no
 * layout, so the browser lane owns the measurement; what this file can pin is
 * that nothing in the chain from the sentence up to its row asks for a clamp,
 * and that the whole sentence is in the DOM rather than only in a `title`.
 */
const CLAMPING_CLASS =
  /(^|\s)(truncate|text-ellipsis|whitespace-nowrap|line-clamp-\d+)(\s|$)/;

/**
 * A flex child that may shrink is compressed to fit its container instead of
 * growing it. That is what turned a 74px row into a 47px one and printed one
 * row's wrapped sentence over the next. JSDOM measures nothing, so what is
 * assertable here is the class that permits it — which is the mistake itself.
 * The browser lane owns the two pixel sizes.
 */
const NO_SHRINK_CLASS = /(^|\s)(shrink-0|flex-none)(\s|$)/;

function assertCannotShrink(element, what) {
  assert.match(
    element.getAttribute("class") ?? "",
    NO_SHRINK_CLASS,
    `${what} is a flex child that may shrink; its wrapped text will be clipped`,
  );
}

function detail(pubkey) {
  const node = document.querySelector(
    `[data-testid="coding-session-people-detail-${pubkey}"]`,
  );
  assert.ok(node, `no detail line for ${pubkey}`);
  return node;
}

function assertSentenceUnclamped(pubkey) {
  const node = detail(pubkey);
  const row = node.closest(
    `[data-testid="coding-session-people-row-${pubkey}"]`,
  );
  assert.ok(row, "the detail line must live inside its row");
  assert.ok(
    (node.textContent ?? "").trim().length > 0,
    "the sentence must render as text, not only as a title",
  );
  assert.equal(
    node.getAttribute("title"),
    null,
    "a tooltip is not where this fact belongs",
  );
  const chain = [...node.querySelectorAll("*"), node];
  for (let element = node; element && element !== row; ) {
    element = element.parentElement;
    if (element && element !== row) chain.push(element);
  }
  for (const element of chain) {
    assert.doesNotMatch(
      element.getAttribute("class") ?? "",
      CLAMPING_CLASS,
      `clamped: ${element.getAttribute("data-testid") ?? element.tagName}`,
    );
  }
}

function kindBadge(pubkey) {
  return document.querySelector(
    `[data-testid="coding-session-people-kind-${pubkey}"]`,
  );
}

const RESOLVED_PROFILES = {
  [FOUNDER]: {
    display_name: "Brian",
    avatar_url: null,
    nip05_handle: null,
    owner_pubkey: null,
    is_agent: false,
  },
  [NAMED]: {
    display_name: "Brian",
    avatar_url: null,
    nip05_handle: null,
    owner_pubkey: null,
    is_agent: false,
  },
  [AGENT]: {
    display_name: "Helios",
    avatar_url: null,
    nip05_handle: null,
    owner_pubkey: null,
    is_agent: false,
  },
};

test("a provider is a provider, never an unexplained collaborator", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  const text = rowText(PROVIDER);
  assert.match(kindBadge(PROVIDER).textContent, /Provider/);
  assert.match(text, /a computer, not a person/);
  assert.match(text, /codex/);
  // It still says what it may do, in the invite menu's own words.
  assert.match(text, /Can edit and interact/);
});

test("a seated key names the seat it holds", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  assert.match(kindBadge(SEAT).textContent, /builder/i);
  assert.match(rowText(SEAT), /Holds the builder seat in this session/);
});

test("a known agent reads as an agent even with `is_agent: false`", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  // Its profile carries no attestation; the managed ∪ relay baseline does.
  assert.match(kindBadge(AGENT).textContent, /Agent/);
});

test("a key with no evidence stays unidentified, named or not", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  assert.match(kindBadge(STRANGER).textContent, /Unidentified/);
  assert.match(rowText(STRANGER), /No profile and no agent evidence/);
  assert.match(rowText(STRANGER), /Read-only access/);
  // A resolved display name is not evidence of personhood either.
  assert.match(kindBadge(NAMED).textContent, /Unidentified/);
  assert.match(rowText(NAMED), /No agent evidence held for this key/);
});

test("the founder row is the Owner and carries no second kind badge", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  assert.equal(kindBadge(FOUNDER), null);
  assert.match(rowText(FOUNDER), /Full access, including managing members/);
  assert.match(rowText(FOUNDER), /Created this session/);
});

test("identical display names on different keys stay distinguishable", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  assert.notEqual(rowText(FOUNDER), rowText(NAMED));
  assert.match(rowText(NAMED), /6f6f/);
});

test("no kind pops in late: profile-derived badges wait, chain ones do not", async () => {
  await mount();
  // Profiles have not settled.
  assert.equal(kindBadge(AGENT), null);
  assert.equal(kindBadge(STRANGER), null);
  assert.equal(kindBadge(NAMED), null);
  // The chain already settled these; nothing about them can change.
  assert.match(kindBadge(PROVIDER).textContent, /Provider/);
  assert.match(kindBadge(SEAT).textContent, /builder/i);
  // The capability sentence is safe to say immediately — it comes from the
  // grant, not from a profile.
  assert.match(rowText(STRANGER), /Read-only access/);
  assert.doesNotMatch(rowText(STRANGER), /agent evidence/);
});

test("the founder gets management controls; nobody else does", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  assert.ok(
    document.querySelector(
      `[data-testid="coding-session-people-menu-${STRANGER}"]`,
    ),
  );
  assert.ok(
    document.querySelector('[data-testid="coding-session-people-invite"]'),
  );
});

test("a non-founder sees the same roster read-only", async () => {
  await mount({ viewer: STRANGER, profiles: RESOLVED_PROFILES });
  assert.equal(
    document.querySelector(
      `[data-testid="coding-session-people-menu-${PROVIDER}"]`,
    ),
    null,
  );
  assert.equal(
    document.querySelector('[data-testid="coding-session-people-invite"]'),
    null,
  );
  // Read-only does not mean less honest: the kinds are still stated.
  assert.match(kindBadge(PROVIDER).textContent, /Provider/);
});

test("a pending invite still says Inviting…, and claims no kind of its own", async () => {
  await mount({
    foldData: fold({
      accepted: [[PROVIDER, "operator"]],
      pending: [
        { pubkey: STRANGER, role: "operator", eventId: "f".repeat(64) },
      ],
    }),
    profiles: RESOLVED_PROFILES,
  });
  const row = rowText(STRANGER);
  assert.match(row, /Inviting…/);
  assert.match(
    row,
    /Invitation pending — requested collaborator access is not confirmed/,
  );
  assert.doesNotMatch(row, /Can edit and interact/);
});

test("the surface says session access is not project membership", async () => {
  const { container } = await mount({ profiles: RESOLVED_PROFILES });
  const text = document.body.textContent ?? container.textContent ?? "";
  assert.match(text, /nothing here changes project membership/);
  assert.match(text, /never verify\s+that a key is a person/);
});

test("the empty roster still says so", async () => {
  await mount({
    foldData: fold(),
    founderPubkey: null,
    profiles: {},
  });
  assert.match(
    document.body.textContent ?? "",
    /No one has access to this session yet\./,
  );
});

test("the evidence sentence is rendered text, never clamped away", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  // The two rows the browser lane measured at 155/598px and 25/308px.
  assertSentenceUnclamped(PROVIDER);
  assertSentenceUnclamped(SEAT);
  assert.match(
    detail(SEAT).textContent,
    /Can edit and interact · Holds the builder seat in this session/,
  );
  assert.match(detail(PROVIDER).textContent, /a computer, not a person/);
  // The name may still be clipped — the key beside it disambiguates.
  const name = document.querySelector(
    `[data-testid="coding-session-people-row-${NAMED}"] .truncate`,
  );
  assert.ok(name, "the name line keeps its clamp");
});

test("opening the dialog shows the roster, not the agent directory", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  const roster = document.querySelector(
    '[data-testid="coding-session-people-roster"]',
  );
  assert.ok(roster);
  assert.equal(
    document.activeElement,
    roster,
    "focus must land on the roster, not the invite field",
  );
  const search = document.querySelector(
    '[data-testid="coding-session-people-invite-recipient-search"]',
  );
  assert.ok(search, "the invite field is still present and tabbable");
  assert.equal(
    search.getAttribute("aria-expanded"),
    "false",
    "the recipient popover must not be covering the roster",
  );
  assert.equal(
    document.querySelector(
      '[data-testid="coding-session-people-invite-recipient-results"]',
    ),
    null,
  );
});

test("the invite picker opens on People with the filter offered", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  const { act, fireEvent } = await import("@testing-library/react");
  const field = document.querySelector(
    '[data-testid="coding-session-people-invite-recipient-field"]',
  );
  assert.ok(field);
  await act(async () => {
    fireEvent.click(field);
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  const filter = document.querySelector(
    '[data-testid="coding-session-people-invite-recipient-kind-filter"]',
  );
  assert.ok(filter, "the People / Agents / All control must render here");
  const people = document.querySelector(
    '[data-testid="coding-session-people-invite-recipient-kind-people"]',
  );
  assert.ok(people);
  assert.equal(
    people.getAttribute("aria-pressed"),
    "true",
    "session invites open on People",
  );
  // Agents stay one press away — filtering is presentation, not authority.
  assert.ok(
    document.querySelector(
      '[data-testid="coding-session-people-invite-recipient-kind-agents"]',
    ),
  );
});

test("a row grows for its own text instead of being squeezed", async () => {
  await mount({ profiles: RESOLVED_PROFILES });
  for (const pubkey of [FOUNDER, PROVIDER, SEAT, AGENT, STRANGER, NAMED]) {
    const row = document.querySelector(
      `[data-testid="coding-session-people-row-${pubkey}"]`,
    );
    assert.ok(row, `no row for ${pubkey}`);
    // The row inside the scrolling roster column…
    assertCannotShrink(row, `row ${pubkey}`);
    // …and the wrapper holding the wrapped sentence inside the row's own
    // inner column, which is the same hazard one level down.
    assertCannotShrink(detail(pubkey), `sentence wrapper ${pubkey}`);
  }
  // The list itself stays shrinkable on purpose — that is what makes it
  // scroll at `max-h-72` rather than pushing the dialog past the viewport.
  const list = document.querySelector(
    '[data-testid="coding-session-people-roster"] ul',
  );
  assert.ok(list);
  assert.doesNotMatch(list.getAttribute("class") ?? "", NO_SHRINK_CLASS);
  assert.match(list.getAttribute("class") ?? "", /overflow-y-auto/);
});
