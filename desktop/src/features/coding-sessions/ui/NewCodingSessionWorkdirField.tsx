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
 * The default walks the same order the provider itself resolves in — the
 * channel's remembered directory, then the most recently used one — so the
 * pre-filled value is the one the create would have used anyway, made visible
 * before it matters instead of after a failed receipt.
 */
export function NewCodingSessionWorkdirField({
  channelId,
  disabled = false,
  onChange,
  value,
}: {
  channelId: string | null;
  disabled?: boolean;
  onChange: (path: string) => void;
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

  React.useEffect(() => {
    if (!state || touchedRef.current || value.trim().length > 0) return;
    const preferred =
      (channelId ? state.byChannel[channelId]?.path : null) ??
      state.mru[0]?.path ??
      "";
    if (preferred) onChange(preferred);
  }, [channelId, onChange, state, value]);

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
  const recents = (state?.mru ?? []).filter(
    (entry) => entry.path !== value.trim(),
  );

  return (
    <div className="flex flex-col gap-2">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-workdir"
      >
        Working directory
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
      ) : null}
      <p className="text-2xs text-muted-foreground">
        Stays on this computer. Working directories are never published to the
        channel.
      </p>
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
