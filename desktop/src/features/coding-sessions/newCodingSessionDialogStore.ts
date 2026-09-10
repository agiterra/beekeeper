import * as React from "react";

import type { NewCodingSessionWorkspaceReuse } from "./lib/codingSessionWorkspaceReuse";

/**
 * The pending "found a coding session" request, and who asked for it.
 *
 * "New coding session" no longer opens a dialog: the click founds the topic
 * (one 44226) and lands on the founded session's page, where everything else
 * is set up. Every entry point — a channel menu, a project sidebar, the
 * "New session in this workspace" action, a deep link — writes its intent
 * here, and one headless host (`ui/CodingSessionFoundingHost.tsx`) performs
 * it. A module store because the intent has to outlive the menu that raised
 * it and reach a host mounted once, in the app shell.
 *
 * **One genesis per click is this store's invariant, not a component's.** A
 * request carries a `phase`: `"requested"` until the host has begun, then
 * `"founding"` until the host clears it. While a request exists, in either
 * phase, every `request*()` call is a no-op — a double click, two entry
 * points firing, or a React StrictMode re-run of the host's effect cannot
 * queue a second founding. The host flips the phase synchronously, before
 * its first `await`, through `markCodingSessionFoundingStarted()`.
 *
 * Nothing here is persisted. The dialog this replaced mirrored its request
 * into `sessionStorage` so a reload mid-create re-attached to it; a founding
 * request that survived a reload would found a *second* genesis for the same
 * click, so the request lives in memory and dies with the page.
 *
 * (The file keeps its old name: renaming it would touch every importer for
 * no behavioural gain.)
 */

/** What a founding request names: where the session lands, and what it reuses. */
export type NewCodingSessionRequest =
  | {
      kind: "channel";
      /**
       * The destination channel, or null when the caller had none to give. A
       * null here cannot be founded — the host says so and clears it.
       */
      channelId: string | null;
    }
  | { kind: "project"; projectId: string }
  /**
   * Reuse one session's existing checkout for a *new* session.
   *
   * The workspace travels with the request because nothing downstream can
   * re-derive it: the founded page's prefill order puts a channel's
   * remembered folder above any fallback, so a request that carried only the
   * session ref would silently land on the channel's directory instead of the
   * one the person chose. `channelId`/`projectId` are where the new session
   * lands, which is a separate question from where it runs.
   *
   * - `path` and `branch` are what the worktree was cut as.
   * - `branchSource` is carried **only when it is `"recorded"`** — the branch
   *   a worktree was created on is a creation-time fact. `"live"` is never
   *   carried: "on disk now" is a claim about the present, and the founded
   *   page reads the head itself, on open.
   * - `alsoHere` — the other sessions recorded at this directory — must not
   *   be added. It is a count taken at one moment, and it belongs where it is
   *   resolved fresh, on the menu that raised this request.
   */
  | {
      kind: "workspace";
      channelId: string | null;
      projectId: string | null;
      sessionRef: string;
      /** Source execution repository; absent means unknown. */
      sourceRepoRef?: string | null;
      workspace: NewCodingSessionWorkspaceReuse;
    };

/**
 * `"requested"` until the host begins founding; `"founding"` from the host's
 * first synchronous step until it clears the request.
 */
export type CodingSessionFoundingPhase = "requested" | "founding";

/** A request as the store holds it: what was asked, and how far it has got. */
export type CodingSessionFoundingRequest = NewCodingSessionRequest & {
  phase: CodingSessionFoundingPhase;
};

let request: CodingSessionFoundingRequest | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const listener of listeners) listener();
}

/**
 * The workspace arm, decoded whole or not at all.
 *
 * A field this parser forgets is a field that silently disappears — a
 * half-read workspace request would found a session with no directory and
 * the worktree toggle back on, which is an ordinary launch wearing this one's
 * title. Anything malformed is refused so nothing is founded on a guess.
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
  const sourceRepoRef = "sourceRepoRef" in parsed ? parsed.sourceRepoRef : null;
  if (sourceRepoRef !== null && typeof sourceRepoRef !== "string") return null;
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
  // value that may be carried, so anything else — `"live"` above all — is a
  // request written by something that did not go through the opener, and is
  // refused rather than founded on.
  const branchSource =
    "branchSource" in workspace ? workspace.branchSource : null;
  if (branchSource !== "recorded" && branchSource !== null) return null;
  return {
    kind: "workspace",
    channelId,
    projectId,
    sessionRef: parsed.sessionRef,
    sourceRepoRef,
    workspace: {
      path: workspace.path,
      branch: workspace.branch,
      branchSource,
    },
  };
}

/**
 * Decode a serialized request, rejecting anything that is not one.
 *
 * The store itself no longer stores anything, so nothing in the app calls
 * this at runtime; it is the executable definition of what a request must
 * look like, and the tests hold every arm's shape against it.
 */
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

/** Admit a new request only when none is pending, in either phase. */
function admit(next: NewCodingSessionRequest): void {
  if (request !== null) return;
  request = { ...next, phase: "requested" };
  emit();
}

/**
 * Ask for a session to be founded in a channel.
 *
 * Ignored while another request is pending or being founded: one click, one
 * genesis. A null channel is accepted so the caller's intent is recorded, but
 * the host cannot found it and says so.
 */
export function requestCodingSessionFounding(channelId?: string | null): void {
  admit({ kind: "channel", channelId: channelId ?? null });
}

/** Ask for a session to be founded in a project, which decides the channel for itself. */
export function requestProjectCodingSessionFounding(projectId: string): void {
  admit({ kind: "project", projectId });
}

/**
 * Ask for a session to be founded on one session's existing checkout.
 *
 * Writing the request is the whole effect: nothing is signed here, no
 * session starts, resumes or stops, no branch moves, and no directory is
 * created. The caller has already verified the path on this computer — this
 * store carries it, it does not check it.
 */
export function requestCodingSessionFoundingInWorkspace(input: {
  channelId?: string | null;
  projectId?: string | null;
  sessionRef: string;
  sourceRepoRef?: string | null;
  workspace: NewCodingSessionWorkspaceReuse;
}): void {
  admit({
    kind: "workspace",
    channelId: input.channelId ?? null,
    projectId: input.projectId ?? null,
    sessionRef: input.sessionRef,
    sourceRepoRef: input.sourceRepoRef ?? null,
    workspace: {
      path: input.workspace.path,
      branch: input.workspace.branch,
      // A live head is dropped here rather than carried: the founded page
      // re-reads it on open, and a carried one would outlive the moment it
      // was true.
      branchSource:
        input.workspace.branchSource === "recorded" ? "recorded" : null,
    },
  });
}

/**
 * The host's first, synchronous step: claim the pending request.
 *
 * Returns true exactly once per request — when it moved from `"requested"`
 * to `"founding"`. A second call (StrictMode's repeated effect, a remount of
 * the lazy project founder) finds the phase already flipped and gets false,
 * and must then do nothing. Must be called before the host's first `await`.
 */
export function markCodingSessionFoundingStarted(): boolean {
  if (request === null || request.phase === "founding") return false;
  request = { ...request, phase: "founding" };
  emit();
  return true;
}

/** Drop the request — the founding is over, whichever way it ended. */
export function clearCodingSessionFoundingRequest(): void {
  if (request === null) return;
  request = null;
  emit();
}

/**
 * Drop the request when the community changes.
 *
 * Wired into `resetCommunityState()`: a channel id belongs to one relay, and
 * a request left across a switch would found a session in a channel that no
 * longer exists here.
 */
export function resetCodingSessionFoundingRequest(): void {
  clearCodingSessionFoundingRequest();
}

function snapshot(): CodingSessionFoundingRequest | null {
  return request;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The pending request, with its phase, or null when nothing is being founded. */
export function useCodingSessionFoundingRequest(): CodingSessionFoundingRequest | null {
  return React.useSyncExternalStore(subscribe, snapshot, () => null);
}
