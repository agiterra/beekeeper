import { ShieldAlert } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/shared/ui/tooltip";

/**
 * What this session's agent is allowed to do — stated, not chosen.
 *
 * t3code puts a permission control here (Supervised / Auto-accept edits /
 * Auto / Full access). Bee Keeper has no such control, and this is deliberately
 * **not** one: `session/request_permission` is answered `allow_once` inside the
 * ACP read loop (`crates/buzz-acp/src/acp.rs:2194`), before any surface could
 * show it. Every tool call a coding session makes is approved automatically —
 * §2 item 13, open since 2026-08-18 and named there as the root blocker for any
 * approval story.
 *
 * A dropdown offering modes we cannot enforce would be exactly the kind of
 * control AGENTS.md calls a bug of the same severity as a crash. So this says
 * the true thing plainly, in the place the control will go.
 */
export function CodingSessionAccessNotice({
  className,
}: {
  className?: string;
}) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className={
              className ??
              "inline-flex items-center gap-1.5 rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1 text-2xs text-foreground"
            }
            data-testid="coding-session-access-notice"
          >
            <ShieldAlert aria-hidden className="size-3.5 shrink-0" />
            Full access
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-72" side="top">
          Every tool call this agent makes — shell commands, file writes — is
          approved automatically. Bee Keeper has no approval control yet, so
          there is nothing to choose here: this states what the session runs
          with.
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
