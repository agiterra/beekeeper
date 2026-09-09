import {
  Bot,
  Download,
  ExternalLink,
  GitCompare,
  Info,
  ListChecks,
  PanelLeft,
  PanelRight,
  Pencil,
  OctagonX,
  RotateCcw,
  Square,
  UserPlus,
  Users,
  X,
} from "lucide-react";
import * as React from "react";
import type { ReactNode } from "react";

import {
  renderCodingSessionContextLoad,
  type CodingSessionContextLoad,
} from "@/features/coding-sessions/lib/codingSessionContextLoad";
import {
  formatCodingSessionHireTally,
  summarizeCodingSessionHireOutcomes,
  type CodingSessionHireOutcomeState,
} from "@/features/coding-sessions/lib/codingSessionHireAnswer";
import { useCodingSessionHireOutcomes } from "@/features/coding-sessions/hooks/useCodingSessionHire";
import { formatCodingSessionModelSummary } from "@/features/coding-sessions/lib/codingSessionLabels";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type {
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  formatCodingSessionDispositionLine,
  listCodingSessionUmbrellaDispositions,
  type CodingSessionActorNameResolver,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import {
  codingSessionWorkspaceStatusDetail,
  deriveCodingSessionExecutionStatus,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import { CodingSessionHeaderOverflow } from "./CodingSessionHeaderOverflow";
import { useNewSessionInWorkspaceAction } from "@/features/coding-sessions/hooks/useNewSessionInWorkspaceAction";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";
import { CODING_SESSION_ROUTE_RAIL_ID } from "./CodingSessionRouteRail";
import { CODING_SESSION_TASK_RAIL_ID } from "./CodingSessionTaskRail";

/**
 * A compact direct affordance for one surface tab of the shared right-side
 * surface host. Clicking an inactive affordance opens the host on that tab;
 * clicking the active one closes the host.
 */
export type CodingSessionHeaderSurfaceTab = {
  id: string;
  label: string;
  icon: "agents" | "changes" | "inspector";
  count?: number;
  active: boolean;
};

type CodingSessionHeaderProps = {
  /** Multi-execution focus/status chips. Omitted for the effortless N=1 path. */
  agentControls?: ReactNode;
  channelName: string | null;
  compact?: boolean;
  /**
   * Drops this header's own bottom rule because it is row 1 of a taller
   * container that owns the rule itself. Mission passes it and lets the
   * participant bar (row 2) carry the single `border-b`; Conversation, where
   * the header is the whole container, leaves it false.
   */
  flush?: boolean;
  generationLabel: string;
  /**
   * Resolved founder label rendered inside the provenance popover.
   *
   * Absent, the row still renders and reads `unresolved` — W6 says a session
   * always has a founder, so a missing name is a resolution this client has
   * not completed, not a session nobody founded.
   */
  founderDetails?: ReactNode;
  /**
   * Per-execution context occupancy for the provenance popover (D7 / W12).
   *
   * Empty means this surface has no executions to report on and the section
   * is omitted entirely; a listed execution with a `null` load renders an em
   * dash, because nothing reported is not zero.
   */
  contextLoads?: readonly CodingSessionContextRow[];
  /**
   * The routed seats in this umbrella, one line each, for the provenance
   * popover. Omitted entirely when nothing here was routed.
   */
  routedSeats?: readonly CodingSessionRoutedSeatRow[];
  /** Durable session intent, shown directly below the title when present. */
  goalText?: string | null;
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
  /**
   * Stops every live seat of this umbrella at once.
   *
   * Founder-only, and *absent* rather than disabled for anybody else — see
   * `buildCodingSessionStopAll`. A greyed-out control invites the click that
   * teaches you the authority is not yours.
   */
  onStopAll?: () => void;
  /** How many seats {@link onStopAll} would stop. Named on the control. */
  stopAllCount?: number;
  /**
   * `Stop all (2 seats)` — the Mission overflow item's label.
   *
   * Present only in Mission. Conversation keeps `Stop all (2)` on its flat
   * button, byte for byte.
   */
  stopAllLabel?: string | null;
  /** The liveness split sentence, from the same W1 map the seat chips read. */
  stopAllSentence?: string | null;
  /**
   * Collapse the Mission Route rail to its 40 px scrubber, and back.
   *
   * B1: the Inspector has had a collapse control in this group since §19 and
   * the rail had none. Absent outside Mission, where there is no rail.
   */
  onToggleRouteRail?: () => void;
  /** Is that rail currently expanded? Reported as `aria-expanded`. */
  routeRailExpanded?: boolean;
  /**
   * Would expanding the rail have to close the Inspector to fit?
   *
   * F3: a control that silently cannot act is worse than one that says what
   * acting costs. This puts the cost in the title.
   */
  routeRailDisplacesInspector?: boolean;
  /**
   * Collapse the six actions into one `⋯` (DESIGN-SPEC A7). Mission only —
   * Conversation's flat run of six buttons and its DOM do not move (I8).
   */
  missionActions?: boolean;
  /**
   * Dismisses the surface this header sits in — the pop-out window, or the
   * create dialog. Absent in the main window, where the app's own
   * back/forward in the top chrome is the way out of a session; a second
   * arrow here was that same gesture wearing a different icon.
   */
  onClose?: () => void;
  /** Accessible name for the close control; say what it dismisses. */
  closeLabel?: string;
  onExport?: () => void;
  /**
   * The session this header belongs to, for the `⋯` menu's "New session in
   * this workspace" item. Absent — a pending, loading or unavailable header —
   * the item is not offered, because there is no session whose workspace
   * could be looked up. `providerAuthorityPubkey` is reused as the viewed
   * execution's provider, which is the comparison that tells a foreign
   * execution from one whose location is simply unknown.
   */
  workspaceReuse?: { channelId: string; sessionRef: string | null } | null;
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
  /**
   * The agent seat this execution runs as, or null for a human-created one.
   *
   * Labelled by `formatCodingSessionExecutionLabel` so a seated execution
   * reads the same way here, on the pending screen, and on its execution card
   * — an agent's work must never present itself as a person's.
   */
  seat?: { label: string } | null;
  sessionTitle?: string | null;
  sessionClosed?: boolean;
  status: CodingSessionWorkspaceStatus;
  /** Aggregate label for an umbrella; per-agent truth lives in agentControls. */
  statusLabelOverride?: string | null;
  /** DOM id of the surface host panel, for `aria-controls`. */
  surfaceHostId?: string;
  surfaceTabs?: readonly CodingSessionHeaderSurfaceTab[];
  taskCount?: number;
  taskRailOpen?: boolean;
  /** Local-only Conversation/Mission choice, kept beside the session title. */
  viewControl?: ReactNode;
};

export function CodingSessionHeader({
  agentControls,
  channelName,
  compact = false,
  flush = false,
  contextLoads,
  routedSeats,
  generationLabel,
  founderDetails,
  goalText = null,
  isExporting = false,
  model = null,
  onAddProvider,
  closeLabel = "Close",
  onClose,
  onCloseSession,
  onExport,
  onOpenPeople,
  onStopAll,
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
  seat = null,
  sessionTitle = null,
  sessionClosed = false,
  status,
  statusLabelOverride = null,
  missionActions = false,
  onToggleRouteRail,
  routeRailDisplacesInspector = false,
  routeRailExpanded = false,
  stopAllCount = 0,
  stopAllLabel = null,
  stopAllSentence = null,
  surfaceHostId,
  surfaceTabs,
  taskCount = 0,
  taskRailOpen = false,
  viewControl,
  workspaceReuse = null,
}: CodingSessionHeaderProps) {
  // "New session in this workspace" — the `⋯` menu's one item, resolved on
  // menu open by the same hook the sidebar row's item uses, so a session
  // cannot read two ways at once. The hook itself reads nothing until asked.
  const newSessionHere = useNewSessionInWorkspaceAction({
    channelId: workspaceReuse?.channelId ?? null,
    executionProviderPubkey: providerAuthorityPubkey,
    sessionRef: workspaceReuse?.sessionRef ?? null,
  });
  const { resolveNow: resolveNewSessionHere } = newSessionHere;
  const handleOverflowOpenChange = React.useCallback(
    (open: boolean) => {
      if (open) resolveNewSessionHere();
    },
    [resolveNewSessionHere],
  );
  const title = sessionTitle?.trim() || "Coding session";
  // A demoted status carries its own history clause; the badge states both so
  // the header never presents a stale report as the current condition.
  const statusDetail = codingSessionWorkspaceStatusDetail(status);
  const statusText =
    statusLabelOverride ??
    (statusDetail === null
      ? status.label
      : `${status.label} · ${statusDetail}`);
  const conciseGenerationLabel = removeRepeatedTitle(generationLabel, title);
  const modelLabel = model ? formatCodingSessionModelSummary(model) : null;
  const linkedProject = onOpenProject ? projectName?.trim() || null : null;
  const contextLabels = uniqueNonemptyLabels([
    // A linked project is rendered on its own so it stays clickable; only an
    // unlinked one folds into the plain context line.
    linkedProject ? null : projectName,
    repoName,
    runtimeLabel,
    modelLabel,
    conciseGenerationLabel,
  ]);

  return (
    <header
      // Mission mounts the participant bar as row 2 of the same container, and
      // the container gets exactly one bottom rule — below row 2. In
      // Conversation this header IS the whole container, so it keeps its own.
      //
      // Written as two whole literals rather than a `cn(...)` merge, and the
      // `data-flush` attribute omitted rather than set to "false", so the
      // Conversation lens emits byte-identical markup to before this prop
      // existed (I8). A `cn` merge reorders the class string, which is
      // cosmetically identical and still a DOM change.
      className={
        flush
          ? "flex h-14 shrink-0 items-center gap-3 bg-background/85 px-4 backdrop-blur-xl"
          : "flex h-14 shrink-0 items-center gap-3 border-b border-border/60 bg-background/85 px-4 backdrop-blur-xl"
      }
      data-compact={compact ? "true" : "false"}
      data-flush={flush ? "true" : undefined}
      data-tauri-drag-region="deep"
      data-testid="coding-session-header"
    >
      {onClose ? (
        <Button
          aria-label={closeLabel}
          data-testid="coding-session-dismiss"
          onClick={onClose}
          size="icon"
          type="button"
          variant="ghost"
        >
          <X />
        </Button>
      ) : null}
      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1">
          <h1 className="truncate text-sm font-semibold">{title}</h1>
          {seat ? (
            <Badge
              className="shrink-0 gap-1.5"
              data-testid="coding-session-header-seat"
              title={`Seated: ${seat.label}`}
              variant="outline"
            >
              <Bot aria-hidden className="size-3" />
              {seat.label}
            </Badge>
          ) : null}
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
          {viewControl ? (
            <div className="ml-2 shrink-0">{viewControl}</div>
          ) : null}
        </div>
        <p className="truncate text-xs text-muted-foreground">
          {goalText?.trim() ? (
            goalText.trim()
          ) : linkedProject ? (
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
      {agentControls ? (
        <div className="hidden min-w-0 max-w-[min(38vw,36rem)] md:flex">
          {agentControls}
        </div>
      ) : null}
      {!agentControls ? (
        <Badge
          aria-label={`Session status: ${sessionClosed ? "Closed" : statusText}`}
          className={cn("gap-1.5", compact && "px-2")}
          data-testid="coding-session-status-badge"
          title={sessionClosed ? "Closed" : statusText}
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
              {sessionClosed ? "Closed" : statusText}
            </span>
          ) : sessionClosed ? (
            "Closed"
          ) : (
            statusText
          )}
        </Badge>
      ) : null}
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
          <CodingSessionProvenanceDetails
            channelName={channelName}
            contextLoads={contextLoads}
            founderDetails={founderDetails}
            generationLabel={generationLabel}
            projectName={projectName}
            providerAuthorityPubkey={providerAuthorityPubkey}
            routedSeats={routedSeats}
          />
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
      {(onToggleSurface && surfaceTabs) || onOpenPeople ? (
        <fieldset
          aria-label="Session details"
          className="flex shrink-0 items-center gap-0.5 rounded-xl border border-border/55 bg-muted/20 p-0.5"
        >
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
                  {tab.icon === "agents" ? (
                    <Users />
                  ) : tab.icon === "inspector" ? (
                    <PanelRight />
                  ) : (
                    <GitCompare />
                  )}
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
          {onToggleRouteRail ? (
            <Button
              // F7: the IDREF has to land on something. Both the expanded rail
              // and the 40 px scrubber carry this id, because the control
              // governs whichever of the two is mounted.
              aria-controls={CODING_SESSION_ROUTE_RAIL_ID}
              aria-expanded={routeRailExpanded}
              aria-label={
                routeRailExpanded ? "Collapse route rail" : "Expand route rail"
              }
              aria-pressed={routeRailExpanded}
              data-testid="coding-session-route-toggle"
              onClick={onToggleRouteRail}
              size={compact ? "icon" : "sm"}
              title={
                routeRailExpanded
                  ? "Collapse the route rail to its scrubber"
                  : routeRailDisplacesInspector
                    ? "Expand the route rail — closes the Inspector, which this width cannot hold beside it"
                    : "Expand the route rail"
              }
              type="button"
              variant={routeRailExpanded ? "secondary" : "ghost"}
            >
              <PanelLeft />
              <span className={compact ? "sr-only" : undefined}>Route</span>
            </Button>
          ) : null}
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
        </fieldset>
      ) : null}
      {!missionActions && onAddProvider ? (
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
      {!missionActions && onStopAll ? (
        <Button
          aria-label={`Stop ${stopAllCount} live ${
            stopAllCount === 1 ? "seat" : "seats"
          }`}
          data-testid="coding-session-stop-all"
          onClick={onStopAll}
          size={compact ? "icon" : "sm"}
          title="Stop every live seat in this session. The session stays open; a stopped seat cannot be resumed."
          type="button"
          variant="ghost"
        >
          <OctagonX />
          <span className={compact ? "sr-only" : undefined}>
            Stop all{stopAllCount > 0 ? ` (${stopAllCount})` : ""}
          </span>
        </Button>
      ) : null}
      {!missionActions && onCloseSession ? (
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
      {!missionActions && onReopenSession ? (
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
      {!missionActions && onExport ? (
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
      {!missionActions && onPopout ? (
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
      {/* Always mounted, and empty-safe: the overflow renders nothing at all
          when it has no items. Mission still collapses its six actions in
          here; every other lens passes none of them, so what appears outside
          Mission is a `⋯` holding exactly one item — the workspace one — and
          the flat button run below is untouched (I8). */}
      <CodingSessionHeaderOverflow
        isExporting={isExporting}
        newSessionInWorkspaceDetail={newSessionHere.detail}
        onAddProvider={missionActions ? onAddProvider : undefined}
        onCloseSession={missionActions ? onCloseSession : undefined}
        onExport={missionActions ? onExport : undefined}
        onNewSessionInWorkspace={
          workspaceReuse ? newSessionHere.start : undefined
        }
        onOpenChange={handleOverflowOpenChange}
        onPopout={missionActions ? onPopout : undefined}
        onReopenSession={missionActions ? onReopenSession : undefined}
        onStopAll={missionActions ? onStopAll : undefined}
        stopAllLabel={
          stopAllLabel ??
          `Stop all (${stopAllCount} ${stopAllCount === 1 ? "seat" : "seats"})`
        }
        stopAllSentence={
          stopAllSentence ??
          `Stop ${stopAllCount} ${stopAllCount === 1 ? "seat" : "seats"}. A stopped seat cannot be resumed.`
        }
      />
    </header>
  );
}

/**
 * One routed seat's line in the provenance popover.
 *
 * `line` is `describeCodingSessionRouting`'s sentence verbatim — `routed:
 * builder/standard → claude-primary/sonnet (medium) — <reason>`. Rendered as
 * one line, never a panel: a routing decision that needs its own screen to be
 * readable is a decision nobody reads. A seat nothing routed contributes no
 * row at all, because "not routed" and "routed to the default" are different
 * facts and only one of them happened.
 */
export type CodingSessionRoutedSeatRow = {
  key: string;
  line: string;
};

/** One execution's line in the provenance popover's `Context` section. */
export type CodingSessionContextRow = {
  key: string;
  /** How the seat names itself — `Actor · Role`, or runtime and model. */
  label: string;
  /** What the wire reported, or null when nothing has. */
  load: CodingSessionContextLoad | null;
};

/**
 * The provenance popover's body (SURFACES.md D7).
 *
 * The 2026-08-29 walk (finding 5) read the shipped popover in full — channel,
 * signed projection, verified source — and found it answered none of the
 * questions it exists to answer: no founder, though the 44226 genesis carries
 * one and **W6 says a founder is never unknown**, and no context, though the
 * 44225 usage items were on the wire and `bee sessions status` printed 27% of
 * a 1M window from exactly them. Both rows live here now, rendered the way
 * the CLI renders them so the two cannot drift apart.
 *
 * Exported so the copy can be asserted directly: Radix does not mount popover
 * content until it opens, so a test that renders the header alone sees none
 * of this.
 */
export function CodingSessionProvenanceDetails({
  channelName = null,
  contextLoads,
  founderDetails,
  generationLabel,
  projectName = null,
  providerAuthorityPubkey = null,
  routedSeats,
}: {
  channelName?: string | null;
  contextLoads?: readonly CodingSessionContextRow[];
  founderDetails?: ReactNode;
  generationLabel: string;
  projectName?: string | null;
  providerAuthorityPubkey?: string | null;
  /** The routed seats in this umbrella, one line each. */
  routedSeats?: readonly CodingSessionRoutedSeatRow[];
}) {
  return (
    <div data-testid="coding-session-provenance-details">
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
        <div>
          <dt className="text-muted-foreground">Founded by</dt>
          <dd className="mt-0.5 wrap-break-word">
            {founderDetails ?? (
              <span
                data-testid="coding-session-provenance-founder-unresolved"
                title="No genesis or create naming this session's founder has reached this client yet."
              >
                unresolved
              </span>
            )}
          </dd>
        </div>
        {providerAuthorityPubkey ? (
          <div>
            <dt className="text-muted-foreground">Verified source</dt>
            <dd className="mt-0.5 font-mono wrap-break-word">
              {shortPubkey(providerAuthorityPubkey)}
            </dd>
          </div>
        ) : null}
        {routedSeats && routedSeats.length > 0 ? (
          <div data-testid="coding-session-provenance-routing">
            <dt className="text-muted-foreground">Routing</dt>
            {routedSeats.map((row) => (
              <dd
                className="mt-0.5 truncate wrap-break-word"
                data-testid="coding-session-routed-line"
                key={row.key}
                title={row.line}
              >
                {row.line}
              </dd>
            ))}
          </div>
        ) : null}
        {contextLoads && contextLoads.length > 0 ? (
          <div data-testid="coding-session-provenance-context">
            <dt className="text-muted-foreground">Context</dt>
            {contextLoads.map((row) => (
              <dd
                className="mt-0.5 flex items-baseline justify-between gap-2 wrap-break-word"
                key={row.key}
              >
                <span className="min-w-0 truncate">{row.label}</span>
                {row.load === null ? (
                  <span
                    className="shrink-0 text-muted-foreground"
                    title="no usage reported"
                  >
                    {renderCodingSessionContextLoad(null)}
                  </span>
                ) : (
                  <span className="shrink-0 font-mono">
                    {renderCodingSessionContextLoad(row.load)}
                  </span>
                )}
              </dd>
            ))}
          </div>
        ) : null}
      </dl>
    </div>
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

/**
 * The umbrella's disposition strip: one line per execution, the lead's first.
 *
 * It sits directly under the header because it answers a header question —
 * *is this team still working?* — that the aggregate status badge cannot: a
 * lead that has delivered its verdict and a builder still standing by are one
 * "Working" between them (ledger 77, "Umbrella UI (a)").
 *
 * Every line comes from the 44223 facts the umbrella already holds; nothing
 * here fetches. The status is the same reachability-demoted one the focus
 * chips use, so a provider nothing is answering for can never read `live`, and
 * an execution with no transcript says so rather than reporting an age it does
 * not have.
 *
 * Since 2026-08-30 it carries one more line: **hires**. This host answers
 * `session.hire` in the app shell, out of sight of every session screen, and
 * until now the outcomes it produced were discarded by the runner that mounted
 * it — a hire could be read, judged and thrown away with nothing on screen at
 * all (ledger draft 97, live). The counts are the only place a person can see
 * that this computer is answering hires, so they sit next to the seats those
 * hires produce. Nothing has happened → no line.
 */
export function CodingSessionDispositionStrip({
  actorNames,
  canSteer = false,
  hireOutcomes,
  nowMs,
  resolveReachability,
  umbrella,
}: {
  /** Resolves a seat's actor pubkey to a display name, when one is known. */
  actorNames?: CodingSessionActorNameResolver;
  /**
   * Whether this viewer may prompt executions. It chooses which of W1's two
   * waiting strings a waiting seat reads, and nothing else.
   */
  canSteer?: boolean;
  /**
   * The hire outcomes to count. Defaults to what this app's hire host has
   * published, which is the only source in the running app; supplied directly
   * by tests, which have no host mounted.
   */
  hireOutcomes?: readonly {
    state: CodingSessionHireOutcomeState;
    detail: string | null;
  }[];
  /** Fixed clock for tests; defaults to now at render time. */
  nowMs?: number;
  resolveReachability: CodingSessionReachabilityResolver;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const items = React.useMemo(
    () =>
      listCodingSessionUmbrellaDispositions(
        umbrella,
        (execution) =>
          deriveCodingSessionExecutionStatus(
            execution,
            resolveReachability(execution.activeGeneration.commandTarget),
          ),
        actorNames,
        canSteer,
      ),
    [actorNames, canSteer, resolveReachability, umbrella],
  );
  const published = useCodingSessionHireOutcomes();
  const hires = hireOutcomes ?? published;
  const hireTally = React.useMemo(
    () => summarizeCodingSessionHireOutcomes(hires),
    [hires],
  );
  const hireLine = formatCodingSessionHireTally(hireTally);
  if (items.length === 0 && hireLine === null) return null;
  const at = nowMs ?? Date.now();
  return (
    <ul
      aria-label="Session team disposition"
      className="flex flex-wrap items-center gap-x-4 gap-y-0.5 border-b border-border/60 bg-background/70 px-4 py-1 text-2xs text-muted-foreground"
      data-testid="coding-session-disposition-strip"
    >
      {items.map((item) => (
        <li
          className="min-w-0 truncate"
          data-testid="coding-session-disposition-row"
          key={item.executionKey}
        >
          {formatCodingSessionDispositionLine(item, at)}
        </li>
      ))}
      {hireLine === null ? null : (
        <li
          className="min-w-0 truncate"
          data-testid="coding-session-hires-row"
          // The newest reason, on hover. One line on the strip cannot carry a
          // relay sentence, and a count with no way to reach the reason is a
          // number that tells you something is wrong and nothing else.
          title={hireTally.lastReason ?? undefined}
        >
          {hireLine}
        </li>
      )}
    </ul>
  );
}
