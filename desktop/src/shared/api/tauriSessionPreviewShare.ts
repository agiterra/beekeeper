import { listen } from "@tauri-apps/api/event";

import { toSessionPreviewError } from "@/shared/api/tauriSessionPreview";
import { invokeTauri } from "@/shared/api/tauri";

// Mirrors `src-tauri/src/session_preview/broadcast*.rs` and
// `share_commands.rs` (C5, V-CONTRACT VB → VC). Sharing a session's Browser
// with the session: Rust announces the preview (kind 30626) and publishes
// snapshots (kind 44253) over HTTP; frames (kind 24321) are signed in Rust and
// published by `SessionPreviewBroadcastPump` over the relay WebSocket.
// Errors reject as `SessionPreviewError` ({code, message}).

/** Why this Browser cannot be shared (fully). */
export type SessionPreviewShareUnavailable = {
  code: "no_session_ref" | "no_provider" | "not_macos" | string;
  sentence: string;
};

export type SessionPreviewShareState = {
  channelId: string;
  sessionRef: string | null;
  /** The person's Share toggle (default true, D5). */
  share: boolean;
  /** Last 30626 status the relay accepted from this desktop. */
  announced: "open" | "closed" | "none";
  /**
   * Share on → frames; share off → none: nothing is published (one bare
   * `closed` announce if it had been announced open).
   */
  stream: "frames" | "none";
  /** Live watcher pubkeys (hex), expired at 45 s. */
  watchers: string[];
  /** Current frame spacing (2000 base, backs off to 8000 on rate-limited:). */
  cadenceMs: number;
  framesLastMinute: number;
  /** Unix ms of the last 24321 sent. */
  lastFrameAt: number | null;
  unavailable: SessionPreviewShareUnavailable | null;
};

export const SESSION_PREVIEW_SHARE_STATE_EVENT =
  "session-preview://share-state";
export const SESSION_PREVIEW_BROADCAST_PUBLISH_EVENT =
  "session-preview://broadcast-publish";

/** Payload of `session-preview://broadcast-publish`. */
export type SessionPreviewBroadcastPublish = {
  channelId: string;
  /** A signed kind 24321 event, as JSON text. */
  event: string;
};

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

/** Configure sharing for a channel's preview; returns the share state. */
export function sessionPreviewShareConfigure(input: {
  channelId: string;
  sessionRef: string | null;
  share: boolean;
  providerPubkey: string | null;
}): Promise<SessionPreviewShareState> {
  return call("session_preview_share_configure", input);
}

export function sessionPreviewShareState(
  channelId: string,
): Promise<SessionPreviewShareState> {
  return call("session_preview_share_state", { channelId });
}

/** The camera button: publish a 44253 of the preview now. */
export function sessionPreviewShareSnapshot(input: {
  channelId: string;
  alt?: string | null;
}): Promise<{ eventId: string; url: string; sha256: string }> {
  return call("session_preview_share_snapshot", {
    channelId: input.channelId,
    alt: input.alt ?? null,
  });
}

/** Listen to one channel's share state; resolves to its unlisten. */
export async function listenSessionPreviewShare(
  channelId: string,
  onState: (state: SessionPreviewShareState) => void,
): Promise<() => void> {
  return listen<SessionPreviewShareState>(
    SESSION_PREVIEW_SHARE_STATE_EVENT,
    (event) => {
      if (event.payload?.channelId === channelId) onState(event.payload);
    },
  );
}

/** Pump only: forward a 24320 addressed to this desktop. */
export function sessionPreviewShareWatch(input: {
  channelId: string;
  sessionRef: string;
  watcherPubkey: string;
  action: "watch" | "stop" | "resync" | "snapshot";
}): Promise<void> {
  return call("session_preview_share_watch", input);
}

/** Pump only: report the relay's OK for a frame the pump published. */
export function sessionPreviewShareNotePublish(input: {
  channelId: string;
  accepted: boolean;
  message: string;
}): Promise<void> {
  return call("session_preview_share_note_publish", input);
}
