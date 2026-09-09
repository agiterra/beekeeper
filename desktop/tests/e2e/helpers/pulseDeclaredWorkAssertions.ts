import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { hexToBytes } from "@noble/hashes/utils.js";
import { expect, type Locator, type Page } from "@playwright/test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_PROJECT,
  KIND_PULSE_ENTRY,
} from "@/shared/constants/kinds";

import { waitForAnimations } from "../../helpers/animations";
import { TEST_IDENTITIES } from "../../helpers/bridge";

/**
 * Machinery for the declared-work Pulse spec.
 *
 * Three things live here rather than in the spec: the seam that answers both
 * native Pulse commands, the signed relay bytes the digest is folded from, and
 * the screenshot bookkeeping that refuses to write two identical PNGs under
 * two different captions.
 *
 * The declared-work payload is **derived from the Rust-pinned fixture**
 * (`pulseDeclaredWork.fixture.json`, written by
 * `crates/buzz-core/src/pulse_declared_work_tests.rs`) rather than hand-typed:
 * every field name, vocabulary and nesting below is the producer's. Only two
 * things are rebound here — the session identity, so the response names the
 * sessions this spec actually seeded, and the settlement state of one
 * assignment, so a closed session carrying *unresolved* work exists at all.
 * That state is the one the spec is about and the pinned fixture has no
 * example of it.
 */

const here = dirname(fileURLToPath(import.meta.url));

/** Where this lane's screenshots land. Playwright wipes it each run. */
export const DECLARED_SHOT_DIR = "test-results/pulse-declared-work";

/** `general` in the mock channel fixture; `h` tags must match it exactly. */
export const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
export const GENERAL_CHANNEL_NAME = "general";
/** `DEFAULT_MOCK_IDENTITY.pubkey` in the bridge — the project's owner. */
export const MOCK_IDENTITY_PUBKEY = "deadbeef".repeat(8);
export const PROJECT_NAME = "Declared Demo";
export const PROJECT_DTAG = "declared-demo";
export const PROJECT_COORDINATE = `30621:${MOCK_IDENTITY_PUBKEY}:${PROJECT_DTAG}`;

/** The open session an assignment is running in, and its closed sibling. */
export const OPEN_SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
export const CLOSED_SESSION_REF = "8c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
/** A session whose 44244 records the fold could not read at all. */
export const UNREADABLE_SESSION_REF = "3c4d5e6f-7081-4293-a4b5-c6d7e8f90123";
/** The genesis ids the Rust fixture pins, and one for the unreadable session. */
export const OPEN_GENESIS_REF = "aa".repeat(32);
export const UNREADABLE_GENESIS_REF = "cc".repeat(32);

/** The human who posts a plan, and the agent an assignment names. */
export const HUMAN = TEST_IDENTITIES.alice;
export const AGENT = TEST_IDENTITIES.bob;

const HUMAN_SECRET = hexToBytes(HUMAN.privateKey);
const AGENT_SECRET = hexToBytes(AGENT.privateKey);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER_PUBKEY = getPublicKey(PROVIDER_SECRET);

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

type SessionTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

function sessionTarget(sessionId: string): SessionTarget {
  return {
    driver: "claude-agent-acp",
    instanceId: "declared-instance",
    sessionId,
    generation: 1,
  };
}

const OPEN_TARGET = sessionTarget("11111111-2222-3333-4444-555555555555");
const CLOSED_TARGET = sessionTarget("22222222-3333-4444-5555-666666666666");
const UNREADABLE_TARGET = sessionTarget("33333333-4444-5555-6666-777777777777");

/** One 44240 entry, really signed: the Pulse read fails closed on anything else. */
export function pulseEntry(input: {
  secret: Uint8Array;
  createdAtOffset: number;
  type: "plan" | "milestone" | "note" | "handoff" | "blocker";
  text: string;
  branch?: string | null;
  codeAreas?: string[];
  sessionRef?: string | null;
}): RelayEvent {
  const branch = input.branch ?? null;
  const tags: string[][] = [
    ["a", PROJECT_COORDINATE],
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
        supersedes: null,
      }),
    },
    input.secret,
  ) as unknown as RelayEvent;
}

/** The project head that names the channel the session facts live in. */
export function projectHeadEvent(): RelayEvent {
  return {
    id: `project-${PROJECT_DTAG}`.padEnd(64, "0"),
    pubkey: MOCK_IDENTITY_PUBKEY,
    created_at: nowSeconds() - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", PROJECT_DTAG],
      ["name", PROJECT_NAME],
      ["description", "Injected head for the declared-work read."],
      ["channel", GENERAL_CHANNEL_ID],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

function sessionAuthorityEvents(input: {
  target: SessionTarget;
  sessionRef: string;
  commandId: string;
}): RelayEvent[] {
  const command = buildCodingSessionCreateEvent({
    channelId: GENERAL_CHANNEL_ID,
    commandId: input.commandId,
    projectRef: PROJECT_COORDINATE,
    repoRef: null,
    sessionRef: input.sessionRef,
    providerInstanceRef: input.target.instanceId,
    providerAuthorityPubkey: PROVIDER_PUBKEY,
    model: "sonnet",
    title: null,
    initialTurn: null,
  });
  return [
    finalizeEvent(
      { ...command, created_at: nowSeconds() - 14_400 },
      HUMAN_SECRET,
    ) as unknown as RelayEvent,
    finalizeEvent(
      {
        kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        created_at: nowSeconds() - 14_390,
        tags: [
          ["h", GENERAL_CHANNEL_ID],
          ["cslr-v", "cslr1-1"],
          ["csl-command", input.commandId],
          ["csl-key", lifecycleReceiptSemanticKey(input.commandId)],
        ],
        content: JSON.stringify({
          schema: "buzz-coding-session-lifecycle-receipt/v1",
          commandId: input.commandId,
          status: "created",
          session: input.target,
          error: null,
        }),
      },
      PROVIDER_SECRET,
    ) as unknown as RelayEvent,
  ];
}

function sessionMetadataEvent(input: {
  target: SessionTarget;
  sessionRef: string;
  title: string;
  branch: string | null;
  createdAtOffset: number;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: nowSeconds() - input.createdAtOffset,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csm-key", codingSessionMetadataSemanticKey(input.target)],
      ],
      content: JSON.stringify({
        schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
        session: input.target,
        projectRef: PROJECT_COORDINATE,
        repoRef: null,
        title: input.title,
        agentRef: null,
        provider: "claude-agent-acp",
        runtime: "claude-agent-acp",
        model: "sonnet",
        status: "idle",
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

function sessionLeaseEvent(input: {
  target: SessionTarget;
  commandId: string;
}): RelayEvent {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LEASE,
      created_at: nowSeconds() - 30,
      tags: [
        ["h", GENERAL_CHANNEL_ID],
        ["cslease-v", "cslease1-1"],
        ["cs-target", buildCodingSessionTargetKey(input.target)],
        ["csl-command", input.commandId],
        ["cslease-seq", "1"],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-lease/v1",
        target: input.target,
        state: "live",
        leaseSequence: 1,
      }),
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

/** The 44240 entries: one plan per author, plus a note that is not regrouped. */
export function seededEntries(): RelayEvent[] {
  return [
    pulseEntry({
      secret: HUMAN_SECRET,
      createdAtOffset: 1_800,
      type: "plan",
      text: "Paging the declared-work read over visible sessions.",
      branch: "work/declared-work",
      codeAreas: ["desktop/src/features/project-pulse/lib"],
    }),
    pulseEntry({
      secret: AGENT_SECRET,
      createdAtOffset: 900,
      type: "note",
      text: "Picked up the surface lane.",
    }),
  ];
}

/** The signed session facts the digest folds into two umbrella sessions. */
export function seededSessionFacts(): RelayEvent[] {
  return [
    ...sessionAuthorityEvents({
      target: OPEN_TARGET,
      sessionRef: OPEN_SESSION_REF,
      commandId: "declared-create-open",
    }),
    sessionMetadataEvent({
      target: OPEN_TARGET,
      sessionRef: OPEN_SESSION_REF,
      title: "Declared work",
      branch: "work/declared-work-fable",
      createdAtOffset: 600,
    }),
    sessionNameEvent(OPEN_SESSION_REF, "Declared work"),
    sessionLeaseEvent({
      target: OPEN_TARGET,
      commandId: "declared-create-open",
    }),
    ...sessionAuthorityEvents({
      target: CLOSED_TARGET,
      sessionRef: CLOSED_SESSION_REF,
      commandId: "declared-create-closed",
    }),
    sessionMetadataEvent({
      target: CLOSED_TARGET,
      sessionRef: CLOSED_SESSION_REF,
      title: "Ended lane",
      branch: "work/ended-lane",
      createdAtOffset: 5_400,
    }),
    sessionNameEvent(CLOSED_SESSION_REF, "Ended lane"),
    sessionClosureEvent(CLOSED_SESSION_REF),
    // The third umbrella is a real, visible session whose 44244 records the
    // fold could not read. Seeding it keeps the scan sentence honest: a
    // response that named a session the digest never saw would make the read
    // claim it scanned more sessions than were visible.
    ...sessionAuthorityEvents({
      target: UNREADABLE_TARGET,
      sessionRef: UNREADABLE_SESSION_REF,
      commandId: "declared-create-unreadable",
    }),
    sessionMetadataEvent({
      target: UNREADABLE_TARGET,
      sessionRef: UNREADABLE_SESSION_REF,
      title: "Unreadable records",
      branch: null,
      createdAtOffset: 9_000,
    }),
    sessionNameEvent(UNREADABLE_SESSION_REF, "Unreadable records"),
  ];
}

type WireSession = Record<string, unknown> & {
  sessionKey: string;
  sessionRef: string;
  genesisRef: string;
  channelId: string;
  name: string | null;
  lifecycle: string;
  unreadable: string | null;
  assignments: Array<Record<string, unknown>>;
};
type WireResponse = Record<string, unknown> & {
  sessions: WireSession[];
  errors: Array<{ scope: string; message: string }>;
};

/** The Rust-pinned fixture, or a named failure. Never a hand-typed stand-in. */
function pinnedDeclaredWork(): WireResponse {
  const path = resolve(
    here,
    "../../../src/features/project-pulse/lib/pulseDeclaredWork.fixture.json",
  );
  if (!existsSync(path)) {
    throw new Error(
      `the Rust-pinned declared-work fixture is missing at ${path}; this spec derives its wire bytes from the producer and will not invent them`,
    );
  }
  return JSON.parse(readFileSync(path, "utf8")) as WireResponse;
}

/**
 * The response this spec's seeded project would produce.
 *
 * Session identity is rebound to the seeded sessions; the open session's
 * assignment is bound to the agent (so a name, not a hash, is under test); the
 * closed session carries two assignments — one still unresolved, one settled —
 * and a third session is unreadable. Every other field is the producer's.
 */
export function declaredWorkFixture(): WireResponse {
  const pinned = pinnedDeclaredWork();
  const [openSource, closedSource] = pinned.sessions;
  if (!openSource || !closedSource) {
    throw new Error(
      "the pinned declared-work fixture no longer carries two sessions to derive from",
    );
  }
  // The pinned fixture's timestamps are the producer's fixed test instants, so
  // every row would read "371d ago" on screen. Ages are rebound relative to
  // this run's clock — a presentation fact this spec controls anyway, since the
  // request carries `nowUnix` — while every other field stays the producer's.
  const recent = <T extends Record<string, unknown>>(
    assignment: T,
    ageSeconds: number,
  ): T => ({
    ...assignment,
    createdAt: nowSeconds() - ageSeconds,
    reports: ((assignment.reports as Array<Record<string, unknown>>) ?? []).map(
      (report) => ({ ...report, createdAt: nowSeconds() - ageSeconds + 300 }),
    ),
    dispositions: (
      (assignment.dispositions as Array<Record<string, unknown>>) ?? []
    ).map((disposition) => ({
      ...disposition,
      createdAt: nowSeconds() - ageSeconds + 600,
    })),
  });
  for (const source of [openSource, closedSource]) {
    if (typeof source.genesisRef !== "string" || source.genesisRef === "") {
      throw new Error(
        `the pinned declared-work fixture session ${source.sessionKey} carries no genesisRef; the decoder refuses a session without one`,
      );
    }
  }
  const settledAssignment = recent(closedSource.assignments[0], 7_200);
  const unresolvedAssignment = {
    ...recent(settledAssignment, 1_800),
    sourceEventId: "d1".repeat(32),
    objective: "Finish the session read while the umbrella was open.",
    reports: [],
    dispositions: [],
    settlement: {
      settled: false,
      governedReportEventId: null,
      dispositionEventId: null,
      acknowledgementEventId: null,
    },
    status: "unresolved",
  };
  return {
    ...pinned,
    viewerPubkey: MOCK_IDENTITY_PUBKEY,
    sessions: [
      {
        ...openSource,
        sessionKey: OPEN_SESSION_REF,
        sessionRef: OPEN_SESSION_REF,
        channelId: GENERAL_CHANNEL_ID,
        name: "Declared work",
        lifecycle: "open",
        assignments: openSource.assignments.map((assignment) => ({
          ...recent(assignment, 3_600),
          assignerPubkey: HUMAN.pubkey,
          assigneeActor: AGENT.pubkey,
        })),
      },
      {
        ...closedSource,
        terminal:
          closedSource.terminal === null ||
          typeof closedSource.terminal !== "object"
            ? closedSource.terminal
            : {
                ...(closedSource.terminal as Record<string, unknown>),
                at: nowSeconds() - 5_400,
              },
        sessionKey: CLOSED_SESSION_REF,
        sessionRef: CLOSED_SESSION_REF,
        channelId: GENERAL_CHANNEL_ID,
        name: "Ended lane",
        lifecycle: "closed",
        assignments: [
          {
            ...unresolvedAssignment,
            assignerPubkey: HUMAN.pubkey,
            assigneeActor: AGENT.pubkey,
          },
          {
            ...settledAssignment,
            assignerPubkey: HUMAN.pubkey,
            assigneeActor: AGENT.pubkey,
          },
        ],
      },
      {
        ...openSource,
        sessionKey: UNREADABLE_SESSION_REF,
        sessionRef: UNREADABLE_SESSION_REF,
        // Its own umbrella, so two sessions never share one genesis id.
        genesisRef: UNREADABLE_GENESIS_REF,
        channelId: GENERAL_CHANNEL_ID,
        name: "Unreadable records",
        lifecycle: "open",
        terminal: null,
        unreadable:
          "relay closed the subscription before the 44244 page finished",
        excludedCount: 0,
        assignments: [],
      },
    ],
    errors: pinned.errors,
  };
}

/**
 * Answer both native Pulse commands, leaving every other command to the bridge.
 *
 * The bridge's own command switch is not this lane's file, so the seam is a
 * trap installed on `window.__TAURI_INTERNALS__` before the bridge mounts —
 * the pattern `mockPulseMissionRows` established. Both commands are answered:
 * a declared-work spec that silently broke the missions section would be
 * testing a screen no user has.
 */
export async function mockPulseNativeReads(
  page: Page,
  payloads: { declaredWork: unknown; missionRows: unknown },
): Promise<void> {
  await page.addInitScript((answers) => {
    type Internals = Record<string, unknown> & { invoke?: unknown };
    let internals: Internals | undefined;
    const wrap = (target: Internals) => {
      if (!target || Object.hasOwn(target, "__pulseDeclaredWrapped")) return;
      let inner:
        | ((command: string, args?: unknown, options?: unknown) => unknown)
        | undefined;
      Object.defineProperty(target, "invoke", {
        configurable: true,
        get:
          () => async (command: string, args?: unknown, options?: unknown) => {
            if (command === "pulse_declared_work") {
              return structuredClone(answers.declaredWork);
            }
            if (command === "pulse_mission_rows") {
              return structuredClone(answers.missionRows);
            }
            if (!inner) {
              throw new Error(`no mocked Tauri command: ${command}`);
            }
            return inner(command, args, options);
          },
        set: (next) => {
          inner = next as typeof inner;
        },
      });
      Object.defineProperty(target, "__pulseDeclaredWrapped", { value: true });
    };
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value: Internals) => {
        internals = value;
        wrap(value);
      },
    });
  }, payloads as never);
}

/** The mission fixture, so the missions section keeps working beside this one. */
export function pulseMissionFixture(): Record<string, unknown> {
  return JSON.parse(
    readFileSync(
      resolve(
        here,
        "../../../src/features/project-pulse/lib/pulseMissionResponse.fixture.json",
      ),
      "utf8",
    ),
  ) as Record<string, unknown>;
}

const digests = new Map<string, string>();

/**
 * Capture one scoped shot and refuse a duplicate subject.
 *
 * Two byte-identical PNGs of different subjects mean the spec photographed one
 * state twice and captioned them differently — worse than no screenshot, since
 * a reviewer believes the pixels. Scoped to a locator, so two states of one
 * screen differ in the file rather than only in the name.
 */
export async function captureDeclaredShot(
  page: Page,
  locator: Locator,
  name: string,
): Promise<string> {
  mkdirSync(DECLARED_SHOT_DIR, { recursive: true });
  await expect(locator).toBeVisible();
  await waitForAnimations(page);
  const path = `${DECLARED_SHOT_DIR}/${name}.png`;
  const buffer = await locator.screenshot({ path });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of digests) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  digests.set(name, digest);
  return path;
}

/** Every shot this run took, so the spec can print the hashes it produced. */
export function declaredShotDigests(): ReadonlyMap<string, string> {
  return digests;
}

/** Root font-size zoom, the way the desktop app implements Cmd +/-. */
export async function setRootZoom(page: Page, percent: number): Promise<void> {
  await page.evaluate((value) => {
    document.documentElement.style.fontSize = `${value}%`;
  }, percent);
}
