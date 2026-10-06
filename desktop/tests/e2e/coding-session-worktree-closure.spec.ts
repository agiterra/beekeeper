import { expect, test, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import {
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";
import { E2E_IDENTITY_OVERRIDE_STORAGE_KEY } from "../helpers/onboarding";
import { waitForAnimations } from "../helpers/animations";

/**
 * L11 — what the close dialog owes a person about the directories on their
 * disk.
 *
 * The bug this exists for: the hire host cuts one git worktree per seat and
 * nothing ever removed one, so 70 of them and 331 GB accumulated on the
 * machine this was written for. Closing a session is now the moment those
 * directories are accounted for — but the accounting must never cost anyone
 * their uncommitted work, so the two facts under test are:
 *
 *   1. A seat worktree holding uncommitted work reads `held: {N} uncommitted
 *      files` and is the **only** row offering a removal control.
 *   2. Closing the session publishes the closure and leaves that directory
 *      exactly where it is.
 */

// Written under the worktree, then copied into batch3/l11-shots/ by the
// lane report step — a spec must not write outside the repository.
const SHOTS = "test-results/l11-shots";

// The founder is the E2E bridge's own known identity (tyler), not a fresh
// key — matching the pattern `coding-session-mission-lens.spec.ts` already
// established. Two things need it to be a *recognized* identity rather than
// a random one: `installMockBridge`'s mock relay keys channel membership to
// identities the bridge knows (a fresh key is never a member), and the
// genesis event's signature must verify for real
// (`classifyCodingSessionGenesisEvent` → `hasValidSignature`,
// `codingSessionCreateObservations.ts:329-333`) — the mock transport does
// *not* skip verification for coding-session events the way it does for
// plain chat-message fixtures elsewhere. So this alone is not enough to make
// the *viewer* the founder: `openSessionAndCloseDialog` below also seeds
// `E2E_IDENTITY_OVERRIDE_STORAGE_KEY` with this same keypair before the
// bridge installs, so `get_identity`/`sign_event` report tyler as "self" —
// only then does `currentUserPubkey === umbrella.founderPubkey`
// (`CodingSessionWorkspace.tsx`'s `canCloseSession`) actually hold.
const FOUNDER_SECRET = hexToBytes(
  "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
);
const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

const CHANNEL_NAME = "engineering";
const CHANNEL_ID = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const COMMAND_ID = "csl-l11-worktree-session";
const BASE_CREATED_AT = 1_800_000_000;
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};
const TARGET_KEY = buildCodingSessionTargetKey(TARGET);

const HELD_PATH =
  "/Users/mock/Code/beekeeper.worktrees/fix-reconnect-refuter-1";
const CLEAN_PATH =
  "/Users/mock/Code/beekeeper.worktrees/fix-reconnect-builder-1";

/** Exactly the shape `list_coding_session_seat_worktrees` answers with. */
const SEAT_WORKTREES = [
  {
    key: `${SESSION_REF}/builder-1`,
    sessionRef: SESSION_REF,
    seatLabel: "builder-1",
    path: CLEAN_PATH,
    branch: "fix-reconnect-builder-1",
    repoRoot: "/Users/mock/Code/beekeeper",
    disposition: "within-grace",
    dirtyFiles: 0,
    reclaimableBytes: 18_400_000_000,
    reclaimableLabel: "18.4 GB",
    reclaimableNow: true,
    graceRemainingSecs: 7 * 24 * 60 * 60,
    exists: true,
    tipOnRelayKnown: true,
    detail: `${CLEAN_PATH}: clean and pushed, kept 7 more days`,
  },
  {
    key: `${SESSION_REF}/refuter-1`,
    sessionRef: SESSION_REF,
    seatLabel: "refuter-1",
    path: HELD_PATH,
    branch: "fix-reconnect-refuter-1",
    repoRoot: "/Users/mock/Code/beekeeper",
    disposition: "held",
    dirtyFiles: 3,
    reclaimableBytes: 4_100_000_000,
    reclaimableLabel: "4.1 GB",
    reclaimableNow: true,
    graceRemainingSecs: null,
    exists: true,
    tipOnRelayKnown: true,
    detail: "held: 3 uncommitted files",
  },
];

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

function seededEvents(): RelayEvent[] {
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
    title: "Fix the reconnect bug",
    initialTurn: null,
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
        ["cs-target", TARGET_KEY],
        ["csm-key", codingSessionMetadataSemanticKey(TARGET)],
      ],
      JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: TARGET,
        projectRef: null,
        repoRef: null,
        title: "Fix the reconnect bug",
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
  ];
}

async function openSessionAndCloseDialog(page: Page) {
  // Must run before `installMockBridge` mounts the app: React reads identity
  // state on mount, so seeding after the bridge installs is too late.
  await page.addInitScript(
    ({ storageKey, identity }) => {
      window.localStorage.setItem(storageKey, JSON.stringify(identity));
    },
    {
      storageKey: E2E_IDENTITY_OVERRIDE_STORAGE_KEY,
      identity: {
        privateKey: bytesToHex(FOUNDER_SECRET),
        pubkey: FOUNDER_PUBKEY,
        username: "tyler",
      },
    },
  );
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [
        { pubkey: PROVIDER_PUBKEY, label: "This computer (coding sessions)" },
      ],
    },
    searchProfiles: [{ pubkey: FOUNDER_PUBKEY, displayName: "Tyler" }],
    codingSessionSeatWorktrees: SEAT_WORKTREES,
  });
  await page.goto("/");
  await page.getByTestId(`channel-${CHANNEL_NAME}`).click();
  await page.evaluate(
    ({ channelName, events }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of events) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, events: seededEvents() },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  await expect(page.getByTestId("coding-session-workspace")).toBeVisible({
    timeout: 15_000,
  });

  // Close session lives in the header's `⋯` session-actions menu.
  await page.getByTestId("coding-session-overflow").click();
  await page.getByTestId("coding-session-overflow-close-session").click();
  await expect(
    page.getByTestId("coding-session-closure-worktrees"),
  ).toBeVisible();
  await waitForAnimations(page);
}

test("held work is named in the close dialog and only that row offers removal", async ({
  page,
}) => {
  await openSessionAndCloseDialog(page);

  const rows = page.getByTestId("coding-session-closure-worktree");
  await expect(rows).toHaveCount(2);

  // The clean tree states the grace window; the held tree states its count.
  await expect(rows.nth(0)).toContainText("clean and pushed, kept 7 more days");
  await expect(rows.nth(1)).toContainText("held: 3 uncommitted files");

  // One prune control, on the held row and nowhere else.
  await expect(page.getByTestId("coding-session-worktree-prune")).toHaveCount(
    1,
  );
  await expect(
    rows.nth(1).getByTestId("coding-session-worktree-prune"),
  ).toBeVisible();
  await expect(
    rows.nth(0).getByTestId("coding-session-worktree-prune"),
  ).toHaveCount(0);

  // And the session closes either way — said out loud, not implied.
  await expect(
    page.getByTestId("coding-session-closure-held-note"),
  ).toContainText("The session closes either way");

  await page
    .getByTestId("coding-session-closure-worktrees")
    .screenshot({ path: `${SHOTS}/01-closure-worktrees.png` });
});

test("the prune confirm names the path and the count before removing anything", async ({
  page,
}) => {
  await openSessionAndCloseDialog(page);

  await page.getByTestId("coding-session-worktree-prune").click();
  const confirm = page.getByTestId("coding-session-worktree-prune-confirm");
  await expect(confirm).toBeVisible();
  await waitForAnimations(page);

  const dialog = page.locator('[role="alertdialog"]').filter({
    has: confirm,
  });
  await expect(dialog).toContainText(HELD_PATH);
  await expect(dialog).toContainText("3 uncommitted files");
  await dialog.screenshot({ path: `${SHOTS}/02-prune-held-confirm.png` });
});

test("build output is offered on the held row too, because no commit lives there", async ({
  page,
}) => {
  await openSessionAndCloseDialog(page);

  const reclaim = page.getByTestId("coding-session-worktree-reclaim");
  await expect(reclaim).toHaveCount(2);
  await expect(reclaim.nth(0)).toContainText("18.4 GB");
  await expect(reclaim.nth(1)).toContainText("4.1 GB");

  await page
    .getByTestId("coding-session-closure-worktree")
    .nth(1)
    .screenshot({ path: `${SHOTS}/03-held-row-reclaim.png` });
});
