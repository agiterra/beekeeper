import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";

import { expect, type Locator, type Page } from "@playwright/test";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { installMockBridge, TEST_IDENTITIES } from "../../helpers/bridge";
import { E2E_IDENTITY_OVERRIDE_STORAGE_KEY } from "../../helpers/onboarding";

/**
 * Fixtures and assertions for the People/Agents setup slice (Lane V).
 *
 * Everything here drives the **real** components through the mock Tauri
 * bridge and the mock relay: the roster is a genuine NIP-CSAT authority chain
 * (signed kind:44228 links, each with a relay-signed kind:40099 receipt), and
 * the recipient picker reads the same `search_users` path the app uses in
 * production. No handler inside `src/testing/e2eBridge.ts` is touched — every
 * fixture below rides seams that file already exposes (`searchProfiles`,
 * `relayAgents`, `relaySelf`, `__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__`).
 *
 * The one rule every assertion here enforces: **there is no positive evidence
 * that a key is a person.** `isAgent: false` means "this profile carried no
 * attestation we verified", and people, un-attested agents, and keys with no
 * profile at all collapse into it. So no surface may claim personhood, and a
 * key with no resolved profile must be offered as *unidentified*.
 */

// ---------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------

/**
 * The signed-in viewer *and* the session founder.
 *
 * Both, deliberately: the invite picker only renders for the session owner
 * (`CodingSessionPeoplePopover`'s `isOwner`), and the genesis signature is
 * verified for real, so the founder must be a keypair the bridge can sign as.
 */
const FOUNDER_SECRET = hexToBytes(TEST_IDENTITIES.tyler.privateKey);
export const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);

/** Fixed, not generated: screenshots and short keys must be reproducible. */
const PROVIDER_SECRET = hexToBytes("11".repeat(32));
const RELAY_SECRET = hexToBytes("22".repeat(32));
/** The provider authority key a hire grants `grant-operator` to. */
export const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);
export const RELAY_PUBKEY = getPublicKey(RELAY_SECRET);

/**
 * A display-only fixture key: 8 hex of prefix, 4 of suffix, a constant middle.
 *
 * `truncatePubkey` shows exactly the first 8 and last 4 characters, so this
 * makes every fixture row's short key visibly distinct — which is the whole
 * point of the identical-names scenario.
 */
function fixtureKey(prefix: string, suffix: string): string {
  return `${prefix}${"7".repeat(52)}${suffix}`;
}

/** Seated on the crew with a receipt-backed `grant-seat`. */
export const SEATED_AGENT_PUBKEY = fixtureKey("5ea70a9e", "b17d");
/** Granted, and nothing in this app resolves it to anything. */
export const UNRESOLVED_GRANTEE_PUBKEY = fixtureKey("0f1e2d3c", "9a8b");
/** A profiled collaborator with no agent evidence of any kind. */
export const PRIYA_PUBKEY = fixtureKey("917a0000", "1111");
/** Two different keys, one display name. */
export const SAM_ONE_PUBKEY = fixtureKey("5a11c0de", "0001");
export const SAM_TWO_PUBKEY = fixtureKey("5a12face", "0002");
/** The one person the agent-heavy first page hides. */
export const ZOE_PUBKEY = fixtureKey("20e01234", "beef");
/** An agent the relay registry knows whose owner nobody attested. */
export const NOMAD_AGENT_PUBKEY = fixtureKey("40ad5555", "cafe");
/** Seeded with a profile that resolves to no name at all. */
export const NAMELESS_PUBKEY = fixtureKey("dead1234", "5678");
/** Never seeded anywhere — only reachable by typing the whole key. */
export const DIRECT_ENTRY_PUBKEY = fixtureKey("beefcafe", "4321");

/** 52 look-alike agents, exactly the wall Brian hit. */
export const BUILDER_AGENT_PUBKEYS = Array.from({ length: 52 }, (_, index) =>
  fixtureKey(
    `b0${index.toString(16).padStart(2, "0")}a1c3`,
    index.toString(16).padStart(4, "0"),
  ),
);

function builderName(index: number): string {
  return `Beekeeper Builder ${(index + 1).toString().padStart(2, "0")}`;
}

/** The search text that isolates this fixture's population from the mock's. */
export const FIXTURE_QUERY = "beekeeper";

export const SAM_NAME = "Beekeeper Sam Rivera";
export const ZOE_NAME = "Beekeeper Zoe Winters";
export const NOMAD_NAME = "Beekeeper Nomad";
export const SEATED_AGENT_NAME = "Nova";
export const PRIYA_NAME = "Priya Raman";

export function shortKey(pubkey: string): string {
  return truncatePubkey(pubkey);
}

// ---------------------------------------------------------------------------
// Session fixture
// ---------------------------------------------------------------------------

export const CHANNEL_NAME = "engineering";
export const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-people-setup-session";
const BASE_CREATED_AT = 1_800_000_000;
const SESSION_TITLE = "Make people and agents legible";
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function signed(
  kind: number,
  createdAt: number,
  tags: string[][],
  content: string,
  secret: Uint8Array,
): RelayEvent {
  return finalizeEvent(
    { kind, created_at: createdAt, tags, content },
    secret,
  ) as unknown as RelayEvent;
}

/** One accepted chain link: the transition, and the receipt that accepted it. */
function link(input: {
  genesisRef: string;
  prevAccepted: string | null;
  seq: number;
  type: string;
  granteePubkey: string;
  role?: string;
  createdAt: number;
}): { transition: RelayEvent; receipt: RelayEvent } {
  const payload = {
    genesisRef: input.genesisRef,
    prevAccepted: input.prevAccepted,
    seq: input.seq,
    type: input.type,
    granteePubkey: input.granteePubkey,
    ...(input.role ? { role: input.role } : {}),
  };
  const transition = signed(
    KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    input.createdAt,
    [
      ["h", CHANNEL_ID],
      ["csat-v", "csat1-1"],
      ["csat-genesis", input.genesisRef],
    ],
    JSON.stringify(payload),
    FOUNDER_SECRET,
  );
  const receipt = signed(
    40099,
    input.createdAt + 1,
    [["h", CHANNEL_ID]],
    JSON.stringify({
      type: "coding_session_authority_transition_accepted",
      genesisRef: input.genesisRef,
      acceptedEventId: transition.id,
      seq: input.seq,
      transitionType: input.type,
      granteePubkey: input.granteePubkey,
      ...(input.role ? { role: input.role } : {}),
    }),
    RELAY_SECRET,
  );
  return { transition, receipt };
}

/**
 * The roster Brian actually saw, rebuilt from real events:
 *
 * - the founder (this viewer) as Owner,
 * - the **provider authority key** holding `grant-operator` — the hire path
 *   grants it to the provider itself, and the provider publishes no kind:0,
 *   so today it renders as a truncated hex "Collaborator",
 * - a seated agent holding both an operator grant and a `grant-seat`,
 * - a granted key nothing resolves,
 * - a profiled collaborator with no agent evidence.
 */
function sessionEvents(): RelayEvent[] {
  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  const genesis = signed(
    builtGenesis.kind,
    BASE_CREATED_AT - 2,
    builtGenesis.tags,
    builtGenesis.content,
    FOUNDER_SECRET,
  );
  const builtCreate = buildCodingSessionCreateEvent({
    channelId: CHANNEL_ID,
    commandId: COMMAND_ID,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: SESSION_TITLE,
    initialTurn: null,
  });
  const grants = [
    { type: "grant-operator", granteePubkey: PROVIDER_PUBKEY },
    { type: "grant-operator", granteePubkey: SEATED_AGENT_PUBKEY },
    { type: "grant-seat", granteePubkey: SEATED_AGENT_PUBKEY, role: "builder" },
    { type: "grant-viewer", granteePubkey: UNRESOLVED_GRANTEE_PUBKEY },
    { type: "grant-operator", granteePubkey: PRIYA_PUBKEY },
  ];
  const chain: RelayEvent[] = [];
  let prevAccepted: string | null = null;
  grants.forEach((grant, index) => {
    const built = link({
      genesisRef: genesis.id,
      prevAccepted,
      seq: index + 1,
      type: grant.type,
      granteePubkey: grant.granteePubkey,
      ...(grant.role ? { role: grant.role } : {}),
      createdAt: BASE_CREATED_AT + 10 + index * 4,
    });
    prevAccepted = built.transition.id;
    chain.push(built.transition, built.receipt);
  });
  return [
    genesis,
    signed(
      builtCreate.kind,
      BASE_CREATED_AT - 1,
      builtCreate.tags,
      builtCreate.content,
      FOUNDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", COMMAND_ID],
        ["csl-key", lifecycleReceiptSemanticKey(COMMAND_ID)],
      ],
      JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId: COMMAND_ID,
        status: "created",
        session: TARGET,
        error: null,
      }),
      PROVIDER_SECRET,
    ),
    signed(
      KIND_CODING_SESSION_METADATA,
      BASE_CREATED_AT,
      [
        ["h", CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      JSON.stringify({
        schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: SESSION_TITLE,
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: "running",
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: false,
          plan: true,
        },
        sessionRef: SESSION_REF,
      }),
      PROVIDER_SECRET,
    ),
    ...chain,
  ];
}

/**
 * The searchable directory: 52 look-alike agents ahead of the people, plus
 * the adversarial edges (identical names, an un-attested registry agent, a
 * profile that resolves to no name).
 */
function searchProfiles(mixedFirstPage = false) {
  return [
    { pubkey: FOUNDER_PUBKEY, displayName: "Tyler" },
    ...BUILDER_AGENT_PUBKEYS.map((pubkey, index) => ({
      pubkey,
      displayName: builderName(index),
      isAgent: true,
      ownerPubkey: FOUNDER_PUBKEY,
    })),
    // Owner unknown and the profile carries no attestation — only the relay
    // registry knows this is an agent. It must still be reachable.
    { pubkey: NOMAD_AGENT_PUBKEY, displayName: NOMAD_NAME, isAgent: false },
    { pubkey: SAM_ONE_PUBKEY, displayName: SAM_NAME },
    { pubkey: SAM_TWO_PUBKEY, displayName: SAM_NAME },
    {
      pubkey: ZOE_PUBKEY,
      // Sort after the first 49 builders: page one contains 49 agents and
      // one person, leaving a nonempty People list too short to scroll.
      displayName: mixedFirstPage ? "Beekeeper Builder 49 person" : ZOE_NAME,
    },
    { pubkey: NAMELESS_PUBKEY, displayName: null },
    { pubkey: PRIYA_PUBKEY, displayName: PRIYA_NAME },
    {
      pubkey: SEATED_AGENT_PUBKEY,
      displayName: SEATED_AGENT_NAME,
      isAgent: true,
    },
  ];
}

/** Open the channel, seed the session, and land on its workspace. */
export async function openPeopleSetupSession(
  page: Page,
  options: {
    viewport?: { width: number; height: number };
    textScale?: number;
    mixedFirstPage?: boolean;
  } = {},
): Promise<void> {
  await page.addInitScript(
    ({ storageKey, identity, scale, scaleKey }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(identity));
      if (scale !== undefined)
        window.localStorage.setItem(scaleKey, String(scale));
    },
    {
      storageKey: E2E_IDENTITY_OVERRIDE_STORAGE_KEY,
      scaleKey: "buzz:text-scale",
      scale: options.textScale,
      identity: {
        privateKey: bytesToHex(FOUNDER_SECRET),
        pubkey: FOUNDER_PUBKEY,
        username: "tyler",
      },
    },
  );
  await installMockBridge(page, {
    relaySelf: RELAY_PUBKEY,
    searchProfiles: searchProfiles(options.mixedFirstPage),
    // The registry fact that makes the un-attested agent an agent.
    relayAgents: [
      { pubkey: NOMAD_AGENT_PUBKEY, name: NOMAD_NAME, agentType: "goose" },
      {
        pubkey: SEATED_AGENT_PUBKEY,
        name: SEATED_AGENT_NAME,
        agentType: "goose",
      },
    ],
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    codingSessionProviderStatus: {
      provisioned: true,
      running: true,
      providerPubkey: PROVIDER_PUBKEY,
      instanceId: "0123456789abcdef",
    },
  });
  // 1280 keeps the channel rail on screen for the seeding clicks; the caller's
  // narrower viewport is applied once the session is open.
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: sessionEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").first().click();
  await expect(page.getByTestId("coding-session-workspace")).toBeVisible({
    timeout: 15_000,
  });
  if (options.viewport) await page.setViewportSize(options.viewport);
}

export function peopleDialog(page: Page): Locator {
  // The Details People row opens the People surface where it is available
  // (Wave B, SV-24) and the dialog only where it is not.
  return page
    .getByTestId("coding-session-surface-panel-people")
    .or(page.getByTestId("coding-session-people"));
}

/** Open the session People surface and wait for its roster to settle. */
export async function openPeopleDialog(page: Page): Promise<Locator> {
  // People is a row inside the header's Details popover; it opens the People
  // surface, or the People dialog where the surface is unavailable.
  await page.getByTestId("coding-session-provenance-toggle").click();
  await page.getByTestId("coding-session-people-toggle").click();
  const dialog = peopleDialog(page);
  await expect(dialog).toBeVisible();
  await expect(
    dialog.getByTestId(`coding-session-people-row-${FOUNDER_PUBKEY}`),
  ).toBeVisible({ timeout: 15_000 });
  return dialog;
}

export function rosterRow(page: Page, pubkey: string): Locator {
  return page.getByTestId(`coding-session-people-row-${pubkey}`);
}

// ---------------------------------------------------------------------------
// Recipient picker
// ---------------------------------------------------------------------------

const INVITE_PREFIX = "coding-session-people-invite";

export function recipientSearch(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-search`);
}

/**
 * The results popover.
 *
 * Radix portals `PopoverContent` to the document body, so nothing below may be
 * scoped to the dialog — the picker's markup is a *sibling* of it in the DOM,
 * not a descendant. Scoping to the dialog was the first thing this helper got
 * wrong; the testids are globally unique, so scope to the page.
 */
export function recipientPopover(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-popover`);
}

export function recipientResults(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-results`);
}

export function recipientEmpty(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-empty`);
}

export function recipientOption(page: Page, pubkey: string): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-option-${pubkey}`);
}

export function recipientOptionMeta(page: Page, pubkey: string): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-option-meta-${pubkey}`);
}

export function recipientKindNote(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-kind-note`);
}

export function recipientKindGroup(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-kind-filter`);
}

export type RecipientKind = "people" | "agents" | "all";

/** Lane A's segmented control: `<prefix>-recipient-kind-<kind>`. */
export function recipientKindFilter(page: Page, kind: RecipientKind): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-kind-${kind}`);
}

/** The one bounded page-advance press. One page per press, never a loop. */
export function loadMoreResults(page: Page): Locator {
  return page.getByTestId(`${INVITE_PREFIX}-recipient-load-more`);
}

/** The exhausted-view strings, pinned by Lane A's own table test. */
export const EXHAUSTED_MESSAGE: Record<RecipientKind, string> = {
  people: "No people found.",
  agents: "No agents found.",
  all: "No matches found.",
};

/** The note that keeps the People tab honest. */
export const PEOPLE_KIND_NOTE =
  "Keys we hold no agent evidence for. Absence of evidence, not proof of identity.";

/** Type a query and wait for the deferred ranking to settle on it. */
export async function searchRecipients(
  page: Page,
  query: string,
): Promise<void> {
  await recipientSearch(page).fill(query);
  await expect(
    recipientResults(page).getByRole("status", { name: "Loading people" }),
  ).toHaveCount(0, { timeout: 15_000 });
}

/** Open the picker without typing anything. */
export async function openRecipientPicker(page: Page): Promise<void> {
  await recipientSearch(page).click();
  await expect(recipientPopover(page)).toBeVisible();
}

// ---------------------------------------------------------------------------
// Roster rows
// ---------------------------------------------------------------------------

export function rosterKind(page: Page, pubkey: string): Locator {
  return page.getByTestId(`coding-session-people-kind-${pubkey}`);
}

export function rosterDetail(page: Page, pubkey: string): Locator {
  return page.getByTestId(`coding-session-people-detail-${pubkey}`);
}

/**
 * Every capability sentence lives in the row detail line. `ENTITY_ROLE_
 * DESCRIPTIONS` are the three sentences the invite role menu already used;
 * a row that carries none of them is a row that never says what it may do.
 */
export const CAPABILITY_SENTENCES = [
  "Full access, including managing members",
  "Can edit and interact",
  "Read-only access",
];

// ---------------------------------------------------------------------------
// The vocabulary rule
// ---------------------------------------------------------------------------

/**
 * Phrases that would assert personhood. None of them can be true: nothing in
 * this app positively establishes that a key belongs to a person.
 */
export const PERSONHOOD_CLAIMS: RegExp[] = [
  /\bhumans?\b/i,
  /\bverified (person|human)\b/i,
  /\b(person|human) (confirmed|verified)\b/i,
  /\bconfirmed (person|human)\b/i,
  /\breal (person|human)\b/i,
  /\bproven (person|human)\b/i,
  /\bnot an agent\b/i,
  /\bnot a bot\b/i,
];

/** Wording that discloses what the People view actually means (§0). */
const AGENT_EVIDENCE_DISCLOSURE =
  /(agent evidence|no evidence|no sign|nothing here proves|we hold no|not known to be an agent)/i;

export async function expectNoPersonhoodClaim(scope: Locator): Promise<string> {
  const text = (await scope.innerText()).replace(/\s+/g, " ");
  for (const claim of PERSONHOOD_CLAIMS) {
    expect(
      claim.test(text),
      `surface claims personhood (${claim}) in: ${text.slice(0, 600)}`,
    ).toBe(false);
  }
  return text;
}

export function expectAgentEvidenceDisclosure(text: string): void {
  expect(
    AGENT_EVIDENCE_DISCLOSURE.test(text),
    `People view never states that it means "no agent evidence we hold". Accepted phrasings match ${AGENT_EVIDENCE_DISCLOSURE}. Saw: ${text.slice(0, 600)}`,
  ).toBe(true);
}

export function expectUnidentified(text: string, where: string): void {
  expect(
    /unidentified/i.test(text),
    `${where} must be presented as unidentified. Saw: ${text.slice(0, 400)}`,
  ).toBe(true);
}

// ---------------------------------------------------------------------------
// "Navigation mutates nothing"
// ---------------------------------------------------------------------------

/**
 * Reading commands are allowed while browsing People. Everything else is not.
 *
 * The allowlist is a set of *prefixes* plus named exceptions, and the spec
 * prints the observed delta on every run so a new command shows up as a
 * printed line, not a silent pass.
 */
export const READ_COMMAND_PREFIXES = [
  "get_",
  "list_",
  "query_",
  "search_",
  "read_",
  "resolve_",
  "load_",
  "fetch_",
  "is_",
  "has_",
  "check_",
  "relay_",
  "count_",
];

/**
 * Non-prefixed commands a read-only browse legitimately issues. Each one is
 * here because it was observed and justified — none writes app state.
 */
export const READ_COMMAND_EXCEPTIONS = new Set<string>([
  "relay_self",
  "plugin:event|listen",
  "plugin:event|unlisten",
  "plugin:window|is_maximized",
  "plugin:window|is_focused",
  "plugin:window|theme",
  "plugin:window|scale_factor",
  "plugin:webview|set_webview_zoom",
  "plugin:os|platform",
  "create_auth_event",
  // A read: the session's full-access grant on this computer (its setter is
  // `set_coding_session_full_access`). The query starts once the session record
  // names its provider and session id; with ingress publishes batched per frame
  // that first read can land just after the baseline is taken.
  "coding_session_full_access",
  // A pure computation: folds observation events already held in memory into
  // the session's view model, writing nothing (`fold_adapter` in
  // `coding_session_observation_fold.rs`). Since Wave B (SV-24) People opens
  // as a surface beside the live transcript instead of a modal over it, so the
  // session keeps folding while People is browsed.
  "fold_coding_session_observations_command",
  // Transport, not app state. A publish would still need `sign_event`, which
  // this allowlist denies, so an EVENT frame cannot slip through here.
  "plugin:websocket|connect",
  "plugin:websocket|send",
  "update_tray_agent_activity",
  "clear_tray_agent_activity",
  "take_tray_actions",
  "requeue_tray_actions",
]);

/** Commands that would change something. None may fire from browsing. */
export const MUTATING_COMMAND_PATTERNS = [
  /sign_event/,
  /publish/,
  /grant/,
  /revoke/,
  /^start_/,
  /^stop_/,
  /^create_/,
  /^delete_/,
  /^update_/,
  /^add_/,
  /^remove_/,
  /^send_/,
  /^set_/,
  /^apply_/,
  /^install_/,
];

export function isReadOnlyCommand(command: string): boolean {
  if (READ_COMMAND_EXCEPTIONS.has(command)) return true;
  if (MUTATING_COMMAND_PATTERNS.some((pattern) => pattern.test(command))) {
    return false;
  }
  return READ_COMMAND_PREFIXES.some((prefix) => command.startsWith(prefix));
}

export async function commandCount(page: Page): Promise<number> {
  return page.evaluate(() => window.__BEEKEEPER_E2E_COMMANDS__?.length ?? 0);
}

export async function commandsSince(
  page: Page,
  from: number,
): Promise<string[]> {
  return page.evaluate(
    (start) => (window.__BEEKEEPER_E2E_COMMANDS__ ?? []).slice(start),
    from,
  );
}

/** Every kind:44228 the app asked its keyring to sign, newest last. */
export async function signedAuthorityTransitions(
  page: Page,
): Promise<Array<{ granteePubkey: string; type: string }>> {
  return page.evaluate((kind) => {
    const events = window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? [];
    return events
      .filter((event) => event.kind === kind)
      .map((event) => {
        const parsed = JSON.parse(event.content) as {
          granteePubkey: string;
          type: string;
        };
        return { granteePubkey: parsed.granteePubkey, type: parsed.type };
      });
  }, KIND_CODING_SESSION_AUTHORITY_TRANSITION);
}

/**
 * Print the allowlist and assert the observed delta against it.
 *
 * Printed on every run on purpose: an allowlist nobody reads is a rubber
 * stamp, and the point of this check is that a reviewer can see exactly what
 * browsing People cost.
 */
export function assertReadOnly(observed: string[], phase: string): void {
  const unique = [...new Set(observed)].sort();
  console.log(
    `[people-setup] ${phase}: ${observed.length} commands, ${unique.length} distinct`,
  );
  console.log(
    `[people-setup] ${phase} allowlist prefixes: ${READ_COMMAND_PREFIXES.join(" ")}`,
  );
  console.log(
    `[people-setup] ${phase} allowlist exceptions: ${[...READ_COMMAND_EXCEPTIONS].sort().join(" ") || "(none)"}`,
  );
  console.log(
    `[people-setup] ${phase} observed: ${unique.join(" ") || "(none)"}`,
  );
  const violations = unique.filter((command) => !isReadOnlyCommand(command));
  expect(
    violations,
    `${phase} issued commands outside the read-only allowlist: ${violations.join(", ")}`,
  ).toEqual([]);
}

// ---------------------------------------------------------------------------
// Screenshots
// ---------------------------------------------------------------------------

export const SHOTS = "test-results/people-setup";

export async function capture(
  target: Locator | Page,
  name: string,
): Promise<void> {
  await target.screenshot({ path: `${SHOTS}/${name}.png` });
}

/**
 * Two shots with the same bytes captured the same state — one of them proves
 * nothing. Gated in-spec so a duplicate can never reach a report.
 *
 * Reads the directory rather than an in-process list on purpose: Playwright
 * recycles the worker process after a failing test, so module-level state does
 * not survive a red run — and a gate that quietly forgets what it was meant to
 * check is worse than no gate. The directory is also exactly what gets copied
 * into the report, so this hashes the shipped artefact.
 */
export function assertScreenshotsDistinct(expected = 5): void {
  const files = readdirSync(SHOTS)
    .filter((file) => file.endsWith(".png"))
    .sort();
  const hashes = files.map((file) => ({
    path: `${SHOTS}/${file}`,
    sha256: createHash("sha256")
      .update(readFileSync(`${SHOTS}/${file}`))
      .digest("hex"),
  }));
  for (const { path, sha256 } of hashes) {
    console.log(`[people-setup] ${sha256}  ${path}`);
  }
  const seen = new Map<string, string>();
  for (const { path, sha256 } of hashes) {
    const previous = seen.get(sha256);
    expect(
      previous,
      `${path} is byte-identical to ${previous} — the spec captured the same state twice`,
    ).toBeUndefined();
    seen.set(sha256, path);
  }
  expect(
    hashes.length,
    `only ${hashes.length} screenshots were captured: ${files.join(", ")}`,
  ).toBeGreaterThanOrEqual(expected);
}
