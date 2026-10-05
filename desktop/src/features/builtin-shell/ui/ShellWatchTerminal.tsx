import * as React from "react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import {
  type ObserverStatus,
  type ShellObserverTarget,
  useShellObserver,
} from "../observe/useShellObserver";

/**
 * A teammate's shared terminal, watched read-only (NIP-ST) — the compact
 * form the session drawer embeds (SV-25). The full-screen observer with
 * collaborator typing is `ShellObserveScreen`; this one never wires an input
 * path at all: stdin is disabled and there is no `onData` handler, so a
 * keystroke here goes nowhere.
 *
 * The grid follows the owner's dimensions, letterboxed in the drawer.
 */
export function ShellWatchTerminal({
  onStatus,
  target,
}: {
  target: ShellObserverTarget;
  /** The observer's status as it changes: connecting, live, stalled, ended. */
  onStatus?: (status: ObserverStatus) => void;
}) {
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const termRef = React.useRef<Terminal | null>(null);

  React.useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    container.replaceChildren();
    const rootPx =
      Number.parseFloat(
        window.getComputedStyle(document.documentElement).fontSize,
      ) || 16;
    const term = new Terminal({
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

  const stableTarget = React.useMemo(
    () => ({
      ownerPubkey: target.ownerPubkey,
      sessionId: target.sessionId,
      projectRef: target.projectRef,
    }),
    [target.ownerPubkey, target.sessionId, target.projectRef],
  );
  const { status } = useShellObserver(stableTarget, {
    onWrite: React.useCallback((bytes: Uint8Array) => {
      termRef.current?.write(bytes);
    }, []),
    onResize: React.useCallback((rows: number, cols: number) => {
      termRef.current?.resize(cols, rows);
    }, []),
  });

  const onStatusRef = React.useRef(onStatus);
  onStatusRef.current = onStatus;
  React.useEffect(() => {
    onStatusRef.current?.(status);
  }, [status]);

  return (
    <div
      className="flex min-h-0 flex-1 justify-center overflow-auto bg-[#1e1e2e]"
      data-observer-status={status}
      data-testid="shell-watch-terminal"
    >
      <div ref={containerRef} />
    </div>
  );
}
