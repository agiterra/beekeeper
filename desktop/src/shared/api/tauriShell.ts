import { invokeTauri } from "@/shared/api/tauri";

// Mirrors the Rust commands in `src-tauri/src/commands/shell_sessions.rs` —
// the "Built-in Shell" experiment. Sessions are PTYs hosted by the desktop
// process itself. WRITES require the owner's per-session interaction consent
// (sessionConsent.ts). Agent access goes through the local session broker
// under the workspace id `shell:<sessionId>`, gated by the backend-held
// per-session agent consent below.

export type ShellSessionInfo = {
  sessionId: string;
  title: string;
  currentDirectory: string;
  shell: string;
  /** Unix seconds the session was created. */
  createdAt: number;
  rows: number;
  cols: number;
  /** False once the shell process has exited. */
  running: boolean;
  /** True for a session restored from disk with no live shell yet — resume it
   * to respawn a shell in its saved directory with history replayed. */
  restorable: boolean;
  /** Project container coordinate (`30621:<owner>:<slug>`) this session is
   * grouped under in the sidebar. A session with a real project ref is
   * announced to that project per NIP-ST. */
  projectRef?: string | null;
  /** Whether project members may observe this session read-only (NIP-ST).
   * Default on; meaningless without a projectRef. */
  shared?: boolean;
};

/** The broker/consent workspace id for a built-in shell session. */
export function shellWorkspaceId(sessionId: string): string {
  return `shell:${sessionId}`;
}

/** Spawn a new shell session (defaults: `$SHELL`, `$HOME`). */
export function createShellSession(options?: {
  cwd?: string;
  title?: string;
  command?: string;
  projectRef?: string | null;
}): Promise<ShellSessionInfo> {
  return invokeTauri<ShellSessionInfo>("create_shell_session", {
    cwd: options?.cwd ?? null,
    title: options?.title ?? null,
    command: options?.command ?? null,
    projectRef: options?.projectRef ?? null,
  });
}

/** Move a shell session into a project (or clear with null). Local-only. */
export function setShellSessionProject(
  sessionId: string,
  projectRef: string | null,
): Promise<void> {
  return invokeTauri("set_shell_session_project", { sessionId, projectRef });
}

/** All built-in shell sessions, oldest first (live + restorable). */
export function listShellSessions(): Promise<ShellSessionInfo[]> {
  return invokeTauri<ShellSessionInfo[]>("list_shell_sessions");
}

/** Bring a restorable session back to life in its saved directory. */
export function resumeShellSession(
  sessionId: string,
): Promise<ShellSessionInfo> {
  return invokeTauri<ShellSessionInfo>("resume_shell_session", { sessionId });
}

/** Kill the shell (if running) and remove the session (and its saved history). */
export function closeShellSession(sessionId: string): Promise<void> {
  return invokeTauri("close_shell_session", { sessionId });
}

/** Rename a session. The new title persists across restarts; empty is rejected. */
export function renameShellSession(
  sessionId: string,
  title: string,
): Promise<void> {
  return invokeTauri("rename_shell_session", { sessionId, title });
}

/** Whether shell sessions are persisted across restarts (default on). */
export function shellPersistenceEnabled(): Promise<boolean> {
  return invokeTauri<boolean>("shell_persistence_enabled");
}

/** Turn session persistence on or off (off purges saved history). */
export function setShellPersistenceEnabled(enabled: boolean): Promise<void> {
  return invokeTauri("set_shell_persistence_enabled", { enabled });
}

/** WRITE keystrokes into the PTY. Interaction-consent contract applies. */
export function writeShellSession(
  sessionId: string,
  data: string,
): Promise<void> {
  return invokeTauri("write_shell_session", { sessionId, data });
}

/** Resize the PTY to match the rendered terminal. */
export function resizeShellSession(
  sessionId: string,
  rows: number,
  cols: number,
): Promise<void> {
  return invokeTauri("resize_shell_session", { sessionId, rows, cols });
}

/** ANSI-stripped text snapshot (read-only; same surface agents read). */
export function readShellSession(
  sessionId: string,
  scrollback: boolean,
): Promise<string> {
  return invokeTauri<string>("read_shell_session", { sessionId, scrollback });
}

/** Raw scrollback as base64, for terminal replay on (re)attach. Read-only. */
export function attachShellSession(sessionId: string): Promise<string> {
  return invokeTauri<string>("attach_shell_session", { sessionId });
}

/** A pending agent access request awaiting the owner's decision. */
export type ShellAccessRequest = {
  id: string;
  workspaceId: string;
  sessionTitle: string | null;
  /** The single command the agent wants to run, if it named one. */
  command: string | null;
  reason: string | null;
  /** The requesting agent's npub. */
  caller: string | null;
};

export type ShellAccessDecision = "once" | "full" | "deny";

/** Pending access requests, for a UI mounting after one arrived. */
export function listShellAccessRequests(): Promise<ShellAccessRequest[]> {
  return invokeTauri<ShellAccessRequest[]>("list_shell_access_requests");
}

/** Answer a pending access request, waking the agent's blocked call. */
export function resolveShellAccessRequest(
  requestId: string,
  decision: ShellAccessDecision,
): Promise<void> {
  return invokeTauri("resolve_shell_access_request", { requestId, decision });
}

export const SHELL_ACCESS_REQUEST_EVENT = "shell-access-request";
export const SHELL_ACCESS_REQUEST_RESOLVED_EVENT =
  "shell-access-request-resolved";

/** Payload of the `shell-session-output` Tauri event. */
export type ShellSessionOutputEvent = {
  sessionId: string;
  dataB64: string;
};

/** Payload of the `shell-session-exit` Tauri event. */
export type ShellSessionExitEvent = {
  sessionId: string;
};

export const SHELL_SESSION_OUTPUT_EVENT = "shell-session-output";
export const SHELL_SESSION_EXIT_EVENT = "shell-session-exit";

// ── Agent-interaction consent. Backend-held (not localStorage): the session
// broker enforces it, so agents can drive only sessions the owner has allowed
// here. Default off, per session.

/** Allow or disallow agents to drive a session. */
export function setSessionAgentConsent(
  workspaceId: string,
  allowed: boolean,
): Promise<void> {
  return invokeTauri("set_session_agent_consent", { workspaceId, allowed });
}

/** Workspace ids agents are currently allowed to drive. */
export function listSessionAgentConsent(): Promise<string[]> {
  return invokeTauri<string[]>("list_session_agent_consent");
}

// ── NIP-ST shared terminals (project members observe read-only) ──────────

/** Flip a session's share flag. Off retracts the announce + ends streams. */
export function setShellSessionShared(
  sessionId: string,
  shared: boolean,
): Promise<void> {
  return invokeTauri("set_shell_session_shared", { sessionId, shared });
}

/** Forward a validated watch event to the broadcaster; returns signed
 * attach-bundle frame events (JSON strings) to publish. */
export function shellBroadcastWatch(
  sessionId: string,
  watcherPubkey: string,
  action: "watch" | "stop" | "resync",
): Promise<string[]> {
  return invokeTauri<string[]>("shell_broadcast_watch", {
    sessionId,
    watcherPubkey,
    action,
  });
}

/** Pubkeys currently watching a session (pull fallback for the indicator). */
export function shellBroadcastWatchers(sessionId: string): Promise<string[]> {
  return invokeTauri<string[]>("shell_broadcast_watchers", { sessionId });
}

/** Build + sign a kind:24310 watch event for a session we want to observe. */
export function buildShellWatchEvent(input: {
  ownerPubkey: string;
  sessionId: string;
  projectRef: string;
  action: "watch" | "stop" | "resync";
}): Promise<string> {
  return invokeTauri<string>("build_shell_watch_event", { ...input });
}

/** Signed frame events ready to publish over the relay WebSocket. */
export const SHELL_BROADCAST_PUBLISH_EVENT = "shell-broadcast-publish";
/** `{ sessionId, watchers }` roster updates for the owner's indicator. */
export const SHELL_BROADCAST_WATCHERS_EVENT = "shell-broadcast-watchers";
