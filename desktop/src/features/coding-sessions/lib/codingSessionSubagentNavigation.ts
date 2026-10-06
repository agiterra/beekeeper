/**
 * Opening a subagent as its own page (SV-79), for any row that names one.
 *
 * The contract the transcript's subagent rows and the Agents surface's
 * "Direct spawns" rows build on: `useOpenCodingSessionSubagent()` gives a
 * function that opens the page for a Task/Agent call by its `toolCallId` (the
 * `parentToolId` its items carry), `useOpenCodingSessionSubagentId()` says
 * which page is open, and `useCloseCodingSessionSubagent()` closes it. The
 * session view is read from the surface context both workspaces provide, so a
 * caller passes nothing else; outside a session workspace there is no page to
 * open, and `useCanOpenCodingSessionSubagent()` says so rather than handing
 * out a button that does nothing.
 */

import * as React from "react";

import {
  type CodingSessionSurfaceCtx,
  useCodingSessionSurfaceCtx,
} from "@/features/coding-sessions/ui/surfaces/codingSessionSurfaceContext";
import {
  closeCodingSessionSubagentPage,
  codingSessionSubagentPageScopeKey,
  openCodingSessionSubagentPage,
  readCodingSessionSubagentPage,
  subscribeCodingSessionSubagentPages,
} from "./codingSessionSubagentPageStore";

export { resetCodingSessionSubagentPages } from "./codingSessionSubagentPageStore";

/** This session view's store key, or `null` outside a session workspace. */
export function codingSessionSubagentScopeOf(
  ctx: Pick<
    CodingSessionSurfaceCtx,
    "communityScope" | "channelId" | "sessionKey" | "layout" | "focusedRecord"
  > | null,
): string | null {
  if (!ctx) return null;
  return codingSessionSubagentPageScopeKey({
    communityScope: ctx.communityScope,
    channelId: ctx.channelId,
    sessionKey: ctx.sessionKey,
    layout: ctx.layout,
    generationId: ctx.focusedRecord?.generationId ?? null,
  });
}

/** The current session view's store key; `null` outside a workspace. */
export function useCodingSessionSubagentScope(): string | null {
  const ctx = useCodingSessionSurfaceCtx();
  return codingSessionSubagentScopeOf(ctx);
}

/** Whether a subagent page can open here (inside a session workspace). */
export function useCanOpenCodingSessionSubagent(): boolean {
  return useCodingSessionSubagentScope() !== null;
}

/** Opens the page for the call whose `toolCallId` is `parentToolId`. */
export function useOpenCodingSessionSubagent(): (parentToolId: string) => void {
  const scope = useCodingSessionSubagentScope();
  return React.useCallback(
    (parentToolId: string) => {
      if (scope !== null) openCodingSessionSubagentPage(scope, parentToolId);
    },
    [scope],
  );
}

/** The open page's `parentToolId` in this session view, or `null`. */
export function useOpenCodingSessionSubagentId(): string | null {
  const scope = useCodingSessionSubagentScope();
  const read = React.useCallback(
    () => (scope === null ? null : readCodingSessionSubagentPage(scope)),
    [scope],
  );
  return React.useSyncExternalStore(
    subscribeCodingSessionSubagentPages,
    read,
    read,
  );
}

/**
 * Closes this view's page; `returnTo` (the owning call's `toolCallId`) asks
 * the parent transcript to scroll back to that subagent's row.
 */
export function useCloseCodingSessionSubagent(): (
  returnTo?: string | null,
) => void {
  const scope = useCodingSessionSubagentScope();
  return React.useCallback(
    (returnTo?: string | null) => {
      if (scope !== null) closeCodingSessionSubagentPage(scope, returnTo);
    },
    [scope],
  );
}

/** Closes the page in a known view, for a caller outside React. */
export function closeCodingSessionSubagent(
  scopeKey: string,
  returnTo: string | null = null,
): void {
  closeCodingSessionSubagentPage(scopeKey, returnTo);
}
