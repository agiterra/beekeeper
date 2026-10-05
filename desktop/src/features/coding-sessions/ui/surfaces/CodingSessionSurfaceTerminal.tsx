import * as React from "react";
import { SquareTerminal } from "lucide-react";

import { SESSION_SHARED_TERMINALS_READ_LIMIT } from "@/features/builtin-shell/observe/useSessionSharedTerminals";
import {
  codingSessionTreeElsewhereClause,
  codingSessionTreeProviderName,
} from "@/features/coding-sessions/lib/codingSessionTerminalModel";

import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import { CodingSessionSurfaceStubPanel } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfaceTerminalBadge } from "./CodingSessionSurfaceTerminalBadge";
import {
  codingSessionTerminalExtension,
  useCodingSessionTerminalExtension,
} from "./CodingSessionSurfaceTerminalExtension";

/**
 * Where the tree is, as a whole sentence, when another computer has it.
 *
 * Built on the clause the Files surface shares
 * (`codingSessionTreeElsewhereClause`), so the two surfaces state one fact
 * one way. The name is the provider key's (`providerAuthorityPubkey`, the
 * execution's signer), never the person who created the session — and a
 * provider key's name is not the machine's owner, so the sentence says whose
 * provider runs the session, not whose computer it is. When nothing resolves
 * the provider, it names nobody.
 *
 * Decision (fix round 1): the Wave B brief's §3 wording ("on {owner}'s
 * computer") named the computer's owner from a provider label, which no fact
 * here supports; the Files wording stands for both.
 *
 * `sharedTruncated`: the shared-terminal read came back full, so "no
 * terminal there is shared" would claim more than was read.
 */
export function codingSessionTreeElsewhereReason(
  ctx: Pick<
    CodingSessionSurfaceCtx,
    "focusedExecution" | "focusedRecord" | "resolveActorName"
  >,
  sharedTruncated = false,
): string {
  const clause = codingSessionTreeElsewhereClause(
    codingSessionTreeProviderName(ctx),
  );
  return sharedTruncated
    ? `${clause}, and no terminal shared there was found among the project's newest ${SESSION_SHARED_TERMINALS_READ_LIMIT}.`
    : `${clause}, and no terminal there is shared.`;
}

/**
 * What a default tree is, when the session's own worktree is not what
 * answered: `null` for the session's own worktree.
 */
export function codingSessionTreeDefaultNotice(
  ctx: Pick<CodingSessionSurfaceCtx, "tree">,
): string | null {
  if (!ctx.tree.available) return null;
  switch (ctx.tree.source) {
    case "project":
      return "Opens this project's checkout, not a worktree recorded for this session.";
    case "channel":
      return "Opens this channel's folder, not a worktree recorded for this session.";
    default:
      return null;
  }
}

/**
 * The terminal opens where this machine has the session's tree (DB9), or,
 * on a machine without it, where a teammate shares a terminal for this
 * session (DB11) — watched read-only.
 */
export function codingSessionSurfaceTerminalAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  if (ctx.tree.available) return { available: true };
  if ((codingSessionTerminalExtension(ctx)?.shared.length ?? 0) > 0) {
    return { available: true };
  }
  if (ctx.tree.state === "loading") {
    return {
      available: false,
      reason: "Checking where this session's working tree is.",
    };
  }
  if (ctx.tree.refusal === "notLocal") {
    return {
      available: false,
      reason: codingSessionTreeElsewhereReason(
        ctx,
        codingSessionTerminalExtension(ctx)?.sharedTruncated ?? false,
      ),
    };
  }
  return {
    available: false,
    reason:
      ctx.tree.reason ??
      "No working tree for this session is recorded on this computer.",
  };
}

/**
 * The drawer itself, loaded on first open: it pulls in xterm, which neither
 * the launcher nor the badge needs (and the registry's unit tests cannot
 * load).
 */
const CodingSessionTerminalDrawer = React.lazy(() =>
  import("../CodingSessionTerminalDrawer").then((module) => ({
    default: module.CodingSessionTerminalDrawer,
  })),
);

/**
 * The drawer's Terminal panel (SV-25): the session's terminals where this
 * machine has the tree or a teammate shares one; otherwise the one sentence
 * that says why there is no terminal here, and no "New terminal".
 */
export function CodingSessionSurfaceTerminalPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceTerminalAvailability(ctx);
  const extension = codingSessionTerminalExtension(ctx);
  if (!availability.available || !extension) {
    // No terminal here, and no "New terminal": only the reason.
    return (
      <div
        className="flex min-h-0 flex-1 flex-col"
        data-session-key={ctx.sessionKey}
        data-testid="coding-session-terminal-unavailable"
      >
        <CodingSessionSurfaceStubPanel
          availability={availability}
          id="terminal"
          label="Terminal"
        >
          {/* Not `null`: the stub would fill a nullish child with its
              "no panel yet" line, which is not true here. */}
          {false}
        </CodingSessionSurfaceStubPanel>
      </div>
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-terminal"
    >
      {ctx.tree.available ? (
        <div className="px-3 pt-1 text-2xs">
          <CodingSessionTreeDefaultNotice ctx={ctx} />
        </div>
      ) : null}
      <React.Suspense
        fallback={
          <p className="px-3 py-2 text-2xs text-muted-foreground">
            Loading the terminal…
          </p>
        }
      >
        <CodingSessionTerminalDrawer ctx={ctx} extension={extension} />
      </React.Suspense>
    </div>
  );
}

export const codingSessionSurfaceTerminal: CodingSessionSurfaceDefinition = {
  id: "terminal",
  label: "Terminal",
  icon: SquareTerminal,
  shortcut: "T",
  order: 30,
  placement: "drawer",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceTerminalAvailability,
  Badge: CodingSessionSurfaceTerminalBadge,
  Panel: CodingSessionSurfaceTerminalPanel,
  readExtension: useCodingSessionTerminalExtension,
};

/** The default-tree sentence, where one applies (Terminal and Files). */
export function CodingSessionTreeDefaultNotice({
  ctx,
}: {
  ctx: Pick<CodingSessionSurfaceCtx, "tree">;
}) {
  const notice = codingSessionTreeDefaultNotice(ctx);
  return notice ? (
    <p
      className="text-muted-foreground"
      data-testid="coding-session-tree-default-notice"
    >
      {notice}
    </p>
  ) : null;
}
