import { createHash } from "node:crypto";

import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_PROJECT,
} from "@/shared/constants/kinds";

import { installMockBridge } from "../helpers/bridge";
import { openDashboardTab } from "../helpers/dashboard";
import { waitForAnimations } from "../helpers/animations";

/**
 * The Roles tab says which project's role instructions it will install, and
 * — per `docs/ROLES_USABILITY_SPEC.md` — lets a person with no protocol
 * knowledge answer, per role: what it is for, which agents can take it,
 * which version is available on this computer, and what version running
 * agents are actually reporting.
 *
 * Ledger 85 pre-chose `<checkout>/personas/roles` for the installer and then
 * found the pre-chosen folder unreachable: the Dashboard route names no
 * project, so nothing was ever resolved and the whole path was dead code that
 * every unit test still passed. This spec exists so that cannot happen twice —
 * it drives the surface an operator actually reaches, with more than one
 * project to get wrong.
 */

const IDENTITY = {
  privateKey:
    "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
  pubkey: "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34",
  username: "tyler",
};

const PROJECT_FEATURES = JSON.stringify({ projects: true });
const SNAPSHOTS = "test-results/roles-usability";

/** Kind:30621 is not signature-checked by the client, like the other mock
 * fixtures, so a hand-built head is enough to give this viewer projects. */
function projectHead(dtag: string, name: string): RelayEvent {
  return {
    id: `project-${dtag}`.padEnd(64, "0"),
    pubkey: IDENTITY.pubkey,
    created_at: Math.floor(Date.now() / 1000) - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", dtag],
      ["name", name],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

async function openAgentsTab(page: Page) {
  await page.addInitScript((identity) => {
    window.localStorage.setItem(
      "buzz:e2e-identity-override.v1",
      JSON.stringify(identity),
    );
  }, IDENTITY);
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [projectHead("attic", "Attic"), projectHead("skunkworks", "Skunkworks")],
  );
  await installMockBridge(page, {});
  await page.goto("/");
  await openDashboardTab(page, "agents");
  await expect(page.getByTestId("agents-library-teams")).toBeVisible({
    timeout: 15_000,
  });
}

async function openInstaller(page: Page) {
  await page.getByTestId("new-team-card").click();
  await page.getByTestId("install-crew-roles").click();
  await expect(page.getByTestId("install-crew-roles-dialog")).toBeVisible();
}

test("the resolved project is named on the tab and in the installer, and switching moves both", async ({
  page,
}) => {
  await openAgentsTab(page);

  // More than one project to get wrong, so the choice is on the surface.
  const selector = page.getByTestId("role-packs-project-selector");
  await expect(selector).toBeVisible();
  const firstName = (
    await page.getByTestId("role-packs-project-trigger").textContent()
  )?.trim();
  expect(firstName).toBeTruthy();
  await expect(selector).toContainText("Role packs for:");

  // …and the dialog opens naming the same project, not a folder from nowhere.
  await openInstaller(page);
  await expect(page.getByTestId("install-crew-roles-folder-label")).toHaveText(
    `The project's role packs — ${firstName}`,
  );
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("install-crew-roles-dialog")).toHaveCount(0);

  // Switching re-labels the tab. The menu is non-modal on purpose: Radix's
  // modal variant leaves `pointer-events: none` on the body for a beat after a
  // selection, which swallowed the very next click on this page.
  await page.getByTestId("role-packs-project-trigger").click();
  const options = page.getByRole("menuitem");
  const optionCount = await options.count();
  expect(optionCount).toBeGreaterThan(1);
  const otherName = (await options.nth(optionCount - 1).textContent())?.trim();
  expect(otherName).not.toBe(firstName);
  await options.nth(optionCount - 1).click();
  await expect(page.getByTestId("role-packs-project-trigger")).toHaveText(
    otherName ?? "",
  );

  // …and the installer re-opens on the project that is now named, having
  // scanned that project's checkout rather than the one it replaced.
  await openInstaller(page);
  await expect(page.getByTestId("install-crew-roles-folder-label")).toHaveText(
    `The project's role packs — ${otherName}`,
  );
});

/**
 * Opens the (renamed) Roles tab of the mock `general` project's own screen —
 * `project-tab-packs` keeps its id and route; only its label changed.
 */
async function openRolesTab(page: Page) {
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-packs").click();
}

test("the Roles tab explains what a role is in plain language, and Technical details stays collapsed until opened", async ({
  page,
}) => {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, PROJECT_FEATURES);
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await openRolesTab(page);

  // Tab label reads "Roles"; id/route are unchanged (project-tab-packs).
  await expect(page.getByTestId("project-tab-packs")).toHaveText("Roles");

  await expect(page.getByTestId("roles-subtitle")).toHaveText(
    "Each role is a set of instructions an agent follows in this project. Here you can see what each role is for, which agents can take it, and which version of its instructions is available on this computer and reported by running agents.",
  );

  // The mock fixture's two roles come from two different rungs (lead is
  // "project", reviewer is "shipped"), so the source line names the mix
  // rather than picking one.
  await expect(page.getByTestId("packs-source-sentence")).toHaveText(
    "Roles come from more than one place; see Technical details.",
  );

  const recheck = page.getByTestId("roles-recheck");
  await expect(recheck).toBeVisible();
  await expect(recheck).toHaveText("Check again");

  // Cards: no origin badge or bare version in the header, just name + slug.
  const leadCard = page.getByTestId("role-card-lead");
  await expect(leadCard).toBeVisible();
  await expect(leadCard.getByTestId("role-origin-lead")).toHaveCount(0);
  await expect(leadCard.getByTestId("role-version-lead")).toHaveCount(0);

  const leadAvailable = page.getByTestId("role-available-lead");
  await expect(leadAvailable).toHaveAttribute("data-availability", "available");
  await expect(leadAvailable).toContainText("9f2e1d0c");
  await expect(leadAvailable).toContainText("project's repository");

  const reviewerAvailable = page.getByTestId("role-available-reviewer");
  await expect(reviewerAvailable).toHaveAttribute(
    "data-availability",
    "available",
  );
  await expect(reviewerAvailable).toContainText("built-in defaults");

  // No reports yet.
  const leadReports = page.getByTestId("role-reports-lead");
  await expect(leadReports).toHaveAttribute("data-reports", "none");
  await expect(leadReports).toContainText(
    "No agent has reported running this role yet.",
  );

  // Skills are collapsed behind a native <details>.
  const skills = page.getByTestId("role-skills-lead");
  await expect(skills).toBeVisible();
  await expect(
    page.locator('[data-testid="role-skills-lead"] summary'),
  ).toHaveText("Skills (2)");

  // Technical details is collapsed by default…
  const details = page.getByTestId("roles-technical-details");
  await expect(details).toBeVisible();
  await expect(details).not.toHaveJSProperty("open", true);
  // The view model's sentence is "" here (nothing is uncertain), but the
  // uncertainty line always has a sentence (copy rule): it substitutes
  // ROLES_UNCERTAINTY_NONE and marks itself `data-uncertain="false"`.
  const uncertainty = page.getByTestId("roles-uncertainty-summary");
  await expect(uncertainty).toBeVisible();
  await expect(uncertainty).toHaveAttribute("data-uncertain", "false");
  await expect(uncertainty).toHaveText(
    "Nothing in this view is reported as missing or unconfirmed.",
  );

  // …and opens with the keyboard, natively (no click needed).
  const summary = details.locator("summary").first();
  await summary.focus();
  await page.keyboard.press("Enter");
  await expect(details).toHaveJSProperty("open", true);

  await expect(page.getByTestId("roles-diagnostics-source")).toBeVisible();
  await expect(page.getByTestId("roles-diagnostics-history")).toContainText(
    "Report history (0)",
  );
  await expect(page.getByTestId("roles-diagnostics-checks")).toBeVisible();
});

// ── Distinct report cases ────────────────────────────────────────────────

/** `general` in the mock channel fixture; the `h` tag must match exactly. */
const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";

/**
 * An owner pubkey for the hand-built kind:30621 below. Kind:30621 is not
 * signature-checked by the client (see `projectHead` above), so any 64-hex
 * value fixes this project's address (`30621:<owner>:general`)
 * deterministically for this test.
 */
const RANKED_PROJECT_OWNER = "a1".repeat(32);
const RANKED_PROJECT_ADDRESS = `${KIND_PROJECT}:${RANKED_PROJECT_OWNER}:general`;

/**
 * Publishes a real project under dtag `general` — the same tab the test
 * above opens via `project-open-general`. Unlike the local synthetic
 * placeholder the app shows before any real `general` project exists
 * (`makeLocalGeneral`, whose `channelIds` is always empty), this one
 * declares the mock `general` channel as its own with a `channel` tag, so
 * `useProjectPacksView`'s project-channel filter (`declared.has(channel.id)`
 * in `useProjectPacksView.ts`) admits it and the Roles tab reads that
 * channel's signed 44223 metadata for this project.
 */
function generalProjectWithChannel(): RelayEvent {
  return {
    id: "project-general-ranked".padEnd(64, "0"),
    pubkey: RANKED_PROJECT_OWNER,
    created_at: Math.floor(Date.now() / 1000) - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", "general"],
      ["name", "General"],
      ["channel", GENERAL_CHANNEL_ID],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

/** The mock lead pack's own repo and sha (`handleListProjectRolePacks` in
 * `e2eBridge.ts`) — the fixture's "current" answer for `compare_project_pack_revisions`. */
const RANKED_REPO = `30617:${"c".repeat(64)}:packs`;
const RANKED_CURRENT_SHA = "9f2e1d0c7b6a59483726150e4d3c2b1a0f9e8d7c";
const RANKED_EARLIER_SHA = "aa11bb22cc33dd44ee55ff660011223344556677";

const RANKED_IDLE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "ranked-idle-seat",
  sessionId: "77777777-8888-9999-aaaa-bbbbbbbbbbbb",
  generation: 1,
};
const RANKED_RUNNING_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "ranked-running-seat",
  sessionId: "cccccccc-dddd-eeee-ffff-000000000000",
  generation: 1,
};
/** Names a role but ships no version coordinate at all (case (b) in the
 * spec) — `role` requires an `agentRef`, so this is a seated report, unlike
 * the two ranked targets above (`agentRef: null`). */
const NO_VERSION_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "no-version-seat",
  sessionId: "eeeeeeee-ffff-0000-1111-222222222222",
  generation: 1,
};
/** Names no role and carries no coordinate (case (c)) — never rendered on a
 * card, only counted in Report history and the uncertainty sentence. */
const NO_ROLE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "no-role-seat",
  sessionId: "99999999-aaaa-bbbb-cccc-dddddddddddd",
  generation: 1,
};

const RANKED_CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: true,
  context: false,
  diff: false,
  plan: true,
};

/**
 * A real, signed kind:44223 for the general channel — this is what
 * `useGlobalCodingSessionCatalog`'s "open" authority mode actually trusts
 * (channel membership, read off signature-verified events), not a
 * hand-waved fixture. Each generation is its own fresh keypair, matching how
 * `coding-session-reachability.spec.ts` and `coding-session-seat-bee.spec.ts`
 * seed provider metadata.
 */
function rankedMetadataEvent(input: {
  target: typeof RANKED_IDLE_TARGET;
  status: "idle" | "running";
  sha: string;
  reportedSecondsAgo: number;
  /** Fixed signer, for a scenario that must name a specific provider. */
  secret?: Uint8Array;
  /** The umbrella session this generation's create claimed, when proving
   * provenance needs one echoed back (additive on the wire — omitted keeps
   * every existing caller's content byte-identical). */
  sessionRef?: string;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: Math.floor(Date.now() / 1000) - input.reportedSecondsAgo,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: RANKED_PROJECT_ADDRESS,
        repoRef: null,
        title: `Ranked ${input.status} seat`,
        agentRef: null,
        provider: input.target.driver,
        runtime: input.target.driver,
        model: "sonnet",
        status: input.status,
        branch: null,
        capabilities: RANKED_CAPABILITIES,
        // Key order past `capabilities` is load-bearing: the shared strict
        // metadata decoder (`sessionCoordinationStrictJson.ts`
        // `metadataFieldForms`) only recognizes amendment keys in one fixed
        // sequence — `sessionRef` before `packRef` — so `sessionRef` must be
        // spread in first, or the whole event reads as malformed and the fold
        // never groups it into a session at all.
        ...(input.sessionRef !== undefined
          ? { sessionRef: input.sessionRef }
          : {}),
        packRef: {
          repo: RANKED_REPO,
          sha: input.sha,
          role: "lead",
          path: "personas/roles/lead",
        },
      }),
    },
    input.secret ?? generateSecretKey(),
  ) as unknown as RelayEvent;
}

/**
 * Case (b): names the "lead" role (which requires a seated `agentRef`) but
 * omits `packRef` entirely — a report a card must count as "version not
 * reported", never inventing a coordinate for it.
 */
function noVersionRoleMetadataEvent(): RelayEvent {
  const agentSecret = generateSecretKey();
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: Math.floor(Date.now() / 1000) - 20,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(NO_VERSION_TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(NO_VERSION_TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: NO_VERSION_TARGET,
        projectRef: RANKED_PROJECT_ADDRESS,
        repoRef: null,
        title: "Lead with no version reported",
        agentRef: getPublicKey(agentSecret),
        provider: NO_VERSION_TARGET.driver,
        runtime: NO_VERSION_TARGET.driver,
        model: "sonnet",
        status: "idle",
        branch: null,
        capabilities: RANKED_CAPABILITIES,
        role: "lead",
        // packRef intentionally omitted — see case (b) in the spec.
      }),
    },
    generateSecretKey(),
  ) as unknown as RelayEvent;
}

/** Case (c): no `role` key and no `packRef` at all. */
function noRoleMetadataEvent(): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: Math.floor(Date.now() / 1000) - 10,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(NO_ROLE_TARGET)],
        ["csm-key", codingSessionMetadataSemanticKey(NO_ROLE_TARGET)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: NO_ROLE_TARGET,
        projectRef: RANKED_PROJECT_ADDRESS,
        repoRef: null,
        title: "No role named",
        agentRef: null,
        provider: NO_ROLE_TARGET.driver,
        runtime: NO_ROLE_TARGET.driver,
        model: "sonnet",
        status: "idle",
        branch: null,
        capabilities: RANKED_CAPABILITIES,
        // role and packRef both omitted — see case (c) in the spec.
      }),
    },
    generateSecretKey(),
  ) as unknown as RelayEvent;
}

// ── A commissioned generation's provenance chain ─────────────────────────
//
// Everything above proves only that a signature and a channel are readable —
// the ranked rows above never seed a lifecycle chain, so both stay
// `proof-unavailable`. This block builds the one thing that can read
// `commissioned`: a founder-signed genesis, a founder-signed generation-1
// create naming it, and a provider-signed receipt accepting that create —
// using the same production builders `codingSessionCreateObservations.test.mjs`
// uses for the equivalent unit fixtures, not hand-rolled tags.

const COMMISSIONED_SESSION_REF = "11111111-2222-4333-8444-555555555555";
const COMMISSIONED_SHA = "dd11ee22ff33001122334455667788990011aabb";
const COMMISSIONED_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "commissioned-seat",
  sessionId: "dddddddd-eeee-ffff-0000-111111111111",
  generation: 1,
};

function commissionedGenerationEvents(): {
  founderPubkey: string;
  events: RelayEvent[];
} {
  const founderSecret = generateSecretKey();
  const providerSecret = generateSecretKey();
  const commandId = "csl-commissioned-1";
  const nowSeconds = Math.floor(Date.now() / 1000);

  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: GENERAL_CHANNEL_ID,
    sessionRef: COMMISSIONED_SESSION_REF,
  });
  const genesis = finalizeEvent(
    {
      kind: builtGenesis.kind,
      created_at: nowSeconds - 600,
      tags: builtGenesis.tags,
      content: builtGenesis.content,
    },
    founderSecret,
  ) as unknown as RelayEvent;

  const builtCreate = buildCodingSessionCreateEvent({
    channelId: GENERAL_CHANNEL_ID,
    commandId,
    projectRef: null,
    repoRef: null,
    sessionRef: COMMISSIONED_SESSION_REF,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: getPublicKey(providerSecret),
    model: null,
    title: "Commissioned generation",
    initialTurn: null,
  });
  const create = finalizeEvent(
    {
      kind: builtCreate.kind,
      created_at: nowSeconds - 500,
      tags: builtCreate.tags,
      content: builtCreate.content,
    },
    // Founder-signed: the create is the command a founder-signed chain
    // commissioned, not merely a member's claim about one.
    founderSecret,
  ) as unknown as RelayEvent;

  const receipt = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: nowSeconds - 480,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status: "created",
        session: COMMISSIONED_TARGET,
        error: null,
      }),
    },
    providerSecret,
  ) as unknown as RelayEvent;

  const metadata = rankedMetadataEvent({
    target: COMMISSIONED_TARGET,
    status: "idle",
    sha: COMMISSIONED_SHA,
    reportedSecondsAgo: 60,
    secret: providerSecret,
    sessionRef: COMMISSIONED_SESSION_REF,
  });

  return {
    founderPubkey: getPublicKey(founderSecret),
    events: [genesis, create, receipt, metadata],
  };
}

test("the Roles tab distinguishes a version, no version reported, and no role at all", async ({
  page,
}) => {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, PROJECT_FEATURES);
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [generalProjectWithChannel()],
  );
  await installMockBridge(page, {});
  // Patch the mock's revision-comparison answer after `installMockBridge`'s
  // own init script runs (it overwrites `window.__BUZZ_E2E__.mock` wholesale),
  // so this merge survives rather than being clobbered by it. There is no
  // typed knob for this in `tests/helpers/bridge.ts` — `packRevisionRelations`
  // lives only on `e2eBridge.ts`'s own `E2eConfig`, which reads
  // `window.__BUZZ_E2E__` directly.
  await page.addInitScript(
    (behind) => {
      const testWindow = window as unknown as {
        __BUZZ_E2E__?: { mock?: Record<string, unknown> };
      };
      if (!testWindow.__BUZZ_E2E__) return;
      testWindow.__BUZZ_E2E__.mock = {
        ...(testWindow.__BUZZ_E2E__.mock ?? {}),
        packRevisionRelations: {
          [behind.sha]: { relation: "earlier", behind: behind.behind },
        },
      };
    },
    { sha: RANKED_EARLIER_SHA, behind: 1 },
  );
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await openRolesTab(page);

  const leadCard = page.getByTestId("role-card-lead");
  await expect(leadCard).toBeVisible({ timeout: 15_000 });

  await waitForAnimations(page);
  const pageBuffer = await page.screenshot({
    path: `${SNAPSHOTS}/01-roles-page.png`,
    fullPage: true,
  });

  // Seed the generations only once the tab is open — the channel's live
  // subscription (armed on navigation) must already exist for the seeded
  // events to be delivered rather than dropped.
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of events as never[]) seed({ channelName, event });
    },
    {
      channelName: "general",
      events: [
        rankedMetadataEvent({
          target: RANKED_IDLE_TARGET,
          status: "idle",
          sha: RANKED_CURRENT_SHA,
          reportedSecondsAgo: 300,
        }),
        rankedMetadataEvent({
          target: RANKED_RUNNING_TARGET,
          status: "running",
          sha: RANKED_EARLIER_SHA,
          reportedSecondsAgo: 120,
        }),
        noVersionRoleMetadataEvent(),
        noRoleMetadataEvent(),
      ] as unknown as never[],
    },
  );

  // Case (a) + (b): the lead card's own reports sentence and version lines.
  // 3 lead reports: current (same version as here), earlier (1 behind), and
  // the no-coordinate report (no version reported). The no-role report is
  // never counted here — it names no role at all.
  const leadReports = page.getByTestId("role-reports-lead");
  await expect(leadReports).toHaveAttribute("data-reports", "some", {
    timeout: 15_000,
  });
  await expect(leadReports).toContainText("3 reports");
  await expect(leadReports).toContainText(
    "1 on the same version as this computer",
  );
  await expect(leadReports).toContainText("1 on an earlier version");
  await expect(leadReports).toContainText("1 with no version reported");
  await expect(leadReports).toContainText("same version as here");
  await expect(leadReports).toContainText("earlier, 1 behind");
  await expect(leadReports).toContainText("version not reported");

  // Case (c): the no-role report never spawns a third card.
  await expect(page.locator('[data-testid^="role-card-"]')).toHaveCount(2);

  await waitForAnimations(page);
  const cardBuffer = await leadCard.screenshot({
    path: `${SNAPSHOTS}/02-card-lead.png`,
  });

  // Open Technical details and its nested groups to see Report history and
  // the source coordinates — collapsed by default, but every reported row
  // still exists in the DOM (and is countable) whether or not it is open.
  const details = page.getByTestId("roles-technical-details");
  await details.locator("summary").first().click();
  await expect(details).toHaveJSProperty("open", true);

  const sourceGroup = page.getByTestId("roles-diagnostics-source");
  await sourceGroup.locator("summary").first().click();
  const historyGroup = page.getByTestId("roles-diagnostics-history");
  await historyGroup.locator("summary").first().click();

  const rows = page.getByTestId("role-pack-reported-row");
  await expect(rows).toHaveCount(4, { timeout: 15_000 });

  const currentRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-relation="current"]',
  );
  await expect(currentRow).toHaveCount(1);
  await expect(currentRow).toHaveAttribute(
    "data-provenance",
    "proof-unavailable",
  );

  const earlierRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-relation="earlier"]',
  );
  await expect(earlierRow).toHaveCount(1);
  await expect(earlierRow).toContainText("running");

  const incompleteRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-relation="incomplete"]',
  );
  await expect(incompleteRow).toHaveCount(2); // the no-version-lead and no-role rows

  const checkoutStatus = page.getByTestId("role-pack-checkout-status");
  await expect(checkoutStatus).toContainText("On 9f2e1d0c");
  await expect(checkoutStatus).toContainText("git answered at");

  // The uncertainty summary names the no-role report.
  await expect(page.getByTestId("roles-uncertainty-summary")).toContainText(
    "1 reports name no role",
  );

  await waitForAnimations(page);
  const detailsBuffer = await details.screenshot({
    path: `${SNAPSHOTS}/03-technical-details-open.png`,
  });

  // Now seed a founder-signed genesis + generation-1 create + provider
  // receipt for a third generation, and confirm the mock relay's generic
  // kind+#h history path — the same one `emitMockHistory` already serves
  // 44223 reads through — answers `fetchRolePackProvenanceEvents`'s 44221 /
  // 44224 / 44226 reads too, with no special-casing and no bridge knob.
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of events as never[]) seed({ channelName, event });
    },
    {
      channelName: "general",
      events: commissionedGenerationEvents().events as unknown as never[],
    },
  );

  await expect(rows).toHaveCount(5, { timeout: 15_000 });
  const commissionedRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-provenance="commissioned"]',
  );
  await expect(commissionedRow).toHaveCount(1, { timeout: 15_000 });
  await expect(commissionedRow).toContainText(
    "Reported by the assigned provider",
  );
  // Never worded as verified execution or adoption (constraint 2).
  await expect(commissionedRow).not.toContainText("verified execution");
  await expect(commissionedRow).not.toContainText("verified adoption");

  const digest = (buffer: Buffer) =>
    createHash("sha256").update(buffer).digest("hex");
  const distinctHashes = new Set([
    digest(pageBuffer),
    digest(cardBuffer),
    digest(detailsBuffer),
  ]);
  expect(
    distinctHashes.size,
    "01-roles-page.png, 02-card-lead.png, and 03-technical-details-open.png must be visually distinct",
  ).toBe(3);
});
