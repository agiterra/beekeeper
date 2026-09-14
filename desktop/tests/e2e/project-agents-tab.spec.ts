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
 * The project Agents tab (`docs/PROJECT_AGENTS_TAB_SPEC.md`) explains who is
 * working in a project and why. The case that motivated it: a builder seated
 * inside the lead's session is an execution of that umbrella, not a shelf row,
 * and used to be invisible. This drives the real surface with signed 44223
 * metadata for a lead and a worker sharing one `sessionRef`, plus an installed
 * verifier that has never been seated.
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
          role: "builder",
          agentPubkey: BOB.pubkey,
          packRef: packRef("builder"),
        },
        {
          role: "verifier",
          agentPubkey: SAGE.pubkey,
          packRef: packRef("verifier"),
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
  ) => ({
    pubkey: agent.pubkey,
    name: agent.name,
    avatarUrl: null,
    status: "running",
    homeRole,
    channelIds: [GENERAL_CHANNEL_ID],
    hasRolePack: true,
  });
  await installMockBridge(page, {
    managedAgents: [
      managed(LOOM, "lead"),
      managed(BOB, "builder"),
      managed(SAGE, "verifier"),
      { ...managed(HOMEBODY, "builder"), channelIds: [] },
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

test("the Agents tab explains a worker seated in the lead's session and an installed agent awaiting work", async ({
  page,
}) => {
  await openProjectAgentsTab(page);

  // The strip reads Overview · Agents · Roles; Contributors is gone.
  const tabs = page.getByTestId("project-page-tabs");
  await expect(page.getByTestId("project-tab-agents")).toHaveText("Agents");
  await expect(tabs).not.toContainText("Contributors");

  // Before any seat: installed agents wait; the home-role-only agent is absent.
  const installed = page.getByTestId("project-agents-installed");
  await expect(installed).toContainText("Sage", { timeout: 15_000 });
  await expect(installed).toContainText("Installed as Verifier");
  await expect(page.getByTestId("project-agents-screen")).not.toContainText(
    HOMEBODY.name,
  );

  await seedSeats(page);

  const working = page.getByTestId("project-agents-working");
  await expect(working).toContainText("Bob", { timeout: 15_000 });
  const bob = working.locator(
    `[data-testid="project-agent-row"][data-agent-pubkey="${BOB.pubkey}"]`,
  );
  await expect(bob.getByTestId("project-agent-relationship")).toContainText(
    "Builder in",
  );
  await expect(bob).toContainText("claude-primary · sonnet");
  await expect(bob).toContainText("Instructions builder @ f0132d13");
  await expect(working).toContainText("Loom");
  // Sage is still waiting: an install is not participation.
  await expect(installed).toContainText("Sage");
  await expect(page.getByTestId("project-agents-screen")).not.toContainText(
    HOMEBODY.name,
  );

  await waitForAnimations(page);
  await page.screenshot({
    path: `${SNAPSHOTS}/01-agents-tab.png`,
    fullPage: true,
  });

  // The Roles page now counts the worker seated inside the lead's session.
  await page.getByTestId("project-tab-packs").click();
  const builderAgents = page.getByTestId("role-agents-builder");
  await expect(builderAgents).toContainText("Bob", { timeout: 15_000 });
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
