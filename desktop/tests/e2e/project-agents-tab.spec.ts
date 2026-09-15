import { expect, test, type Page } from "@playwright/test";
import { finalizeEvent, generateSecretKey } from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import type { ProjectInstalledRoles } from "@/features/roles/lib/projectInstalledRoles";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_PROJECT,
} from "@/shared/constants/kinds";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The project Agents tab (`docs/PROJECT_AGENTS_TAB_SPEC.md`, corrected by
 * `docs/PROJECT_AGENT_HIRING_IMPL.md` § UI rules) explains who belongs to a
 * project and who is working in it. Membership is association
 * (`ManagedAgent.projectRef`), never a seat: a builder seated inside the
 * lead's session without the association is a borrowed participant, not a
 * project agent. This drives the real surface with signed 44223 metadata for
 * an associated lead and an unassociated worker sharing one `sessionRef`, an
 * associated verifier that has never been seated, and an agent setup
 * installed whose record still lacks the association.
 */

const PROJECT_FEATURES = JSON.stringify({ projects: true });
const SNAPSHOTS = "test-results/project-agents";
const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const PROJECT_OWNER = "a1".repeat(32);
const PROJECT_ADDRESS = `${KIND_PROJECT}:${PROJECT_OWNER}:general`;
const PACKS_REPO = `30617:${"c".repeat(64)}:packs`;
const INSTALLED_SHA = "f0132d132e5fa4015402257b3dfb5102ae2771d1";
const SESSION_REF = "5e5e5e5e-1111-4222-8333-444455556666";

const LOOM = { pubkey: "b2".repeat(32), name: "Loom" };
const BOB = { pubkey: "b3".repeat(32), name: "Bob" };
const SAGE = { pubkey: "b4".repeat(32), name: "Sage" };
/** Managed here with a builder home role, but nothing places it in this project. */
const HOMEBODY = { pubkey: "b5".repeat(32), name: "Homebody" };
/** Installed for this project by setup; its record never got the association. */
const KEEL = { pubkey: "b6".repeat(32), name: "Keel" };

const CAPABILITIES = {
  threadTurnStart: true,
  threadTurnInterrupt: true,
  threadSteer: true,
  context: false,
  diff: false,
  plan: true,
};

function packRef(role: string) {
  return {
    repo: PACKS_REPO,
    sha: INSTALLED_SHA,
    role,
    path: `personas/roles/${role}`,
  };
}

function generalProjectWithChannel(): RelayEvent {
  return {
    id: "project-general-agents".padEnd(64, "0"),
    pubkey: PROJECT_OWNER,
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

/** A signed seat report. Key order is the strict decoder's own sequence. */
function seatMetadataEvent(input: {
  instanceId: string;
  sessionId: string;
  agentRef: string;
  role: string;
  title: string;
  model: string;
  status: "idle" | "running";
  secondsAgo: number;
}): RelayEvent {
  const target = {
    driver: "claude-agent-acp",
    instanceId: input.instanceId,
    sessionId: input.sessionId,
    generation: 1,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: Math.floor(Date.now() / 1000) - input.secondsAgo,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(target)],
        ["csm-key", codingSessionMetadataSemanticKey(target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: target,
        projectRef: PROJECT_ADDRESS,
        repoRef: null,
        title: input.title,
        agentRef: input.agentRef,
        provider: "claude-primary",
        runtime: "claude-agent-acp",
        model: input.model,
        status: input.status,
        branch: null,
        capabilities: CAPABILITIES,
        sessionRef: SESSION_REF,
        role: input.role,
        packRef: packRef(input.role),
      }),
    },
    generateSecretKey(),
  ) as unknown as RelayEvent;
}

async function openProjectAgentsTab(page: Page) {
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
  const installed: ProjectInstalledRoles[] = [
    {
      projectRef: PROJECT_ADDRESS,
      setupId: "setup-1",
      publicationId: "publication-1",
      teamId: "team-1",
      source: { repoRef: PACKS_REPO, sha: INSTALLED_SHA, packPath: "personas" },
      leadChannelId: null,
      roles: [
        { role: "lead", agentPubkey: LOOM.pubkey, packRef: packRef("lead") },
        {
          role: "verifier",
          agentPubkey: SAGE.pubkey,
          packRef: packRef("verifier"),
        },
        {
          role: "runner",
          agentPubkey: KEEL.pubkey,
          packRef: packRef("runner"),
        },
      ],
    },
  ];
  await page.addInitScript((roles) => {
    (
      window as unknown as { __BUZZ_E2E_PROJECT_TEAM_SETUP__: unknown }
    ).__BUZZ_E2E_PROJECT_TEAM_SETUP__ = { installedRoles: roles };
  }, installed);
  const managed = (
    agent: { pubkey: string; name: string },
    homeRole: string,
    projectRef: string | null,
  ) => ({
    pubkey: agent.pubkey,
    name: agent.name,
    avatarUrl: null,
    status: "running" as const,
    homeRole,
    projectRef,
    channelIds: [GENERAL_CHANNEL_ID],
    hasRolePack: true,
  });
  await installMockBridge(page, {
    managedAgents: [
      managed(LOOM, "lead", PROJECT_ADDRESS),
      managed(BOB, "builder", null),
      managed(SAGE, "verifier", PROJECT_ADDRESS),
      managed(KEEL, "runner", null),
      { ...managed(HOMEBODY, "builder", null), channelIds: [] },
    ],
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-agents").click();
  await expect(page.getByTestId("project-agents-screen")).toBeVisible();
}

async function seedSeats(page: Page) {
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("mock signed-event seam is missing");
      for (const event of events as never[]) seed({ channelName, event });
    },
    {
      channelName: "general",
      events: [
        seatMetadataEvent({
          instanceId: "loom-lead",
          sessionId: "aaaa1111-2222-4333-8444-555566667777",
          agentRef: LOOM.pubkey,
          role: "lead",
          title: "Project team setup",
          model: "opus",
          status: "idle",
          secondsAgo: 600,
        }),
        seatMetadataEvent({
          instanceId: "bob-builder",
          sessionId: "bbbb1111-2222-4333-8444-555566667777",
          agentRef: BOB.pubkey,
          role: "builder",
          title: "Bob builder seat",
          model: "sonnet",
          status: "running",
          secondsAgo: 120,
        }),
      ] as unknown as never[],
    },
  );
}

function row(page: Page, pubkey: string) {
  return page.locator(
    `[data-testid="project-agent-row"][data-agent-pubkey="${pubkey}"]`,
  );
}

test("the Agents tab lists associated agents as members and a worker seated without association as borrowed", async ({
  page,
}) => {
  await openProjectAgentsTab(page);

  // The strip reads Overview · Agents · Roles; Contributors is gone.
  const tabs = page.getByTestId("project-page-tabs");
  await expect(page.getByTestId("project-tab-agents")).toHaveText("Agents");
  await expect(tabs).not.toContainText("Contributors");

  // Before any seat: the associated agents are members and Available; the
  // installed-but-unassociated agent is listed with the warning that the lead
  // can't hire it; home-role-only agents are absent.
  const members = page.getByTestId("project-agents-members");
  await expect(members).toContainText("Sage", { timeout: 15_000 });
  const sage = row(page, SAGE.pubkey);
  await expect(sage).toHaveAttribute("data-agent-state", "available");
  await expect(sage.getByTestId("project-agent-role")).toHaveText("Verifier");
  await expect(sage.getByTestId("project-agent-state")).toHaveText("Available");
  const keel = row(page, KEEL.pubkey);
  await expect(keel).toHaveAttribute("data-agent-state", "not-associated");
  await expect(
    keel.getByTestId("project-agent-association-missing"),
  ).toContainText("the lead can't hire it");
  const screen = page.getByTestId("project-agents-screen");
  await expect(screen).not.toContainText(HOMEBODY.name);
  await expect(screen).not.toContainText(BOB.name);

  await seedSeats(page);

  // Bob holds a running seat in the lead's session, but a seat is not
  // membership: he is borrowed, and says so.
  const borrowed = page.getByTestId("project-agents-borrowed");
  await expect(borrowed).toContainText("Bob", { timeout: 15_000 });
  const bob = borrowed.locator(
    `[data-testid="project-agent-row"][data-agent-pubkey="${BOB.pubkey}"]`,
  );
  await expect(bob).toHaveAttribute("data-agent-state", "working");
  await expect(bob.getByTestId("project-agent-badge")).toHaveText("Borrowed");
  await expect(bob.getByTestId("project-agent-not-member")).toContainText(
    "Not a General agent",
  );
  await expect(bob.getByTestId("project-agent-relationship")).toContainText(
    "Builder in",
  );
  await expect(bob).toContainText("claude-primary · sonnet");
  await expect(bob).toContainText("Instructions builder @ f0132d13");
  await expect(
    members.locator('[data-testid="project-agent-row"]'),
  ).toHaveCount(3);
  await expect(
    members.locator(`[data-agent-pubkey="${BOB.pubkey}"]`),
  ).toHaveCount(0);

  // Loom is a member whose newest open execution is idle.
  const loom = row(page, LOOM.pubkey);
  await expect(loom).toHaveAttribute("data-agent-state", "idle", {
    timeout: 15_000,
  });
  await expect(loom.getByTestId("project-agent-badge")).toHaveText(
    "Project agent",
  );
  // Sage is still Available: an install is not participation.
  await expect(sage).toHaveAttribute("data-agent-state", "available");
  await expect(screen).not.toContainText(HOMEBODY.name);

  await waitForAnimations(page);
  await page.screenshot({
    path: `${SNAPSHOTS}/01-agents-tab.png`,
    fullPage: true,
  });

  // The Roles page lists the borrowed worker beside the builder role, marked
  // as not a project agent rather than counted as one.
  await page.getByTestId("project-tab-packs").click();
  const nonProject = page.getByTestId("role-non-project-agents-builder");
  await expect(nonProject).toContainText("Bob", { timeout: 15_000 });
  await expect(nonProject).toContainText("not a project agent");
  await expect(page.getByTestId("role-agents-builder")).not.toContainText(
    "Bob",
  );
  await expect(page.getByTestId("roles-agents-pointer")).toBeVisible();
  await waitForAnimations(page);
  await page.getByTestId("role-card-builder").screenshot({
    path: `${SNAPSHOTS}/02-roles-builder-card.png`,
  });
});

test("the old Contributors path lands on the Agents tab", async ({ page }) => {
  await openProjectAgentsTab(page);
  // The app routes through the URL hash; rewrite only the route inside it.
  const url = page.url();
  expect(url).toMatch(/#\/projects\/[^/]+\/agents$/);
  await page.getByTestId("project-tab-packs").click();
  await expect(page.getByTestId("project-tab-packs")).toHaveAttribute(
    "data-state",
    "active",
  );
  await page.goto(url.replace(/\/agents$/, "/contributors"));
  await expect(page.getByTestId("project-agents-screen")).toBeVisible({
    timeout: 15_000,
  });
  expect(page.url()).toMatch(/\/agents$/);
});
