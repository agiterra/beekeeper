import * as React from "react";

import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionMissionTransactionInput } from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type {
  CodingSessionMissionDecisionInput,
  CodingSessionMissionDecisionRequestInput,
} from "@/features/coding-sessions/lib/codingSessionMissionDecisions";
import type { CodingSessionMissionOpenHolds } from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import type { CodingSessionObservationView } from "@/features/coding-sessions/lib/codingSessionObservationView";
import type { CodingSessionSubagentPanel } from "@/features/coding-sessions/lib/codingSessionSubagents";
import type { CodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type { CodingSessionUmbrellaTimelineEntry } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type {
  CodingSessionObservedChanges,
  CodingSessionTranscriptModel,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionStatus,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionObservationFoldResult } from "@/features/coding-sessions/lib/invokeCodingSessionObservationFold";
import type {
  CodingSessionTreeQuery,
  CodingSessionTreeRefusal,
  CodingSessionTreeSource,
} from "@/shared/api/tauriCodingSessionTree";
import type {
  CodingSessionSurfacePanelActions,
  CodingSessionSurfacePanelState,
} from "./useCodingSessionSurfacePanels";
import type { CodingSessionObservationLive } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";

/**
 * The one value every session surface reads (SV-38).
 *
 * Built once per workspace from data the workspace already derives, and
 * handed to every surface definition's `availability`, `Badge` and `Panel`.
 * A surface never reaches into the workspace for anything else, which is what
 * lets a new surface be one file plus one line in
 * `codingSessionBuiltinSurfaces.ts`.
 *
 * Honesty rules the fields keep:
 * - **No path.** The working tree is a resolution (`tree`), never a
 *   directory; `tree.query` names the session, not its disk.
 * - **Unknown is not empty.** `observations` and `openRulings` say when they
 *   were not read, rather than reading as "nothing happened".
 * - **Live is the signed status.** Each execution carries the 44223 word it
 *   signed (`wireStatus`) beside the status the workspace shows.
 *
 * A React context, not a module cache: nothing outlives the render tree, so
 * `resetCommunityState()` has nothing to reset.
 */
export type CodingSessionSurfaceLens = "conversation" | "mission";

/** One signed `decision.request` row, as `ctx.decisionRequests` carries it. */
export type CodingSessionSurfaceDecisionRequestRow = {
  readonly sourceEventId: string;
  readonly type: "decision.request";
  /** Unix seconds from the signed event. */
  readonly createdAt: number;
};

/** Maps the evidence's signed request bodies to `ctx.decisionRequests` rows. */
export function codingSessionSurfaceDecisionRequestRows(
  requests: readonly CodingSessionMissionDecisionRequestInput[] | undefined,
): readonly CodingSessionSurfaceDecisionRequestRow[] {
  return (requests ?? []).map((request) => ({
    sourceEventId: request.requestId,
    type: "decision.request" as const,
    createdAt: request.createdAt,
  }));
}

/** A surface can open, or says in one sentence why not (SV-23). */
export type CodingSessionSurfaceAvailability =
  | { available: true }
  | { available: false; reason: string };

/** One execution with its signed status and the status the view shows. */
export type CodingSessionSurfaceExecutionEntry = {
  execution: CodingSessionExecution;
  /** The lifecycle word the provider signed (44223). */
  wireStatus: CodingSessionStatus;
  /** The word the workspace shows: wire status corrected by reachability. */
  status: CodingSessionWorkspaceStatus;
};

/**
 * This machine's answer for "where is the session's working tree?" (DB9).
 *
 * `state: "loading"` before the host answers, and while this machine has not
 * yet said whether its provider runs the session (`isLocalProvider: null`);
 * `"error"` when the host could not be asked (a browser build, a failed IPC)
 * or could not say whether it runs the session — then `available` is false
 * and `reason` says so, and `refusal` is null: only a status that was read
 * and names another provider yields `notLocal`. Never a path.
 */
export type CodingSessionSurfaceTree = {
  state: "loading" | "resolved" | "error";
  available: boolean;
  source: CodingSessionTreeSource | null;
  label: string;
  reason: string | null;
  /** Why not, as a code: another computer, nothing recorded, store unread. */
  refusal: CodingSessionTreeRefusal | null;
  /** The reference to list entries with; names the session, not its disk. */
  query: CodingSessionTreeQuery;
};

/**
 * The view's single kind-44246 observations read.
 *
 * `not-read` when the session has no genesis to scope a read by (the reason
 * says so); otherwise the read's own loading/error/result. One read per
 * view: the umbrella layout reuses Mission's.
 *
 * `assignmentsChecked` is false while the session's assignments (the Mission
 * fold's) have not been read. The view's `assignmentUnresolved` marks are
 * then all false and its `unresolved` list empty: a pointer nobody looked up
 * is "not checked", never "resolves to nothing".
 */
export type CodingSessionSurfaceObservations =
  | { state: "not-read"; reason: string }
  | {
      state: "read";
      isLoading: boolean;
      errorMessage: string | null;
      result: CodingSessionObservationFoldResult | null;
      view: CodingSessionObservationView;
      assignmentsChecked: boolean;
      /** When `result` was read (this computer's clock), or null. */
      readAtMs: number | null;
      /**
       * Whether the live 44246 subscription is up. Anything but `subscribed`
       * makes `result` a snapshot: a running gate then reads "not live — read
       * at HH:MM" (`codingSessionObservationNotLive`).
       */
      live: CodingSessionObservationLive;
      refresh: () => void;
    };

/**
 * Mission's three panels, built by the Mission surface hook exactly as
 * before. Present only in the Mission lens.
 */
export type CodingSessionSurfaceMissionContent = {
  inspector: React.ReactNode;
  context: React.ReactNode;
  audit: React.ReactNode;
};

/** Everything except what a surface derives for itself (`extensions`). */
export type CodingSessionSurfaceBaseCtx = {
  /** Which workspace built this: one execution, or an umbrella narrative. */
  layout: "single" | "umbrella";
  channelId: string;
  /** Normalized relay URL of the community this view belongs to. */
  communityScope: string;
  /** `sessionRef`, or the umbrella key for an implicit umbrella of one. */
  sessionKey: string;
  umbrella: CodingSessionUmbrellaRecord;
  /** The execution the view is focused on, when one resolves. */
  focusedExecution: CodingSessionExecution | null;
  /** The focused execution's live generation record. */
  focusedRecord: CodingSessionCatalogRecord | null;
  /** Every execution, in umbrella order, with its signed status. */
  executions: readonly CodingSessionSurfaceExecutionEntry[];
  /**
   * The transcript items this view renders: the focused execution's in the
   * single layout, every execution's (all generations) in the umbrella.
   */
  transcript: readonly CodingSessionCatalogRecord["transcript"][number][];
  /** The single layout's turn model (its turns); `null` in the umbrella. */
  transcriptModel: CodingSessionTranscriptModel | null;
  /**
   * The umbrella layout's turns: its chronological narrative exactly as
   * `buildUmbrellaTimeline` orders it — one `turn-block` entry per
   * (execution, turn) with that turn's items (the person's prompt is its
   * first `user` item), lane messages and lifecycle rows between. This is
   * the Conversation lens's reading order; Mission's densities filter it.
   * `null` in the single layout, which carries `transcriptModel` instead.
   */
  umbrellaTimeline: readonly CodingSessionUmbrellaTimelineEntry[] | null;
  observedChanges: CodingSessionObservedChanges;
  subagents: CodingSessionSubagentPanel;
  /** The focused execution's plan, when it published one. */
  taskModel: CodingSessionTaskModel | null;
  /**
   * This machine's provider runs the focused execution: `true`, `false`, or
   * `null` while that is unknown (the provider status has not loaded, or
   * could not be read). `null` is never "not local" — read `tree` for what
   * this machine can open.
   */
  isLocalProvider: boolean | null;
  projectRef: string | null;
  repoRef: string | null;
  /** The owning project as this client resolves it, when any. */
  project: { id: string; name: string } | null;
  genesisRef: string | null;
  founderPubkey: string | null;
  currentUserPubkey: string | null;
  sessionClosed: boolean;
  lens: CodingSessionSurfaceLens;
  /** The right panel's active surface, or null (launcher or closed). */
  activeSurfaceId: string | null;
  panelState: CodingSessionSurfacePanelState;
  panels: CodingSessionSurfacePanelActions;
  observations: CodingSessionSurfaceObservations;
  /**
   * Open handoffs (`deriveCodingSessionMissionOpenHolds`) whenever the
   * session has a genesis, in both layouts and both lenses, once the Mission
   * fold has been read. `null` means "no genesis", or "not read yet / could
   * not be read" — never "none".
   */
  openRulings: CodingSessionMissionOpenHolds | null;
  /**
   * The team fold's `decisions[]`, verbatim (DB8: an open
   * `decision.request` is a ruling waiting on a person; the badges read it).
   * `null` when the session has no genesis or the fold has not been read —
   * never "none".
   */
  decisions: readonly CodingSessionMissionDecisionInput[] | null;
  /**
   * The signed `decision.request` rows (kind 44244) that date `decisions[]`,
   * for the minimap's waiting-ruling mark (SV-27, DB8). `null` when the
   * session has no genesis or the evidence has not been read — never "none".
   */
  decisionRequests: readonly CodingSessionSurfaceDecisionRequestRow[] | null;
  /**
   * The team's signed 44244 transactions, from the same Mission-evidence read
   * as `decisions` (the Agents orchestration's assigned phases and ruling
   * holds, SV-40). `null` when the session has no genesis or the evidence has
   * not been read — never "none".
   */
  teamTransactions: readonly CodingSessionMissionTransactionInput[] | null;
  tree: CodingSessionSurfaceTree;
  /** The transcript pane's minimap slot (`coding-session-minimap-slot`). */
  minimapSlotRef: React.RefObject<HTMLDivElement | null>;
  resolveActorName: CodingSessionActorNameResolver;
  resolveReachability: CodingSessionReachabilityResolver;
  /** Mission's Inspector, Context and Audit; `null` outside Mission. */
  mission: CodingSessionSurfaceMissionContent | null;
  /** Opens the People dialog; absent for a session with no genesis. */
  onOpenPeople?: () => void;
};

/**
 * The surface context. `extensions[id]` holds what the surface `id`'s own
 * `readExtension` hook returned for this view, so a surface can compute a
 * value once and share it between its badge and its panel without editing
 * this type or either workspace.
 */
export type CodingSessionSurfaceCtx = CodingSessionSurfaceBaseCtx & {
  extensions: Readonly<Record<string, unknown>>;
};

const CodingSessionSurfaceContext =
  React.createContext<CodingSessionSurfaceCtx | null>(null);

/** Provides the surface context to every surface mounted beneath it. */
export function CodingSessionSurfaceCtxProvider({
  children,
  value,
}: {
  children: React.ReactNode;
  value: CodingSessionSurfaceCtx;
}) {
  return (
    <CodingSessionSurfaceContext.Provider value={value}>
      {children}
    </CodingSessionSurfaceContext.Provider>
  );
}

/** The surface context, or `null` outside a session workspace. */
export function useCodingSessionSurfaceCtx(): CodingSessionSurfaceCtx | null {
  return React.useContext(CodingSessionSurfaceContext);
}

/**
 * The absolutely positioned element at the transcript pane's left edge that
 * the minimap (SV-26) portals into. The workspace renders it; the ref travels
 * in `ctx.minimapSlotRef`.
 */
export function CodingSessionMinimapSlot({
  slotRef,
}: {
  slotRef: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div
      className="pointer-events-none absolute inset-y-0 left-0 z-20 w-10 [&>*]:pointer-events-auto"
      data-testid="coding-session-minimap-slot"
      ref={slotRef}
    />
  );
}

/**
 * The placeholder a surface renders until its lane builds its content: the
 * surface's availability and, when it cannot open, its reason (SV-23).
 */
export function CodingSessionSurfaceStubPanel({
  availability,
  children,
  id,
  label,
}: {
  availability: CodingSessionSurfaceAvailability;
  children?: React.ReactNode;
  id: string;
  label: string;
}) {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-4 text-sm"
      data-available={availability.available ? "true" : "false"}
      data-testid={`coding-session-surface-panel-${id}`}
    >
      {availability.available ? null : (
        <p
          className="text-muted-foreground"
          data-testid={`coding-session-surface-reason-${id}`}
        >
          {availability.reason}
        </p>
      )}
      {children ?? (
        <p className="text-muted-foreground">
          {label} has no panel in this build yet.
        </p>
      )}
    </div>
  );
}
