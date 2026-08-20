import { createHash } from "node:crypto";

import { hexToBytes } from "@noble/hashes/utils.js";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_PROJECT,
  KIND_PULSE_ENTRY,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

/**
 * Project Pulse, end to end through the mock relay.
 *
 * The entries are really signed and really decoded: the screen fails closed on
 * an unsigned event, so seeding real bytes is the only way to see it paint.
 * The read-only states arrive through `__BUZZ_E2E_EXTRA_PROJECT_EVENTS__`,
 * which stores arbitrary kinds and matches them by `filter.kinds` + `#a` —
 * exactly the filter the Pulse read issues.
 *
 * Session facts take the other path on purpose: 44223/44227/44229/44230 carry
 * no `a` tag, so the Pulse read finds them by `#h` over the project's own
 * channels. They are seeded into a real mock channel with
 * `__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__`, and the project head that names that
 * channel is injected as a 30621 — the same join the product uses.
 *
 * Authors are signed with the bridge's fixture identities, whose display names
 * the mock profile store resolves, because "names, not hashes" is the property
 * under test and a random key would only ever render as a short hash.
 */

const AUTHOR_SECRET = hexToBytes(TEST_IDENTITIES.alice.privateKey);
const PEER_SECRET = hexToBytes(TEST_IDENTITIES.bob.privateKey);
const PROVIDER_SECRET = generateSecretKey();
/** `DEFAULT_MOCK_IDENTITY.pubkey` in the bridge — the project's owner. */
const MOCK_IDENTITY_PUBKEY = "deadbeef".repeat(8);
const PROJECT_DTAG = "pulse-demo";
const QUIET_DTAG = "quiet-demo";
const SESSIONS_DTAG = "sessions-demo";
const PROJECT_COORDINATE = `30621:${MOCK_IDENTITY_PUBKEY}:${PROJECT_DTAG}`;
const SESSIONS_COORDINATE = `30621:${MOCK_IDENTITY_PUBKEY}:${SESSIONS_DTAG}`;
/** `general` in the mock channel fixture; the `h` tag must match exactly. */
const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const GENERAL_CHANNEL_NAME = "general";
const THEME_STORAGE_KEY = "buzz-theme";

const ACTIVE_SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const UNOBSERVED_SESSION_REF = "8c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const LAST_SEEN_SESSION_REF = "1d2e3f4a-5b6c-4d7e-8f90-a1b2c3d4e5f6";

const SCREENSHOT_DIR = "test-results/project-pulse";
const hashes = new Map<string, string>();

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

function pulseEntry(input: {
  secret: Uint8Array;
  createdAtOffset: number;
  type: "plan" | "milestone" | "note" | "handoff" | "blocker";
  text: string;
  branch?: string | null;
  codeAreas?: string[];
  supersedes?: string | null;
  coordinate?: string;
  sessionRef?: string | null;
}): RelayEvent {
  const branch = input.branch ?? null;
  const tags: string[][] = [
    ["a", input.coordinate ?? PROJECT_COORDINATE],
    ["pu-v", "pu1-1"],
    ["pu-type", input.type],
  ];
  if (branch !== null) tags.push(["branch", branch]);
  if (input.sessionRef) tags.push(["pu-session", input.sessionRef]);
  return finalizeEvent(
    {
      kind: KIND_PULSE_ENTRY,
      created_at: nowSeconds() - input.createdAtOffset,
      tags,
      content: JSON.stringify({
        schema: "buzz-pulse-entry/v1",
        type: input.type,
        text: input.text,
        codeAreas: input.codeAreas ?? [],
        branch,
        supersedes: input.supersedes ?? null,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

function seededEntries(): RelayEvent[] {
  const original = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 3_600,
    type: "plan",
    text: "First pass at the wire contract.",
    branch: "wip/project-pulse",
  });
  const revision = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 1_800,
    type: "plan",
    text: "Wire contract landed; starting relay ingest.",
    branch: "wip/project-pulse",
    codeAreas: ["crates/buzz-relay/src/handlers/ingest.rs"],
    supersedes: original.id,
  });
  const blocker = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 900,
    type: "blocker",
    text: "Do not touch pool.rs; the creation path is half-migrated.",
    codeAreas: ["crates/buzz-acp/src/pool.rs"],
  });
  const peerClaim = pulseEntry({
    secret: PEER_SECRET,
    createdAtOffset: 300,
    type: "plan",
    text: "Picking pool.rs back up.",
    supersedes: blocker.id,
  });
  return [original, revision, blocker, peerClaim];
}

/**
 * A project head that names a real mock channel, so the Pulse read has
 * somewhere to look for session facts. Kind 30621 is not signature-checked by
 * the client (the mock fixtures are unsigned too), unlike the 44240 entries
 * and 442xx session facts, which are.
 */
function projectHeadEvent(input: {
  dtag: string;
  name: string;
  channelIds?: string[];
}): RelayEvent {
  return {
    id: `project-${input.dtag}`.padEnd(64, "0"),
    pubkey: MOCK_IDENTITY_PUBKEY,
    created_at: nowSeconds() - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", input.dtag],
      ["name", input.name],
      ["description", "Injected head for the Pulse session read."],
      ...(input.channelIds ?? []).map((id) => ["channel", id]),
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

/** One 44223 observation, with the all-four-or-none fact block kept honest. */
function sessionMetadataEvent(input: {
  sessionRef: string;
  sessionId: string;
  title: string;
  status: string;
  branch: string | null;
  createdAtOffset: number;
  facts?: {
    observedCommit: string | null;
    dirty: boolean | null;
    relayReachable: boolean | null;
    verifiedAt: number | null;
  };
}): RelayEvent {
  const target = {
    driver: "claude-agent-acp",
    instanceId: "pulse-instance",
    sessionId: input.sessionId,
    generation: 1,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: nowSeconds() - input.createdAtOffset,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: SESSIONS_COORDINATE,
        repoRef: null,
        title: input.title,
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: input.status,
        branch: input.branch,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: false,
          diff: true,
          plan: true,
        },
        sessionRef: input.sessionRef,
        ...(input.facts ?? {}),
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function sessionNameEvent(sessionRef: string, name: string): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_NAME,
      created_at: nowSeconds() - 600,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["d", sessionRef],
        ["csnm-v", "csnm1-1"],
      ],
      content: name,
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function sessionGoalEvent(sessionRef: string, goal: string): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_GOAL,
      created_at: nowSeconds() - 600,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["d", sessionRef],
        ["csgl-v", "csgl1-1"],
      ],
      content: goal,
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

function sessionClosureEvent(sessionRef: string): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_CLOSURE,
      created_at: nowSeconds() - 300,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["d", sessionRef],
        ["csc-v", "csc1-1"],
      ],
      content: JSON.stringify({
        action: "closed",
        genesisRef: "a".repeat(64),
        sessionRef,
        v: 1,
      }),
    },
    PROVIDER_SECRET,
  ) as unknown as RelayEvent;
}

/**
 * Three sessions covering the states the card can be in: observed working with
 * a confirmed commit, observed working with nothing observed about its
 * worktree, and one whose last observation is hours old and whose umbrella has
 * been closed.
 */
function seededSessionFacts(): RelayEvent[] {
  return [
    sessionMetadataEvent({
      sessionRef: ACTIVE_SESSION_REF,
      sessionId: "11111111-2222-3333-4444-555555555555",
      title: "Pulse plumbing",
      status: "running",
      branch: "wip/project-pulse",
      createdAtOffset: 120,
      facts: {
        observedCommit: "9f2c1ab34de5f6a7",
        dirty: true,
        relayReachable: true,
        verifiedAt: nowSeconds() - 150,
      },
    }),
    sessionNameEvent(ACTIVE_SESSION_REF, "Pulse plumbing"),
    sessionGoalEvent(ACTIVE_SESSION_REF, "Land the digest fold and its twin."),
    sessionMetadataEvent({
      sessionRef: UNOBSERVED_SESSION_REF,
      sessionId: "22222222-3333-4444-5555-666666666666",
      title: "Relay ingest",
      status: "waiting_for_input",
      branch: null,
      createdAtOffset: 240,
    }),
    sessionNameEvent(UNOBSERVED_SESSION_REF, "Relay ingest"),
    sessionMetadataEvent({
      sessionRef: LAST_SEEN_SESSION_REF,
      sessionId: "33333333-4444-5555-6666-777777777777",
      title: "Conformance corpus",
      status: "disconnected",
      branch: "main",
      createdAtOffset: 10_800,
      facts: {
        observedCommit: "1c0ffee2b3a45678",
        dirty: false,
        relayReachable: false,
        verifiedAt: nowSeconds() - 10_900,
      },
    }),
    sessionNameEvent(LAST_SEEN_SESSION_REF, "Conformance corpus"),
    sessionClosureEvent(LAST_SEEN_SESSION_REF),
  ];
}

async function boot(
  page: Page,
  extras: RelayEvent[],
  options: {
    theme?: string;
    hangKinds?: number[];
    rejectKinds?: number[];
  } = {},
) {
  if (options.theme) {
    // Before the bridge installs: ThemeProvider reads storage on first mount,
    // and the bridge is what triggers that mount.
    await page.addInitScript(
      ({ key, value }) => {
        window.localStorage.setItem(key, value);
      },
      { key: THEME_STORAGE_KEY, value: options.theme },
    );
  }
  await page.addInitScript(
    ({ events, hangKinds, rejectKinds }) => {
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events as never;
      if (hangKinds) {
        window.__BUZZ_E2E_HANG_PROJECT_QUERY_KINDS__ = hangKinds;
      }
      if (rejectKinds) {
        window.__BUZZ_E2E_REJECT_PROJECT_QUERY_KINDS__ = rejectKinds;
      }
    },
    {
      events: extras as never,
      hangKinds: options.hangKinds ?? null,
      rejectKinds: options.rejectKinds ?? null,
    },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
}

/** Seed already-signed session facts into the project's channel store. */
async function seedSessionFacts(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of seeds as never[]) {
        seed({ channelName, event });
      }
    },
    { channelName: GENERAL_CHANNEL_NAME, seeds: events as never },
  );
}

async function createProject(page: Page, name: string) {
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill(name);
  await page.getByTestId("create-project-container-submit").click();
}

async function openProjectScreen(page: Page, dtag: string) {
  const group = page.getByTestId(`project-group-${dtag}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId(`project-open-${dtag}`).click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
}

async function openPulseFromSidebar(page: Page, dtag: string) {
  const group = page.getByTestId(`project-group-${dtag}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.getByTestId("project-pulse-row").click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/pulse$/);
}

function remember(name: string, buffer: Buffer) {
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

async function capture(page: Page, name: string) {
  await waitForAnimations(page);
  remember(
    name,
    await page.screenshot({ path: `${SCREENSHOT_DIR}/${name}.png` }),
  );
}

/** Scoped shot — the only way two states of one screen differ in the pixels. */
async function captureLocator(page: Page, locator: Locator, name: string) {
  await waitForAnimations(page);
  remember(
    name,
    await locator.screenshot({ path: `${SCREENSHOT_DIR}/${name}.png` }),
  );
}

test("the project home card opens a Pulse that keeps every claim honest", async ({
  page,
}) => {
  await boot(page, seededEntries());
  await createProject(page, "Pulse Demo");
  await openProjectScreen(page, PROJECT_DTAG);

  const card = page.getByTestId("project-pulse-card");
  await expect(card).toBeVisible({ timeout: 10_000 });
  await expect(
    card.getByTestId("project-pulse-card-entry").first(),
  ).toBeVisible({ timeout: 10_000 });
  // The card names its authors: two people disagreeing must not read as one
  // person contradicting themselves.
  await expect(card).toContainText("alice");
  await expect(card).toContainText("bob");
  // The blocker leads, whatever the newest-first order says.
  await expect(
    card.getByTestId("project-pulse-card-entry").first(),
  ).toHaveAttribute("data-entry-type", "blocker");
  await capture(page, "01-project-home-card");

  await page.getByTestId("project-screen-open-pulse").click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/pulse$/);
  const screen = page.getByTestId("project-pulse-screen");
  await expect(screen).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("pulse-header-subtitle")).toHaveText(
    "Explicit updates and observed session state.",
  );
  // The screen names the project it describes, dates its own read, and offers
  // a way back to the project home.
  await expect(page.getByTestId("pulse-header-title")).toHaveText(
    "Pulse · Pulse Demo",
  );
  await expect(page.getByTestId("pulse-read-age")).toContainText("read");
  await expect(page.getByTestId("pulse-back")).toBeVisible();

  // The peer's claim is shown as a claim, in plain language, on both rows —
  // and the blocker it names is still in the active set.
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(3, {
    timeout: 10_000,
  });
  await expect(
    page.getByTestId("pulse-entry-supersession-claimed"),
  ).toContainText("bob says this is resolved");
  await expect(page.getByTestId("pulse-entry-unhonored-claim")).toContainText(
    "Do not touch pool.rs",
  );
  await expect(screen).not.toContainText("supersession claimed by");
  await expect(screen).toContainText(
    "Do not touch pool.rs; the creation path is half-migrated.",
  );
  // No 64-char hash anywhere in the body text.
  await expect(screen).not.toHaveText(/[0-9a-f]{64}/);
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  await expect(screen).not.toContainText("Live summaries");
  await expect(screen).not.toContainText("Automatic summary");
  // Nothing is running here, and the screen says so rather than going dark.
  await expect(page.getByTestId("pulse-sessions-empty")).toContainText(
    "No coding sessions observed in this project's channels.",
  );
  await capture(page, "02-pulse-screen");

  // The author's own revision retired its predecessor — disclosed, not deleted,
  // and labelled with who retired it.
  const toggle = page.getByTestId("pulse-superseded-toggle");
  await expect(toggle).toContainText("1 superseded entry");
  await toggle.click();
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(4);
  await expect(screen).toContainText("First pass at the wire contract.");
  await expect(page.getByTestId("pulse-entry-superseded-badge")).toBeVisible();
  await expect(page.getByTestId("pulse-entry-replaced-by")).toContainText(
    "Replaced by alice",
  );
  await capture(page, "03-superseded-disclosed");

  // "no branch" is a real group, not a merge into a named branch.
  await expect(
    page.getByTestId("pulse-branch-chip").filter({ hasText: "no branch" }),
  ).toBeVisible();
});

test("the sidebar row reaches the same Pulse the home card does", async ({
  page,
}) => {
  await boot(page, seededEntries());
  await createProject(page, "Pulse Demo");
  // The row is present whether or not the project has Pulse content, so it is
  // asserted against the group rather than against seeded entries.
  const group = page.getByTestId(`project-group-${PROJECT_DTAG}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  const row = group.getByTestId("project-pulse-row");
  await expect(row).toBeVisible({ timeout: 10_000 });
  // "Project Pulse", not "Pulse": the pinned social feed already owns that word.
  await expect(row).toHaveText("Project Pulse");
  await row.click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/pulse$/);
  await expect(page.getByTestId("project-pulse-screen")).toBeVisible({
    timeout: 10_000,
  });
});

test("a quiet project reads as confirmed empty, never as unavailable", async ({
  page,
}) => {
  await boot(page, []);
  await createProject(page, "Quiet Demo");
  await openProjectScreen(page, QUIET_DTAG);
  await page.getByTestId("project-screen-open-pulse").click();

  const empty = page.getByTestId("pulse-empty");
  await expect(empty).toBeVisible({ timeout: 10_000 });
  await expect(empty).toContainText("This read completed");
  await expect(empty).toContainText("Quiet.");
  // The quiet state teaches the first action and admits the screen is read-only.
  await expect(page.getByTestId("pulse-write-hint")).toContainText(
    "buzz pulse update --project",
  );
  await expect(page.getByTestId("pulse-unavailable")).toHaveCount(0);
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(0);
  await capture(page, "04-confirmed-empty");
});

test("session cards render every observation as what it is", async ({
  page,
}) => {
  await boot(page, [
    projectHeadEvent({
      dtag: SESSIONS_DTAG,
      name: "Sessions Demo",
      channelIds: [GENERAL_CHANNEL_ID],
    }),
    pulseEntry({
      secret: AUTHOR_SECRET,
      createdAtOffset: 600,
      type: "handoff",
      text: "Handing the ingest path over; see the running session.",
      branch: "wip/project-pulse",
      coordinate: SESSIONS_COORDINATE,
      sessionRef: ACTIVE_SESSION_REF,
    }),
  ]);
  await seedSessionFacts(page, seededSessionFacts());
  await openPulseFromSidebar(page, SESSIONS_DTAG);

  const screen = page.getByTestId("project-pulse-screen");
  await expect(screen).toBeVisible({ timeout: 10_000 });
  const activeWork = page.getByTestId("pulse-active-work");
  await expect(activeWork.getByTestId("pulse-session-card")).toHaveCount(2, {
    timeout: 10_000,
  });
  const lastSeen = page.getByTestId("pulse-last-seen");
  await expect(lastSeen.getByTestId("pulse-session-card")).toHaveCount(1);
  await expect(lastSeen).toContainText("Disconnected · last observed 3h ago");
  await expect(lastSeen.getByTestId("pulse-session-closed")).toHaveText(
    "Closed",
  );
  // An entry's `pu-session` tag resolves to the session's name, and still
  // renders at project level rather than inside that session's card.
  const reference = page.getByTestId("pulse-entry-session-reference");
  await expect(reference).toHaveText("references session “Pulse plumbing”");
  expect(
    await activeWork
      .locator("[data-testid='pulse-entry-session-reference']")
      .count(),
  ).toBe(0);
  await captureLocator(page, screen, "05-sessions-active-and-last-seen");

  // The confirmed observation and the tri-state nulls are two different cards.
  const observed = activeWork
    .getByTestId("pulse-session-card")
    .filter({ hasText: "Pulse plumbing" });
  await expect(observed.getByTestId("pulse-session-commit")).toHaveText(
    "9f2c1ab34de5",
  );
  await expect(observed.getByTestId("pulse-session-dirty")).toHaveText(
    "Worktree dirty",
  );
  await expect(
    observed.getByTestId("pulse-session-commit-confirmation"),
  ).toContainText("Commit confirmed on relay ·");
  await captureLocator(page, observed, "06-session-card-observed");

  const unobserved = activeWork
    .getByTestId("pulse-session-card")
    .filter({ hasText: "Relay ingest" });
  await expect(unobserved.getByTestId("pulse-session-commit")).toHaveText(
    "Commit unknown",
  );
  await expect(unobserved.getByTestId("pulse-session-dirty")).toHaveText(
    "Worktree not observed",
  );
  await expect(
    unobserved.getByTestId("pulse-session-commit-confirmation"),
  ).toHaveText("Commit not checked");
  await captureLocator(page, unobserved, "07-session-card-not-observed");
});

test("a branch chip filters the rows, and its count agrees with them", async ({
  page,
}) => {
  await boot(page, [
    projectHeadEvent({
      dtag: SESSIONS_DTAG,
      name: "Sessions Demo",
      channelIds: [GENERAL_CHANNEL_ID],
    }),
    pulseEntry({
      secret: AUTHOR_SECRET,
      createdAtOffset: 600,
      type: "plan",
      text: "Wire contract landed; starting relay ingest.",
      branch: "wip/project-pulse",
      coordinate: SESSIONS_COORDINATE,
    }),
    pulseEntry({
      secret: PEER_SECRET,
      createdAtOffset: 900,
      type: "note",
      text: "Backport notes live on main.",
      branch: "main",
      coordinate: SESSIONS_COORDINATE,
    }),
  ]);
  await seedSessionFacts(page, seededSessionFacts());
  await openPulseFromSidebar(page, SESSIONS_DTAG);

  const screen = page.getByTestId("project-pulse-screen");
  await expect(screen).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("pulse-session-card")).toHaveCount(3, {
    timeout: 10_000,
  });
  const mainChip = page
    .getByTestId("pulse-branch-chip")
    .filter({ hasText: "main" });
  await expect(mainChip).toHaveText("main2");
  await mainChip.click();
  await expect(mainChip).toHaveAttribute("aria-pressed", "true");
  // The count promised two rows: one session and one entry, and no more.
  await expect(page.getByTestId("pulse-session-card")).toHaveCount(1);
  await expect(page.getByTestId("pulse-entry-row")).toHaveCount(1);
  await expect(screen).toContainText("Backport notes live on main.");
  await captureLocator(page, screen, "08-branch-filtered");
});

test("a read still in flight says so, and offers no verdict in the meantime", async ({
  page,
}) => {
  // The entries REQ is never answered, so the screen has no verdict yet — and
  // says exactly that, rather than borrowing the confirmed-empty sentence.
  await boot(page, seededEntries(), { hangKinds: [KIND_PULSE_ENTRY] });
  await createProject(page, "Pulse Demo");
  await openPulseFromSidebar(page, PROJECT_DTAG);
  await expect(page.getByTestId("pulse-loading")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("pulse-loading")).toContainText("Reading…");
  await expect(page.getByTestId("pulse-loading-skeleton")).toBeVisible();
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  await captureLocator(
    page,
    page.getByTestId("project-pulse-screen"),
    "09-loading",
  );
});

test("a head this community cannot read never renders as an empty project", async ({
  page,
}) => {
  await boot(page, seededEntries());
  // Hash routing, and the e2e server is a plain static file server: a deep
  // path 404s, the hash form is a real client-side navigation.
  await page.goto("/#/projects/no-such-project/pulse", {
    waitUntil: "domcontentloaded",
  });
  const unavailable = page.getByTestId("pulse-unavailable");
  await expect(unavailable).toBeVisible({ timeout: 10_000 });
  await expect(unavailable).toContainText("Not readable.");
  await expect(unavailable).toContainText(
    "not a claim that the project is empty",
  );
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  await captureLocator(
    page,
    page.getByTestId("project-pulse-screen"),
    "10-unavailable",
  );
});

test("a source that did not answer renders as a partial read, not a quiet project", async ({
  page,
}) => {
  await boot(page, seededEntries(), { rejectKinds: [KIND_PULSE_ENTRY] });
  await createProject(page, "Pulse Demo");
  await openPulseFromSidebar(page, PROJECT_DTAG);
  const partial = page.getByTestId("pulse-partial");
  await expect(partial).toBeVisible({ timeout: 10_000 });
  await expect(partial).toContainText("Partial read.");
  await expect(partial).toContainText("entries:");
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  await captureLocator(
    page,
    page.getByTestId("project-pulse-screen"),
    "11-partial",
  );
});

test("a complete read that lost an event says so instead of looking exhaustive", async ({
  page,
}) => {
  const dangling = pulseEntry({
    secret: AUTHOR_SECRET,
    createdAtOffset: 300,
    type: "plan",
    text: "Superseding an entry this read cannot see.",
    branch: "wip/project-pulse",
    supersedes: "f".repeat(64),
  });
  await boot(page, [...seededEntries(), dangling]);
  await createProject(page, "Pulse Demo");
  await openPulseFromSidebar(page, PROJECT_DTAG);
  const excluded = page.getByTestId("pulse-excluded");
  await expect(excluded).toBeVisible({ timeout: 10_000 });
  await expect(excluded).toContainText("Some events were excluded.");
  await expect(excluded).toContainText("unresolved-supersedes");
  await expect(page.getByTestId("pulse-empty")).toHaveCount(0);
  // The row keeps the claim visible in words, without printing the hash.
  await expect(
    page
      .getByTestId("pulse-entry-row")
      .filter({ hasText: "Superseding an entry this read cannot see." })
      .getByTestId("pulse-entry-unhonored-claim"),
  ).toContainText("not visible in this read");
  await captureLocator(
    page,
    page.getByTestId("project-pulse-screen"),
    "12-excluded-events",
  );
});

/**
 * Dark mode is where the superseded treatment is most likely to disappear: a
 * 25% dim over a dark card is nearly nothing, which is why the retired row
 * carries a badge and a sentence rather than opacity alone.
 */
test("the Pulse screen, its retired entries and its session cards survive dark mode", async ({
  page,
}) => {
  await boot(
    page,
    [
      projectHeadEvent({
        dtag: SESSIONS_DTAG,
        name: "Sessions Demo",
        channelIds: [GENERAL_CHANNEL_ID],
      }),
      ...(() => {
        const original = pulseEntry({
          secret: AUTHOR_SECRET,
          createdAtOffset: 3_600,
          type: "plan",
          text: "First pass at the wire contract.",
          branch: "wip/project-pulse",
          coordinate: SESSIONS_COORDINATE,
        });
        return [
          original,
          pulseEntry({
            secret: AUTHOR_SECRET,
            createdAtOffset: 1_800,
            type: "plan",
            text: "Wire contract landed; starting relay ingest.",
            branch: "wip/project-pulse",
            coordinate: SESSIONS_COORDINATE,
            supersedes: original.id,
          }),
          pulseEntry({
            secret: PEER_SECRET,
            createdAtOffset: 900,
            type: "blocker",
            text: "Do not touch pool.rs; the creation path is half-migrated.",
            codeAreas: ["crates/buzz-acp/src/pool.rs"],
            coordinate: SESSIONS_COORDINATE,
          }),
        ];
      })(),
    ],
    { theme: "buzz-dark" },
  );
  await seedSessionFacts(page, seededSessionFacts());
  await openPulseFromSidebar(page, SESSIONS_DTAG);

  const screen = page.getByTestId("project-pulse-screen");
  await expect(screen).toBeVisible({ timeout: 10_000 });
  expect(
    await page.evaluate(() =>
      document.documentElement.classList.contains("dark"),
    ),
  ).toBe(true);
  await expect(page.getByTestId("pulse-session-card")).toHaveCount(3, {
    timeout: 10_000,
  });
  await captureLocator(page, screen, "13-dark-pulse-screen");

  const observed = page
    .getByTestId("pulse-session-card")
    .filter({ hasText: "Pulse plumbing" });
  await captureLocator(page, observed, "14-dark-session-card");

  const toggle = page.getByTestId("pulse-superseded-toggle");
  await expect(toggle).toContainText("1 superseded entry");
  await toggle.click();
  const retired = page
    .getByTestId("pulse-entry-row")
    .filter({ hasText: "First pass at the wire contract." });
  await expect(retired).toHaveAttribute("data-entry-active", "false");
  await expect(
    retired.getByTestId("pulse-entry-superseded-badge"),
  ).toBeVisible();
  await captureLocator(page, retired, "15-dark-superseded-entry");
});
