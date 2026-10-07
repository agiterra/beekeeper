/**
 * The Git-backed Diff surface (SV-30).
 *
 * Two git scopes from the session's signed checkpoints (kind 44231) — one
 * turn (`baseTree` → `tree`) or the whole session (first baseline → latest
 * tree) — plus the observed-edits rail it replaces as the default. The patch
 * comes from this computer's own checkout (`coding_session_checkpoint_diff`);
 * when the trees are not here the surface lists the files the checkpoint
 * signed and says the full diff lives on the computer that ran the turn.
 *
 * Never an empty diff: a range git could not read, a missing baseline, or a
 * measured no-change each say which one it is.
 */
import * as React from "react";
import { useQuery } from "@tanstack/react-query";
import { FileDiff } from "lucide-react";

import { FileContentBlock } from "@/features/agents/ui/FileContentBlock";
import {
  type CodingSessionCheckpointDiffAnswer,
  type CodingSessionCheckpointDiffFile,
  fetchCodingSessionCheckpointDiff,
} from "@/shared/api/codingSessionCheckpointDiff";
import { cn } from "@/shared/lib/cn";
import {
  type CodingSessionDiffListedFile,
  type CodingSessionDiffScope,
  codingSessionPatchLines,
  resolveCodingSessionDiffRange,
} from "../lib/codingSessionCheckpointChanges";
import type {
  CodingSessionCheckpointEntry,
  CodingSessionGenerationCheckpoints,
} from "../lib/codingSessionCheckpoints";
import { CodingSessionSurfaceSubheader } from "./CodingSessionChangesRailSubheader";
import {
  codingSessionGenerationCheckpointsOf,
  useCodingSessionCheckpointsRead,
} from "./CodingSessionCheckpointsContext";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";

export const CODING_SESSION_REMOTE_DIFF_SENTENCE =
  "Full diff lives on the computer that ran this turn.";

type DiffView = CodingSessionDiffScope | "observed";

/**
 * The generation the Diff surface reads: the focused execution's, else (the
 * umbrella with nothing focused) the one with the newest checkpoint. Null
 * when no generation in view has a turn checkpoint.
 */
export function useCodingSessionDiffGeneration(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionGenerationCheckpoints | null {
  const read = useCodingSessionCheckpointsRead();
  const focusedId = ctx.focusedRecord?.generationId ?? null;
  return React.useMemo(() => {
    const focused = codingSessionGenerationCheckpointsOf(read, focusedId);
    if (focused && focused.turns.length > 0) return focused;
    if (!read || focusedId !== null) return null;
    let newest: CodingSessionGenerationCheckpoints | null = null;
    let newestAt = -1;
    for (const generation of read.fold.byScope.values()) {
      const at = generation.turns.at(-1)?.createdAt ?? -1;
      if (at > newestAt) {
        newest = generation;
        newestAt = at;
      }
    }
    return newest;
  }, [focusedId, read]);
}

export function CodingSessionDiffSurface({
  ctx,
  generation,
  observed,
}: {
  ctx: CodingSessionSurfaceCtx;
  generation: CodingSessionGenerationCheckpoints;
  /** The observed-edits rail, offered as the third view. */
  observed: React.ReactNode;
}) {
  const read = useCodingSessionCheckpointsRead();
  const [view, setView] = React.useState<DiffView>("turn");
  const [turnEventId, setTurnEventId] = React.useState<string | null>(null);
  const turn =
    generation.turns.find((entry) => entry.eventId === turnEventId) ??
    generation.turns.at(-1) ??
    null;
  const turnNumber = turn ? generation.turns.indexOf(turn) + 1 : 0;
  const range = React.useMemo(
    () =>
      resolveCodingSessionDiffRange({
        generation,
        scope: view === "session" ? "session" : "turn",
        turn,
      }),
    [generation, turn, view],
  );
  const sessionId = generation.turns[0]?.payload.session.sessionId ?? "";
  const sessionRef = ctx.umbrella.sessionRef ?? "";
  const projectRef = ctx.projectRef;
  const fromTree = range.kind === "range" ? range.fromTree : null;
  const toTree = range.kind === "range" ? range.toTree : null;
  const diff = useQuery({
    queryKey: [
      "coding-session-checkpoint-diff",
      sessionRef,
      sessionId,
      projectRef,
      fromTree,
      toTree,
    ],
    enabled: view !== "observed" && fromTree !== null && toTree !== null,
    retry: false,
    // Tree ids name immutable content: one answer per pair is the answer.
    staleTime: Number.POSITIVE_INFINITY,
    queryFn: () =>
      fetchCodingSessionCheckpointDiff({
        sessionRef,
        target: sessionId,
        fromTree,
        toTree: toTree ?? "",
        projectRef,
      }),
  });
  const answer = diff.data ?? null;
  const rejected = read
    ? read.fold.rejected.foreignSigner +
      read.fold.rejected.malformed +
      read.fold.rejected.invalidSignature
    : 0;

  const meta =
    view === "observed"
      ? "Observed edits · not a git diff"
      : view === "session"
        ? `From git · whole session · ${generation.turns.length} ${generation.turns.length === 1 ? "checkpoint" : "checkpoints"}`
        : `From git · checkpoint ${turnNumber} of ${generation.turns.length}`;

  return (
    <div
      className="flex h-full min-h-0 flex-1 flex-col bg-muted/10"
      data-testid="coding-session-diff-surface"
      data-view={view}
    >
      <CodingSessionSurfaceSubheader
        meta={<span data-testid="coding-session-diff-provenance">{meta}</span>}
        title="Diff"
      />
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-border/60 px-3 py-2">
        <fieldset
          aria-label="Diff scope"
          className="m-0 inline-flex min-w-0 rounded-lg border-0 bg-muted/40 p-0.5"
        >
          {(
            [
              ["turn", "Turn"],
              ["session", "Session"],
              ["observed", "Observed"],
            ] as const
          ).map(([id, label]) => (
            <button
              aria-pressed={view === id}
              className={cn(
                "rounded-md px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:text-foreground",
                view === id && "bg-background text-foreground shadow-sm",
              )}
              data-testid={`coding-session-diff-scope-${id}`}
              key={id}
              onClick={() => setView(id)}
              type="button"
            >
              {label}
            </button>
          ))}
        </fieldset>
        {view === "turn" && generation.turns.length > 1 ? (
          <select
            aria-label="Checkpointed turn"
            className="min-w-0 rounded-md border border-border/60 bg-background px-1.5 py-0.5 text-xs"
            data-testid="coding-session-diff-turn"
            onChange={(event) => setTurnEventId(event.currentTarget.value)}
            value={turn?.eventId ?? ""}
          >
            {generation.turns.map((entry, index) => (
              <option key={entry.eventId} value={entry.eventId}>
                {turnOptionLabel(entry, index)}
              </option>
            ))}
          </select>
        ) : null}
      </div>
      {view === "observed" ? (
        <div className="flex min-h-0 flex-1 flex-col">{observed}</div>
      ) : (
        <div className="min-h-0 flex-1 space-y-2 overflow-y-auto overscroll-contain p-3">
          {range.kind === "range" && range.note ? (
            <Note testId="coding-session-diff-range-note">{range.note}</Note>
          ) : null}
          {range.kind === "none" ? (
            <>
              <Note testId="coding-session-diff-none">{range.reason}</Note>
              <ListedFiles files={range.files} filesNotListed={0} />
            </>
          ) : diff.isLoading ? (
            <>
              <Note testId="coding-session-diff-loading">
                Reading the diff from this computer&apos;s checkout…
              </Note>
              <ListedFiles
                files={range.files}
                filesNotListed={range.filesNotListed}
              />
            </>
          ) : diff.error ? (
            <>
              <Note testId="coding-session-diff-error">
                The diff could not be read here:{" "}
                {diff.error instanceof Error
                  ? diff.error.message
                  : String(diff.error)}
              </Note>
              <ListedFiles
                files={range.files}
                filesNotListed={range.filesNotListed}
              />
            </>
          ) : answer ? (
            <DiffAnswer
              answer={answer}
              files={range.files}
              filesNotListed={range.filesNotListed}
            />
          ) : null}
          {read?.capped ? (
            <Note testId="coding-session-diff-capped">
              Older checkpoints may be missing: the read reached its limit.
            </Note>
          ) : null}
          {rejected > 0 ? (
            <Note testId="coding-session-diff-rejected">
              {rejected}{" "}
              {rejected === 1 ? "checkpoint was" : "checkpoints were"} refused
              (not signed by this session&apos;s provider, or malformed).
            </Note>
          ) : null}
        </div>
      )}
    </div>
  );
}

function turnOptionLabel(
  entry: CodingSessionCheckpointEntry,
  index: number,
): string {
  const { payload } = entry;
  if (!payload.git) return `Checkpoint ${index + 1} · no git facts`;
  if (payload.git.baseTree === null) {
    return `Checkpoint ${index + 1} · baseline not captured`;
  }
  const total = payload.files.length + payload.filesNotListed;
  return `Checkpoint ${index + 1} · ${total} ${total === 1 ? "file" : "files"}`;
}

function DiffAnswer({
  answer,
  files,
  filesNotListed,
}: {
  answer: CodingSessionCheckpointDiffAnswer;
  files: readonly CodingSessionDiffListedFile[];
  filesNotListed: number;
}) {
  if (answer.state === "local") {
    if (answer.files.length === 0 && answer.filesNotListed === 0) {
      return (
        <Note testId="coding-session-diff-no-change">
          Git found no change between these checkpoints.
        </Note>
      );
    }
    return (
      <div className="space-y-2" data-testid="coding-session-diff-local">
        <p className="flex items-center justify-between text-2xs text-muted-foreground">
          <span>
            Read from this computer&apos;s{" "}
            {answer.checkout === "seat_worktree"
              ? "seat worktree"
              : "project checkout"}
          </span>
          <Stats additions={answer.additions} deletions={answer.deletions} />
        </p>
        {answer.files.map((file) => (
          <PatchFile file={file} key={file.path} />
        ))}
        {answer.filesNotListed > 0 ? (
          <Note testId="coding-session-diff-not-listed">
            {answer.filesNotListed} more{" "}
            {answer.filesNotListed === 1 ? "file" : "files"} changed but not
            listed.
          </Note>
        ) : null}
      </div>
    );
  }
  if (answer.state === "baseline_missing") {
    return (
      <>
        <Note testId="coding-session-diff-none">Baseline not captured.</Note>
        <ListedFiles files={files} filesNotListed={filesNotListed} />
      </>
    );
  }
  return (
    <>
      <Note testId="coding-session-diff-remote">
        {CODING_SESSION_REMOTE_DIFF_SENTENCE}{" "}
        {answer.state === "objects_missing"
          ? "This computer's checkout does not hold these checkpoints."
          : "This computer records no checkout for this session."}
      </Note>
      <ListedFiles files={files} filesNotListed={filesNotListed} />
    </>
  );
}

function PatchFile({ file }: { file: CodingSessionCheckpointDiffFile }) {
  const lines = React.useMemo(
    () =>
      codingSessionPatchLines(file.patch).filter(
        (line) => line.kind !== "meta",
      ),
    [file.patch],
  );
  return (
    <details
      className="group overflow-hidden rounded-lg border border-border/50 bg-background/40"
      data-testid="coding-session-diff-file"
      open
    >
      <summary className="flex min-h-9 cursor-pointer list-none items-center gap-2 px-3 hover:bg-muted/35">
        <FileDiff className="size-3.5 shrink-0 text-muted-foreground" />
        <span
          className="min-w-0 flex-1 truncate font-mono text-xs"
          title={file.path}
        >
          {file.path}
        </span>
        <Stats additions={file.additions} deletions={file.deletions} />
      </summary>
      <div className="border-t border-border/50 p-2">
        {lines.length > 0 ? (
          <FileContentBlock
            footerText={file.truncated ? `${file.path} · truncated` : undefined}
            lines={lines}
            path={file.path}
          />
        ) : (
          <p className="text-2xs text-muted-foreground">
            No line changes to show (binary, mode or rename only).
          </p>
        )}
      </div>
    </details>
  );
}

function ListedFiles({
  files,
  filesNotListed,
}: {
  files: readonly CodingSessionDiffListedFile[];
  filesNotListed: number;
}) {
  if (files.length === 0 && filesNotListed === 0) return null;
  return (
    <div className="space-y-1" data-testid="coding-session-diff-listed">
      <p className="text-2xs text-muted-foreground">
        Files the checkpoint lists
      </p>
      {files.map((file) => (
        <div
          className="flex min-h-8 items-center gap-2 rounded-lg border border-border/50 bg-background/40 px-3"
          data-testid="coding-session-diff-listed-file"
          key={file.path}
        >
          <span
            className="shrink-0 font-mono text-2xs text-muted-foreground"
            title={file.status}
          >
            {file.status.charAt(0).toUpperCase()}
          </span>
          <span
            className="min-w-0 flex-1 truncate font-mono text-xs"
            title={file.path}
          >
            {file.from ? `${file.from} → ${file.path}` : file.path}
          </span>
          {file.additions !== null && file.deletions !== null ? (
            <Stats additions={file.additions} deletions={file.deletions} />
          ) : null}
        </div>
      ))}
      {filesNotListed > 0 ? (
        <p className="text-2xs text-muted-foreground">
          {filesNotListed} more {filesNotListed === 1 ? "file" : "files"} not
          listed.
        </p>
      ) : null}
    </div>
  );
}

function Note({
  children,
  testId,
}: {
  children: React.ReactNode;
  testId: string;
}) {
  return (
    <p
      className="rounded-lg bg-muted/40 px-3 py-2 text-xs leading-5 text-muted-foreground"
      data-testid={testId}
    >
      {children}
    </p>
  );
}

function Stats({
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
