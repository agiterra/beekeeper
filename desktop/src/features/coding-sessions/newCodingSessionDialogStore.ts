import * as React from "react";

/**
 * Who has the "new coding session" dialog open, and for what.
 *
 * Creating a session used to be a route. As a dialog it needs somewhere for
 * that intent to live, and a module store is the same shape the agent-card
 * and mint surfaces already use.
 *
 * The request is mirrored into `sessionStorage` on purpose. The route it
 * replaced pinned its channel into the URL so that reloading mid-create
 * re-attached to the durable transaction still in flight; a dialog has no URL
 * to pin, so the request itself is what survives the reload. It is
 * session-scoped, not local: a create abandoned days ago should not reopen a
 * dialog on the next launch.
 */

/** What the dialog is being opened for. */
export type NewCodingSessionRequest =
  | {
      kind: "channel";
      /** Pre-selected channel, or null to let the picker choose a default. */
      channelId: string | null;
    }
  | { kind: "project"; projectId: string };

const STORAGE_KEY = "buzz.new-coding-session-dialog.v1";

let request: NewCodingSessionRequest | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

function storage(): Pick<Storage, "getItem" | "setItem" | "removeItem"> | null {
  try {
    return window.sessionStorage;
  } catch {
    // Storage can be denied outright (private mode, hardened webview). The
    // dialog still works; only reload-survival is lost.
    return null;
  }
}

function persist(next: NewCodingSessionRequest | null) {
  const store = storage();
  if (!store) return;
  try {
    if (next === null) store.removeItem(STORAGE_KEY);
    else store.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // A full or read-only store is not worth failing an open over.
  }
}

/** Decode a stored request, rejecting anything that is not one. */
export function parseNewCodingSessionRequest(
  raw: string | null,
): NewCodingSessionRequest | null {
  if (raw === null) return null;
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    if (!("kind" in parsed)) return null;
    if (
      parsed.kind === "channel" &&
      "channelId" in parsed &&
      (typeof parsed.channelId === "string" || parsed.channelId === null)
    ) {
      return { kind: "channel", channelId: parsed.channelId };
    }
    if (
      parsed.kind === "project" &&
      "projectId" in parsed &&
      typeof parsed.projectId === "string" &&
      parsed.projectId.length > 0
    ) {
      return { kind: "project", projectId: parsed.projectId };
    }
    return null;
  } catch {
    return null;
  }
}

let restored = false;

/** Open the dialog for a channel — or for no particular channel. */
export function openNewCodingSessionDialog(channelId?: string | null): void {
  restored = true;
  request = { kind: "channel", channelId: channelId ?? null };
  persist(request);
  emit();
}

/** Open the dialog for a project, which decides the channel for itself. */
export function openNewProjectCodingSessionDialog(projectId: string): void {
  restored = true;
  request = { kind: "project", projectId };
  persist(request);
  emit();
}

export function closeNewCodingSessionDialog(): void {
  restored = true;
  if (request === null) return;
  request = null;
  persist(null);
  emit();
}

/**
 * Drop the open request when the community changes.
 *
 * Wired into `resetCommunityState()`: a channel id belongs to one relay, and
 * a dialog left open across a switch would be pointed at a channel that no
 * longer exists.
 */
export function resetNewCodingSessionDialog(): void {
  restored = true;
  request = null;
  persist(null);
  emit();
}

function snapshot(): NewCodingSessionRequest | null {
  if (!restored) {
    restored = true;
    const store = storage();
    let raw: string | null = null;
    try {
      raw = store?.getItem(STORAGE_KEY) ?? null;
    } catch {
      raw = null;
    }
    request = parseNewCodingSessionRequest(raw);
  }
  return request;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The open request, or null when the dialog is closed. */
export function useNewCodingSessionRequest(): NewCodingSessionRequest | null {
  return React.useSyncExternalStore(subscribe, snapshot, () => null);
}
