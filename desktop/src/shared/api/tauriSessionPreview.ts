import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { invokeTauri, TauriInvokeError } from "@/shared/api/tauri";

// Mirrors the Rust commands in `src-tauri/src/session_preview/` (SV-33 S1/S2,
// contract WIRE-C4 § 3). The preview is a native WKWebView the desktop draws
// above this window's DOM at the rect the Browser surface's slot reports; the
// renderer never sees its pixels except as the freeze frame Rust hands back
// while an overlay covers it. Everything here is machine-scoped: a local
// server list, a local page. None of it is community data, and none of it
// goes to the relay.

/** The provider runtime session a preview is bound to (the composer's target). */
export type SessionPreviewTarget = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

export type SessionPreviewStatus =
  | "absent"
  | "loading"
  | "ready"
  | "closed_by_person"
  | "unavailable";

export type SessionPreviewPlacement = "docked" | "popped_out" | "none";

/**
 * Who may drive the preview (WIRE-C4 § 9.8): `none` = undriveable until an
 * agent opens it; `person` = opened from a session's surface, driven by that
 * session's grants; `agent` = opened by that execution.
 */
export type SessionPreviewBoundTo =
  | { kind: "none" }
  | { kind: "person"; sessionId: string; generation: number }
  | {
      kind: "agent";
      executionId: string;
      sessionId: string;
      generation: number;
    };

/** The agent op currently driving the preview, or null. */
export type SessionPreviewDriving = {
  executionId: string;
  sessionId: string;
  lastOp: string;
  /** Unix seconds. */
  at: number;
};

export type SessionPreviewUnavailableCode =
  | "not_macos"
  | "no_session"
  | "content_filter_failed"
  | "webview_failed";

/** `PreviewState` (WIRE-C4 § 3). */
export type SessionPreviewState = {
  channelId: string;
  status: SessionPreviewStatus;
  url: string | null;
  title: string | null;
  generation: number;
  canGoBack: boolean;
  canGoForward: boolean;
  placement: SessionPreviewPlacement;
  hidden: boolean;
  occluded: boolean;
  /** `data:image/png;base64,…` taken as the view was hidden, or null. */
  freezeFrame: string | null;
  boundTo: SessionPreviewBoundTo | null;
  driving: SessionPreviewDriving | null;
  dataStore: "per_session" | "incognito" | null;
  unavailable: { code: SessionPreviewUnavailableCode; sentence: string } | null;
};

/** One listening loopback TCP server on this machine (lsof only: never contacted; `title` is always null). */
export type SessionPreviewServer = {
  port: number;
  url: string;
  address: string;
  pid: number | null;
  process: string | null;
  title: string | null;
};

/** A slot rect in logical points, relative to its window's webview. */
export type SessionPreviewRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/** A refusal from a preview command: a stable code and the user sentence. */
export class SessionPreviewError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "SessionPreviewError";
    this.code = code;
  }
}

/**
 * Normalize whatever a preview command rejected with into a
 * `SessionPreviewError`. Rust rejects with `{code, message}`; anything else
 * keeps its message under the code `preview_error`.
 */
export function toSessionPreviewError(error: unknown): SessionPreviewError {
  if (error instanceof SessionPreviewError) return error;
  const payload =
    error instanceof TauriInvokeError ? error.payload : (error as unknown);
  if (
    payload &&
    typeof payload === "object" &&
    typeof (payload as { code?: unknown }).code === "string"
  ) {
    const raw = payload as { code: string; message?: unknown };
    return new SessionPreviewError(
      raw.code,
      typeof raw.message === "string" ? raw.message : raw.code,
    );
  }
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : "The Browser did not answer.";
  return new SessionPreviewError("preview_error", message);
}

async function call<T>(
  command: string,
  args: Record<string, unknown>,
): Promise<T> {
  try {
    return await invokeTauri<T>(command, args);
  } catch (error) {
    throw toSessionPreviewError(error);
  }
}

/** Report the slot's rect; `rect: null` means no slot is mounted here. */
export function sessionPreviewSetRect(input: {
  channelId: string;
  windowLabel: string;
  rect: SessionPreviewRect | null;
  seq: number;
}): Promise<SessionPreviewState> {
  return call("session_preview_set_rect", input);
}

/** True while an app overlay covers the slot (Rust snapshots, then hides). */
export function sessionPreviewSetOccluded(
  channelId: string,
  occluded: boolean,
): Promise<SessionPreviewState> {
  return call("session_preview_set_occluded", { channelId, occluded });
}

/**
 * Navigate (creating the preview when absent). `target` is the session the
 * surface binds the preview to: only that session's grants may drive it.
 */
export function sessionPreviewNavigate(input: {
  channelId: string;
  url: string;
  target: SessionPreviewTarget | null;
}): Promise<SessionPreviewState> {
  return call("session_preview_navigate", input);
}

export function sessionPreviewReload(
  channelId: string,
): Promise<SessionPreviewState> {
  return call("session_preview_reload", { channelId });
}

export function sessionPreviewBack(
  channelId: string,
): Promise<SessionPreviewState> {
  return call("session_preview_back", { channelId });
}

export function sessionPreviewForward(
  channelId: string,
): Promise<SessionPreviewState> {
  return call("session_preview_forward", { channelId });
}

/** The person closes the preview (`closed_by_person`). */
export function sessionPreviewClose(
  channelId: string,
): Promise<SessionPreviewState> {
  return call("session_preview_close", { channelId });
}

/** Pop out into its own window (`true`) or bring it back to the slot. */
export function sessionPreviewPopout(
  channelId: string,
  popped: boolean,
): Promise<SessionPreviewState> {
  return call("session_preview_popout", { channelId, popped });
}

/** This machine's local servers; each call keeps Rust's 3 s poll alive. */
export async function sessionPreviewServers(): Promise<SessionPreviewServer[]> {
  const result = await call<{ servers: SessionPreviewServer[] }>(
    "session_preview_servers",
    {},
  );
  return result.servers;
}

/**
 * Close every open native preview (community switch): they are keyed by the
 * old relay's channel ids, and a pop-out window must not outlive them.
 */
export async function sessionPreviewCloseAll(): Promise<void> {
  await call<{ closed: number }>("session_preview_close_all", {});
}

export function sessionPreviewStatus(
  channelId: string,
): Promise<SessionPreviewState> {
  return call("session_preview_status", { channelId });
}

export const SESSION_PREVIEW_STATE_EVENT = "session-preview://state";
export const SESSION_PREVIEW_ACTIVITY_EVENT = "session-preview://activity";
export const SESSION_PREVIEW_DRIVING_EVENT = "session-preview://driving";
export const SESSION_PREVIEW_OPEN_REQUESTED_EVENT =
  "session-preview://open-requested";

export type SessionPreviewActivity = {
  channelId: string;
  executionId: string;
  sessionId: string;
  op: string;
  at: number;
};

export type SessionPreviewDrivingEvent = {
  channelId: string;
  driving: boolean;
  executionId: string;
  sessionId: string;
  at: number;
};

/** Listen to one channel's preview events; resolves to one unlisten. */
export async function listenSessionPreview(
  channelId: string,
  handlers: {
    onState?: (state: SessionPreviewState) => void;
    onActivity?: (activity: SessionPreviewActivity) => void;
    onDriving?: (event: SessionPreviewDrivingEvent) => void;
    onOpenRequested?: (event: { channelId: string; url: string }) => void;
  },
): Promise<UnlistenFn> {
  const mine = <T extends { channelId: string }>(fn?: (value: T) => void) =>
    fn
      ? (event: { payload: T }) => {
          if (event.payload?.channelId === channelId) fn(event.payload);
        }
      : null;
  const pairs: Array<[string, ((event: { payload: never }) => void) | null]> = [
    [SESSION_PREVIEW_STATE_EVENT, mine(handlers.onState)],
    [SESSION_PREVIEW_ACTIVITY_EVENT, mine(handlers.onActivity)],
    [SESSION_PREVIEW_DRIVING_EVENT, mine(handlers.onDriving)],
    [SESSION_PREVIEW_OPEN_REQUESTED_EVENT, mine(handlers.onOpenRequested)],
  ];
  const unlistens = await Promise.all(
    pairs.flatMap(([name, handler]) =>
      handler ? [listen(name, handler as never)] : [],
    ),
  );
  return () => {
    for (const unlisten of unlistens) unlisten();
  };
}
