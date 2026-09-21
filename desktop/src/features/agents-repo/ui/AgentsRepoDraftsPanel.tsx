import { formatRelativeTime } from "@/features/forum/lib/time";
import { Button } from "@/shared/ui/button";

import type { AgentsRepoAccess } from "../lib/agentsRepoAccess";
import { agentsRepoCopy as copy } from "../lib/agentsRepoCopy";
import type { AgentsRepoDraftDigest } from "../lib/agentsRepoDraftFold";

export function AgentsRepoDraftsPanel({
  digest,
  truncated,
  commitAccess,
  personName,
  onOpen,
  onCommit,
}: {
  digest: AgentsRepoDraftDigest | null;
  truncated: boolean;
  commitAccess: AgentsRepoAccess;
  personName: (pubkey: string) => string;
  onOpen: (path: string) => void;
  onCommit: () => void;
}) {
  const open = digest?.paths ?? [];
  return (
    <aside className="space-y-3" data-testid="agents-repo-drafts-panel">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-2xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">
          {copy.drafts}
        </h3>
        {open.length > 0 && commitAccess.kind === "writable" ? (
          <Button
            data-testid="agents-repo-commit-open"
            onClick={onCommit}
            size="sm"
            type="button"
          >
            {copy.commit}
          </Button>
        ) : null}
      </div>
      {open.length > 0 && commitAccess.kind === "read-only" ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="agents-repo-commit-denied"
        >
          {commitAccess.reason}
        </p>
      ) : null}
      {open.length === 0 ? (
        <p className="text-sm text-muted-foreground">{copy.noDrafts}</p>
      ) : (
        <ul className="space-y-1">
          {open.map((entry) => (
            <li key={entry.path}>
              <button
                className="w-full rounded-md px-2 py-1 text-left text-sm hover:bg-muted/60"
                data-testid={`agents-repo-draft-row-${entry.path}`}
                onClick={() => onOpen(entry.path)}
                type="button"
              >
                <span className="block truncate font-mono text-xs">
                  {entry.path}
                </span>
                <span className="block text-2xs text-muted-foreground">
                  {entry.head.op === "file.put"
                    ? "edit"
                    : entry.head.op === "file.move"
                      ? `move → ${entry.head.to}`
                      : "delete"}{" "}
                  by {personName(entry.head.author)},{" "}
                  {formatRelativeTime(entry.head.createdAt)}
                  {entry.diverged ? " · diverged" : ""}
                  {entry.superseded.length > 0
                    ? ` · ${entry.superseded.length} superseded`
                    : ""}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      {digest && digest.ignored > 0 ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="agents-repo-ignored"
        >
          {copy.ignored(digest.ignored)}
        </p>
      ) : null}
      {digest && digest.otherRepo > 0 ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="agents-repo-other-repo"
        >
          {copy.otherRepo(digest.otherRepo)}
        </p>
      ) : null}
      {truncated ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="agents-repo-truncated"
        >
          {copy.truncated}
        </p>
      ) : null}
      {digest && digest.commits.length > 0 ? (
        <div>
          <h3 className="mb-1 text-2xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">
            {copy.commits}
          </h3>
          <ul className="space-y-1">
            {digest.commits.slice(0, 5).map((record) => (
              <li className="text-2xs text-muted-foreground" key={record.id}>
                <span className="font-mono">{record.commit.slice(0, 8)}</span>{" "}
                by {personName(record.by)},{" "}
                {formatRelativeTime(record.createdAt)} — {record.paths.length}{" "}
                file{record.paths.length === 1 ? "" : "s"}
                {record.message ? ` · ${record.message}` : ""}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </aside>
  );
}
