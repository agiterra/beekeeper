import { Bot, Pencil, RotateCcw, X } from "lucide-react";
import * as React from "react";
import type { ReactNode } from "react";

import { formatCodingSessionModelSummary } from "@/features/coding-sessions/lib/codingSessionLabels";
import { UNTITLED_SESSION_NAME } from "@/features/coding-sessions/lib/codingSessionTitle";
import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

import { CodingSessionHeaderOverflow } from "./CodingSessionHeaderOverflow";
import { CodingSessionFullAccessBadge } from "./CodingSessionFullAccessBadge";
import {
  CodingSessionHeaderDetailsGroup,
  CodingSessionHeaderPlanToggle,
  CodingSessionHeaderStatusBadge,
  type CodingSessionHeaderSurfaceTab,
} from "./CodingSessionHeaderParts";
import { CodingSessionHeaderDetails } from "./CodingSessionHeaderDetails";
import type {
  CodingSessionContextRow,
  CodingSessionRoutedSeatRow,
} from "./CodingSessionHeaderProvenance";
import { useNewSessionInWorkspaceAction } from "@/features/coding-sessions/hooks/useNewSessionInWorkspaceAction";

// The header was split at the 1,000-line ceiling; these names keep their old
// import path so no caller had to move.
export { CodingSessionDispositionStrip } from "./CodingSessionHeaderDispositionStrip";
export {
  CodingSessionProvenanceDetails,
  type CodingSessionContextRow,
  type CodingSessionRoutedSeatRow,
} from "./CodingSessionHeaderProvenance";
export type { CodingSessionHeaderSurfaceTab } from "./CodingSessionHeaderParts";

/**
 * The session header: title, status, the full-access badge, and a handful of
 * primary controls; every other action lives in the `⋯` menu.
 *
 * SESSION_VIEW_UX_PLAN L4. The header used to carry about fourteen controls
 * in one row — a flat run of Add provider, Stop all, Close, Reopen, Export and
 * Pop out beside the surface tabs, People, Plan and the provenance popover.
 * Mission had already collapsed the six actions into `⋯` (DESIGN-SPEC A7);
 * every lens does now. What stays in the row is what a person reads or
 * toggles while watching — status, the live counts on the surface tabs, and
 * one Details control (People with its count, plus provenance) — plus Reopen on a closed session, which is that
 * session's one primary action. Nothing was removed: each moved control keeps
 * its handler, label and consequence line in the menu
 * (`CodingSessionHeader.reachability.test.mjs` holds every one of them to it).
 */
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
   * `Stop all (2 seats)` — the `⋯` menu item's label, in every lens.
   *
   * Absent, the header builds the same shape from {@link stopAllCount}.
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
   * Historical: collapsed the six actions into one `⋯` (DESIGN-SPEC A7) in
   * Mission only. Every lens collapses them now (SESSION_VIEW_UX_PLAN L4), so
   * this no longer changes the header; accepted so callers need not change.
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
  workspaceReuse?: {
    channelId: string;
    sessionRef: string | null;
    sourceRepoRef?: string | null;
  } | null;
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
  /**
   * Shows or hides the plan. The header offers it only while the plan is not
   * already on screen ({@link taskRailOpen} false): the dock rail above the
   * composer is the plan's one persistent place, and this is the way back to
   * it — on a narrow window (a sheet) or after the rail was dismissed.
   */
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
  /** This execution's local full-access grant (`useCodingSessionFullAccess`). */
  fullAccess?: React.ComponentProps<
    typeof CodingSessionHeaderOverflow
  >["fullAccess"];
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
  fullAccess = null,
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
    sourceRepoRef: workspaceReuse?.sourceRepoRef ?? null,
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
  // Callers pass the shared resolver's name; when none reaches the header it
  // reads the resolver's own last fallback, the text web and mobile show.
  const title = sessionTitle?.trim() || UNTITLED_SESSION_NAME;
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
  // `missionActions` used to gate the `⋯` collapse to Mission. Every lens
  // collapses now, so the prop no longer changes what is offered; it is kept
  // because callers still pass it.
  void missionActions;

  return (
    <header
      // Mission mounts the participant bar as row 2 of the same container, and
      // the container gets exactly one bottom rule — below row 2. In
      // Conversation this header IS the whole container, so it keeps its own.
      className={
        flush
          ? "flex h-14 shrink-0 items-center gap-2 bg-background/85 px-4 backdrop-blur-xl"
          : "flex h-14 shrink-0 items-center gap-2 border-b border-border/60 bg-background/85 px-4 backdrop-blur-xl"
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
      <div className="group/title min-w-0 flex-1">
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
          <CodingSessionFullAccessBadge fullAccess={fullAccess} />
          {onRename ? (
            // An edit affordance of the title, not an action in the run: it
            // shows on hover or keyboard focus of the title, and stays in the
            // tab order the whole time.
            <Button
              aria-label="Rename session"
              className="shrink-0 opacity-0 transition-opacity group-hover/title:opacity-100 focus-visible:opacity-100"
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
        <>
          <div className="hidden min-w-0 max-w-[min(38vw,36rem)] md:flex">
            {agentControls}
          </div>
          {/* The chips need room a narrow window does not have; the status
              they summarise must not leave with them. */}
          <div className="shrink-0 md:hidden">
            <CodingSessionHeaderStatusBadge
              compact={compact}
              sessionClosed={sessionClosed}
              status={status}
              statusText={statusText}
              testId="coding-session-status-badge-narrow"
            />
          </div>
        </>
      ) : (
        <CodingSessionHeaderStatusBadge
          compact={compact}
          sessionClosed={sessionClosed}
          status={status}
          statusText={statusText}
        />
      )}
      {onToggleTaskRail && !taskRailOpen ? (
        <CodingSessionHeaderPlanToggle
          compact={compact}
          onToggle={onToggleTaskRail}
          open={taskRailOpen}
          taskCount={taskCount}
        />
      ) : null}
      <CodingSessionHeaderDetailsGroup
        compact={compact}
        onToggleRouteRail={onToggleRouteRail}
        onToggleSurface={onToggleSurface}
        routeRailDisplacesInspector={routeRailDisplacesInspector}
        routeRailExpanded={routeRailExpanded}
        surfaceHostId={surfaceHostId}
        surfaceTabs={surfaceTabs}
      />
      <CodingSessionHeaderDetails
        channelName={channelName}
        compact={compact}
        contextLoads={contextLoads}
        founderDetails={founderDetails}
        generationLabel={generationLabel}
        onOpenPeople={onOpenPeople}
        peopleCount={peopleCount}
        projectName={projectName}
        providerAuthorityPubkey={providerAuthorityPubkey}
        routedSeats={routedSeats}
      />
      {onReopenSession ? (
        // A closed session's one primary action stays in the row; every
        // other action is in the menu below.
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
      {/* Empty-safe: renders nothing when it has no items. */}
      <CodingSessionHeaderOverflow
        fullAccess={fullAccess}
        isExporting={isExporting}
        newSessionInWorkspaceDetail={newSessionHere.detail}
        onAddProvider={onAddProvider}
        onCloseSession={onCloseSession}
        onExport={onExport}
        onNewSessionInWorkspace={
          workspaceReuse ? newSessionHere.start : undefined
        }
        onOpenChange={handleOverflowOpenChange}
        onPopout={onPopout}
        onStopAll={onStopAll}
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
