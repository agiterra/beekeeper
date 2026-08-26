import { CircleAlert, GitBranch } from "lucide-react";
import * as React from "react";

import {
  listCodingSessionWorktreeBranches,
  planCodingSessionWorktree,
  type CodingSessionWorktreeBranches,
  type CodingSessionWorktreePlan,
} from "@/shared/api/tauriCodingSessionWorktrees";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { cn } from "@/shared/lib/cn";
import { shouldAdoptSuggestion } from "../lib/codingSessionNameSuggestion";
import { codingSessionWorktreeSlug } from "../lib/codingSessionWorktreeName";
import { resolveWorktreeSourceSelection } from "../lib/codingSessionWorktreeSource";

/**
 * Run this session in its own git worktree.
 *
 * On by default, because the alternative is the agent sharing one index, one
 * HEAD, and one set of uncommitted changes with the person who opened the
 * checkout — and that shows up as a stash conflict long after the session
 * that caused it.
 *
 * The name follows the session's name until someone edits it. The *path* is
 * never guessed here: the host plans it against the real repository and the
 * real directory listing, so what this shows is either where the worktree
 * will land or the one sentence explaining why it cannot.
 *
 * The source picker lists the repository's real branches and sits on the
 * trunk by default — not the checkout's HEAD, which is whatever branch the
 * last session happened to park there.
 */
export function NewCodingSessionWorktreeField({
  checked,
  disabled = false,
  name,
  onCheckedChange,
  onNameChange,
  onSourceChange,
  sessionName,
  source,
  workdir,
}: {
  checked: boolean;
  disabled?: boolean;
  name: string;
  onCheckedChange: (checked: boolean) => void;
  onNameChange: (name: string) => void;
  /** The branch the worktree starts from changed — by the picker or a reset. */
  onSourceChange: (source: string | null) => void;
  /** The session's name, whose slug prefills the worktree name. */
  sessionName: string;
  /** The branch the worktree starts from; null until branches are known. */
  source: string | null;
  workdir: string;
}) {
  const autoFilledRef = React.useRef<string | null>(null);
  React.useEffect(() => {
    const suggestion = codingSessionWorktreeSlug(sessionName);
    if (
      !shouldAdoptSuggestion({
        current: name,
        lastAutoFilled: autoFilledRef.current,
        suggestion,
      })
    ) {
      return;
    }
    autoFilledRef.current = suggestion;
    onNameChange(suggestion);
  }, [name, onNameChange, sessionName]);

  const [plan, setPlan] = React.useState<CodingSessionWorktreePlan | null>(
    null,
  );
  const [branches, setBranches] =
    React.useState<CodingSessionWorktreeBranches | null>(null);
  const trimmedWorkdir = workdir.trim();
  const trimmedName = name.trim();
  React.useEffect(() => {
    if (!checked || trimmedWorkdir.length === 0) {
      setBranches(null);
      return;
    }
    let cancelled = false;
    // Same 200ms settle as the plan below: the workdir is typed too.
    const handle = window.setTimeout(() => {
      void listCodingSessionWorktreeBranches({ workdir: trimmedWorkdir })
        .then((next) => {
          if (!cancelled) setBranches(next);
        })
        .catch(() => {
          // No host, or git missing: no picker, and the create falls back to
          // the host's own default.
          if (!cancelled) setBranches(null);
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [checked, trimmedWorkdir]);

  // The selection follows the repository: adopt the default when there is no
  // choice yet, keep a choice that still exists, and reset one that names a
  // branch this repository does not have.
  React.useEffect(() => {
    const resolved = resolveWorktreeSourceSelection({
      branches,
      current: source,
    });
    if (resolved !== source) {
      onSourceChange(resolved);
    }
  }, [branches, onSourceChange, source]);

  React.useEffect(() => {
    if (!checked || trimmedWorkdir.length === 0 || trimmedName.length === 0) {
      setPlan(null);
      return;
    }
    let cancelled = false;
    // The plan runs `git` twice; the same 200ms settle the workdir field uses
    // keeps a typed name from spawning one process per character.
    const handle = window.setTimeout(() => {
      void planCodingSessionWorktree({
        workdir: trimmedWorkdir,
        name: trimmedName,
        source,
      })
        .then((next) => {
          if (!cancelled) setPlan(next);
        })
        .catch(() => {
          // No host, or git missing: the checkbox still reads as intent, and
          // the create surfaces the real failure if it comes to that.
          if (!cancelled) setPlan(null);
        });
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [checked, source, trimmedName, trimmedWorkdir]);

  return (
    <div className="flex flex-col gap-2">
      <label
        className="flex items-center gap-2 text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-worktree-toggle"
      >
        <Checkbox
          checked={checked}
          data-testid="coding-session-worktree-toggle"
          disabled={disabled}
          id="coding-session-worktree-toggle"
          onCheckedChange={(next) => onCheckedChange(next === true)}
        />
        Use a worktree
      </label>
      {checked ? (
        <>
          <div className="flex items-center gap-2">
            <GitBranch className="size-4 shrink-0 text-muted-foreground" />
            <Input
              aria-label="Worktree name"
              autoComplete="off"
              className="font-mono text-xs placeholder:text-muted-foreground/50"
              data-testid="coding-session-worktree-name"
              disabled={disabled}
              onChange={(event) => onNameChange(event.target.value)}
              placeholder="worktree-name"
              spellCheck={false}
              value={name}
            />
          </div>
          {branches !== null && branches.branches.length > 0 ? (
            <div className="flex items-center gap-2">
              <label
                className="w-4 shrink-0 text-right text-2xs text-muted-foreground"
                htmlFor="coding-session-worktree-source"
              >
                from
              </label>
              <select
                aria-label="Source branch"
                className="h-8 min-w-0 flex-1 rounded-md border border-input bg-transparent px-2 font-mono text-xs disabled:opacity-50"
                data-testid="coding-session-worktree-source"
                disabled={disabled}
                id="coding-session-worktree-source"
                onChange={(event) => onSourceChange(event.target.value)}
                value={source ?? branches.defaultBranch ?? ""}
              >
                {branches.branches.map((branch) => (
                  <option key={branch} value={branch}>
                    {branch}
                    {branch === branches.defaultBranch ? " (default)" : ""}
                  </option>
                ))}
              </select>
            </div>
          ) : null}
          <WorktreePlanNote plan={plan} />
        </>
      ) : (
        <p className="text-2xs text-muted-foreground">
          The session will run directly in the working directory, sharing its
          branch and uncommitted changes with anything else open there.
        </p>
      )}
    </div>
  );
}

/** Where the worktree will land, or why it will not. */
export function WorktreePlanNote({
  plan,
}: {
  plan: CodingSessionWorktreePlan | null;
}) {
  if (plan?.problem) {
    return (
      <p
        className="flex items-start gap-1.5 text-xs text-destructive"
        data-testid="coding-session-worktree-problem"
      >
        <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
        {plan.problem}
      </p>
    );
  }
  if (!plan?.path) {
    return (
      <p className="text-2xs text-muted-foreground">
        A new branch off the source branch, in its own directory beside the
        repository. Stays on this computer.
      </p>
    );
  }
  return (
    <p
      className={cn("text-2xs text-muted-foreground")}
      data-testid="coding-session-worktree-plan"
    >
      {plan.disambiguated
        ? "That name was taken, so this session gets "
        : "Creates "}
      <span className="font-mono">{plan.path}</span>
      {" on branch "}
      <span className="font-mono">{plan.branch}</span>
      {" from "}
      <span className="font-mono">{plan.source ?? "this checkout's HEAD"}</span>
      .
    </p>
  );
}
