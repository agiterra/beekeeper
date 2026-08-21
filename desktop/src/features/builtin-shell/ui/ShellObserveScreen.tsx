import * as React from "react";
import { Terminal } from "@xterm/xterm";
import { useNavigate } from "@tanstack/react-router";
import { ArrowLeft, Eye, Keyboard, RefreshCw } from "lucide-react";
import "@xterm/xterm/css/xterm.css";

import { useUsersBatchQuery } from "@/features/profile/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import { buildShellInputEvent } from "@/shared/api/tauriShell";
import type { RelayEvent } from "@/shared/api/types";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import {
  useShellObserver,
  type ObserverStatus,
} from "../observe/useShellObserver";
import { useShellSessionAnnounce } from "../observe/useShellSessionAnnounce";

/** Coalesce keystrokes for this long before sending one input event. */
const INPUT_COALESCE_MS = 30;
/** Raw bytes per input event — base64 expands 4/3, keeping each event's
 * content ≤ 8 KiB (the relay's and the owner host's cap). */
const INPUT_CHUNK_BYTES = 6000;

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

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 1) {
    binary += String.fromCharCode(bytes[i]);
  }
  return window.btoa(binary);
}

/**
 * View of another member's shared terminal (NIP-ST). Everyone gets the live
 * frame stream; the grid follows the owner's dimensions (letterboxed).
 *
 * Viewers are strictly read-only: stdin disabled, keystrokes dropped before
 * any send path. Collaborators (per the owner's announce roster) may type —
 * keystrokes are coalesced (~30 ms), chunked so each event's base64 stays
 * ≤ 8 KiB, signed as kind:24312 via the Rust builder, and published to the
 * owner, who re-verifies everything before the PTY. There is **no local
 * echo**: typed characters appear only when the owner's frames stream them
 * back.
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
    truncatePubkey(ownerPubkey);

  // My role comes from the owner's announce roster — fetched directly so it
  // survives deep links, and kept fresh so a revocation flips this screen
  // back to read-only without a reload.
  const identity = useIdentityQuery();
  const myPubkey = identity.data?.pubkey?.toLowerCase() ?? null;
  const announceQuery = useShellSessionAnnounce(ownerPubkey, sessionId);
  const canType = React.useMemo(() => {
    if (!myPubkey) return false;
    return (
      announceQuery.data?.roster.some(
        (entry) => entry.pubkey === myPubkey && entry.role === "collaborator",
      ) ?? false
    );
  }, [announceQuery.data, myPubkey]);
  const canTypeRef = React.useRef(canType);
  canTypeRef.current = canType;

  // ── Collaborator input pipeline: coalesce → chunk → sign → publish. ──
  const queueRef = React.useRef("");
  const flushTimerRef = React.useRef<number | null>(null);
  // Chain sends so chunks arrive at the relay in typing order.
  const sendChainRef = React.useRef<Promise<void>>(Promise.resolve());

  const flushInput = React.useCallback(() => {
    flushTimerRef.current = null;
    const data = queueRef.current;
    queueRef.current = "";
    if (!data) return;
    const bytes = new TextEncoder().encode(data);
    const chunks: string[] = [];
    for (let i = 0; i < bytes.length; i += INPUT_CHUNK_BYTES) {
      chunks.push(bytesToBase64(bytes.subarray(i, i + INPUT_CHUNK_BYTES)));
    }
    sendChainRef.current = sendChainRef.current.then(async () => {
      for (const contentB64 of chunks) {
        try {
          const json = await buildShellInputEvent({
            ownerPubkey,
            sessionId,
            projectRef,
            contentB64,
          });
          await relayClient.publishEvent(
            JSON.parse(json) as RelayEvent,
            "Timed out sending input to the terminal's owner.",
            "Failed to send input to the terminal's owner.",
          );
        } catch (error) {
          console.warn("shell-observe: input publish failed", error);
        }
      }
    });
  }, [ownerPubkey, sessionId, projectRef]);

  const queueInput = React.useCallback(
    (data: string) => {
      queueRef.current += data;
      if (flushTimerRef.current === null) {
        flushTimerRef.current = window.setTimeout(
          flushInput,
          INPUT_COALESCE_MS,
        );
      }
    },
    [flushInput],
  );
  const queueInputRef = React.useRef(queueInput);
  queueInputRef.current = queueInput;

  React.useEffect(() => {
    return () => {
      if (flushTimerRef.current !== null) {
        window.clearTimeout(flushTimerRef.current);
      }
    };
  }, []);

  React.useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    container.replaceChildren();
    const rootPx =
      Number.parseFloat(
        window.getComputedStyle(document.documentElement).fontSize,
      ) || 16;
    const term = new Terminal({
      // The cursor isn't ours; stdin starts disabled and is enabled at
      // runtime only when the roster grants collaborator.
      cursorBlink: false,
      disableStdin: true,
      fontSize: Math.round(rootPx * 0.8125),
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, 'Cascadia Mono', monospace",
      scrollback: 5000,
    });
    term.open(container);
    termRef.current = term;
    // Keystrokes: dropped unless the roster says collaborator (defense in
    // depth on top of disableStdin). No local echo — the owner's frame
    // stream is the only render path.
    const dataDisposable = term.onData((data) => {
      if (!canTypeRef.current) return;
      queueInputRef.current(data);
    });
    return () => {
      dataDisposable.dispose();
      termRef.current = null;
      term.dispose();
    };
  }, []);

  // Flip stdin with the (live) role, without recreating the terminal.
  React.useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.disableStdin = !canType;
    if (canType) term.focus();
  }, [canType]);

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
          {canType ? (
            <p
              className="flex items-center gap-1 truncate text-2xs text-muted-foreground"
              data-testid="shell-observe-collaborator"
            >
              <Keyboard className="size-3 shrink-0" />
              Collaborator — your keystrokes go to the owner&rsquo;s terminal
            </p>
          ) : (
            <p className="flex items-center gap-1 truncate text-2xs text-muted-foreground">
              <Eye className="size-3 shrink-0" />
              Read-only — you are observing this session
            </p>
          )}
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
