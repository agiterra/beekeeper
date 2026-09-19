import { CircleAlert, FolderOpen } from "lucide-react";
import * as React from "react";

import {
  getCodingSessionWorkdirState,
  pickCodingSessionWorkdir,
  validateCodingSessionWorkdir,
  type CodingSessionWorkdirState,
  type CodingSessionWorkdirValidation,
} from "@/shared/api/tauriCodingSessionWorkdirs";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/cn";

/**
 * Where the session runs, chosen on this machine and never published.
 *
 * A project session has exactly one honest prefill: the folder this computer
 * recorded for the project (`byProject`, written by the project's create,
 * Finish repository setup, or Project settings → This computer). Nothing
 * else is offered for it — not the channel's folder, not a caller's guess,
 * not the most recently used directory — because the first RPG Test team
 * session (2026-09-19) pre-filled the MRU, which was another project's
 * checkout, and the lead worked, committed and asked to record that folder
 * as the project's. An empty field with a blocker naming Finish repository
 * setup is the truth; a plausible path from elsewhere is not.
 *
 * Without a project the old order stands: the channel's remembered directory,
 * the caller's fallback, then the most recently used directory anywhere.
 */
export function preferredWorkdirPrefill({
  channelId,
  fallbackPath,
  projectKey,
  state,
}: {
  channelId: string | null;
  fallbackPath: string | null;
  projectKey: string | null;
  state: Pick<CodingSessionWorkdirState, "byChannel" | "byProject" | "mru">;
}): string {
  if (projectKey) {
    return state.byProject[projectKey]?.path ?? "";
  }
  return (
    (channelId ? state.byChannel[channelId]?.path : null) ??
    fallbackPath ??
    state.mru[0]?.path ??
    ""
  );
}

export function NewCodingSessionWorkdirField({
  channelId,
  disabled = false,
  fallbackPath = null,
  onChange,
  projectKey = null,
  usesWorktree = false,
  value,
}: {
  channelId: string | null;
  disabled?: boolean;
  /** A suggested directory when the provider has nothing remembered — e.g.
   * the local checkout of one of the project's repositories. It resolves
   * async, so it may arrive after a lower-priority prefill already landed;
   * an auto-filled value upgrades, a hand-edited one is never touched. */
  fallbackPath?: string | null;
  onChange: (path: string) => void;
  /** NIP-MP project coordinate whose remembered directory outranks the
   * channel's — a project-scoped session belongs to the project's checkout. */
  projectKey?: string | null;
  /**
   * Whether this session will run in a worktree.
   *
   * The same stored path does two jobs, and only one of them is "the
   * repository". With a worktree the session runs somewhere else entirely and
   * this names the repository the worktree is cut from; without one the agent
   * runs in this very directory, which is a working directory and should say
   * so. One label for both would be wrong half the time, so it follows the
   * answer — which is why the worktree question is asked first.
   */
  usesWorktree?: boolean;
  value: string;
}) {
  const [state, setState] = React.useState<CodingSessionWorkdirState | null>(
    null,
  );
  const [validation, setValidation] =
    React.useState<CodingSessionWorkdirValidation | null>(null);
  const touchedRef = React.useRef(false);

  React.useEffect(() => {
    let cancelled = false;
    void getCodingSessionWorkdirState()
      .then((next) => {
        if (!cancelled) setState(next);
      })
      .catch(() => {
        if (!cancelled) setState(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const autoFilledRef = React.useRef<string | null>(null);

  React.useEffect(() => {
    if (!state || touchedRef.current) return;
    const current = value.trim();
    // A hand-entered value is final; an auto-filled one may still upgrade
    // when a higher-priority source resolves after the first fill.
    if (current.length > 0 && current !== autoFilledRef.current) return;
    const preferred = preferredWorkdirPrefill({
      channelId,
      fallbackPath,
      projectKey,
      state,
    });
    if (preferred && preferred !== current) {
      autoFilledRef.current = preferred;
      onChange(preferred);
    }
  }, [channelId, fallbackPath, onChange, projectKey, state, value]);

  React.useEffect(() => {
    const candidate = value.trim();
    if (candidate.length === 0) {
      setValidation(null);
      return;
    }
    let cancelled = false;
    const handle = window.setTimeout(() => {
      void validateCodingSessionWorkdir(candidate)
        .then((next) => {
          if (!cancelled) setValidation(next);
        })
        .catch(() => {
          if (!cancelled) setValidation(null);
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [value]);

  const handleBrowse = React.useCallback(() => {
    touchedRef.current = true;
    void pickCodingSessionWorkdir()
      .then((picked) => {
        if (picked) onChange(picked);
      })
      .catch(() => {
        // A cancelled or unavailable picker leaves the text field in charge.
      });
  }, [onChange]);

  const problem = describeWorkdirProblem(value, validation);
  // Recents are other projects' folders as often as not; for a project
  // session they are exactly the wrong thing to offer (see the prefill).
  const recents = projectKey
    ? []
    : (state?.mru ?? []).filter((entry) => entry.path !== value.trim());

  return (
    <div className="flex flex-col gap-2">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-workdir"
      >
        {usesWorktree ? "Repository folder" : "Working directory"}
      </label>
      <div className="flex items-center gap-2">
        <Input
          aria-describedby={
            problem ? "coding-session-workdir-problem" : undefined
          }
          aria-invalid={problem !== null}
          autoComplete="off"
          className={cn(
            // The example path is a hint, not a value — at mono/xs the stock
            // placeholder color reads like a real filled-in path.
            "font-mono text-xs placeholder:text-muted-foreground/50",
            problem && "border-destructive/60",
          )}
          data-testid="coding-session-workdir-input"
          disabled={disabled}
          id="coding-session-workdir"
          onChange={(event) => {
            touchedRef.current = true;
            onChange(event.target.value);
          }}
          placeholder="/absolute/path/to/checkout"
          spellCheck={false}
          value={value}
        />
        <Button
          data-testid="coding-session-workdir-browse"
          disabled={disabled}
          onClick={handleBrowse}
          size="sm"
          type="button"
          variant="outline"
        >
          <FolderOpen />
          Browse
        </Button>
      </div>
      {problem ? (
        <p
          className="flex items-start gap-1.5 text-xs text-destructive"
          data-testid="coding-session-workdir-problem"
          id="coding-session-workdir-problem"
        >
          <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
          {problem}
        </p>
      ) : null}
      {recents.length > 0 ? (
        <div className="flex flex-col gap-1">
          <span className="text-2xs font-medium text-muted-foreground">
            Recent folders
          </span>
          <div className="flex flex-wrap gap-1.5">
            {recents.slice(0, 5).map((entry) => (
              <Button
                className="h-6 max-w-full px-2 font-mono text-2xs"
                data-testid="coding-session-workdir-recent"
                disabled={disabled}
                key={entry.path}
                onClick={() => {
                  touchedRef.current = true;
                  onChange(entry.path);
                }}
                size="sm"
                type="button"
                variant="ghost"
              >
                <span className="truncate">{entry.path}</span>
              </Button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

/** The one sentence that explains why this path will not work. */
export function describeWorkdirProblem(
  value: string,
  validation: CodingSessionWorkdirValidation | null,
): string | null {
  if (value.trim().length === 0) return null;
  if (!validation) return null;
  if (!validation.isAbsolute) {
    return "Use an absolute path — the provider treats anything else as unconfigured.";
  }
  if (!validation.exists) return "No such directory on this computer.";
  if (!validation.isDir) return "That path is a file, not a directory.";
  return null;
}
