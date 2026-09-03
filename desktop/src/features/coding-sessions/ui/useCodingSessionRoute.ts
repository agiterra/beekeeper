import * as React from "react";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type { CodingSessionMissionDensity } from "@/features/coding-sessions/lib/codingSessionMissionDensity";
import {
  buildCodingSessionMissionTransactionRows,
  type CodingSessionMissionActorResolver,
} from "@/features/coding-sessions/lib/codingSessionMissionTransactionRows";
import {
  buildCodingSessionRouteTransactions,
  deriveCodingSessionRoute,
  codingSessionRouteFits,
  type CodingSessionRoute,
  type CodingSessionRouteGateRow,
  type CodingSessionRouteHire,
  type CodingSessionRouteParticipant,
} from "@/features/coding-sessions/lib/codingSessionRouteModel";
import type { CodingSessionParticipantPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  deriveCodingSessionMissionOpenHolds,
  type CodingSessionMissionOpenHolds,
} from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import { useElementWidth } from "@/shared/hooks/use-mobile";

import { useCodingSessionRouteWidth } from "./useCodingSessionRouteWidth";

/** What the workspace needs to place the Route rail and drive it. */
export type CodingSessionRouteRailState = {
  route: CodingSessionRoute;
  /**
   * Draw the expanded rail? False draws the 40 px scrubber instead.
   *
   * Two things and only two: there is room for it (the stream stays above its
   * 420 px floor), and the viewer has not collapsed it.
   */
  fits: boolean;
  /** The rail's width in px, for the rail's own style and `aria-valuenow`. */
  widthPx: number;
  /** Did the viewer collapse it, as opposed to the width folding it? */
  collapsedByViewer: boolean;
  /**
   * Is there room for the expanded rail at this width, ignoring the viewer's
   * own choice?
   *
   * The caller needs the two facts apart (REVIEW-L4 F3): at a width where
   * there is no room, a control that only flips the viewer's flag changes
   * nothing on screen and lies about its own state. Knowing there is no room
   * is what lets the caller *make* room.
   */
  roomForRail: boolean;
  /** Set the viewer's collapse choice outright. Persisted per viewer. */
  setCollapsed: (collapsed: boolean) => void;
  /** Pointer drag on the rail's separator. */
  onResizeStart: (event: React.PointerEvent<HTMLButtonElement>) => void;
  /** Arrow-key resize on the rail's separator. */
  onResizeKeyDown: (event: React.KeyboardEvent<HTMLButtonElement>) => void;
  /** Ref for the narrative section, whose width is the free-gutter measurement. */
  sectionRef: React.RefObject<HTMLElement | null>;
  /** Publishes the stream's `revealFact` so the rail can call it. */
  revealRef: React.MutableRefObject<((key: string) => void) | null>;
  /** Reveal one stream row — scroll it into view and ring it. */
  revealRow: (rowKey: string) => void;
  /** Receives the signed seconds of the rows currently on screen. */
  setVisibleAt: (seconds: readonly number[]) => void;
  /** Lift one road's `+N earlier` bound, in place (§9.5). */
  expandRoad: (executionKey: string) => void;
  /**
   * Open assignments and reports (A5), from the same inputs the map is drawn
   * from. Derived here so the rail head's clause and the Inspector's Team line
   * are two renderings of one derivation, not two derivations that agree.
   */
  openHolds: CodingSessionMissionOpenHolds;
};

/**
 * Derive the Route (DESIGN-SPEC §9) for one Mission workspace.
 *
 * Split out of `CodingSessionUmbrellaWorkspace.tsx` because that file is at
 * the repository's 1,000-line ceiling and the rule is to split rather than
 * raise it. Everything here reads the same sources the roster chips and the
 * stream rows read — `streamPresence` for W1, the transaction projection for
 * the rows, Lane D's delivery and seat-authority projections — so the gutter
 * cannot state a second version of any fact.
 */
export function useCodingSessionRoute(input: {
  bodyWidthPx: number;
  /** The workspace body, whose width clamps the rail against the stream floor. */
  bodyRef: React.RefObject<HTMLElement | null>;
  deliveries: readonly CodingSessionTeamWakeDelivery[];
  /**
   * What to call the founder's own party in an open-hold line — `you` when the
   * viewer *is* the founder, their name otherwise.
   *
   * F2: the founder holds no seat, so resolving a hold's counterparty against
   * the seat roster alone printed `holder not resolved` over the commonest
   * hold there is: a seat filed a report and the founder owes the verdict.
   */
  founderLabel?: string;
  /** Folded kind-44246 gate rows; each earns one sign on its author's road. */
  gateRows?: readonly CodingSessionRouteGateRow[];
  density: CodingSessionMissionDensity;
  founderPubkey: string | null;
  participants: readonly CodingSessionParticipantPresence[];
  resolveMissionActor: CodingSessionMissionActorResolver;
  seatAuthorities: readonly CodingSessionSeatAuthority[];
  transactions: readonly CodingSessionMissionTransactionInput[];
  umbrella: CodingSessionUmbrellaRecord;
}): CodingSessionRouteRailState {
  const [visibleAt, setVisibleAt] = React.useState<readonly number[]>([]);
  // Roads whose `+N earlier` the reader clicked. In memory, per workspace; a
  // lifted bound is a reading choice, not a preference worth persisting.
  const [expandedRoads, setExpandedRoads] = React.useState<readonly string[]>(
    [],
  );
  const expandRoad = React.useCallback((executionKey: string) => {
    setExpandedRoads((current) =>
      current.includes(executionKey) ? current : [...current, executionKey],
    );
  }, []);
  const revealRef = React.useRef<((key: string) => void) | null>(null);
  // The rail is a sibling of the narrative section, so the section's own width
  // is what is left after the Inspector *and* the rail. §9.2's gate adds the
  // rail's width back when it is already shown, which is what stops the
  // decision from oscillating on the exact pixel where it flips.
  const [sectionRef, sectionWidthPx] = useElementWidth<HTMLElement>();
  const [railShown, setRailShown] = React.useState(false);
  const railWidth = useCodingSessionRouteWidth(input.bodyRef);
  const roomForRail = codingSessionRouteFits({
    railShown,
    railWidthPx: railWidth.width,
    sectionWidthPx,
  });
  // The viewer's collapse choice and the width's fold are two different
  // facts and are kept that way: a fold hides the rail without touching what
  // the viewer stored, so the rail comes back the way they left it when the
  // window comes back.
  const fits = roomForRail && !railWidth.collapsed;
  React.useEffect(() => {
    setRailShown(fits);
  }, [fits]);

  const participants = React.useMemo<CodingSessionRouteParticipant[]>(
    () =>
      input.participants.flatMap((participant) => {
        const execution = input.umbrella.executions.find(
          (candidate) => candidate.executionKey === participant.executionKey,
        );
        if (execution === undefined) return [];
        const record = execution.activeGeneration;
        return [
          {
            executionKey: participant.executionKey,
            label: participant.label,
            actorPubkey: record.agentRef,
            role: record.role,
            targetKey: record.commandTarget
              ? buildCodingSessionTargetKey(record.commandTarget)
              : null,
            // W1's own answer, from the map the chips read. One voice.
            word: participant.disposition,
            live: participant.status.kind === "working",
            firstSignedAt: firstSignedSecond(execution),
            // A released seat is one the provider signed `stopped` for; every
            // other status leaves the road running to Now rather than guessing
            // an end for it.
            releasedAt:
              record.status === "stopped" && record.statusAt !== null
                ? Math.floor(record.statusAt / 1_000)
                : null,
          } satisfies CodingSessionRouteParticipant,
        ];
      }),
    [input.participants, input.umbrella.executions],
  );

  // The hire, from the accepted create's own signed `created_at` (REVIEW-A4
  // F1/F9). `createdAt`/`createEventId` are carried onto the execution by
  // `groupCodingSessionCatalog`, which already resolved the create to read
  // `attachOrderMs`; nothing new is fetched or inferred. A seat whose create
  // was never observed still passes `at: null`, and the model then draws no
  // junction at all rather than pinning one to a 44225 item's claimed clock.
  const hires = React.useMemo<CodingSessionRouteHire[]>(
    () =>
      input.umbrella.executions.flatMap((execution) =>
        execution.activeGeneration.agentRef === null
          ? []
          : [
              {
                executionKey: execution.executionKey,
                hiredByPubkey: execution.operatorPubkey,
                at: execution.createdAt,
                sourceEventId: execution.createEventId,
              } satisfies CodingSessionRouteHire,
            ],
      ),
    [input.umbrella.executions],
  );

  const rows = React.useMemo(
    () =>
      buildCodingSessionRouteTransactions(
        buildCodingSessionMissionTransactionRows({
          transactions: input.transactions,
          resolveActor: input.resolveMissionActor,
          founderPubkey: input.founderPubkey,
          deliveries: input.deliveries,
          density: input.density,
        }),
        input.transactions,
      ),
    [
      input.deliveries,
      input.density,
      input.founderPubkey,
      input.resolveMissionActor,
      input.transactions,
    ],
  );

  const route = React.useMemo(
    () =>
      deriveCodingSessionRoute({
        founderPubkey: input.founderPubkey,
        participants,
        transactions: rows,
        deliveries: input.deliveries,
        seatAuthorities: input.seatAuthorities,
        // L5.6. Empty while Mission holds no observation fold, which draws no
        // sign — the rail never invents a fact it has not been handed.
        gateRows: input.gateRows,
        hires,
        visibleAt,
        expandedRoads,
        nowMs: Date.now(),
      }),
    [
      expandedRoads,
      hires,
      input.deliveries,
      input.founderPubkey,
      input.gateRows,
      input.seatAuthorities,
      participants,
      rows,
      visibleAt,
    ],
  );

  const openHolds = React.useMemo(
    () =>
      deriveCodingSessionMissionOpenHolds({
        founderLabel: input.founderLabel,
        founderPubkey: input.founderPubkey,
        nowMs: Date.now(),
        participants,
        transactions: rows,
      }),
    [input.founderLabel, input.founderPubkey, participants, rows],
  );

  const revealRow = React.useCallback((rowKey: string) => {
    revealRef.current?.(rowKey);
  }, []);

  return React.useMemo(
    () => ({
      route,
      fits,
      openHolds,
      collapsedByViewer: railWidth.collapsed,
      roomForRail,
      widthPx: railWidth.width,
      setCollapsed: railWidth.setCollapsed,
      onResizeKeyDown: railWidth.onResizeKeyDown,
      onResizeStart: railWidth.onResizeStart,
      sectionRef,
      revealRef,
      revealRow,
      setVisibleAt,
      expandRoad,
    }),
    [
      expandRoad,
      fits,
      openHolds,
      railWidth.collapsed,
      railWidth.onResizeKeyDown,
      railWidth.onResizeStart,
      railWidth.setCollapsed,
      railWidth.width,
      revealRow,
      roomForRail,
      route,
      sectionRef,
    ],
  );
}

/**
 * The earliest signed moment this execution is known by, in unix seconds.
 *
 * The road's start when no create dates it. Read from every generation's own
 * transcript, so a resumed seat's road begins where its history begins rather
 * than at the generation currently running. `null` when nothing in the
 * transcript carries a parseable time — a road with no start says it has none
 * instead of starting at the epoch.
 */
function firstSignedSecond(execution: CodingSessionExecution): number | null {
  const records = [...execution.priorGenerations, execution.activeGeneration];
  let earliest: number | null = null;
  for (const record of records) {
    for (const item of record.transcript) {
      const parsed = Date.parse(item.timestamp);
      if (!Number.isFinite(parsed)) continue;
      const seconds = Math.floor(parsed / 1_000);
      if (earliest === null || seconds < earliest) earliest = seconds;
    }
  }
  return earliest;
}
