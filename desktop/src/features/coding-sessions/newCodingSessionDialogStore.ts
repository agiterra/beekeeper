import * as React from "react";

import type { NewCodingSessionWorkspaceReuse } from "./ui/NewCodingSessionDialog";

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
  | { kind: "project"; projectId: string }
  /**
   * Reuse one session's existing checkout for a *new* conversation.
   *
   * The workspace travels with the request because nothing downstream can
   * re-derive it: the launcher's prefill order puts a channel's remembered
   * folder above any fallback, so a request that carried only the session ref
   * would silently open on the channel's directory instead of the one the
   * person chose. `channelId`/`projectId` are where the new session lands,
   * which is a separate question from where it runs.
   *
   * What may be persisted here is decided by one question: can this still be
   * true after a reload? The request is written to `sessionStorage` and
   * re-read on the next launch, so anything with a clock in it becomes a lie
   * in storage.
   *
   * - `path` and `branch` are what the worktree was cut as. They stay.
   * - `branchSource` may be persisted **only when it is `"recorded"`** — the
   *   branch a worktree was created on is a creation-time fact and does not
   *   go stale. `"live"` is never stored: "on disk now" restored from
   *   storage would be a claim about the present made from a record, so the
   *   draft reads the head itself, on open.
   * - `alsoHere` — the other sessions recorded at this directory — must not
   *   be added. It is not the `branchSource` case: it is a count taken at one
   *   moment, and a stale "3 other sessions are here" is worse than not
   *   saying it. That line belongs where it is resolved fresh, on the menu
   *   that opens this draft.
   */
  | {
      kind: "workspace";
      channelId: string | null;
      projectId: string | null;
      sessionRef: string;
      workspace: NewCodingSessionWorkspaceReuse;
    };

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

/**
 * The workspace arm, decoded whole or not at all.
 *
 * A stored request is what survives a reload, and a field this parser forgets
 * is a field that silently disappears — a half-read workspace request would
 * reopen the launcher with no directory and the worktree toggle back on,
 * which is an ordinary launch wearing this one's title. Anything malformed is
 * refused so the dialog stays closed rather than opening on a guess.
 */
function parseWorkspaceRequest(parsed: object): NewCodingSessionRequest | null {
  if (!("sessionRef" in parsed)) return null;
  if (typeof parsed.sessionRef !== "string" || parsed.sessionRef.length === 0) {
    return null;
  }
  if (!("channelId" in parsed) || !("projectId" in parsed)) return null;
  const channelId = parsed.channelId;
  const projectId = parsed.projectId;
  if (typeof channelId !== "string" && channelId !== null) return null;
  if (typeof projectId !== "string" && projectId !== null) return null;
  if (!("workspace" in parsed)) return null;
  const workspace = parsed.workspace;
  if (typeof workspace !== "object" || workspace === null) return null;
  if (!("path" in workspace) || !("branch" in workspace)) return null;
  if (typeof workspace.path !== "string" || workspace.path.length === 0) {
    return null;
  }
  if (typeof workspace.branch !== "string" && workspace.branch !== null) {
    return null;
  }
  // Absent is the ordinary case (nothing claimed). `"recorded"` is the only
  // value that may be stored, so anything else — `"live"` above all — is a
  // request written by something that did not go through the opener, and is
  // refused rather than shown.
  const branchSource =
    "branchSource" in workspace ? workspace.branchSource : null;
  if (branchSource !== "recorded" && branchSource !== null) return null;
  return {
    kind: "workspace",
    channelId,
    projectId,
    sessionRef: parsed.sessionRef,
    workspace: {
      path: workspace.path,
      branch: workspace.branch,
      branchSource,
    },
  };
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
    if (parsed.kind === "workspace") {
      return parseWorkspaceRequest(parsed);
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

/**
 * Open the dialog on one session's existing checkout.
 *
 * Opening is the whole effect: nothing is signed, no session starts, resumes
 * or stops, no branch moves, and no directory is created. The caller has
 * already verified the path on this computer — this store carries it, it does
 * not check it.
 */
export function openNewCodingSessionDialogInWorkspace(input: {
  channelId?: string | null;
  projectId?: string | null;
  sessionRef: string;
  workspace: NewCodingSessionWorkspaceReuse;
}): void {
  restored = true;
  request = {
    kind: "workspace",
    channelId: input.channelId ?? null,
    projectId: input.projectId ?? null,
    sessionRef: input.sessionRef,
    workspace: {
      path: input.workspace.path,
      branch: input.workspace.branch,
      // A live head is dropped here rather than stored: the draft re-reads it
      // on open, and a stored one would outlive the moment it was true.
      branchSource:
        input.workspace.branchSource === "recorded" ? "recorded" : null,
    },
  };
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
