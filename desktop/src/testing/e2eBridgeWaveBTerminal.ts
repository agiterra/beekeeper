import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock Tauri commands for the session terminal drawer (SV-25) and its badge
 * (SV-22), owned by lane B4.
 *
 * Tried before the bridge's built-in `switch` (`e2eBridge.ts`). Inert unless
 * a spec declares `window.__BEEKEEPER_E2E_WAVE_B_TERMINAL__` before the app loads,
 * so every other spec keeps the bridge's own shell mocks.
 *
 * The declared object is live state the spec can change mid-test:
 *
 * - `tree`: what `resolve_coding_session_tree` answers — `"local"` (this
 *   session's worktree) or `"elsewhere"` (another computer's provider runs
 *   it and this host cut no tree).
 * - `runningShellIds`: shells the foreground read reports as running a
 *   command; everything else reads as at the prompt.
 * - `announces`: kind:30623 events a `query_relay_filters` read for the
 *   project's shared terminals returns (one explicit batch, as
 *   `useSessionSharedTerminals` sends it).
 *
 * Shells created here are kept in `shells` on the same object, so a spec can
 * read what the drawer opened. The renderer-facing shape matches the host's:
 * a session shell's `currentDirectory` is empty.
 */
export type WaveBTerminalMockState = {
  tree: "local" | "elsewhere";
  runningShellIds: string[];
  announces: Array<{ kind: number; tags: string[][] }>;
  shells?: Array<Record<string, unknown>>;
  nextShell?: number;
};

declare global {
  interface Window {
    __BEEKEEPER_E2E_WAVE_B_TERMINAL__?: WaveBTerminalMockState;
  }
}

function state(): WaveBTerminalMockState | null {
  if (typeof window === "undefined") return null;
  const value = window.__BEEKEEPER_E2E_WAVE_B_TERMINAL__;
  if (!value) return null;
  value.shells ??= [];
  value.nextShell ??= 1;
  return value;
}

function handled(value: unknown): WaveBMockCommandResult {
  return { handled: true, value };
}

type Filter = { kinds?: number[]; "#a"?: string[] };

const PROMPT_B64 = typeof btoa === "function" ? btoa("$ ") : "";

export async function handleWaveBTerminalMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  const mock = state();
  if (!mock) return null;
  const shells = mock.shells ?? [];
  switch (command) {
    case "resolve_coding_session_tree":
      return handled(
        mock.tree === "local"
          ? {
              available: true,
              source: "session",
              label: "this session's worktree",
              reason: null,
              refusal: null,
            }
          : {
              available: false,
              source: null,
              label: "no working tree",
              reason: "The working tree is on another computer.",
              refusal: "notLocal",
            },
      );
    case "list_shell_sessions":
      return handled(shells.map((shell) => ({ ...shell })));
    case "create_shell_session": {
      const input = (payload ?? {}) as {
        title?: string | null;
        codingSession?: Record<string, unknown> | null;
        projectRef?: string | null;
      };
      if (input.codingSession && mock.tree !== "local") {
        throw new Error(
          "No working tree for this session is recorded on this computer.",
        );
      }
      const id = `wave-b-shell-${mock.nextShell ?? 1}`;
      mock.nextShell = (mock.nextShell ?? 1) + 1;
      const shell = {
        sessionId: id,
        title: input.title ?? "Terminal",
        currentDirectory: input.codingSession ? "" : "/Users/you",
        shell: "/bin/zsh",
        createdAt: 1_800_700_000 + shells.length,
        rows: 24,
        cols: 80,
        running: true,
        restorable: false,
        projectRef: input.projectRef ?? null,
        shared: true,
        roster: [],
        codingSession: input.codingSession ?? null,
      };
      shells.push(shell);
      mock.shells = shells;
      return handled({ ...shell });
    }
    case "close_shell_session": {
      const id = (payload as { sessionId?: string } | null)?.sessionId;
      mock.shells = shells.filter((shell) => shell.sessionId !== id);
      mock.runningShellIds = mock.runningShellIds.filter(
        (running) => running !== id,
      );
      return handled(null);
    }
    case "attach_shell_session":
      return handled(PROMPT_B64);
    case "shell_sessions_foreground": {
      const ids =
        (payload as { sessionIds?: string[] } | null)?.sessionIds ?? [];
      return handled(
        ids.map((sessionId) => ({
          sessionId,
          runningCommand: mock.runningShellIds.includes(sessionId),
        })),
      );
    }
    case "query_relay_filters": {
      const filters = (payload as { filters?: Filter[] } | null)?.filters;
      if (
        !filters ||
        filters.length === 0 ||
        !filters.every(
          (filter) => filter.kinds?.length === 1 && filter.kinds[0] === 30623,
        )
      ) {
        return null;
      }
      const addresses = new Set(filters.flatMap((f) => f["#a"] ?? []));
      return handled(
        mock.announces.filter((event) =>
          event.tags.some(
            (tag) => tag[0] === "a" && addresses.has(tag[1] ?? ""),
          ),
        ),
      );
    }
    default:
      return null;
  }
}
