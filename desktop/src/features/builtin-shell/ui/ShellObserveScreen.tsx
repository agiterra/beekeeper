import * as React from "react";
import { Terminal } from "@xterm/xterm";
import { useNavigate } from "@tanstack/react-router";
import { ArrowLeft, Eye, RefreshCw } from "lucide-react";
import "@xterm/xterm/css/xterm.css";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import {
  useShellObserver,
  type ObserverStatus,
} from "../observe/useShellObserver";

const STATUS_BADGE: Record<
  ObserverStatus,
  { label: string; className: string }
> = {
  connecting: {
    label: "Connecting…",
    className: "border-border bg-muted text-muted-foreground",
  },
  live: {
    label: "LIVE",
    className: "border-emerald-500/40 bg-emerald-500/15 text-emerald-500",
  },
  stalled: {
    label: "Not streaming",
    className: "border-amber-500/40 bg-amber-500/15 text-amber-500",
  },
  ended: {
    label: "Ended",
    className: "border-border bg-muted text-muted-foreground",
  },
};

/**
 * Read-only view of another member's shared terminal (NIP-ST). The terminal
 * has stdin disabled and no data handler — there is no code path from this
 * screen into the owner's PTY. The grid follows the owner's dimensions
 * (letterboxed) rather than fitting the container.
 */
export function ShellObserveScreen({
  ownerPubkey,
  sessionId,
  projectRef,
}: {
  ownerPubkey: string;
  sessionId: string;
  projectRef: string;
}) {
  const navigate = useNavigate();
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const termRef = React.useRef<Terminal | null>(null);

  const ownerProfiles = useUsersBatchQuery([ownerPubkey]);
  const ownerProfile = ownerProfiles.data?.profiles[ownerPubkey.toLowerCase()];
  const ownerName =
    ownerProfile?.displayName ??
    ownerProfile?.name ??
    `${ownerPubkey.slice(0, 8)}…${ownerPubkey.slice(-4)}`;

  React.useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    container.replaceChildren();
    const rootPx =
      Number.parseFloat(
        window.getComputedStyle(document.documentElement).fontSize,
      ) || 16;
    const term = new Terminal({
      // Read-only: no cursor blink (it isn't ours), stdin disabled, and no
      // onData handler exists anywhere on this screen.
      cursorBlink: false,
      disableStdin: true,
      fontSize: Math.round(rootPx * 0.8125),
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, 'Cascadia Mono', monospace",
      scrollback: 5000,
    });
    term.open(container);
    termRef.current = term;
    return () => {
      termRef.current = null;
      term.dispose();
    };
  }, []);

  const target = React.useMemo(
    () => ({ ownerPubkey, sessionId, projectRef }),
    [ownerPubkey, sessionId, projectRef],
  );
  const { status, resync } = useShellObserver(target, {
    onWrite: React.useCallback((bytes: Uint8Array) => {
      termRef.current?.write(bytes);
    }, []),
    onResize: React.useCallback((rows: number, cols: number) => {
      termRef.current?.resize(cols, rows);
    }, []),
  });

  const badge = STATUS_BADGE[status];

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      data-testid="shell-observe-screen"
    >
      <header className="flex items-center gap-3 border-b border-border px-4 py-3">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={() => void navigate({ to: "/" })}
          aria-label="Back"
          data-testid="shell-observe-back"
        >
          <ArrowLeft className="size-4" />
        </Button>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold">
            {ownerName}&rsquo;s terminal
          </p>
          <p className="flex items-center gap-1 truncate text-2xs text-muted-foreground">
            <Eye className="size-3 shrink-0" />
            Read-only — you are observing this session
          </p>
        </div>
        <span
          className={cn(
            "rounded-full border px-2 py-0.5 text-2xs font-medium",
            badge.className,
          )}
          data-testid="shell-observe-status"
        >
          {badge.label}
        </span>
        {status !== "ended" ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void resync().catch(() => {})}
            data-testid="shell-observe-resync"
          >
            <RefreshCw className="mr-2 size-4" />
            Refresh
          </Button>
        ) : null}
      </header>
      {/* Letterbox: the grid keeps the owner's dimensions; the container
          centers it instead of stretching. */}
      <div className="flex min-h-0 flex-1 justify-center overflow-auto bg-[#1e1e2e]">
        <div ref={containerRef} data-testid="shell-observe-terminal" />
      </div>
    </div>
  );
}
