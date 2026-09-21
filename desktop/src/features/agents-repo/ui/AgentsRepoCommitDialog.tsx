import * as React from "react";

import type {
  AgentsRepoCommitResult,
  AgentsRepoDraftChange,
} from "@/shared/api/agentsRepoTypes";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import type {
  AgentsRepoDraftDigest,
  DraftPath,
} from "../lib/agentsRepoDraftFold";

/** The host's change for one head. */
export function changeOf(entry: DraftPath): AgentsRepoDraftChange {
  const head = entry.head;
  return {
    id: head.id,
    author: head.author,
    op: head.op,
    path: head.path,
    to: head.to,
    text: head.text,
    base: head.base,
    message: head.message,
  };
}

/** Whether a head is stale against the listing's blob for its path. */
export function isStale(entry: DraftPath, blobOnMain: string | null): boolean {
  return entry.head.base !== blobOnMain;
}

export function AgentsRepoCommitDialog({
  open,
  onOpenChange,
  digest,
  tip,
  blobFor,
  personName,
  onCommit,
  result,
  busy,
  recordError,
  onRetryRecord,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  digest: AgentsRepoDraftDigest;
  tip: string | null;
  blobFor: (path: string) => string | null;
  personName: (pubkey: string) => string;
  onCommit: (drafts: DraftPath[], message: string) => Promise<void>;
  result: AgentsRepoCommitResult | null;
  busy: boolean;
  recordError: string | null;
  onRetryRecord: () => void;
}) {
  // A move claims two paths; one head per op id.
  const heads = React.useMemo(() => {
    const seen = new Set<string>();
    return digest.paths.filter((entry) => {
      if (seen.has(entry.head.id)) return false;
      seen.add(entry.head.id);
      return true;
    });
  }, [digest]);
  const [selected, setSelected] = React.useState<Set<string>>(() => new Set());
  const [message, setMessage] = React.useState("");
  React.useEffect(() => {
    if (!open) return;
    setSelected(
      new Set(
        heads
          .filter((h) => !isStale(h, blobFor(h.head.path)))
          .map((h) => h.head.id),
      ),
    );
    setMessage("");
  }, [open, heads, blobFor]);

  const chosen = heads.filter((h) => selected.has(h.head.id));
  const showResult = result !== null;

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="agents-repo-commit-dialog">
        <DialogHeader>
          <DialogTitle>{copy.commitTitle}</DialogTitle>
          <DialogDescription>
            {tip ? `main is at ${tip.slice(0, 8)}. ` : ""}
            Every chosen draft is checked against main, the whole tree is
            validated, and one commit is pushed under a lease. Nothing is pushed
            if anything refuses.
          </DialogDescription>
        </DialogHeader>
        {!showResult ? (
          <div className="space-y-3">
            <ul className="max-h-64 space-y-1 overflow-auto">
              {heads.map((entry) => {
                const stale = isStale(entry, blobFor(entry.head.path));
                return (
                  <li
                    className="flex items-start gap-2 text-sm"
                    key={entry.head.id}
                  >
                    <input
                      checked={selected.has(entry.head.id)}
                      className="mt-1"
                      data-testid={`agents-repo-commit-pick-${entry.head.path}`}
                      disabled={busy}
                      onChange={(event) => {
                        setSelected((prev) => {
                          const next = new Set(prev);
                          if (event.target.checked) next.add(entry.head.id);
                          else next.delete(entry.head.id);
                          return next;
                        });
                      }}
                      type="checkbox"
                    />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-mono text-xs">
                        {entry.head.op === "file.move"
                          ? `${entry.head.path} → ${entry.head.to}`
                          : entry.head.path}
                      </span>
                      <span className="block text-2xs text-muted-foreground">
                        {entry.head.op} by {personName(entry.head.author)}
                        {stale
                          ? " · main changed this file since; the commit will refuse it"
                          : ""}
                        {entry.diverged ? " · diverged" : ""}
                      </span>
                    </span>
                  </li>
                );
              })}
            </ul>
            <input
              aria-label={copy.commitMessage}
              className="w-full rounded-md border border-border bg-background px-2 py-1 text-sm"
              data-testid="agents-repo-commit-message"
              onChange={(event) => setMessage(event.target.value)}
              placeholder={copy.commitMessage}
              value={message}
            />
          </div>
        ) : (
          <div
            className="space-y-2 text-sm"
            data-testid={`agents-repo-commit-result-${result.pushed}`}
          >
            {result.pushed === "yes" ? (
              <p>
                {copy.pushedYes(
                  result.paths.length,
                  result.commit?.slice(0, 8) ?? "?",
                )}
              </p>
            ) : result.pushed === "no" ? (
              <p className="text-destructive">{copy.pushedNo}</p>
            ) : (
              <p className="text-destructive">{copy.pushedUnknown}</p>
            )}
            {result.unknownReason ? (
              <p className="text-xs text-muted-foreground">
                {result.unknownReason}
              </p>
            ) : null}
            {result.refusals.length > 0 ? (
              <ul className="space-y-1">
                {result.refusals.map((refusal) => (
                  <li
                    className="text-xs"
                    data-testid={`agents-repo-refusal-${refusal.code}`}
                    key={`${refusal.code}:${refusal.path ?? ""}:${refusal.message}`}
                  >
                    {refusal.path ? (
                      <span className="font-mono">{refusal.path}: </span>
                    ) : null}
                    {refusal.message}
                  </li>
                ))}
              </ul>
            ) : null}
            {result.paths.length > 0 ? (
              <ul className="space-y-0.5 font-mono text-xs text-muted-foreground">
                {result.paths.map((p) => (
                  <li key={p.path}>
                    {p.status} {p.path}
                  </li>
                ))}
              </ul>
            ) : null}
            {result.actions ? (
              <p className="text-2xs text-muted-foreground">
                actions.yml: {result.actions}
              </p>
            ) : null}
            {recordError ? (
              <div
                className="rounded-md bg-amber-500/10 px-2 py-1 text-xs text-amber-800 dark:text-amber-200"
                data-testid="agents-repo-record-error"
              >
                <p>{copy.recordFailed(result.commit?.slice(0, 8) ?? "?")}</p>
                <Button
                  className="mt-1"
                  onClick={onRetryRecord}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  {copy.retryMarking}
                </Button>
              </div>
            ) : null}
          </div>
        )}
        <DialogFooter>
          {!showResult ? (
            <Button
              data-testid="agents-repo-commit-confirm"
              disabled={busy || chosen.length === 0}
              onClick={() => void onCommit(chosen, message)}
              type="button"
            >
              {busy
                ? "Committing…"
                : `Commit ${chosen.length} draft${chosen.length === 1 ? "" : "s"}`}
            </Button>
          ) : (
            <Button
              data-testid="agents-repo-commit-close"
              onClick={() => onOpenChange(false)}
              type="button"
              variant="outline"
            >
              Close
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
