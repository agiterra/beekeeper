import { createHash } from "node:crypto";

import { expect, type Locator, type Page } from "@playwright/test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";
import { hexToBytes } from "@noble/hashes/utils.js";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { buildCodingSessionCreateEvent } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
export { WORKSPACE_REUSE_SEAM_LANDED } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";

import { waitForAnimations } from "../../helpers/animations";

/**
 * Fixtures and seams for "New session in this workspace" (Lane V).
 *
 * Everything here answers one question: *what did the app do*, not *what is
 * it wired to*. The host commands the feature reads are stubbed in the
 * spec's own init script rather than in `src/testing/e2eBridge.ts` — that
 * file is over the size ratchet, and the house rule for host commands is
 * opt-in-or-throw, which a spec-local stub honours exactly.
 *
 * The workdir stub is a *live* store, not a constant: `record_..._use` and
 * `stage_..._hint` mutate it the way `workdir_store.rs` does. That is what
 * makes "nothing is remembered" a behaviour assertion — if the app records
 * the one-off directory, the next ordinary draft prefills it and the test
 * fails on the visible consequence, not on a spy count alone.
 */

/** The bridge's own known identity, so seeded genesis signatures verify. */
export const FOUNDER_SECRET_HEX =
  "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03";
export const FOUNDER_SECRET = hexToBytes(FOUNDER_SECRET_HEX);
export const FOUNDER_PUBKEY = getPublicKey(FOUNDER_SECRET);
export const FOUNDER_IDENTITY = {
  privateKey: FOUNDER_SECRET_HEX,
  pubkey: FOUNDER_PUBKEY,
  username: "tyler",
};

/** `general` in the mock channel fixture; the `h` tag must match exactly. */
export const CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
export const CHANNEL_NAME = "general";

export const MOCK_PROJECT_OWNER = "deadbeef".repeat(8);
export const PROJECT_DTAG = "buzz";
export const PROJECT_REF = `30621:${MOCK_PROJECT_OWNER}:${PROJECT_DTAG}`;

/** This computer's provider. Anything else is a foreign host. */
export const LOCAL_PROVIDER_SECRET = hexToBytes(
  "5b1f0a4c2d3e4f50617283940a1b2c3d4e5f60718293a4b5c6d7e8f901234567",
);
export const LOCAL_PROVIDER_PUBKEY = getPublicKey(LOCAL_PROVIDER_SECRET);
export const FOREIGN_PROVIDER_SECRET = hexToBytes(
  "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809",
);
export const FOREIGN_PROVIDER_PUBKEY = getPublicKey(FOREIGN_PROVIDER_SECRET);

export const LOCAL_INSTANCE_ID = "0123456789abcdef";

/** The canonical checkout an ordinary launch must keep offering. */
export const CANONICAL_REPO = "/Users/mock/Code/beekeeper";
/** The one session's recorded workspace. */
export const REUSE_PATH = "/Users/mock/Code/beekeeper.worktrees/repo-wt-a";
export const REUSE_BRANCH = "repo-wt-a";

export const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
export const SESSION_TITLE = "Fix the reconnect bug";
const COMMAND_ID = "csl-workspace-reuse-session";
const BASE_CREATED_AT = 1_800_000_000;

export type CommandRecord = { command: string; args: unknown };

/** A recorded seat worktree, exactly as `list_coding_session_seat_worktrees`. */
export function seatWorktreeRow(input: {
  sessionRef: string;
  path: string;
  branch: string | null;
  exists?: boolean;
  seatLabel?: string;
}) {
  const seatLabel = input.seatLabel ?? "builder-1";
  return {
    key: `${input.sessionRef}/${seatLabel}`,
    sessionRef: input.sessionRef,
    seatLabel,
    path: input.path,
    branch: input.branch,
    repoRoot: CANONICAL_REPO,
    disposition: "within-grace",
    dirtyFiles: 2,
    reclaimableBytes: null,
    reclaimableLabel: "unknown",
    reclaimableNow: false,
    graceRemainingSecs: 7 * 24 * 60 * 60,
    exists: input.exists ?? true,
    tipOnRelayKnown: true,
    detail: `${input.path}: held: 2 uncommitted files`,
  };
}

export type WorkspaceReuseStubConfig = {
  /** Rows the host answers with for every seat-worktree read. */
  seatWorktrees: Record<string, unknown>[];
  /** Answer for `validate_coding_session_workdir`, by path. */
  validationByPath: Record<
    string,
    { exists: boolean; isDir: boolean; isAbsolute: boolean }
  >;
  /** The fallback answer for any path not named above. */
  defaultValidation: { exists: boolean; isDir: boolean; isAbsolute: boolean };
  /** Seeds the MRU so an ordinary draft has a canonical checkout to offer. */
  canonicalRepo: string;
  /** Result of the OS folder picker, when a spec ever clicks it. */
  pickResult: string | null;
};

export function defaultStubConfig(
  overrides: Partial<WorkspaceReuseStubConfig> = {},
): WorkspaceReuseStubConfig {
  return {
    seatWorktrees: [],
    validationByPath: {},
    defaultValidation: { exists: false, isDir: false, isAbsolute: true },
    canonicalRepo: CANONICAL_REPO,
    pickResult: null,
    ...overrides,
  };
}

/**
 * Wrap the bridge's `invoke` before it is installed.
 *
 * Same shape `coding-session-launch-form.spec.ts` uses: define the property
 * on `window.__TAURI_INTERNALS__` so the wrapper is in place whichever order
 * the app and the bridge mount in. Every command is recorded, including the
 * ones that fall through to the bridge — scenario 3's allowlist depends on
 * the log being complete rather than selective.
 */
export function workspaceReuseInitScript() {
  return (input: WorkspaceReuseStubConfig) => {
    type Invoke = (
      cmd: string,
      args?: Record<string, unknown>,
      options?: unknown,
    ) => Promise<unknown>;

    const log: { command: string; args: unknown }[] = [];
    const state = {
      version: 1,
      byProject: {} as Record<string, { path: string; updatedAt: string }>,
      byChannel: {} as Record<string, { path: string; updatedAt: string }>,
      mru: [{ path: input.canonicalRepo, lastUsedAt: "2026-09-09T00:00:00Z" }],
      pending: {} as Record<string, string>,
    };
    const globals = window as unknown as {
      __WR_COMMANDS__: typeof log;
      __WR_STATE__: typeof state;
    };
    globals.__WR_COMMANDS__ = log;
    globals.__WR_STATE__ = state;
    const snapshot = () => JSON.parse(JSON.stringify(state)) as typeof state;

    let internals: Record<string, unknown> | undefined;
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      get: () => internals,
      set: (value: Record<string, unknown>) => {
        internals = value;
        let real: Invoke | undefined;
        const wrapped: Invoke = async (cmd, args, options) => {
          // Recorded before the switch, so a stubbed command and a
          // fall-through command are equally visible to the allowlist.
          log.push({ command: cmd, args: args ?? null });
          switch (cmd) {
            case "get_coding_session_workdir_state":
              return snapshot();
            case "record_coding_session_workdir_use": {
              const path = (args as { path: string }).path;
              state.mru = [
                { path, lastUsedAt: "2026-09-09T12:00:00Z" },
                ...state.mru.filter((entry) => entry.path !== path),
              ].slice(0, 10);
              return snapshot();
            }
            case "stage_coding_session_create_hint": {
              const staged = args as {
                commandId: string;
                path: string;
                projectRef: string | null;
              };
              state.pending[staged.commandId] = staged.path;
              // `workdir_store.rs` `stage_hint_for_project`: a project with
              // no default yet acquires one. Reproduced here because that is
              // exactly the remembering §3 forbids for a reuse launch.
              if (staged.projectRef && !state.byProject[staged.projectRef]) {
                state.byProject[staged.projectRef] = {
                  path: staged.path,
                  updatedAt: "2026-09-09T12:00:00Z",
                };
              }
              return snapshot();
            }
            case "clear_coding_session_create_hint": {
              const { commandId } = args as { commandId: string };
              delete state.pending[commandId];
              return snapshot();
            }
            case "validate_coding_session_workdir": {
              const path = (args as { path: string }).path;
              return input.validationByPath[path] ?? input.defaultValidation;
            }
            case "list_coding_session_seat_worktrees":
              return input.seatWorktrees;
            case "list_coding_session_worktree_branches":
              // Unconfigured on purpose: the live-head read must be allowed
              // to fail, and the recorded branch is then the honest answer.
              throw new Error(`Unsupported mocked Tauri command: ${cmd}`);
            case "pick_coding_session_workdir":
              return input.pickResult;
            case "stage_coding_session_actor_seat":
              return { packStaged: true, packRef: null };
            case "clear_coding_session_actor_seat":
              return null;
            default:
              break;
          }
          if (!real) throw new Error("mock invoke is not installed yet");
          return real(cmd, args, options);
        };
        Object.defineProperty(value, "invoke", {
          configurable: true,
          get: () => (real ? wrapped : undefined),
          set: (fn: Invoke) => {
            real = fn;
          },
        });
      },
    });
  };
}

/** Every Tauri command the app has issued, in order. */
export async function recordedCommands(page: Page): Promise<CommandRecord[]> {
  return page.evaluate(
    () =>
      (window as unknown as { __WR_COMMANDS__?: CommandRecord[] })
        .__WR_COMMANDS__ ?? [],
  );
}

/** Every event this app has signed, in order. */
export async function signedEvents(
  page: Page,
): Promise<Array<{ kind: number; content: string; tags: string[][] }>> {
  return page.evaluate(
    () =>
      (window.__BEEKEEPER_E2E_SIGNED_EVENTS__ ?? []) as Array<{
        kind: number;
        content: string;
        tags: string[][];
      }>,
  );
}

export function hintCalls(commands: CommandRecord[]) {
  return commands
    .filter((entry) => entry.command === "stage_coding_session_create_hint")
    .map(
      (entry) =>
        entry.args as {
          commandId: string;
          path: string;
          projectRef: string | null;
        },
    );
}

export function workdirUseCalls(commands: CommandRecord[]) {
  return commands
    .filter((entry) => entry.command === "record_coding_session_workdir_use")
    .map((entry) => (entry.args as { path: string }).path);
}

export function commandNames(commands: CommandRecord[]): string[] {
  return [...new Set(commands.map((entry) => entry.command))].sort();
}

/**
 * The signed 44221 `session.create` actions, decoded.
 *
 * A create is the one place the reused directory has to arrive intact, and
 * its `commandId` is the key the staged hint is filed under — so the two are
 * only provably the same launch when both are read from the same run.
 */
export function createActions(
  events: Array<{ kind: number; content: string }>,
): Array<{
  commandId: string;
  sessionRef: string | null;
  projectRef: string | null;
}> {
  const out: Array<{
    commandId: string;
    sessionRef: string | null;
    projectRef: string | null;
  }> = [];
  for (const event of events) {
    if (event.kind !== 44221) continue;
    try {
      const payload = JSON.parse(event.content) as {
        commandId?: string;
        action?: { type?: string; sessionRef?: string; projectRef?: string };
      };
      if (payload.action?.type !== "session.create") continue;
      out.push({
        commandId: payload.commandId ?? "",
        sessionRef: payload.action.sessionRef ?? null,
        projectRef: payload.action.projectRef ?? null,
      });
    } catch {
      // A create this test cannot decode is a failure of the fixture, not a
      // silent pass: leave it out and let the count assertion speak.
    }
  }
  return out;
}

/** Signed events that name a given session ref anywhere in their bytes. */
export function eventsMentioning(
  events: Array<{ kind: number; content: string; tags: string[][] }>,
  needle: string,
) {
  return events.filter((event) =>
    JSON.stringify({ content: event.content, tags: event.tags }).includes(
      needle,
    ),
  );
}

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

/**
 * One settled coding session on the wire: genesis, create, receipt, metadata.
 *
 * `providerSecret` decides whether the execution belongs to this computer —
 * the foreign-host case differs only in which key signs the receipt and the
 * metadata, which is exactly the fact the resolution is supposed to read.
 */
export function seededSessionEvents(input: {
  sessionRef?: string;
  title?: string;
  providerSecret?: Uint8Array;
  providerPubkey?: string;
  instanceId?: string;
  sessionId?: string;
}): { events: RelayEvent[]; targetKey: string } {
  const sessionRef = input.sessionRef ?? SESSION_REF;
  const providerSecret = input.providerSecret ?? LOCAL_PROVIDER_SECRET;
  const providerPubkey = input.providerPubkey ?? LOCAL_PROVIDER_PUBKEY;
  const instanceId = input.instanceId ?? LOCAL_INSTANCE_ID;
  const target = {
    driver: "claude-agent-acp",
    instanceId,
    sessionId: input.sessionId ?? "11111111-2222-3333-4444-555555555555",
    generation: 1,
  };
  const targetKey = buildCodingSessionTargetKey(target);
  const commandId = `${COMMAND_ID}-${sessionRef.slice(0, 8)}`;
  const title = input.title ?? SESSION_TITLE;

  const builtGenesis = buildCodingSessionGenesisEvent({
    channelId: CHANNEL_ID,
    sessionRef,
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
    commandId,
    projectRef: PROJECT_REF,
    repoRef: null,
    sessionRef,
    genesisRef: genesis.id,
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: providerPubkey,
    model: "sonnet",
    title,
    initialTurn: null,
  });
  return {
    targetKey,
    events: [
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
          ["csl-command", commandId],
          ["csl-key", lifecycleReceiptSemanticKey(commandId)],
        ],
        JSON.stringify({
          schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
          commandId,
          status: "created",
          session: target,
          error: null,
        }),
        providerSecret,
      ),
      signed(
        KIND_CODING_SESSION_METADATA,
        BASE_CREATED_AT,
        [
          ["h", CHANNEL_ID],
          ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
          ["cs-target", targetKey],
          ["csm-key", codingSessionMetadataSemanticKey(target)],
        ],
        JSON.stringify({
          schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
          session: target,
          projectRef: PROJECT_REF,
          repoRef: null,
          title,
          agentRef: null,
          provider: "claude-agent-acp",
          runtime: "claude-agent-acp",
          model: "sonnet",
          status: "running",
          branch: REUSE_BRANCH,
          capabilities: {
            threadTurnStart: true,
            threadTurnInterrupt: true,
            threadSteer: true,
            context: false,
            diff: false,
            plan: true,
          },
          sessionRef,
        }),
        providerSecret,
      ),
    ],
  };
}

export async function seedEvents(page: Page, events: RelayEvent[]) {
  await page.evaluate(
    ({ channelName, seeds }) => {
      const seed = window.__BEEKEEPER_E2E_SEED_MOCK_SIGNED_EVENT__;
      if (!seed) throw new Error("signed-event seeding hook is missing");
      for (const event of seeds as never[]) seed({ channelName, event });
    },
    { channelName: CHANNEL_NAME, seeds: events as never },
  );
}

/* ── screenshots ─────────────────────────────────────────────────────────── */

const hashes = new Map<string, string>();

/**
 * Scoped capture with a pairwise-distinct hash gate.
 *
 * Two "different" screenshots of the same pixels is the most common way a
 * screenshot pack lies, so the gate is in the spec rather than in a later
 * `shasum` a person has to remember to run.
 */
export async function captureLocator(
  page: Page,
  locator: Locator,
  dir: string,
  name: string,
): Promise<string> {
  await waitForAnimations(page);
  const path = `${dir}/${name}.png`;
  const buffer = await locator.screenshot({ path });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
  return digest;
}
