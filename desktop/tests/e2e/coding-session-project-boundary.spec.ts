import { expect, test } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  codingSessionMetadataSemanticKey,
  CODING_SESSION_METADATA_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  codingSessionTranscriptSemanticKey,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
} from "@/features/coding-sessions/lib/codingSessionTranscriptPresentation";
import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { installMockBridge } from "../helpers/bridge";

// The provider discloses what each generation runs inside with one transcript
// item, enqueued on every open with no turn
// (`execution_scope::boundary_status_item` in `crates/buzz-session-provider`):
// `{ kind: "status", status, reason }`, where `reason` is the backend when
// enforced and a stable slug when not. These tests sign exactly that shape
// and check what a reader sees; nothing here infers protection from any other
// field.

const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "bbbbbbbb-cccc-dddd-eeee-ffffffffffff",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const ENFORCED_TEXT =
  "Enforced — this session and every process it starts run inside this project's boundary; other projects' files are outside it (macOS Seatbelt)";
const NOT_ENFORCED_TEXT =
  "Not enforced — this session is not isolated from other projects' files (this platform has no boundary backend)";

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_200_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  return signed(
    KIND_CODING_SESSION_METADATA,
    0,
    {
      schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
      session,
      projectRef: null,
      repoRef: null,
      title: "Plan the work",
      agentRef: null,
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "sonnet",
      status: "completed",
      branch: null,
      capabilities: {
        threadTurnStart: true,
        threadTurnInterrupt: true,
        threadSteer: true,
        context: false,
        diff: false,
        plan: false,
      },
    },
    [
      ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
      ["cs-target", targetKey],
      ["csm-key", codingSessionMetadataSemanticKey(session)],
    ],
  );
}

function transcript(
  seq: number,
  turnId: string | null,
  item: unknown,
): RelayEvent {
  return signed(
    KIND_CODING_SESSION_TRANSCRIPT,
    seq,
    {
      schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
      session,
      eventSeq: seq,
      timestamp: 1_800_200_000_000 + seq * 1_000,
      turnId,
      item,
    },
    [
      ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
      ["cs-target", targetKey],
      ["cst-seq", String(seq)],
      ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
    ],
  );
}

/** A generation's opening disclosure (if any) followed by one ordinary turn. */
function events(boundary: { status: string; reason: string } | null) {
  const rows: RelayEvent[] = [metadata()];
  let seq = 1;
  if (boundary) {
    rows.push(transcript(seq++, null, { kind: "status", ...boundary }));
  }
  rows.push(
    transcript(seq++, "turn-1", {
      kind: "user_prompt",
      content: "Draft the plan for this goal.",
    }),
    transcript(seq, "turn-1", {
      kind: "assistant_text",
      text: "Drafting the plan from the goal.",
    }),
  );
  return rows;
}

async function openSeededSession(
  page: import("@playwright/test").Page,
  seeded: RelayEvent[],
) {
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Boundary provider" }],
    },
  });
  await page.goto("/");
  await page.getByTestId(`channel-${channelName}`).click();
  await page.evaluate(
    ({ channelName: name, events: signedEvents }) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of signedEvents) seed({ channelName: name, event });
    },
    { channelName, events: seeded },
  );
  const trigger = page.getByTestId("channel-coding-sessions-trigger");
  await expect(trigger).toHaveAttribute("aria-label", "Coding sessions (1)", {
    timeout: 15_000,
  });
  await trigger.click();
  await page.getByTestId("channel-coding-session-open").click();
  const workspace = page.getByTestId("coding-session-workspace");
  await expect(workspace).toContainText("Drafting the plan from the goal.");
  return workspace;
}

test("an enforced boundary shows what it covers and the backend that enforced it", async ({
  page,
}) => {
  const workspace = await openSeededSession(
    page,
    events({ status: "execution_boundary_enforced", reason: "macos-seatbelt" }),
  );
  await expect(workspace.getByText("Project boundary")).toBeVisible();
  await expect(workspace.getByText(ENFORCED_TEXT)).toBeVisible();
  await expect(workspace).not.toContainText("Not enforced");
  await expect(workspace).not.toContainText("execution_boundary_enforced");
});

test("an unenforced boundary says the session is not isolated, and why", async ({
  page,
}) => {
  const workspace = await openSeededSession(
    page,
    events({
      status: "execution_boundary_not_enforced",
      reason: "no-backend-for-platform",
    }),
  );
  await expect(workspace.getByText("Project boundary")).toBeVisible();
  await expect(workspace.getByText(NOT_ENFORCED_TEXT)).toBeVisible();
  await expect(workspace).not.toContainText("Enforced — ");
  await expect(workspace).not.toContainText("execution_boundary_not_enforced");
});

test("without the provider's disclosure no boundary row claims anything", async ({
  page,
}) => {
  const workspace = await openSeededSession(page, events(null));
  await expect(workspace).not.toContainText("Project boundary");
  await expect(workspace).not.toContainText("Enforced — ");
});
