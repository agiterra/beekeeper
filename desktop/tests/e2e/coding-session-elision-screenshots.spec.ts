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
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The two ways a coding-session transcript can be missing content, rendered.
 *
 * A **redaction** is the provider removing a host-private or
 * credential-bearing value before signing; a **cap** is an item that did not
 * fit the 32 KiB event envelope. They share one pill vocabulary so a reader
 * learns a single idiom, and they never share a label — the reader is owed the
 * difference between "deliberately withheld" and "too big to send".
 */

const SHOTS = "test-results/coding-session-elision";
const secret = generateSecretKey();
const pubkey = getPublicKey(secret);
const channelName = "engineering";
const channelId = "1c7e1c02-87bb-5e88-b2da-5a7a9432d0c9";
const session = {
  driver: "claude-agent-acp",
  instanceId: "fedcba9876543210",
  sessionId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  generation: 1,
};
const targetKey = buildCodingSessionTargetKey(session);

const HOME_DIGEST =
  "eb7930a9a9209e69d829efa946d4ebea3f2e32c6b03f3317321c53bf33597e3f";
const TOKEN_DIGEST =
  "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHELL_DIGEST =
  "9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f9f";

function marker(bytes: number, digest: string): string {
  return `[elided private context: ${bytes} bytes, sha256:${digest}]`;
}

function signed(kind: number, seq: number, content: unknown, tags: string[][]) {
  return finalizeEvent(
    {
      kind,
      created_at: 1_800_100_000 + seq,
      tags: [["h", channelId], ...tags],
      content: JSON.stringify(content),
    },
    secret,
  ) as unknown as RelayEvent;
}

function metadata(): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
    session,
    projectRef: null,
    repoRef: null,
    title: "Trace the git ACL push failure",
    agentRef: null,
    provider: "claude-agent-acp",
    runtime: "claude-agent-acp",
    model: "sonnet",
    status: "completed",
    branch: "feat/project-membership-git-acl",
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: true,
      context: false,
      diff: true,
      plan: true,
    },
  };
  return signed(KIND_CODING_SESSION_METADATA, 0, payload, [
    ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
    ["cs-target", targetKey],
    ["csm-key", codingSessionMetadataSemanticKey(session)],
  ]);
}

function transcript(seq: number, item: unknown): RelayEvent {
  const payload = {
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session,
    eventSeq: seq,
    timestamp: 1_800_100_000_000 + seq * 1_000,
    turnId: "redacted-turn",
    item,
  };
  return signed(KIND_CODING_SESSION_TRANSCRIPT, seq, payload, [
    ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
    ["cs-target", targetKey],
    ["cst-seq", String(seq)],
    ["cst-key", codingSessionTranscriptSemanticKey(session, seq)],
  ]);
}

function elisionEvents(): RelayEvent[] {
  return [
    metadata(),
    transcript(1, {
      kind: "user_prompt",
      content: "What was the result?",
    }),
    // Prose carrying two redactions: a host path and a credential value. This
    // is the shape that first produced the complaint — the whole answer came
    // back as a hash.
    transcript(2, {
      kind: "assistant_text",
      text: `The push failed because the helper was never installed. I read the config at ${marker(148, HOME_DIGEST)} and the stored value is ${marker(42, TOKEN_DIGEST)}, so NIP-98 signing never ran.\n\nInside a fence the marker stays literal, because there the reader is looking at the bytes:\n\n\`\`\`\ngrep -n credential ${marker(148, HOME_DIGEST)}\n\`\`\``,
    }),
    // The Codex argv case: `tool.toolName` is the whole command as prose, so a
    // redacted interpreter path lands mid-label.
    transcript(3, {
      kind: "tool_call",
      tool: {
        toolName: `Ran ${marker(29, SHELL_DIGEST)} -lc "git push origin HEAD"`,
        toolId: "shell-1",
        input: {},
      },
    }),
    transcript(4, {
      kind: "tool_result",
      toolId: "shell-1",
      toolName: "Bash",
      content: "fatal: could not read Username for 'https://hive.agiterra.org'",
      isError: true,
    }),
    // The cap: a whole item the producer could not publish.
    transcript(5, {
      kind: "elided",
      reason: "oversize",
      byteCount: 41_235,
      contentDigest:
        "d4c3b2a1d4c3b2a1d4c3b2a1d4c3b2a1d4c3b2a1d4c3b2a1d4c3b2a1d4c3b2a1",
    }),
    transcript(6, {
      kind: "assistant_text",
      text: "Run `just install-git-credentials` and the push will authenticate.",
    }),
  ];
}

async function openSeededSession(
  page: import("@playwright/test").Page,
  seeded: RelayEvent[],
) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installMockBridge(page, {
    globalAgentConfig: {
      env_vars: {},
      provider: null,
      model: null,
      "allowed-bridge-pubkeys": [{ pubkey, label: "Screenshot provider" }],
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
  return page.getByTestId("coding-session-workspace");
}

test("a redaction reads as a pill and the raw marker never reaches prose", async ({
  page,
}) => {
  const workspace = await openSeededSession(page, elisionEvents());

  const pills = page.locator("[data-redaction-pill]");
  await expect(pills.first()).toBeVisible({ timeout: 15_000 });

  // Prose redactions, one per distinct value.
  await expect(
    page.locator('[data-elision-cause="redaction"]', {
      hasText: "redacted 148 B",
    }),
  ).toHaveCount(1);
  await expect(
    page.locator('[data-elision-cause="redaction"]', {
      hasText: "redacted 42 B",
    }),
  ).toHaveCount(1);

  // The Codex argv case: `toolName` is prose, so a redacted interpreter path
  // lands mid-label on a failed tool row — a different renderer from prose,
  // and one that showed the raw marker until it was wired.
  const failedRow = page.getByText("Tool call failed").locator("..");
  await expect(failedRow).not.toContainText("elided private context");
  await expect(failedRow.locator("[data-redaction-pill]")).toHaveCount(1);

  // Inside a fence the marker is the honest rendering, so it survives verbatim.
  await expect(
    page.locator("pre").filter({ hasText: "grep -n credential" }),
  ).toContainText("elided private context");

  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/01-redacted-prose.png` });
});

test("a redaction and a cap share the pill but never the label", async ({
  page,
}) => {
  const workspace = await openSeededSession(page, elisionEvents());

  const dropped = page.locator('[data-elision-cause="cap"]');
  await expect(dropped).toHaveCount(1, { timeout: 15_000 });
  await expect(dropped).toContainText("dropped 41 KB");
  // "Dropped", never "redacted": the two causes are unrelated and the reader
  // must be able to tell "too big to send" from "deliberately withheld".
  await expect(dropped).not.toContainText("redacted");
  await expect(page.getByText("Content dropped")).toBeVisible();

  await waitForAnimations(page);
  await workspace.screenshot({ path: `${SHOTS}/02-dropped-item.png` });
});

test("hovering a pill reveals the digest it stands for", async ({ page }) => {
  // The digest is what tells two readers whether they are looking at the same
  // hidden value, so it has to stay reachable — behind the pill, not gone.
  await openSeededSession(page, elisionEvents());

  const pill = page
    .locator('[data-elision-cause="redaction"]')
    .filter({ hasText: "redacted 148 B" })
    .first();
  await expect(pill).toBeVisible({ timeout: 15_000 });
  await pill.hover();

  const tooltip = page.getByRole("tooltip").first();
  await expect(tooltip).toContainText(`sha256:${HOME_DIGEST}`);
  await expect(tooltip).toContainText("148 B serialized");

  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/03-digest-tooltip.png`,
    clip: { x: 300, y: 120, width: 900, height: 420 },
  });
});
