import { RotateCcw, X } from "lucide-react";
import * as React from "react";
import type { ReactNode } from "react";

import { formatCodingSessionModelSummary } from "@/features/coding-sessions/lib/codingSessionLabels";
import { UNTITLED_SESSION_NAME } from "@/features/coding-sessions/lib/codingSessionTitle";
import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { Button } from "@/shared/ui/button";

import { CodingSessionHeaderOverflow } from "./CodingSessionHeaderOverflow";
import { CodingSessionHeaderBreadcrumb } from "./CodingSessionHeaderBreadcrumb";
import { CodingSessionHeaderRouteToggle } from "./CodingSessionHeaderParts";
import {
  CodingSessionHeaderDetails,
  codingSessionHeaderMetadataRows,
} from "./CodingSessionHeaderDetails";
import {
  CodingSessionHeaderPanelToggles,
  type CodingSessionHeaderSurfaceShell,
} from "./CodingSessionHeaderPanelToggles";
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

export type { CodingSessionHeaderSurfaceShell } from "./CodingSessionHeaderPanelToggles";

/**
 * The session header (SV-20): `project / title ● Working` on the left; on the
 * right Details, Reopen (closed sessions only), the `⋯` menu, and the bottom-
 * and right-panel toggles (⌘J, ⌘⌥B), as T3 Code's `ChatHeader` and
 * `PanelLayoutControls` lay it out.
 *
 * Surfaces left the header: the segmented surface toggles and the Plan toggle
 * became the launcher's rows and letters (SV-21), and the right-panel toggle's
 * dot keeps any off-screen live badge visible without a click. The metadata
 * line (goal, repository, runtime, model, generation) is the first rows of
 * Details. What stayed: the status word (the honesty rule outranks T3's
 * silence), the seat chip, the full-access badge, the lens control, Mission's
 * Route toggle, and the multi-execution `agentControls`.
 *
 * Before that, SESSION_VIEW_UX_PLAN L4 collapsed the six actions (Add
 * provider, Stop all, Close, Export, Pop out, Full access) into `⋯` in every
 * lens. Nothing was removed: `CodingSessionHeader.reachability.test.mjs`
 * holds every moved control to the place it names.
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
  /** Durable session intent: the first row of Details when present. */
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
  /** Opens the owning project. Given one, the project crumb is a link;
   * without one it is plain text. */
  onOpenProject?: () => void;
  /** People with access to the session (owner + grants), for the badge. */
  peopleCount?: number;
  projectName?: string | null;
  providerAuthorityPubkey?: string | null;
  repoName?: string | null;
  runtimeLabel?: string | null;
  /**
   * In a session with more than one seat, the seat whose execution `model`
   * and `runtimeLabel` describe (the focused one). Details then labels those
   * rows as that seat's rather than the session's. Null for one execution.
   */
  metadataFocusedSeat?: string | null;
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
  /** DOM id of the surface host panel, for the right toggle's `aria-controls`. */
  surfaceHostId?: string;
  /**
   * The workspace's surface shell: the panel toggles, their state, and the
   * surfaces whose off-screen badges the right toggle's dot summarises.
   * Absent on a header with no session view behind it (pending, loading,
   * founded), which then has no panel toggles at all.
   */
  surfaceShell?: CodingSessionHeaderSurfaceShell | null;
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
  peopleCount = 0,
  projectName = null,
  providerAuthorityPubkey = null,
  repoName = null,
  runtimeLabel = null,
  metadataFocusedSeat = null,
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
  surfaceShell = null,
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
  // A demoted status carries its own history clause. The word stays on
  // screen; the clause rides in the status's tooltip and accessible name, so
  // the header never presents a stale report as the current condition.
  const statusDetail =
    statusLabelOverride === null
      ? codingSessionWorkspaceStatusDetail(status)
      : null;
  const statusWord = statusLabelOverride ?? status.label;
  const metadata = codingSessionHeaderMetadataRows({
    goal: goalText,
    repo: repoName,
    runtime: runtimeLabel,
    model: model ? formatCodingSessionModelSummary(model) : null,
    generation: removeRepeatedTitle(generationLabel, title),
    focusedSeat: metadataFocusedSeat,
  });
  // SV-24: the Details People row opens the People surface where the view
  // hosts one and it can open; elsewhere it keeps the People dialog.
  const peopleSurface = surfaceShell?.surfaces.find(
    ({ definition }) => definition.id === "people",
  );
  const openPeople =
    onOpenPeople && surfaceShell && peopleSurface?.availability.available
      ? () => {
          surfaceShell.ctx.panels.open("people");
          focusPeopleSurfaceRoster();
        }
      : onOpenPeople;
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
      <CodingSessionHeaderBreadcrumb
        fullAccess={fullAccess}
        onOpenProject={onOpenProject}
        onRename={onRename}
        projectName={projectName}
        seat={seat}
        sessionClosed={sessionClosed}
        status={status}
        statusDetail={statusDetail}
        statusWord={statusWord}
        title={title}
        viewControl={viewControl}
      />
      {agentControls ? (
        // The chips need room a narrow window does not have; the status they
        // detail stays beside the title either way.
        <div className="hidden min-w-0 max-w-[min(38vw,36rem)] md:flex">
          {agentControls}
        </div>
      ) : null}
      {onToggleRouteRail ? (
        <CodingSessionHeaderRouteToggle
          compact={compact}
          displacesInspector={routeRailDisplacesInspector}
          expanded={routeRailExpanded}
          onToggle={onToggleRouteRail}
        />
      ) : null}
      <CodingSessionHeaderDetails
        channelName={channelName}
        compact={compact}
        contextLoads={contextLoads}
        founderDetails={founderDetails}
        generationLabel={generationLabel}
        metadata={metadata}
        onOpenPeople={openPeople}
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
      {surfaceShell ? (
        <CodingSessionHeaderPanelToggles
          shell={surfaceShell}
          surfaceHostId={surfaceHostId}
        />
      ) : null}
    </header>
  );
}

function removeRepeatedTitle(generationLabel: string, title: string): string {
  const prefix = `${title} · `;
  return generationLabel.startsWith(prefix)
    ? generationLabel.slice(prefix.length)
    : generationLabel;
}

/**
 * Land keyboard focus on the People surface's roster once it has rendered, as
 * the People dialog does on open. Without this the Details popover hands focus
 * back to its trigger, and the roster and its invite sit ten Tabs away behind
 * the header (people-setup S12). Two frames: one for the panel to mount, one
 * to run after the popover's own focus restore.
 */
function focusPeopleSurfaceRoster(): void {
  if (typeof window === "undefined") return;
  window.requestAnimationFrame(() => {
    window.requestAnimationFrame(() => {
      document
        .querySelector<HTMLElement>(
          '[data-testid="coding-session-people-surface"] [data-testid="coding-session-people-roster"]',
        )
        ?.focus();
    });
  });
}
