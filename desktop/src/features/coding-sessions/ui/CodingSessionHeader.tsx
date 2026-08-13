import {
  ArrowLeft,
  Download,
  ExternalLink,
  Info,
  ListChecks,
  UserPlus,
} from "lucide-react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { CODING_SESSION_TASK_RAIL_ID } from "./CodingSessionTaskRail";

type CodingSessionHeaderProps = {
  channelName: string | null;
  compact?: boolean;
  generationLabel: string;
  isExporting?: boolean;
  model?: string | null;
  /**
   * Opens the join flow that attaches another provider execution to this
   * session (design §B). Absent when the session cannot take one — a
   * pre-umbrella session with no `sessionRef`, or a non-member view.
   */
  onAddProvider?: () => void;
  onBack: () => void;
  onExport?: () => void;
  onPopout?: () => void;
  onToggleTaskRail?: () => void;
  projectName?: string | null;
  providerAuthorityPubkey?: string | null;
  repoName?: string | null;
  runtimeLabel?: string | null;
  sessionTitle?: string | null;
  status: CodingSessionWorkspaceStatus;
  taskCount?: number;
  taskRailOpen?: boolean;
};

export function CodingSessionHeader({
  channelName,
  compact = false,
  generationLabel,
  isExporting = false,
  model = null,
  onAddProvider,
  onBack,
  onExport,
  onPopout,
  onToggleTaskRail,
  projectName = null,
  providerAuthorityPubkey = null,
  repoName = null,
  runtimeLabel = null,
  sessionTitle = null,
  status,
  taskCount = 0,
  taskRailOpen = false,
}: CodingSessionHeaderProps) {
  const title = sessionTitle?.trim() || "Coding session";
  const conciseGenerationLabel = removeRepeatedTitle(generationLabel, title);
  const contextLabels = uniqueNonemptyLabels([
    projectName,
    repoName,
    runtimeLabel,
    model,
    conciseGenerationLabel,
  ]);

  return (
    <header
      className="flex h-14 shrink-0 items-center gap-3 border-b border-border/60 bg-background/85 px-4 backdrop-blur-xl"
      data-compact={compact ? "true" : "false"}
      data-tauri-drag-region="deep"
      data-testid="coding-session-header"
    >
      <Button
        aria-label="Back from coding session"
        data-testid="coding-session-back"
        onClick={onBack}
        size="icon"
        type="button"
        variant="ghost"
      >
        <ArrowLeft />
      </Button>
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-sm font-semibold">{title}</h1>
        <p className="truncate text-xs text-muted-foreground">
          {contextLabels.length > 0
            ? contextLabels.join(" · ")
            : channelName
              ? `#${channelName}`
              : generationLabel}
        </p>
      </div>
      <Badge
        aria-label={`Session status: ${status.label}`}
        className={cn("gap-1.5", compact && "px-2")}
        title={status.label}
        variant="outline"
      >
        <span
          aria-hidden
          className={cn(
            "h-2 w-2 rounded-full",
            status.kind === "working"
              ? "bg-emerald-500"
              : status.kind === "idle" || status.kind === "ended"
                ? "bg-muted-foreground/50"
                : "bg-amber-500",
          )}
        />
        {compact ? (
          <span className="sr-only">{status.label}</span>
        ) : (
          status.label
        )}
      </Badge>
      <Popover>
        <PopoverTrigger asChild>
          <Button
            aria-label="Show session provenance"
            data-testid="coding-session-provenance-toggle"
            size="icon-xs"
            type="button"
            variant="ghost"
          >
            <Info />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-72">
          <p className="text-sm font-medium">Shared session details</p>
          <dl className="mt-3 grid gap-2 text-xs">
            {channelName ? (
              <div>
                <dt className="text-muted-foreground">Channel</dt>
                <dd className="mt-0.5 wrap-break-word">#{channelName}</dd>
              </div>
            ) : null}
            <div>
              <dt className="text-muted-foreground">Signed projection</dt>
              <dd className="mt-0.5 wrap-break-word">{generationLabel}</dd>
            </div>
            {providerAuthorityPubkey ? (
              <div>
                <dt className="text-muted-foreground">Verified source</dt>
                <dd className="mt-0.5 font-mono wrap-break-word">
                  {shortPubkey(providerAuthorityPubkey)}
                </dd>
              </div>
            ) : null}
          </dl>
        </PopoverContent>
      </Popover>
      {onToggleTaskRail ? (
        <Button
          aria-controls={CODING_SESSION_TASK_RAIL_ID}
          aria-expanded={taskRailOpen}
          aria-label={taskRailOpen ? "Hide session plan" : "Show session plan"}
          data-testid="coding-session-task-rail-toggle"
          onClick={onToggleTaskRail}
          size={compact ? "icon" : "sm"}
          type="button"
          variant={taskRailOpen ? "secondary" : "ghost"}
        >
          <ListChecks />
          <span className={compact ? "sr-only" : undefined}>Plan</span>
          {taskCount > 0 ? (
            <>
              <span
                aria-hidden
                className={cn(
                  "rounded-full bg-background/70 px-1.5 text-xs",
                  compact && "sr-only",
                )}
              >
                {taskCount}
              </span>
              <span className="sr-only">{taskCount} tasks</span>
            </>
          ) : null}
        </Button>
      ) : null}
      {onAddProvider ? (
        <Button
          aria-label="Add a provider to this session"
          data-testid="coding-session-add-provider"
          onClick={onAddProvider}
          size={compact ? "icon" : "sm"}
          title="Add another provider to this session"
          type="button"
          variant="ghost"
        >
          <UserPlus />
          <span className={compact ? "sr-only" : undefined}>Add provider</span>
        </Button>
      ) : null}
      {onExport ? (
        <Button
          aria-label="Export transcript"
          data-testid="coding-session-export"
          disabled={isExporting}
          onClick={onExport}
          size="icon"
          type="button"
          variant="ghost"
        >
          <Download />
        </Button>
      ) : null}
      {onPopout ? (
        <Button
          data-testid="coding-session-header-popout"
          onClick={onPopout}
          size={compact ? "icon" : "sm"}
          type="button"
          variant="outline"
        >
          <ExternalLink />
          <span className={compact ? "sr-only" : undefined}>Pop out</span>
        </Button>
      ) : null}
    </header>
  );
}

function shortPubkey(value: string): string {
  return value.length <= 20
    ? value
    : `${value.slice(0, 10)}…${value.slice(-8)}`;
}

function removeRepeatedTitle(generationLabel: string, title: string): string {
  const prefix = `${title} · `;
  return generationLabel.startsWith(prefix)
    ? generationLabel.slice(prefix.length)
    : generationLabel;
}

function uniqueNonemptyLabels(
  values: ReadonlyArray<string | null | undefined>,
): string[] {
  const labels: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    const label = value?.trim();
    if (!label || seen.has(label)) continue;
    seen.add(label);
    labels.push(label);
  }
  return labels;
}
