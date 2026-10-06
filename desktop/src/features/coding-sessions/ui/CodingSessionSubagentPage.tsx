import * as React from "react";
import { Bot } from "lucide-react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  useCloseCodingSessionSubagent,
  useCodingSessionSubagentScope,
  useOpenCodingSessionSubagentId,
} from "@/features/coding-sessions/lib/codingSessionSubagentNavigation";
import {
  type CodingSessionSubagentPage as PageReading,
  codingSessionSubagentPageRestingStatus,
  deriveCodingSessionSubagentBar,
  resolveCodingSessionSubagentLineage,
  resolveCodingSessionSubagentPage,
} from "@/features/coding-sessions/lib/codingSessionSubagentPageModel";
import {
  subscribeCodingSessionSubagentPages,
  takeCodingSessionSubagentPageReturn,
} from "@/features/coding-sessions/lib/codingSessionSubagentPageStore";
import { acquireEscapeSurface } from "@/shared/hooks/escapeSurfaces";
import { Markdown } from "@/shared/ui/markdown";
import { CodingSessionColumn } from "./CodingSessionColumn";
import { CodingSessionSubagentBar } from "./CodingSessionSubagentBar";
import { CodingSessionSubagentPageLineage } from "./CodingSessionSubagentPageLineage";
import { CodingSessionSubagentPromptBlock } from "./CodingSessionSubagentPagePrompt";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import { revealCodingSessionSubagentRow } from "./CodingSessionSubagentPageReveal";
import {
  type CodingSessionSurfaceCtx,
  useCodingSessionSurfaceCtx,
} from "./surfaces/codingSessionSurfaceContext";

/**
 * A subagent opened as its own page (SV-79), covering the session's
 * transcript pane while it is open.
 *
 * Mounted once per workspace inside the transcript region; it renders nothing
 * until a row opens a subagent (`useOpenCodingSessionSubagent`). The parent
 * transcript stays mounted underneath, so "Open parent" (or Escape) returns to
 * it exactly where it was and then scrolls to the subagent's row. The page
 * covers the composer too: a subagent takes no prompts, and a composer over
 * its page would address the lead while reading as if it addressed the page.
 *
 * SV-98 (T3's subagent page): a "Subagent of · <parent>" divider with the
 * parent's live status heads the transcript, and the facts bar is docked
 * where the composer was, saying in that spot that nobody types to it.
 */
export function CodingSessionSubagentPage() {
  const ctx = useCodingSessionSurfaceCtx();
  const scope = useCodingSessionSubagentScope();
  const openId = useOpenCodingSessionSubagentId();
  const close = useCloseCodingSessionSubagent();

  // After the page closes, scroll the parent to the row it was opened from.
  React.useEffect(() => {
    if (scope === null || openId !== null) return;
    const scrollBack = () => {
      const returnTo = takeCodingSessionSubagentPageReturn(scope);
      if (returnTo === null) return;
      requestAnimationFrame(() => revealCodingSessionSubagentRow(returnTo));
    };
    scrollBack();
    return subscribeCodingSessionSubagentPages(scrollBack);
  }, [openId, scope]);

  if (!ctx || openId === null) return null;
  return (
    <OpenSubagentPage
      ctx={ctx}
      onClose={close}
      parentToolId={openId}
      key={openId}
    />
  );
}

function OpenSubagentPage({
  ctx,
  onClose,
  parentToolId,
}: {
  ctx: CodingSessionSurfaceCtx;
  onClose: (returnTo?: string | null) => void;
  parentToolId: string;
}) {
  const page = React.useMemo(
    () =>
      resolveCodingSessionSubagentPage({
        parentToolId,
        panel: ctx.subagents,
        transcript: ctx.transcript as readonly TranscriptItem[],
      }),
    [ctx.subagents, ctx.transcript, parentToolId],
  );
  const bar = React.useMemo(
    () =>
      page.kind === "spawn" ? deriveCodingSessionSubagentBar(page.row) : null,
    [page],
  );
  const returnTo = page.kind === "spawn" ? page.row.spawn.call.id : null;
  const openParent = React.useCallback(
    () => onClose(returnTo),
    [onClose, returnTo],
  );
  const lineage = useLineageOf(ctx, page);
  const generationId =
    lineage.generationId ?? ctx.focusedRecord?.generationId ?? "subagent";
  const rootRef = React.useRef<HTMLElement>(null);

  // Focus the page so Escape and the keyboard land here, not on the
  // composer it covers.
  React.useEffect(() => {
    rootRef.current?.focus({ preventScroll: true });
  }, []);
  React.useEffect(() => {
    // Registered as an Escape surface so the app-level "Escape marks the
    // channel read" listener, which registered first, yields to this page.
    const releaseSurface = acquireEscapeSurface();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      if (isEscapeOwnedElsewhere(event.target)) return;
      event.preventDefault();
      openParent();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      releaseSurface();
    };
  }, [openParent]);

  const status = page.kind === "spawn" ? page.row.status : null;
  return (
    <section
      aria-label={`Subagent: ${bar?.title ?? "unknown"}`}
      className="absolute inset-0 z-30 flex min-h-0 flex-col bg-background outline-none"
      data-parent-tool-id={parentToolId}
      data-testid="coding-session-subagent-page"
      ref={rootRef}
      tabIndex={-1}
    >
      <div
        className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain px-4"
        data-testid="coding-session-subagent-page-scroll"
      >
        <CodingSessionColumn className="flex flex-col gap-5 pt-6 pb-6" expanded>
          <CodingSessionSubagentPageLineage
            lineage={lineage}
            onOpenParent={openParent}
            sessionClosed={ctx.sessionClosed}
          />
          {page.kind === "missing-call" ? (
            <PageNotice testId="coding-session-subagent-page-missing-call">
              The call that started this subagent is not in this transcript
              {page.items.length > 0
                ? ", so its status and prompt are unknown. What it published is below."
                : ", and nothing it published is either."}
            </PageNotice>
          ) : (
            <CodingSessionSubagentPromptBlock prompt={page.prompt} />
          )}
          {page.items.length > 0 ? (
            <CodingSessionTranscript
              currentUserPubkey={ctx.currentUserPubkey}
              generationId={generationId}
              isWorking={status === "running"}
              items={page.items}
              restingStatus={codingSessionSubagentPageRestingStatus(status)}
              showWorkingIndicator={false}
            />
          ) : page.kind === "spawn" ? (
            <EmptyActivity
              running={status === "running"}
              unattributed={page.unattributed}
            />
          ) : null}
          {page.kind === "spawn" ? <ReturnedResult page={page} /> : null}
        </CodingSessionColumn>
      </div>
      <div
        className="shrink-0 bg-background px-4 pt-2 pb-4"
        data-testid="coding-session-subagent-bar-dock"
      >
        <CodingSessionColumn expanded>
          <CodingSessionSubagentBar
            bar={bar}
            onOpenParent={openParent}
            parentStatusLabel={
              lineage.status
                ? ctx.sessionClosed
                  ? "Closed"
                  : lineage.status.label
                : null
            }
          />
        </CodingSessionColumn>
      </div>
    </section>
  );
}

/** Whose subagent this is: the execution holding the call (SV-98). */
function useLineageOf(ctx: CodingSessionSurfaceCtx, page: PageReading) {
  const callId = page.kind === "spawn" ? page.row.spawn.call.id : null;
  return React.useMemo(
    () =>
      resolveCodingSessionSubagentLineage({
        callId,
        layout: ctx.layout,
        sessionTitle: ctx.umbrella.title,
        focusedExecution: ctx.focusedExecution,
        focusedRecord: ctx.focusedRecord,
        executions: ctx.executions,
      }),
    [
      callId,
      ctx.executions,
      ctx.focusedExecution,
      ctx.focusedRecord,
      ctx.layout,
      ctx.umbrella.title,
    ],
  );
}

/** Escape inside a field, a dialog or a menu is that control's to handle. */
function isEscapeOwnedElsewhere(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (
    target.closest(
      'input, textarea, select, [contenteditable="true"], [role="dialog"], [role="menu"], [role="listbox"]',
    )
  ) {
    return true;
  }
  return false;
}

function PageNotice({
  children,
  testId,
}: {
  children: React.ReactNode;
  testId: string;
}) {
  return (
    <p
      className="rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-sm text-muted-foreground"
      data-testid={testId}
    >
      {children}
    </p>
  );
}

function EmptyActivity({
  running,
  unattributed,
}: {
  running: boolean;
  unattributed: boolean;
}) {
  return (
    <div
      className="flex min-h-32 flex-col items-center justify-center text-center"
      data-testid="coding-session-subagent-page-empty"
    >
      <Bot aria-hidden className="size-4 text-muted-foreground" />
      <p className="mt-3 text-sm font-medium">
        No activity from this subagent yet
      </p>
      <p className="mt-1 text-sm text-muted-foreground">
        {unattributed
          ? "The provider did not attribute this subagent's steps to its call, so they cannot be shown here."
          : running
            ? "Its steps appear here as the provider publishes them."
            : "None of its steps reached this transcript."}
      </p>
    </div>
  );
}

/** What the subagent handed back to its parent, once it has. */
function ReturnedResult({
  page,
}: {
  page: Extract<PageReading, { kind: "spawn" }>;
}) {
  const { status, spawn } = page.row;
  const result = spawn.call.result.trim();
  if ((status !== "done" && status !== "failed") || !result) return null;
  return (
    <section
      className="flex flex-col gap-1.5 border-t border-border/60 pt-3"
      data-testid="coding-session-subagent-page-result"
    >
      <p className="text-2xs font-semibold tracking-wide text-muted-foreground uppercase">
        {status === "failed"
          ? "Failed — returned to parent"
          : "Returned to parent"}
      </p>
      <Markdown className="text-sm" content={result} />
    </section>
  );
}
