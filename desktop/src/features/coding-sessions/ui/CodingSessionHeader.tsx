import {
  ArrowLeft,
  Download,
  ExternalLink,
  GitCompare,
  Info,
  ListChecks,
  Pencil,
  RotateCcw,
  Square,
  UserPlus,
  Users,
} from "lucide-react";

import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { CODING_SESSION_TASK_RAIL_ID } from "./CodingSessionTaskRail";

/**
 * A compact direct affordance for one surface tab of the shared right-side
 * surface host. Clicking an inactive affordance opens the host on that tab;
 * clicking the active one closes the host.
 */
export type CodingSessionHeaderSurfaceTab = {
  id: string;
  label: string;
  icon: "agents" | "changes";
  count?: number;
  active: boolean;
};

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
  /** Publishes the provider-independent shared closure fact. */
  onCloseSession?: () => void;
  onBack: () => void;
  onExport?: () => void;
  /**
   * Opens the session People surface (roster + invite/share). Absent when
   * the session has no authority chain to share (no genesis).
   */
  onOpenPeople?: () => void;
  onPopout?: () => void;
  /** Opens the founder-authorized session rename flow. */
  onRename?: () => void;
  /** Reopens the durable session without starting a provider execution. */
  onReopenSession?: () => void;
  /** Opens the owning project. Given one, the project reads as a crumb you can
   * follow rather than a word in a context line. */
  onOpenProject?: () => void;
  onToggleTaskRail?: () => void;
  /** Toggles the shared surface host open/closed on the given surface tab. */
  onToggleSurface?: (id: string) => void;
  /** People with access to the session (owner + grants), for the badge. */
  peopleCount?: number;
  projectName?: string | null;
  providerAuthorityPubkey?: string | null;
  repoName?: string | null;
  runtimeLabel?: string | null;
  sessionTitle?: string | null;
  sessionClosed?: boolean;
  status: CodingSessionWorkspaceStatus;
  /** DOM id of the surface host panel, for `aria-controls`. */
  surfaceHostId?: string;
  surfaceTabs?: readonly CodingSessionHeaderSurfaceTab[];
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
  onCloseSession,
  onExport,
  onOpenPeople,
  onOpenProject,
  onPopout,
  onRename,
  onReopenSession,
  onToggleTaskRail,
  onToggleSurface,
  peopleCount = 0,
  projectName = null,
  providerAuthorityPubkey = null,
  repoName = null,
  runtimeLabel = null,
  sessionTitle = null,
  sessionClosed = false,
  status,
  surfaceHostId,
  surfaceTabs,
  taskCount = 0,
  taskRailOpen = false,
}: CodingSessionHeaderProps) {
  const title = sessionTitle?.trim() || "Coding session";
  const conciseGenerationLabel = removeRepeatedTitle(generationLabel, title);
  const linkedProject = onOpenProject ? projectName?.trim() || null : null;
  const contextLabels = uniqueNonemptyLabels([
    // A linked project is rendered on its own so it stays clickable; only an
    // unlinked one folds into the plain context line.
    linkedProject ? null : projectName,
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
        <div className="flex min-w-0 items-center gap-1">
          <h1 className="truncate text-sm font-semibold">{title}</h1>
          {onRename ? (
            <Button
              aria-label="Rename session"
              className="shrink-0"
              data-testid="coding-session-rename"
              onClick={onRename}
              size="icon-xs"
              title="Rename session"
              type="button"
              variant="ghost"
            >
              <Pencil />
            </Button>
          ) : null}
        </div>
        <p className="truncate text-xs text-muted-foreground">
          {linkedProject ? (
            <>
              <button
                className="rounded-sm underline-offset-2 hover:text-foreground hover:underline focus-visible:text-foreground focus-visible:underline"
                data-testid="coding-session-project-crumb"
                onClick={onOpenProject}
                title={`Open ${linkedProject}`}
                type="button"
              >
                {linkedProject}
              </button>
              {contextLabels.length > 0
                ? ` · ${contextLabels.join(" · ")}`
                : ""}
            </>
          ) : contextLabels.length > 0 ? (
            contextLabels.join(" · ")
          ) : channelName ? (
            `#${channelName}`
          ) : (
            generationLabel
          )}
        </p>
      </div>
      <Badge
        aria-label={`Session status: ${sessionClosed ? "Closed" : status.label}`}
        className={cn("gap-1.5", compact && "px-2")}
        title={sessionClosed ? "Closed" : status.label}
        variant="outline"
      >
        <span
          aria-hidden
          className={cn(
            "h-2 w-2 rounded-full",
            !sessionClosed && status.kind === "working"
              ? "bg-emerald-500"
              : status.kind === "idle" || status.kind === "ended"
                ? "bg-muted-foreground/50"
                : // A lifecycle-signed "Disconnected" or "Needs attention"
                  // reads like the execution rail's own attention state, not
                  // like an unread status.
                  status.kind === "unknown" && status.attention
                  ? "bg-destructive"
                  : "bg-amber-500",
          )}
        />
        {compact ? (
          <span className="sr-only">
            {sessionClosed ? "Closed" : status.label}
          </span>
        ) : sessionClosed ? (
          "Closed"
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
            {projectName ? (
              <div>
                <dt className="text-muted-foreground">Project</dt>
                <dd className="mt-0.5 wrap-break-word">{projectName}</dd>
              </div>
            ) : null}
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
      {onToggleSurface && surfaceTabs
        ? surfaceTabs.map((tab) => (
            <Button
              aria-controls={surfaceHostId}
              aria-expanded={tab.active}
              aria-label={
                tab.active
                  ? `Hide ${tab.label.toLowerCase()}`
                  : `Show ${tab.label.toLowerCase()}`
              }
              data-testid={`coding-session-surface-toggle-${tab.id}`}
              key={tab.id}
              onClick={() => onToggleSurface(tab.id)}
              size={compact ? "icon" : "sm"}
              type="button"
              variant={tab.active ? "secondary" : "ghost"}
            >
              {tab.icon === "agents" ? <Users /> : <GitCompare />}
              <span className={compact ? "sr-only" : undefined}>
                {tab.label}
              </span>
              {tab.count !== undefined && tab.count > 0 ? (
                <span
                  className={cn(
                    "rounded-full bg-background/70 px-1.5 text-xs",
                    compact && "sr-only",
                  )}
                >
                  {tab.count}
                </span>
              ) : null}
            </Button>
          ))
        : null}
      {onOpenPeople ? (
        <Button
          aria-label="Show session people"
          data-testid="coding-session-people-toggle"
          onClick={onOpenPeople}
          size={compact ? "icon" : "sm"}
          title="People with access to this session"
          type="button"
          variant="ghost"
        >
          <Users />
          <span className={compact ? "sr-only" : undefined}>People</span>
          {peopleCount > 0 ? (
            <>
              <span
                aria-hidden
                className={cn(
                  "rounded-full bg-background/70 px-1.5 text-xs",
                  compact && "sr-only",
                )}
              >
                {peopleCount}
              </span>
              <span className="sr-only">{peopleCount} people</span>
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
      {onCloseSession ? (
        <Button
          aria-label="Close session"
          data-testid="coding-session-close"
          onClick={onCloseSession}
          size={compact ? "icon" : "sm"}
          title="Move this session to Settled without stopping its providers"
          type="button"
          variant="ghost"
        >
          <Square />
          <span className={compact ? "sr-only" : undefined}>Close</span>
        </Button>
      ) : null}
      {onReopenSession ? (
        <Button
          aria-label="Reopen session"
          data-testid="coding-session-reopen"
          onClick={onReopenSession}
          size={compact ? "icon" : "sm"}
          title="Return this session to Sessions without starting a provider"
          type="button"
          variant="outline"
        >
          <RotateCcw />
          <span className={compact ? "sr-only" : undefined}>Reopen</span>
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
