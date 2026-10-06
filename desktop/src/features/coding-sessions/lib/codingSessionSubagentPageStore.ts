/**
 * Which subagent page (SV-79) each session view has open, outside React.
 *
 * A module-level store rather than component state so the choice survives
 * what remounts a workspace — switching surfaces, tabs or sessions and coming
 * back — and so a row in the transcript and a row in the Agents surface open
 * the same page. Keyed by {@link codingSessionSubagentPageScopeKey}, which
 * includes the community, so one community's open page can never name
 * another's; `resetCodingSessionSubagentPages` still belongs in
 * `resetCommunityState()` with every other community-scoped singleton.
 *
 * Pure: no React, no DOM. The hooks live in `codingSessionSubagentNavigation`.
 */

/** What the store remembers for one session view. */
export type CodingSessionSubagentPageEntry = {
  /** The owning Task/Agent call's `toolCallId` (its items' `parentToolId`). */
  parentToolId: string;
};

const openPages = new Map<string, CodingSessionSubagentPageEntry>();
/** The call a page was closed from, waiting for the parent to scroll to it. */
const pendingReturns = new Map<string, string>();
const listeners = new Set<() => void>();

function notify(): void {
  for (const listener of listeners) listener();
}

/** The key one session view's page is stored under. */
export function codingSessionSubagentPageScopeKey(input: {
  communityScope: string;
  channelId: string;
  sessionKey: string;
  layout: "single" | "umbrella";
  /** The single layout's generation: that view shows one execution only. */
  generationId: string | null;
}): string {
  return [
    input.communityScope,
    input.channelId,
    input.sessionKey,
    input.layout === "single"
      ? `single:${input.generationId ?? ""}`
      : "umbrella",
  ].join("\u0000");
}

export function subscribeCodingSessionSubagentPages(
  listener: () => void,
): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The open page's `parentToolId` in this view, or `null`. */
export function readCodingSessionSubagentPage(scopeKey: string): string | null {
  return openPages.get(scopeKey)?.parentToolId ?? null;
}

export function openCodingSessionSubagentPage(
  scopeKey: string,
  parentToolId: string,
): void {
  const id = parentToolId.trim();
  if (!id) return;
  if (openPages.get(scopeKey)?.parentToolId === id) return;
  openPages.set(scopeKey, { parentToolId: id });
  pendingReturns.delete(scopeKey);
  notify();
}

/**
 * Close this view's page. `returnTo` is the parent row to scroll back to — the
 * owning call's `parentToolId` — taken once by
 * {@link takeCodingSessionSubagentPageReturn} after the parent is visible.
 */
export function closeCodingSessionSubagentPage(
  scopeKey: string,
  returnTo: string | null = null,
): void {
  const had = openPages.delete(scopeKey);
  if (returnTo) pendingReturns.set(scopeKey, returnTo);
  if (had || returnTo) notify();
}

/** The row the parent should scroll to, once; `null` when none is waiting. */
export function takeCodingSessionSubagentPageReturn(
  scopeKey: string,
): string | null {
  const returnTo = pendingReturns.get(scopeKey) ?? null;
  pendingReturns.delete(scopeKey);
  return returnTo;
}

/** Forget every open page — wired into a community switch. */
export function resetCodingSessionSubagentPages(): void {
  if (openPages.size === 0 && pendingReturns.size === 0) return;
  openPages.clear();
  pendingReturns.clear();
  notify();
}
