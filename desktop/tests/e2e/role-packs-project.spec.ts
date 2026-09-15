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
import { openSavedAgentGroups } from "../helpers/agentDirectory";

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
/** The design pass's own screenshot matrix — kept separate from the
 * usability slice's above so a reviewer can diff either generation without
 * one overwriting the other. */
const DESIGN_SNAPSHOTS = "test-results/roles-design";

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
  await openSavedAgentGroups(page);
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
  // Exercise an existing project, independent of background General migration.
  await page.addInitScript((event) => {
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [event];
  }, generalProjectWithChannel());
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await openRolesTab(page);

  // Tab label reads "Roles"; id/route are unchanged (project-tab-packs).
  await expect(page.getByTestId("project-tab-packs")).toHaveText("Roles");

  // The design pass's one-liner (`rolesCopy.ts` `ROLES_SUBTITLE`) — the
  // longer explanation it replaced (`ROLES_EXPLANATION`) is still true and
  // still kept, just moved into Technical details (asserted below).
  await expect(page.getByTestId("roles-subtitle")).toHaveText(
    "What each role is for, its project participants, and the versions available here or reported by agents.",
  );

  // The mock fixture's two roles come from two different rungs (lead is
  // "project", reviewer is "shipped"), so the source line names the mix
  // rather than picking one.
  await expect(page.getByTestId("packs-source-sentence")).toHaveText(
    "Roles come from more than one place; see Technical details.",
  );

  const recheck = page.getByTestId("roles-recheck");
  await expect(recheck).toBeVisible();
  // Initial roster discovery shares the paced relay read budget with startup.
  await expect(recheck).toHaveText("Check again", { timeout: 15_000 });

  // ── Summary strip: 2 roles, 0 agents/sessions/reports — this fixture
  // seeds no managed agent and no coding-session event for this project. ──
  await expect(page.getByTestId("roles-summary")).toBeVisible();
  const rolesTile = page.getByTestId("roles-summary-roles");
  await expect(rolesTile).toContainText("2");
  await expect(rolesTile).toContainText("roles");
  const agentsTile = page.getByTestId("roles-summary-agents");
  await expect(agentsTile).toContainText("0");
  await expect(agentsTile).toContainText("agents");
  const sessionsTile = page.getByTestId("roles-summary-sessions");
  await expect(sessionsTile).toContainText("0");
  await expect(sessionsTile).toContainText("open sessions");
  const reportsTile = page.getByTestId("roles-summary-reports");
  await expect(reportsTile).toContainText("0");
  await expect(reportsTile).toContainText("reports");
  await expect(reportsTile).not.toContainText("unconfirmed");
  await expect(reportsTile).not.toContainText("disputed");

  // The one scope label — reserved for real project-team data root is
  // still building; today it discloses this is a this-computer-only view.
  const scope = page.getByTestId("roles-scope");
  await expect(scope).toHaveText("0 on this computer");
  await expect(scope).toHaveAttribute(
    "title",
    "Counted agents are this project's agents on this computer. Others seen in its sessions are not counted. The Agents tab lists every project agent, including ones on other computers.",
  );

  // Cards: no origin badge or bare version in the header, just name + slug.
  const leadCard = page.getByTestId("role-card-lead");
  await expect(leadCard).toBeVisible();
  await expect(leadCard.getByTestId("role-origin-lead")).toHaveCount(0);
  await expect(leadCard.getByTestId("role-version-lead")).toHaveCount(0);

  // Activity dot: neither role has an open session, so both read "none".
  const leadActivity = page.getByTestId("role-activity-lead");
  await expect(leadActivity).toHaveAttribute("data-activity", "none");
  await expect(leadActivity).toHaveAttribute("aria-label");

  // Version chip: the short face carries the version/sha; the full sentence
  // ("… from the project's repository" / "… built-in defaults") moves to
  // the tooltip.
  const leadAvailable = page.getByTestId("role-available-lead");
  await expect(leadAvailable).toHaveAttribute("data-availability", "available");
  await expect(leadAvailable).toContainText("v1.3.0");
  await expect(leadAvailable).toContainText("9f2e1d0c");
  await expect(leadAvailable).toHaveAttribute("title", /project's repository/);

  const reviewerAvailable = page.getByTestId("role-available-reviewer");
  await expect(reviewerAvailable).toHaveAttribute(
    "data-availability",
    "available",
  );
  await expect(reviewerAvailable).toHaveAttribute("title", /built-in defaults/);

  // Description carries its full text in `title` even though the face is
  // clamped to two lines.
  await expect(leadCard.getByTestId("role-description-lead")).toHaveAttribute(
    "title",
    "Triages findings, briefs lanes, rules on landings.",
  );

  // No reports yet — the per-section empty sentence is folded into the
  // single "quiet" line below (agents, reports and seats are all empty).
  const leadReports = page.getByTestId("role-reports-lead");
  await expect(leadReports).toHaveAttribute("data-reports", "none");

  // Every section empty on both cards → one line each, not three repeated
  // absences.
  await expect(page.getByTestId("role-quiet-lead")).toHaveText(
    "No agents, sessions or reports observed for this role.",
  );
  await expect(page.getByTestId("role-quiet-reviewer")).toHaveText(
    "No agents, sessions or reports observed for this role.",
  );

  // "About this role" holds the summary paragraph and the skills, collapsed
  // behind one native <details> at the card foot.
  const about = page.getByTestId("role-about-lead");
  await expect(about).toBeAttached();
  await expect(about.locator("summary").first()).toHaveText(
    "About this role · Skills (2)",
  );
  await about.locator("summary").first().click();
  await expect(about).toHaveJSProperty("open", true);
  const skills = page.getByTestId("role-skills-lead");
  await expect(skills).toBeVisible();
  await expect(skills.getByTestId("role-skill")).toHaveCount(2);

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

  // The longer explanation the header used to carry lives here now.
  await expect(details).toContainText(
    "Each role is a set of instructions an agent follows in this project. Here you can see what each role is for, which agents can take it, and which version of its instructions is available on this computer and reported by running agents.",
  );

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

const ROLE_RELAY_SECRET = generateSecretKey();

function commissionedGenerationEvents(operator = false): {
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
    operator ? providerSecret : founderSecret,
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

  const authority: RelayEvent[] = [];
  if (operator) {
    const granteePubkey = getPublicKey(providerSecret);
    const transition = finalizeEvent(
      {
        kind: 44228,
        created_at: nowSeconds - 550,
        tags: [
          ["h", GENERAL_CHANNEL_ID],
          ["csat-v", "csat1-1"],
          ["csat-genesis", genesis.id],
        ],
        content: JSON.stringify({
          genesisRef: genesis.id,
          prevAccepted: null,
          seq: 1,
          type: "grant-operator",
          granteePubkey,
        }),
      },
      founderSecret,
    ) as unknown as RelayEvent;
    const accepted = finalizeEvent(
      {
        kind: 40099,
        created_at: nowSeconds - 540,
        tags: [["h", GENERAL_CHANNEL_ID]],
        content: JSON.stringify({
          type: "coding_session_authority_transition_accepted",
          genesisRef: genesis.id,
          acceptedEventId: transition.id,
          seq: 1,
          transitionType: "grant-operator",
          granteePubkey,
        }),
      },
      ROLE_RELAY_SECRET,
    ) as unknown as RelayEvent;
    authority.push(transition, accepted);
  }
  return {
    founderPubkey: getPublicKey(founderSecret),
    events: [genesis, ...authority, create, receipt, metadata],
  };
}

/** A managed agent seeded with home role "lead" so the ranked scenario
 * below exercises a populated Agents row (avatar + name + status) and a
 * non-"none" activity dot, alongside its reports. Deterministic 64-hex,
 * matching the style of `RANKED_PROJECT_OWNER` above. */
const LEAD_AGENT_PUBKEY = "b2".repeat(32);
const LEAD_AGENT_NAME = "Nova";

/**
 * Seeds the ranked "general" project (with its channel declared, so
 * `useProjectPacksView`'s project-channel filter admits it), a managed
 * agent whose home role is "lead", and the earlier-vs-current pack-revision
 * override, then opens its Roles tab. No report is published yet. Shared by
 * the report-cases test and the design screenshot matrix so both draw from
 * one fixture.
 */
async function openRankedProjectRolesTab(page: Page, trustedRelay = false) {
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
  await installMockBridge(page, {
    relaySelf: trustedRelay ? getPublicKey(ROLE_RELAY_SECRET) : null,
    managedAgents: [
      {
        pubkey: LEAD_AGENT_PUBKEY,
        name: LEAD_AGENT_NAME,
        avatarUrl: null,
        status: "running",
        homeRole: "lead",
        // Membership is association: a lead home role alone would list Nova
        // as "not a project agent", never on the lead card.
        projectRef: RANKED_PROJECT_ADDRESS,
        // The mock projects this membership into shared agent discovery.
        channelIds: [GENERAL_CHANNEL_ID],
        hasRolePack: true,
      },
    ],
  });
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
}

/**
 * Publishes the ranked idle/running reports, the no-version-reported report
 * and the no-role report onto the mock `general` channel. Seeded only once
 * the tab is open — the channel's live subscription (armed on navigation)
 * must already exist for the seeded events to be delivered rather than
 * dropped.
 */
async function seedRankedReports(page: Page) {
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
}

for (const operator of [false, true]) {
  test(`the Roles tab distinguishes versions with ${operator ? "operator" : "founder"} commissioning`, async ({
    page,
  }) => {
    await openRankedProjectRolesTab(page, operator);

    const leadCard = page.getByTestId("role-card-lead");
    await expect(leadCard).toBeVisible({ timeout: 15_000 });

    // The seeded managed agent (home role "lead") shows on the card from the
    // start, before any report — its status word ("running") is disclosed in
    // a title somewhere in the row, never color-only.
    const leadAgents = page.getByTestId("role-agents-lead");
    await expect(leadAgents).toContainText(LEAD_AGENT_NAME);
    await expect(leadAgents.locator('[title*="running" i]')).not.toHaveCount(0);
    const leadActivity = page.getByTestId("role-activity-lead");

    await waitForAnimations(page);
    const pageBuffer = await page.screenshot({
      path: `${SNAPSHOTS}/01-roles-page.png`,
      fullPage: true,
    });

    await seedRankedReports(page);

    // Case (a) + (b): the lead card's own reports sentence and version lines.
    // 3 lead reports: current (same version as here), earlier (1 behind), and
    // the no-coordinate report (no version reported). The no-role report is
    // never counted here — it names no role at all.
    const leadReports = page.getByTestId("role-reports-lead");
    await expect(leadReports).toHaveAttribute("data-reports", "some", {
      timeout: 15_000,
    });
    await expect(leadReports).toContainText("3 reports");
    await expect(leadReports).toContainText("1 same version");
    await expect(leadReports).toContainText("1 earlier");
    await expect(leadReports).toContainText("1 no version reported");
    await expect(leadReports).toContainText("same version as here");
    await expect(leadReports).toContainText("earlier, 1 behind");
    await expect(leadReports).toContainText("version not reported");

    // The 3 distinct versions are collapsed behind "Versions (3)", closed by
    // default, keyboard/click-openable, still holding the same version rows.
    const versionsDetails = page.getByTestId("role-versions-lead");
    await expect(versionsDetails).toBeAttached();
    await expect(versionsDetails).not.toHaveJSProperty("open", true);
    await expect(versionsDetails.locator("summary").first()).toHaveText(
      "Versions (3)",
    );
    await versionsDetails.locator("summary").first().click();
    await expect(versionsDetails).toHaveJSProperty("open", true);
    await expect(
      versionsDetails.getByTestId("role-report-version"),
    ).toHaveCount(3);

    // Activity dot: the lead role now has a seat (the no-version report,
    // which names "lead" with a seated `agentRef`) but that seat is `idle`,
    // never `running` — the two ranked reports carry no top-level `role`
    // (only `packRef.role`), so they count toward Report history but not
    // toward this role's own seats (`rolesViewModel.ts`'s stated join key is
    // `session.role`, not `packRef.role`).
    await expect(leadActivity).toHaveAttribute("data-activity", "idle");

    // Sessions: exactly the one seated report that names "lead" directly.
    const leadSeats = page.getByTestId("role-seats-lead");
    const leadSeatRows = leadSeats.getByTestId("seat-row");
    await expect(leadSeatRows).toHaveCount(1, { timeout: 15_000 });
    await expect(leadSeatRows.first()).toHaveAttribute(
      "data-seat-status",
      "idle",
    );

    // Reviewer never gets a report, an agent or a seat in this fixture — it
    // stays the single "quiet" line, distinct from lead's populated card.
    await expect(page.getByTestId("role-quiet-lead")).toHaveCount(0);
    await expect(page.getByTestId("role-quiet-reviewer")).toHaveText(
      "No agents, sessions or reports observed for this role.",
    );

    // Summary strip reacts to the same reads: 2 roles, the one seeded agent,
    // the 4 seated/reported events as open sessions, and 4 reports total
    // (Report history counts the no-role report too; only role cards omit it).
    await expect(page.getByTestId("roles-summary-agents")).toContainText("1");
    await expect(page.getByTestId("roles-summary-sessions")).toContainText("4");
    await expect(page.getByTestId("roles-summary-reports")).toContainText("4");

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
        events: commissionedGenerationEvents(operator)
          .events as unknown as never[],
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

    // The commissioned generation is a 5th report and a 5th open session.
    await expect(page.getByTestId("roles-summary-reports")).toContainText("5");
    await expect(page.getByTestId("roles-summary-sessions")).toContainText("5");

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
}

// ── Design screenshot matrix ─────────────────────────────────────────────

test("the Roles page reads as one calm system across viewports, themes and zoom", async ({
  page,
}) => {
  await openRankedProjectRolesTab(page);
  const leadCard = page.getByTestId("role-card-lead");
  await expect(leadCard).toBeVisible({ timeout: 15_000 });

  await seedRankedReports(page);
  await expect(page.getByTestId("role-reports-lead")).toHaveAttribute(
    "data-reports",
    "some",
    { timeout: 15_000 },
  );

  async function horizontalOverflow() {
    return page.evaluate(() => {
      const root = document.documentElement;
      const screen = document.querySelector(
        '[data-testid="project-packs-screen"]',
      );
      return {
        root: root.scrollWidth - root.clientWidth,
        screen: screen ? screen.scrollWidth - screen.clientWidth : 0,
      };
    });
  }

  async function titleBox() {
    return leadCard
      .locator("h3")
      .first()
      .evaluate((el) => ({
        scrollWidth: el.scrollWidth,
        clientWidth: el.clientWidth,
      }));
  }

  // 01 — light, 1280×900 (the mock bridge's default theme).
  await waitForAnimations(page);
  const lightBuffer = await page.screenshot({
    path: `${DESIGN_SNAPSHOTS}/01-light-1280.png`,
    fullPage: true,
  });

  // 03 — narrow, 640×900, light. No clipped text, no horizontal overflow.
  await page.setViewportSize({ width: 640, height: 900 });
  await waitForAnimations(page);
  const narrowBuffer = await page.screenshot({
    path: `${DESIGN_SNAPSHOTS}/03-narrow-640.png`,
    fullPage: true,
  });
  const narrowOverflow = await horizontalOverflow();
  expect(
    narrowOverflow.root,
    "documentElement overflows horizontally at 640px",
  ).toBeLessThanOrEqual(0);
  expect(
    narrowOverflow.screen,
    "project-packs-screen overflows horizontally at 640px",
  ).toBeLessThanOrEqual(0);
  const narrowTitleBox = await titleBox();
  expect(
    narrowTitleBox.scrollWidth,
    `role card title is clipped at 640px: ${JSON.stringify(narrowTitleBox)}`,
  ).toBeLessThanOrEqual(narrowTitleBox.clientWidth + 1);

  // 04 — 1280×900, 250% zoom via the app's own text-scale mechanism
  // (`useWebviewZoomShortcuts.ts`: root font-size = 16px × zoomFactor;
  // 16 × 2.5 = 40px).
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "40px";
  });
  await waitForAnimations(page);
  const zoomBuffer = await page.screenshot({
    path: `${DESIGN_SNAPSHOTS}/04-zoom-250.png`,
    fullPage: true,
  });
  const zoomOverflow = await horizontalOverflow();
  expect(
    zoomOverflow.root,
    "documentElement overflows horizontally at 250% zoom",
  ).toBeLessThanOrEqual(0);
  expect(
    zoomOverflow.screen,
    "project-packs-screen overflows horizontally at 250% zoom",
  ).toBeLessThanOrEqual(0);
  const zoomTitleBox = await titleBox();
  expect(
    zoomTitleBox.scrollWidth,
    `role card title is clipped at 250% zoom: ${JSON.stringify(zoomTitleBox)}`,
  ).toBeLessThanOrEqual(zoomTitleBox.clientWidth + 1);

  // Reset zoom before the remaining shots so they read at 100%.
  await page.evaluate(() => {
    document.documentElement.style.fontSize = "";
  });

  // 05 — the lead card alone, with its reports, agent and seat.
  await waitForAnimations(page);
  const cardBuffer = await leadCard.screenshot({
    path: `${DESIGN_SNAPSHOTS}/05-card-lead.png`,
  });

  // 06 — Technical details opened.
  const details = page.getByTestId("roles-technical-details");
  await details.locator("summary").first().click();
  await expect(details).toHaveJSProperty("open", true);
  await waitForAnimations(page);
  const detailsBuffer = await details.screenshot({
    path: `${DESIGN_SNAPSHOTS}/06-technical-details-open.png`,
  });

  // 02 — dark, the way the app really does it: `ThemeProvider` keys the whole
  // theme (CSS-variable surfaces included) off the stored theme *name*
  // (`buzz-theme`), read at boot. A stored name only takes effect on a page's
  // first navigation (the app rewrites it afterwards), so — exactly as
  // `badge.spec.ts` and `buzz-theme-screenshots.spec.ts` do — this seeds
  // "buzz-dark" on a fresh page before its first load, then opens the same
  // fixture and re-seeds the same reports. Flipping the `.dark` class alone
  // leaves every `bg-card` surface on its light value, which is not the dark
  // theme a person sees.
  const darkPage = await page.context().newPage();
  // The Buzz theme aliases follow the native appearance, so a stored
  // "buzz-dark" renders as "buzz" under Playwright's default light scheme:
  // emulate a dark scheme and, once the mock bridge is up, emit the native
  // theme-changed event the app listens for (`buzz-theme-screenshots.spec.ts`).
  await darkPage.emulateMedia({ colorScheme: "dark" });
  await darkPage.addInitScript(() => {
    window.localStorage.setItem("buzz-theme", "buzz-dark");
  });
  await openRankedProjectRolesTab(darkPage);
  await darkPage.evaluate(async () => {
    const tauriWindow = window as typeof window & {
      __TAURI_INTERNALS__?: {
        invoke?: (
          command: string,
          payload?: Record<string, unknown>,
        ) => Promise<unknown>;
      };
    };
    const invoke = tauriWindow.__TAURI_INTERNALS__?.invoke;
    if (!invoke) throw new Error("Mock Tauri invoke bridge is unavailable.");
    await invoke("plugin:event|emit", {
      event: "tauri://theme-changed",
      payload: "dark",
    });
  });
  await expect(darkPage.locator("html")).toHaveAttribute(
    "data-buzz-theme",
    "buzz-dark",
    { timeout: 15_000 },
  );
  await expect(darkPage.locator("html")).toHaveClass(/dark/, {
    timeout: 15_000,
  });
  await expect(darkPage.getByTestId("role-card-lead")).toBeVisible({
    timeout: 15_000,
  });
  await seedRankedReports(darkPage);
  await expect(darkPage.getByTestId("role-reports-lead")).toHaveAttribute(
    "data-reports",
    "some",
    { timeout: 15_000 },
  );
  await waitForAnimations(darkPage);
  const darkBuffer = await darkPage.screenshot({
    path: `${DESIGN_SNAPSHOTS}/02-dark-1280.png`,
    fullPage: true,
  });
  await darkPage.close();

  const digest = (buffer: Buffer) =>
    createHash("sha256").update(buffer).digest("hex");
  const hashes = {
    "01-light-1280": digest(lightBuffer),
    "02-dark-1280": digest(darkBuffer),
    "03-narrow-640": digest(narrowBuffer),
    "04-zoom-250": digest(zoomBuffer),
    "05-card-lead": digest(cardBuffer),
    "06-technical-details-open": digest(detailsBuffer),
  };
  expect(
    new Set(Object.values(hashes)).size,
    `every screenshot in the design matrix must be visually distinct: ${JSON.stringify(hashes)}`,
  ).toBe(6);
});

test("project Roles resolves shared identities without showing another project's sessions", async ({
  page,
}) => {
  const remoteKey = getPublicKey(generateSecretKey());
  const otherKey = getPublicKey(generateSecretKey());
  const makeReport = (
    agentRef: string,
    projectRef: string,
    sessionId: string,
    title: string,
  ) => {
    const original = noVersionRoleMetadataEvent();
    const content = JSON.parse(original.content);
    content.agentRef = agentRef;
    content.projectRef = projectRef;
    content.title = title;
    content.session = { ...NO_VERSION_TARGET, sessionId };
    return finalizeEvent(
      {
        kind: KIND_CODING_SESSION_METADATA,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ["h", GENERAL_CHANNEL_ID],
          ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
          ["cs-target", buildCodingSessionTargetKey(content.session)],
          ["csm-key", codingSessionMetadataSemanticKey(content.session)],
        ],
        content: JSON.stringify(content),
      },
      generateSecretKey(),
    );
  };
  await page.addInitScript(
    (features) =>
      window.localStorage.setItem("buzz-feature-overrides-v1", features),
    PROJECT_FEATURES,
  );
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [generalProjectWithChannel(), projectHead("other-scope", "Other scope")],
  );
  await installMockBridge(page, {
    relayAgents: [
      {
        pubkey: remoteKey,
        name: "Andy project lead",
        channelIds: [GENERAL_CHANNEL_ID],
        status: "offline",
      },
      {
        pubkey: otherKey,
        name: "Other project lead",
        channelIds: [GENERAL_CHANNEL_ID],
        status: "online",
      },
    ],
  });
  await page.goto("/");
  await openRolesTab(page);
  await page.evaluate(
    ({ events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seam missing");
      for (const event of events as never[])
        seed({ channelName: "general", event });
    },
    {
      events: [
        makeReport(
          remoteKey,
          RANKED_PROJECT_ADDRESS,
          "11111111-2222-3333-4444-555555555555",
          "Selected project work",
        ),
        makeReport(
          otherKey,
          `30621:${IDENTITY.pubkey}:other-scope`,
          "66666666-7777-8888-9999-000000000000",
          "Other project work",
        ),
      ],
    },
  );
  const card = page.getByTestId("role-card-lead");
  await expect(
    card
      .getByTestId("role-agent-chip")
      .filter({ hasText: "Andy project lead" }),
  ).toBeVisible({ timeout: 15_000 });
  await expect(card).not.toContainText("Other project lead");
  await expect(
    card
      .getByTestId("role-agent-chip")
      .filter({ hasText: "Andy project lead" }),
  ).toHaveAttribute("data-agent-pack", "unknown");
  await expect(
    card.getByRole("button", { name: /Andy project lead/ }),
  ).toBeVisible();
  await expect(page.getByTestId("roles-section")).not.toContainText(
    "Other project work",
  );
});
