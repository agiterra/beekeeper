import { FileDiff, GitCompare } from "lucide-react";

import type { CodingSessionChangedFile } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  FileEditDiffBlock,
  hasFileEditLineDiff,
} from "@/features/agents/ui/FileEditDiffView";
import { cn } from "@/shared/lib/cn";

export const CODING_SESSION_CHANGES_RAIL_ID = "coding-session-changes-rail";

export function CodingSessionChangesRail({
  files,
  variant = "inline",
}: {
  files: CodingSessionChangedFile[];
  variant?: "inline" | "sheet";
}) {
  const additions = sumKnown(files, "additions");
  const deletions = sumKnown(files, "deletions");

  return (
    <aside
      aria-label="Session changes"
      className={cn(
        "flex min-h-0 shrink-0 flex-col bg-muted/10",
        variant === "inline"
          ? "w-96 border-l border-border/60"
          : "h-full w-full",
      )}
      data-testid="coding-session-changes-rail"
      id={CODING_SESSION_CHANGES_RAIL_ID}
    >
      <div
        className={cn(
          "flex h-14 shrink-0 items-center gap-2 border-b border-border/60 px-4",
          variant === "sheet" && "pr-14",
        )}
      >
        <GitCompare className="size-4 text-muted-foreground" />
        <h2 className="text-sm font-semibold">Changes</h2>
        {files.length > 0 ? (
          <span className="ml-auto rounded-full border border-border/60 bg-background/50 px-2 py-0.5 text-2xs font-medium tabular-nums text-muted-foreground">
            {files.length} {files.length === 1 ? "file" : "files"}
          </span>
        ) : null}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
        {files.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center px-6 text-center">
            <FileDiff className="size-5 text-muted-foreground/60" />
            <p className="mt-3 text-sm font-medium">No signed changes yet</p>
            <p className="mt-1 max-w-56 text-xs leading-5 text-muted-foreground">
              File edits reported by this execution will collect here.
            </p>
          </div>
        ) : (
          <div className="space-y-2 p-3">
            {files.map((file) => (
              <ChangedFileDisclosure file={file} key={file.path} />
            ))}
          </div>
        )}
      </div>

      {files.length > 0 ? (
        <div className="flex shrink-0 items-center justify-between border-t border-border/60 px-4 py-3 text-2xs text-muted-foreground">
          <span>From signed session activity</span>
          {additions !== null && deletions !== null ? (
            <DiffStats additions={additions} deletions={deletions} />
          ) : null}
        </div>
      ) : null}
    </aside>
  );
}

function ChangedFileDisclosure({ file }: { file: CodingSessionChangedFile }) {
  const diffs = file.diffs.filter(hasFileEditLineDiff);
  const summary = (
    <>
      <FileDiff className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1" title={file.path}>
        <span className="block truncate font-mono text-xs text-foreground">
          {file.filename}
        </span>
        {file.path !== file.filename ? (
          <span className="mt-0.5 block truncate font-mono text-2xs text-muted-foreground">
            {parentPath(file.path)}
          </span>
        ) : null}
      </span>
      {file.additions !== null && file.deletions !== null ? (
        <DiffStats additions={file.additions} deletions={file.deletions} />
      ) : null}
    </>
  );

  if (diffs.length === 0) {
    return (
      <div className="flex min-h-9 items-center gap-2 rounded-lg border border-border/50 bg-background/40 px-3">
        {summary}
      </div>
    );
  }

  return (
    <details className="group overflow-hidden rounded-lg border border-border/50 bg-background/40">
      <summary className="flex min-h-9 cursor-pointer list-none items-center gap-2 px-3 hover:bg-muted/35">
        {summary}
        <span className="text-2xs text-muted-foreground group-open:hidden">
          View
        </span>
        <span className="hidden text-2xs text-muted-foreground group-open:inline">
          Hide
        </span>
      </summary>
      <div className="space-y-2 border-t border-border/50 p-2">
        {diffs.map((diff) => (
          <FileEditDiffBlock diff={diff} key={diff.id} />
        ))}
      </div>
    </details>
  );
}

function DiffStats({
  additions,
  deletions,
}: {
  additions: number;
  deletions: number;
}) {
  return (
    <span className="inline-flex shrink-0 gap-1 font-mono text-2xs tabular-nums">
      <span className="text-emerald-600 dark:text-emerald-400">
        +{additions}
      </span>
      <span className="text-rose-600 dark:text-rose-400">-{deletions}</span>
    </span>
  );
}

function sumKnown(
  files: CodingSessionChangedFile[],
  key: "additions" | "deletions",
): number | null {
  if (files.some((file) => file[key] === null)) return null;
  return files.reduce((total, file) => total + (file[key] ?? 0), 0);
}

function parentPath(path: string): string {
  const separator = path.lastIndexOf("/");
  return separator < 0 ? "" : path.slice(0, separator);
}
