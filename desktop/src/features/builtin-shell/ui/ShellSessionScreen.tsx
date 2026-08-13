import * as React from "react";
import { useNavigate } from "@tanstack/react-router";
import { ArrowLeft, FolderGit2, Keyboard, X } from "lucide-react";

import { Button } from "@/shared/ui/button";
import { Spinner } from "@/shared/ui/spinner";
import {
  closeShellSession,
  resumeShellSession,
  shellWorkspaceId,
} from "@/shared/api/tauriShell";
import { useSessionConsent } from "../hooks/useSessionConsent";

import {
  upsertShellSession,
  useShellSessions,
} from "../hooks/useShellSessions";
import { ShellTerminal } from "./ShellTerminal";

/**
 * A dark, terminal-colored placeholder with a spinner for the moments where a
 * session is expected to appear shortly (just created, or resuming from disk)
 * but isn't attached yet — reads as "the terminal is about to render" rather
 * than an app-chrome loading state or (worse) a false "this session has
 * ended".
 */
function ShellConnectingView({ label }: { label: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-[#1e1e2e]">
      <Spinner className="border-white/15 border-t-white/70" />
      <p className="text-2xs text-white/60">{label}</p>
    </div>
  );
}

/**
 * A built-in shell session in the main content area: header (title, cwd,
 * close) over a live xterm terminal. Typing requires the same per-session
 * owner "Interact" consent as cmux sessions (granted automatically when the
 * owner creates a shell here, revocable in Settings → Shell); without it the
 * terminal is view-only and shows an enable affordance.
 */
export function ShellSessionScreen({ sessionId }: { sessionId: string }) {
  const navigate = useNavigate();
  const { sessions, loading } = useShellSessions();
  const { isConsented, grant } = useSessionConsent();

  const session = React.useMemo(
    () => sessions.find((s) => s.sessionId === sessionId) ?? null,
    [sessions, sessionId],
  );
  const workspaceId = shellWorkspaceId(sessionId);
  const interactive = isConsented(workspaceId);

  // A restored session has history but no live shell — respawn it in its saved
  // directory the moment its screen opens, so it "just works". Guard so the
  // resume fires once even as the session list refreshes.
  const resumingRef = React.useRef(false);
  React.useEffect(() => {
    if (session?.restorable && !resumingRef.current) {
      resumingRef.current = true;
      void resumeShellSession(sessionId)
        .then(upsertShellSession)
        .catch(() => {
          // The saved directory may be gone; the backend falls back to $HOME.
          // If it still fails, allow another attempt.
          resumingRef.current = false;
        });
    }
  }, [session?.restorable, sessionId]);

  const goBack = React.useCallback(() => {
    void navigate({ to: "/" });
  }, [navigate]);

  const close = React.useCallback(() => {
    void closeShellSession(sessionId)
      .catch(() => {
        // Already gone.
      })
      .then(() => navigate({ to: "/" }));
  }, [navigate, sessionId]);

  if (!session) {
    return loading ? (
      <ShellConnectingView label="Opening shell…" />
    ) : (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        This shell session has ended.
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex items-center gap-3 border-b border-border px-4 py-3">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          onClick={goBack}
          aria-label="Back"
          data-testid="shell-session-back"
        >
          <ArrowLeft className="size-4" />
        </Button>
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-semibold">{session.title}</p>
          <p className="flex items-center gap-1 truncate text-2xs text-muted-foreground">
            <FolderGit2 className="size-3 shrink-0" />
            {session.currentDirectory}
          </p>
        </div>
        {!session.running ? (
          <span className="rounded-full border border-border bg-muted px-2 py-0.5 text-2xs font-medium text-muted-foreground">
            Exited
          </span>
        ) : null}
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={close}
          data-testid="shell-session-close"
        >
          <X className="mr-2 size-4" />
          Close
        </Button>
      </header>

      {!interactive && session.running ? (
        <div className="flex items-center justify-between gap-3 border-b border-border bg-muted/40 px-4 py-2">
          <p className="text-2xs text-muted-foreground">
            Typing is off for this session. Enable interaction to use the
            terminal.
          </p>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => grant(workspaceId)}
            data-testid="shell-session-enable-typing"
          >
            <Keyboard className="mr-2 size-4" />
            Enable typing
          </Button>
        </div>
      ) : null}

      {/* Mount the terminal only once the session is live. For a restorable
          session the resume above spawns the host and replays history into the
          backend first; the terminal then attaches to a populated scrollback,
          so history shows without a race. */}
      {session.running ? (
        <ShellTerminal sessionId={sessionId} interactive={interactive} />
      ) : session.restorable ? (
        <ShellConnectingView label="Resuming session…" />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">
          Session ended.
        </div>
      )}
    </div>
  );
}
