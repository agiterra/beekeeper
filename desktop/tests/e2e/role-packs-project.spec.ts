import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";

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
 * The Agents tab says which project's role packs it will install.
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
const SNAPSHOTS = "test-results/role-pack-snapshots";

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

test("the Packs tab explains local versions and unverified metadata claims", async ({
  page,
}) => {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, PROJECT_FEATURES);
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 850 });
  await page.goto("/");
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-packs").click();

  const snapshots = page.getByTestId("role-pack-snapshots");
  await expect(snapshots).toBeVisible({ timeout: 15_000 });
  await expect(snapshots).toContainText(
    "Versions found on this machine and pack revisions reported in this project’s channels.",
  );
  await expect(snapshots).toContainText(
    "Beekeeper checks who sent each report. Confirming its source does not prove which role instructions were used.",
  );
  await expect(snapshots.getByText("Available here")).toBeVisible();
  await expect(
    snapshots.getByText("Reported revisions", { exact: true }),
  ).toBeVisible();
  await expect(page.getByTestId("role-pack-resolved-row")).toHaveCount(2);
  await expect(snapshots).toContainText("lead · 9f2e1d0c");
  await expect(snapshots).toContainText("reviewer · 0.0.0-e");
  await expect(page.getByTestId("role-pack-reports-empty")).toHaveText(
    "No role-version metadata claims are visible for this project.",
  );
  await expect(snapshots).not.toContainText("44223");

  await waitForAnimations(page);
  await snapshots.screenshot({ path: `${SNAPSHOTS}/available-and-empty.png` });
});

// ── Ranked reported revisions ────────────────────────────────────────────

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
 * in `useProjectPacksView.ts`) admits it and the Packs tab reads that
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

test("the Packs tab ranks reported pack revisions against this machine's checkout", async ({
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
  await page.setViewportSize({ width: 1280, height: 850 });
  await page.goto("/");

  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-packs").click();

  const snapshots = page.getByTestId("role-pack-snapshots");
  await expect(snapshots).toBeVisible({ timeout: 15_000 });

  // Seed the two generations only once the tab is open — the channel's live
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
      ] as unknown as never[],
    },
  );

  const rows = page.getByTestId("role-pack-reported-row");
  await expect(rows).toHaveCount(2, { timeout: 15_000 });

  const currentRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-relation="current"]',
  );
  await expect(currentRow).toHaveCount(1);
  await expect(currentRow).toContainText("Same revision as this machine");
  await expect(currentRow).not.toContainText("Keeps this revision");
  // No lifecycle chain was ever seeded for either ranked row — a signed 44223
  // alone proves a signature and a channel, nothing about who commissioned
  // the generation that sent it (constraint 3: missing is not disputed).
  await expect(currentRow).toHaveAttribute(
    "data-provenance",
    "proof-unavailable",
  );
  await expect(currentRow).toContainText("Unverified · proof unavailable");

  const earlierRow = page.locator(
    '[data-testid="role-pack-reported-row"][data-relation="earlier"]',
  );
  await expect(earlierRow).toHaveCount(1);
  await expect(earlierRow).toContainText(
    "Earlier revision · 1 behind this machine",
  );
  await expect(earlierRow).toContainText("running");
  await expect(earlierRow).toContainText("reported");
  await expect(earlierRow).toContainText(
    "Keeps this revision until its next launch or resume.",
  );
  await expect(earlierRow).toHaveAttribute(
    "data-provenance",
    "proof-unavailable",
  );
  await expect(earlierRow).toContainText("Unverified · proof unavailable");

  const checkoutStatus = page.getByTestId("role-pack-checkout-status");
  await expect(checkoutStatus).toContainText("On 9f2e1d0c");
  await expect(checkoutStatus).toContainText("git answered at");

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

  await expect(rows).toHaveCount(3, { timeout: 15_000 });
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

  await waitForAnimations(page);
  const rankedBuffer = await snapshots.screenshot({
    path: `${SNAPSHOTS}/ranked-rows.png`,
  });
  const rankedDigest = createHash("sha256").update(rankedBuffer).digest("hex");
  const emptyPath = `${SNAPSHOTS}/available-and-empty.png`;
  if (existsSync(emptyPath)) {
    const emptyDigest = createHash("sha256")
      .update(readFileSync(emptyPath))
      .digest("hex");
    expect(
      rankedDigest,
      "ranked-rows.png captured the same pixels as available-and-empty.png",
    ).not.toBe(emptyDigest);
  }
});
