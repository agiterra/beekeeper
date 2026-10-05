import { Eye } from "lucide-react";

import type { RemoteTerminal } from "@/features/builtin-shell/observe/useProjectTerminals";
import type { ObserverStatus } from "@/features/builtin-shell/observe/useShellObserver";
import { ShellWatchTerminal } from "@/features/builtin-shell/ui/ShellWatchTerminal";

/**
 * A teammate's shared terminal for this session, watched read-only in the
 * drawer (SV-25, DB11). The line above it says whose computer it is on and
 * whether frames are arriving — never the host's name — and that this view
 * cannot type. Watching publishes NIP-ST 24310 heartbeats only; there is no
 * input path here at all.
 */
export function CodingSessionTerminalWatch({
  label,
  onStatus,
  terminal,
}: {
  /** "{owner}'s computer · live" (or stalled, ended, connecting). */
  label: string;
  onStatus: (status: ObserverStatus) => void;
  terminal: RemoteTerminal;
}) {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-testid="coding-session-terminal-watch"
    >
      <p className="flex h-6 shrink-0 items-center gap-1.5 border-b border-border/60 px-3 text-2xs text-muted-foreground">
        <Eye aria-hidden className="size-3 shrink-0" />
        <span
          className="truncate"
          data-testid="coding-session-terminal-watch-label"
        >
          {label}
        </span>
        <span className="shrink-0">· read-only</span>
        <span className="ml-auto truncate">{terminal.title}</span>
      </p>
      <ShellWatchTerminal
        onStatus={onStatus}
        target={{
          ownerPubkey: terminal.ownerPubkey,
          sessionId: terminal.sessionId,
          projectRef: terminal.projectRef,
        }}
      />
    </div>
  );
}
