import * as React from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";

import {
  SHELL_SESSION_OUTPUT_EVENT,
  attachShellSession,
  resizeShellSession,
  writeShellSession,
  type ShellSessionOutputEvent,
} from "@/shared/api/tauriShell";

function base64ToBytes(b64: string): Uint8Array {
  const binary = window.atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * The interactive terminal for a built-in shell session. xterm.js renders the
 * raw PTY stream (replayed from scrollback on mount, then live via the
 * `shell-session-output` event). This screen only ever renders the owner's
 * own sessions, and the owner always has interact rights on them — keystrokes
 * are forwarded unconditionally.
 */
export function ShellTerminal({ sessionId }: { sessionId: string }) {
  const containerRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // xterm's dispose() (in this effect's cleanup) releases listeners/timers
    // but never removes the DOM node open() appended to the container. Under
    // React StrictMode's dev-only double-invoke (mount → cleanup → mount),
    // that leaves the first, disposed terminal's element as a dead sibling
    // that still sits in the container and intercepts clicks/typing, while the
    // real, live terminal renders invisibly underneath it. Clear the container
    // first so only the instance created below can ever be present.
    container.replaceChildren();

    // Track the app's rem-based zoom (Cmd +/- scales the root font-size):
    // 0.8125rem ≈ text-xs+, the app's code/meta type size.
    const rootPx =
      Number.parseFloat(
        window.getComputedStyle(document.documentElement).fontSize,
      ) || 16;
    const term = new Terminal({
      cursorBlink: true,
      fontSize: Math.round(rootPx * 0.8125),
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, 'Cascadia Mono', monospace",
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    fit.fit();
    // Grab keyboard focus so the session is typeable the moment it mounts —
    // it's freshly navigated to (from the sidebar or a resume), so focus is
    // still on whatever was clicked, not the new terminal.
    term.focus();

    let disposed = false;

    // Replay retained scrollback so a reopened session shows its history.
    attachShellSession(sessionId)
      .then((b64) => {
        if (!disposed && b64.length > 0) term.write(base64ToBytes(b64));
      })
      .catch(() => {
        // Session may have just closed; the screen handles the empty state.
      });

    let unlisten: UnlistenFn | null = null;
    listen<ShellSessionOutputEvent>(SHELL_SESSION_OUTPUT_EVENT, (event) => {
      if (event.payload.sessionId !== sessionId) return;
      term.write(base64ToBytes(event.payload.dataB64));
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {
        // Not running under Tauri (browser preview); terminal stays static.
      });

    const dataDisposable = term.onData((data) => {
      void writeShellSession(sessionId, data).catch(() => {
        // Session gone; exit event will refresh the surrounding UI.
      });
    });

    const syncSize = () => {
      fit.fit();
      void resizeShellSession(sessionId, term.rows, term.cols).catch(() => {
        // Session gone; nothing to resize.
      });
    };
    syncSize();
    const resizeObserver = new ResizeObserver(syncSize);
    resizeObserver.observe(container);

    // Clicking anywhere in the padded container (not just the xterm canvas)
    // focuses the terminal, so a click near the edge still lets you type.
    const focusOnPointer = () => {
      term.focus();
    };
    container.addEventListener("mousedown", focusOnPointer);

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      container.removeEventListener("mousedown", focusOnPointer);
      dataDisposable.dispose();
      unlisten?.();
      term.dispose();
    };
  }, [sessionId]);

  return (
    <div
      ref={containerRef}
      data-testid="shell-terminal"
      className="min-h-0 flex-1 bg-[#1e1e2e] px-2 py-1"
    />
  );
}
