/**
 * The Route: a second projection of the Mission stream onto a time axis.
 *
 * DESIGN-SPEC §9. The rail in the left gutter is a map of the session — roads
 * for the founder and each seat, junctions where a seat was hired, bridges for
 * the signed handoffs between them, signs for the rows the stream already
 * renders, and dashed stretches where nothing happened or a wake sat queued.
 *
 * Two rules govern everything here.
 *
 * 1. **Never a third source.** A sign exists only for a row the stream itself
 *    renders (a signed 44244 transaction), a 44224 delivery whose badge is
 *    non-null, an accepted-44228 seat authority, or an accepted create. Prose —
 *    a turn, a message, a note inside a turn — never becomes a sign
 *    (SURFACES C4). The word on a sign is the frozen word the row uses, so the
 *    gutter and the stream say the same thing about the same fact.
 * 2. **Unknown is never zero.** A hire with no signed time draws no junction; a
 *    delivery with no observation draws no stretch; a road whose start nothing
 *    dates has `startedAt: null` and says so. Nothing here invents a moment in
 *    order to have something to draw.
 *
 * Geometry is part of the model, not the component: the compression walk in
 * {@link deriveCodingSessionRoute} is the only place that turns seconds into
 * pixels, which is what makes §9.4 testable without a DOM.
 */
import type {
  CodingSessionMissionTransactionType,
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "./codingSessionMissionContracts";
import {
  codingSessionSeatAuthorityCopy,
  codingSessionTeamWakeDeliveryCopy,
} from "./codingSessionMissionContracts";
import { codingSessionParticipantAccent } from "./codingSessionParticipantAccent";
import type { CodingSessionParticipantAccent } from "./codingSessionParticipantAccent";
export * from "./codingSessionRouteTypes";
import type {
  CodingSessionRoute,
  CodingSessionRouteGateRow,
  CodingSessionRouteHere,
  CodingSessionRouteHire,
  CodingSessionRouteParticipant,
  CodingSessionRouteRoad,
  CodingSessionRouteRoadHead,
  CodingSessionRouteRoadKey,
  CodingSessionRouteSign,
  CodingSessionRouteStretch,
  CodingSessionRouteTransaction,
  CodingSessionRouteBridge,
} from "./codingSessionRouteTypes";
import {
  CODING_SESSION_ROUTE_FOUNDER_ROAD,
  formatCodingSessionRouteDuration,
  ROUTE_BAND_MIN_PX,
  ROUTE_LANE_PITCH_PX,
  ROUTE_LANE_PITCH_TIGHT_PX,
  ROUTE_MAX_GAP_SECONDS,
  ROUTE_MAX_ROADS,
  ROUTE_PX_PER_MINUTE,
  ROUTE_SIGN_SLOT_PX,
  ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT,
  ROUTE_SIGNS_PER_ROAD_LIMIT,
  ROUTE_STRETCH_PX,
} from "./codingSessionRouteTypes";

/**
 * The word one sign says, per transaction type.
 *
 * Exported so a test can hold it against the wire's own type list: a verb the
 * relay can sign and this table has no word for is a sign that renders `null`,
 * which is exactly what B1c's three verbs did before this table learned them.
 */
export const CODING_SESSION_ROUTE_SIGN_WORD: Readonly<
  Record<CodingSessionMissionTransactionType, string>
> = {
  assignment: "assignment",
  report: "report",
  refutation: "refutation",
  disposition: "verdict",
  acknowledgement: "acknowledgement",
  "mission.completed": "mission completed",
  "mission.blocked": "mission blocked",
  note: "note",
  "decision.request": "ruling asked",
  "decision.answer": "ruling given",
};

/** Delivery kinds that mean "queued, not started" — the measured stretch (§9.4.3). */
const QUEUED_DELIVERY_KINDS: ReadonlySet<string> = new Set([
  "provider-queued",
  "fallback-grace",
  "fallback-unconfirmed",
  "fallback-queued",
]);

/** A 12-hour clock label, matching the stream's own `time` labels. */
function routeTimeLabel(atSeconds: number): string {
  const date = new Date(atSeconds * 1_000);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : "time not reported";
}

function sameKey(left: string | null, right: string | null): boolean {
  return (
    left !== null &&
    right !== null &&
    left.toLowerCase() === right.toLowerCase()
  );
}

type RoadDraft = {
  key: CodingSessionRouteRoadKey;
  founder: boolean;
  label: string;
  accent: CodingSessionParticipantAccent;
  actorPubkey: string | null;
  targetKey: string | null;
  role: string | null;
  word: string | null;
  live: boolean;
  startedAt: number | null;
  startedAtSource: "hire" | "first-signed" | null;
  endedAt: number | null;
  hireOrder: number;
};

/**
 * Project one mission onto the route.
 *
 * Every input is already-decoded typed evidence: the rows the stream renders,
 * the delivery projection, the accepted seat chain, the accepted creates. This
 * function decides nothing about authority or inclusion — it lays what it is
 * given onto a clock.
 */
export function deriveCodingSessionRoute(input: {
  /** Founder identity, so the founder's own road reads `You`. */
  founderPubkey: string | null;
  /** Label for the founder's road; the surface supplies `You` or a name. */
  founderLabel?: string;
  participants: readonly CodingSessionRouteParticipant[];
  transactions: readonly CodingSessionRouteTransaction[];
  deliveries?: readonly CodingSessionTeamWakeDelivery[];
  seatAuthorities?: readonly CodingSessionSeatAuthority[];
  hires?: readonly CodingSessionRouteHire[];
  /**
   * Folded kind-44246 gate rows (L5.6).
   *
   * A gate row is a signed event the stream itself renders — the Audit tab's
   * `Gate rows` section and the Inspector's `Structured tests` card — so it
   * earns a sign under R1. Checkpoints, findings and phase timings do not:
   * they would flood the gutter, and none of them is a fact worth interrupting
   * a reader for.
   */
  gateRows?: readonly CodingSessionRouteGateRow[];
  /**
   * Unix seconds of the rows currently inside the stream's viewport. Local,
   * not wire — this is the one input that is about the reader rather than the
   * session. Empty or absent means no band is drawn.
   */
  visibleAt?: readonly number[];
  /**
   * Roads whose `+N earlier` marker the reader has clicked. Their per-road
   * bound is lifted to {@link ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT} — lifted,
   * never removed, so the count keeps telling the truth in both states.
   */
  expandedRoads?: readonly string[];
  nowMs: number;
}): CodingSessionRoute {
  const nowAt = Math.floor(input.nowMs / 1_000);
  const expandedRoads = new Set(input.expandedRoads ?? []);
  const hires = input.hires ?? [];
  const hireByExecution = new Map(
    hires.map((hire) => [hire.executionKey, hire] as const),
  );
  const hireOrder = new Map(
    hires.map((hire, index) => [hire.executionKey, index] as const),
  );

  const founderRoad: RoadDraft = {
    key: CODING_SESSION_ROUTE_FOUNDER_ROAD,
    founder: true,
    label: input.founderLabel ?? "You",
    accent: codingSessionParticipantAccent(input.founderPubkey ?? "founder"),
    actorPubkey: input.founderPubkey,
    targetKey: null,
    role: null,
    // R8: the founder is not a seat, so it carries no W1 word at all.
    word: null,
    live: false,
    startedAt: null,
    startedAtSource: null,
    endedAt: null,
    hireOrder: -1,
  };

  const seatRoads: RoadDraft[] = input.participants.map((participant) => {
    const hire = hireByExecution.get(participant.executionKey) ?? null;
    const startedAt = hire?.at ?? participant.firstSignedAt;
    return {
      key: participant.executionKey,
      founder: false,
      label: participant.label,
      accent: codingSessionParticipantAccent(participant.executionKey),
      actorPubkey: participant.actorPubkey,
      targetKey: participant.targetKey,
      role: participant.role,
      word: participant.word,
      live: participant.live,
      startedAt: startedAt ?? null,
      startedAtSource:
        hire?.at != null
          ? "hire"
          : participant.firstSignedAt != null
            ? "first-signed"
            : null,
      endedAt: participant.releasedAt,
      hireOrder:
        hireOrder.get(participant.executionKey) ?? Number.MAX_SAFE_INTEGER,
    } satisfies RoadDraft;
  });
  // Founder first, lead second, the rest in hire order — the order §9.2 fixes.
  seatRoads.sort((left, right) => {
    const leadLeft = left.role === "lead" ? 0 : 1;
    const leadRight = right.role === "lead" ? 0 : 1;
    if (leadLeft !== leadRight) return leadLeft - leadRight;
    return left.hireOrder - right.hireOrder;
  });
  const allRoads = [founderRoad, ...seatRoads];
  const hiddenRoadCount = Math.max(0, allRoads.length - ROUTE_MAX_ROADS);
  const drafts = allRoads.slice(0, ROUTE_MAX_ROADS);
  const lanePitchPx =
    drafts.length > 4 ? ROUTE_LANE_PITCH_TIGHT_PX : ROUTE_LANE_PITCH_PX;
  const laneByKey = new Map<CodingSessionRouteRoadKey, number>(
    drafts.map((draft, index) => [draft.key, index] as const),
  );

  /**
   * Which road an actor pubkey belongs to, or `null` when no drawn road claims
   * it. `null` is a real answer: an off-road sign still appears in the sign
   * column and in the screen-reader list, it simply has no lane to tick back
   * to. Folding it into the founder's lane would attribute a stranger's signed
   * act to the person reading, which is the worst lie this map could tell.
   */
  const roadForActor = (pubkey: string | null): CodingSessionRouteRoadKey => {
    if (pubkey === null) return null;
    for (const draft of drafts) {
      if (!draft.founder && sameKey(draft.actorPubkey, pubkey))
        return draft.key;
    }
    return sameKey(input.founderPubkey, pubkey)
      ? CODING_SESSION_ROUTE_FOUNDER_ROAD
      : null;
  };

  const signs: Omit<CodingSessionRouteSign, "offsetPx">[] = [];
  const bridges: Omit<
    CodingSessionRouteBridge,
    "offsetPx" | "fromLane" | "toLane"
  >[] = [];
  // Facts this route owns but cannot place, per road key. Today only a
  // delivery whose local observation never landed (REVIEW-A4 F7).
  const undatedByRoad = new Map<string, number>();

  // R2 — a hire is a junction: a sign on the *hiring* road, and a curve from
  // that lane to the new one. A hire with no signed time is not drawn at all;
  // the road it created still exists and starts at its first signed moment.
  for (const hire of hires) {
    if (hire.at === null) continue;
    const hired = drafts.find((draft) => draft.key === hire.executionKey);
    if (hired === undefined) continue;
    const hiringRoad = roadForActor(hire.hiredByPubkey);
    signs.push({
      key: `route-hire:${hire.executionKey}`,
      road: hiringRoad,
      at: hire.at,
      kind: "hire",
      word: `hire · ${hired.label}`,
      title: `${hired.label} · Hired · ${routeTimeLabel(hire.at)}`,
      sourceEventId: hire.sourceEventId,
      weight: "standard",
      tone: null,
      timeSource: "signed",
      requiresDecision: false,
      revealKey: null,
    });
    bridges.push({
      key: `route-hire-bridge:${hire.executionKey}`,
      ownerSignKey: `route-hire:${hire.executionKey}`,
      from: hiringRoad,
      to: hire.executionKey,
      at: hire.at,
      kind: "hire",
      sourceEventId: hire.sourceEventId,
    });
  }

  // R3/R4 — one sign per row the stream renders, on the author's road, with a
  // bridge to the counterparty. Prose is not here and cannot get here: the only
  // input is the row list the stream itself was handed.
  for (const row of input.transactions) {
    const road = roadForActor(row.actor.pubkey);
    signs.push({
      key: `route-sign:${row.key}`,
      road,
      at: row.createdAt,
      kind: row.type,
      // A kind with no word says its own wire word — never `undefined`, which
      // the screen-reader sign list would read out as "null".
      word: CODING_SESSION_ROUTE_SIGN_WORD[row.type] ?? row.type,
      title: `${row.title} · ${row.meta.timeLabel}`,
      sourceEventId: row.meta.sourceEventId,
      weight: row.weight === "attention" ? "attention" : "standard",
      tone: row.tone,
      timeSource: "signed",
      requiresDecision: row.requiredAction !== null,
      revealKey: row.key,
    });
    const counterRoad =
      row.counterparty === null ? null : roadForActor(row.counterparty.pubkey);
    if (counterRoad !== null) {
      bridges.push({
        key: `route-bridge:${row.key}`,
        ownerSignKey: `route-sign:${row.key}`,
        from: road,
        to: counterRoad,
        at: row.createdAt,
        kind: row.type,
        sourceEventId: row.meta.sourceEventId,
      });
    }
  }

  // L5.6 — one sign per folded gate row, on its author's road, in the row's
  // own word. A **failed** gate is an attention sign: it is the one thing a
  // person watching a team most needs to be interrupted for, and live-run
  // finding 26 is what happens when nobody is. No bridge: a gate row names no
  // counterparty, and an arrow to nowhere is a mark with nothing behind it.
  for (const gateRow of input.gateRows ?? []) {
    if (gateRow.at === null) continue;
    const road = roadForActor(gateRow.authorPubkey);
    signs.push({
      key: `route-gate:${gateRow.key}`,
      road,
      at: gateRow.at,
      kind: "gate",
      // The row's own outcome, never a second vocabulary for the same fact.
      word: gateRow.outcome,
      title: `${gateRow.gate} · ${gateRow.outcome} · ${routeTimeLabel(gateRow.at)}`,
      sourceEventId: gateRow.sourceEventId,
      weight: gateRow.outcome === "failed" ? "attention" : "standard",
      tone: gateRow.outcome === "failed" ? "critical" : null,
      timeSource: "signed",
      requiresDecision: false,
      // The Audit tab renders these, not the stream, so there is no stream row
      // to reveal. `null` is the honest answer, and it is what the delivery and
      // seat-authority signs already say for the same reason.
      revealKey: null,
    });
  }

  // R4 delivery signs, and R5/§9.4.3's measured queued stretch. Only a delivery
  // whose frozen copy carries a badge is a sign: `provider-started` has none,
  // because a wake that started is not a fact the gutter needs to interrupt for.
  const queuedSpans: {
    road: CodingSessionRouteRoadKey;
    from: number;
    to: number;
    key: string;
  }[] = [];
  for (const delivery of input.deliveries ?? []) {
    const badge = codingSessionTeamWakeDeliveryCopy[delivery.kind].badge;
    if (badge === null) continue;
    const targetRoad =
      drafts.find(
        (draft) =>
          !draft.founder && sameKey(draft.targetKey, delivery.leadTargetKey),
      )?.key ?? null;
    const at =
      delivery.observedAtMs === null
        ? null
        : Math.floor(delivery.observedAtMs / 1_000);
    if (at === null) {
      // §9.9 promises one sign per non-started delivery, and there is no
      // honest place to put this one — so it is counted on its road rather
      // than dropped, and the head says `N undated` (REVIEW-A4 F7).
      const bucket = targetRoad ?? OFF_ROAD;
      undatedByRoad.set(bucket, (undatedByRoad.get(bucket) ?? 0) + 1);
      continue;
    }
    signs.push({
      key: `route-delivery:${delivery.sourceEventId}:${delivery.kind}`,
      road: targetRoad,
      at,
      kind: "delivery",
      word: badge,
      title: `${delivery.detail} · ${routeTimeLabel(at)}`,
      sourceEventId: delivery.sourceEventId,
      weight: delivery.kind === "failed" ? "attention" : "standard",
      tone: delivery.kind === "failed" ? "critical" : null,
      // The only clock a delivery carries is Desktop's own observation.
      timeSource: "local",
      requiresDecision: false,
      revealKey: `transaction:${delivery.sourceEventId}`,
    });
    if (QUEUED_DELIVERY_KINDS.has(delivery.kind) && at < nowAt) {
      queuedSpans.push({
        road: targetRoad,
        from: at,
        to: nowAt,
        key: `route-queued:${delivery.sourceEventId}`,
      });
    }
  }

  // R4 seat authority: a standing condition, not an event, so it marks the road
  // at Now rather than inventing a moment for itself.
  for (const authority of input.seatAuthorities ?? []) {
    if (authority.kind !== "created-ungranted") continue;
    const road = drafts.find((draft) => draft.key === authority.executionKey);
    if (road === undefined) continue;
    const badge = codingSessionSeatAuthorityCopy[authority.kind].badge;
    if (badge === null) continue;
    signs.push({
      key: `route-seat:${authority.executionKey}`,
      road: authority.executionKey,
      at: nowAt,
      kind: "seat-ungranted",
      word: badge,
      title: `${road.label} · ${authority.detail}`,
      sourceEventId: authority.grantEventId,
      weight: "attention",
      tone: "caution",
      timeSource: "signed",
      requiresDecision: false,
      revealKey: null,
    });
  }

  signs.sort(
    (left, right) =>
      left.at - right.at ||
      (left.sourceEventId ?? left.key).localeCompare(
        right.sourceEventId ?? right.key,
      ),
  );

  // §9.5 — ≤ 200 signs per road; the oldest collapse into the road's own
  // `+N earlier` marker rather than disappearing.
  const perRoadCounts = new Map<string, number>();
  for (const sign of signs) {
    const key = sign.road ?? OFF_ROAD;
    perRoadCounts.set(key, (perRoadCounts.get(key) ?? 0) + 1);
  }
  const limitFor = (roadKey: string) =>
    expandedRoads.has(roadKey)
      ? ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT
      : ROUTE_SIGNS_PER_ROAD_LIMIT;
  const hiddenByRoad = new Map<string, number>();
  const keptByRoad = new Map<string, number>();
  for (const [key, count] of perRoadCounts) {
    hiddenByRoad.set(key, Math.max(0, count - limitFor(key)));
  }
  const retained = signs.filter((sign) => {
    const key = sign.road ?? OFF_ROAD;
    const total = perRoadCounts.get(key) ?? 0;
    const seen = (keptByRoad.get(key) ?? 0) + 1;
    keptByRoad.set(key, seen);
    return seen > total - limitFor(key);
  });
  // F8: a bridge belongs to a sign. Filtering here — after retention, before
  // any geometry — is what keeps the arrow count from outrunning the sign
  // count when the bound bites.
  const retainedSignKeys = new Set(retained.map((sign) => sign.key));
  const retainedBridges = bridges.filter((bridge) =>
    retainedSignKeys.has(bridge.ownerSignKey),
  );
  const hiddenSignCount = [...hiddenByRoad.values()].reduce(
    (sum, value) => sum + value,
    0,
  );

  // The compression walk. Every moment the map has to place — a sign, a road
  // start or end, a queued span's edges, Now — becomes a point; the distance
  // between consecutive points is §9.4's rule, and nothing else sets a pixel.
  const points = new Set<number>([nowAt]);
  for (const sign of retained) points.add(sign.at);
  for (const draft of drafts) {
    if (draft.startedAt !== null) points.add(draft.startedAt);
    if (draft.endedAt !== null) points.add(draft.endedAt);
  }
  for (const span of queuedSpans) {
    points.add(span.from);
    points.add(span.to);
  }
  const ordered = [...points].sort((left, right) => left - right);
  const signsAt = new Map<number, number>();
  for (const sign of retained) {
    signsAt.set(sign.at, (signsAt.get(sign.at) ?? 0) + 1);
  }
  // A queued span usually has other moments inside it — the run's own report
  // wake had a hire land four minutes in. The span is still **one** stretch
  // carrying **one** measured duration (§9.4.3), so the segments it covers are
  // counted first and never mint a silence of their own; splitting a 4 m 20 s
  // wake into two labelled stretches would report the same wait twice.
  const segmentSpan = new Map<number, number>();
  const spanSegmentCount = new Map<number, number>();
  for (const [spanIndex, span] of queuedSpans.entries()) {
    for (let index = 0; index + 1 < ordered.length; index += 1) {
      const from = ordered[index] as number;
      const to = ordered[index + 1] as number;
      if (span.from <= from && span.to >= to && !segmentSpan.has(index)) {
        segmentSpan.set(index, spanIndex);
        spanSegmentCount.set(
          spanIndex,
          (spanSegmentCount.get(spanIndex) ?? 0) + 1,
        );
      }
    }
  }
  const offsetAt = new Map<number, number>();
  const stretches: CodingSessionRouteStretch[] = [];
  let cursor = 0;
  for (const [index, at] of ordered.entries()) {
    offsetAt.set(at, cursor);
    const next = ordered[index + 1];
    if (next === undefined) break;
    const gapSeconds = next - at;
    const spanIndex = segmentSpan.get(index);
    let height: number;
    if (spanIndex !== undefined) {
      // The span owes ROUTE_STRETCH_PX in total however it is cut up, so a
      // short wake still reads as a length of road rather than a hairline.
      const share = Math.ceil(
        ROUTE_STRETCH_PX / (spanSegmentCount.get(spanIndex) ?? 1),
      );
      // §9.4.2 applies to a queued wake too (REVIEW-A4 F2). A wake is always
      // its own measured stretch — the *label* is the fact — but a long one is
      // drawn at the silence height rather than scaled: the §2a residual is a
      // wake that runs to Now until the lead or a Desktop returns, so scaling
      // it made an overnight loss a ~5,700 px rail nobody could read.
      const span = queuedSpans[spanIndex];
      const spanSeconds = span === undefined ? gapSeconds : span.to - span.from;
      height =
        spanSeconds > ROUTE_MAX_GAP_SECONDS
          ? share
          : Math.max(
              share,
              Math.round((gapSeconds / 60) * ROUTE_PX_PER_MINUTE),
            );
    } else if (gapSeconds > ROUTE_MAX_GAP_SECONDS) {
      height = ROUTE_STRETCH_PX;
      stretches.push({
        key: `route-silence:${at}`,
        road: null,
        kind: "silence",
        fromAt: at,
        toAt: next,
        durationMs: gapSeconds * 1_000,
        label: formatCodingSessionRouteDuration(gapSeconds * 1_000) ?? "",
        offsetPx: cursor,
        heightPx: ROUTE_STRETCH_PX,
      });
    } else {
      height = Math.round((gapSeconds / 60) * ROUTE_PX_PER_MINUTE);
    }
    // §9.4.4 — signs never overlap. A moment carrying k signs owes the next
    // moment k slots, whatever the clock says the distance was.
    cursor += Math.max(height, (signsAt.get(at) ?? 0) * ROUTE_SIGN_SLOT_PX);
  }
  for (const span of queuedSpans) {
    const fromOffset = offsetAt.get(span.from);
    const toOffset = offsetAt.get(span.to);
    if (fromOffset === undefined || toOffset === undefined) continue;
    const durationMs = (span.to - span.from) * 1_000;
    stretches.push({
      key: span.key,
      road: span.road,
      kind: "queued",
      fromAt: span.from,
      toAt: span.to,
      durationMs,
      label: formatCodingSessionRouteDuration(durationMs) ?? "",
      offsetPx: fromOffset,
      heightPx: Math.max(ROUTE_STRETCH_PX, toOffset - fromOffset),
    });
  }
  stretches.sort((left, right) => left.offsetPx - right.offsetPx);
  const nowOffsetPx = offsetAt.get(nowAt) ?? cursor;

  // Signs at one moment stack downward in signed order inside that moment's
  // slots, which the walk above already reserved.
  const stackedAt = new Map<number, number>();
  const placedSigns: CodingSessionRouteSign[] = retained.map((sign) => {
    const base = offsetAt.get(sign.at) ?? 0;
    const index = stackedAt.get(sign.at) ?? 0;
    stackedAt.set(sign.at, index + 1);
    return { ...sign, offsetPx: base + index * ROUTE_SIGN_SLOT_PX };
  });

  const placedBridges: CodingSessionRouteBridge[] = retainedBridges.map(
    (bridge) => ({
      ...bridge,
      offsetPx: offsetAt.get(bridge.at) ?? nearestOffset(offsetAt, bridge.at),
      fromLane: laneByKey.get(bridge.from) ?? 0,
      toLane: laneByKey.get(bridge.to) ?? 0,
    }),
  );

  // R8 — what each participant is holding, from the rows alone: an assignment
  // it was given with no report back, or a report it filed with no verdict on
  // it. Both are fold facts the stream already renders; neither is a guess.
  const heads = buildRouteHeads({
    drafts,
    founderPubkey: input.founderPubkey,
    transactions: input.transactions,
    seatAuthorities: input.seatAuthorities ?? [],
    nowMs: input.nowMs,
  });

  const roads: CodingSessionRouteRoad[] = drafts.map((draft, index) => {
    const startOffsetPx =
      draft.startedAt === null
        ? 0
        : (offsetAt.get(draft.startedAt) ??
          nearestOffset(offsetAt, draft.startedAt));
    const endOffsetPx =
      draft.endedAt === null
        ? nowOffsetPx
        : (offsetAt.get(draft.endedAt) ??
          nearestOffset(offsetAt, draft.endedAt));
    return {
      key: draft.key,
      founder: draft.founder,
      label: draft.label,
      accent: draft.accent,
      lane: index,
      startedAt: draft.startedAt,
      startedAtSource: draft.startedAtSource,
      endedAt: draft.endedAt,
      live: draft.live,
      startOffsetPx,
      endOffsetPx,
      head: heads.get(draft.key) ?? EMPTY_HEAD,
      hiddenSignCount: hiddenByRoad.get(draft.key ?? OFF_ROAD) ?? 0,
      expanded: draft.key !== null && expandedRoads.has(draft.key),
      undatedSignCount: undatedByRoad.get(draft.key ?? OFF_ROAD) ?? 0,
    } satisfies CodingSessionRouteRoad;
  });

  return {
    roads,
    signs: placedSigns,
    bridges: placedBridges,
    stretches,
    here: buildRouteHere({
      visibleAt: input.visibleAt ?? [],
      offsetAt,
      nowAt,
      nowOffsetPx,
    }),
    nowAt,
    nowOffsetPx,
    heightPx: nowOffsetPx,
    lanePitchPx,
    hiddenRoadCount,
    hiddenSignCount,
    undatedSignCount: [...undatedByRoad.values()].reduce(
      (sum, value) => sum + value,
      0,
    ),
  };
}

/**
 * Bucket for signs whose author no drawn road claims. Deliberately not a legal
 * road key, so it can never collide with an execution key or the founder.
 */
const OFF_ROAD = "route-road:off";

const EMPTY_HEAD: CodingSessionRouteRoadHead = {
  holding: null,
  sinceMs: null,
  sinceAt: null,
  word: null,
  seatAuthorityDetail: null,
  seatAuthorityRemedy: null,
};

/** Offset of the nearest point at or before `at`; 0 when the map has none. */
function nearestOffset(
  offsetAt: ReadonlyMap<number, number>,
  at: number,
): number {
  let best = 0;
  let bestAt = Number.NEGATIVE_INFINITY;
  for (const [pointAt, offset] of offsetAt) {
    if (pointAt <= at && pointAt > bestAt) {
      bestAt = pointAt;
      best = offset;
    }
  }
  return best;
}

/**
 * What each road head is holding, and its seat authority sentence.
 *
 * An assignment is open when its assignee filed no report referencing it; a
 * report is open when no verdict references it. Both are read off the rows the
 * stream renders — `parentEventId` on the row's source, projected by Lane D —
 * and never inferred from silence.
 */
function buildRouteHeads(input: {
  drafts: readonly RoadDraft[];
  founderPubkey: string | null;
  transactions: readonly CodingSessionRouteTransaction[];
  seatAuthorities: readonly CodingSessionSeatAuthority[];
  nowMs: number;
}): Map<CodingSessionRouteRoadKey, CodingSessionRouteRoadHead> {
  const answeredAssignments = new Set<string>();
  const answeredReports = new Set<string>();
  for (const row of input.transactions) {
    if (row.type === "report" && row.parentEventId !== null) {
      answeredAssignments.add(row.parentEventId);
    }
    if (row.type === "disposition" && row.parentEventId !== null) {
      answeredReports.add(row.parentEventId);
    }
  }
  const heads = new Map<
    CodingSessionRouteRoadKey,
    CodingSessionRouteRoadHead
  >();
  for (const draft of input.drafts) {
    const authority = draft.founder
      ? null
      : (input.seatAuthorities.find(
          (candidate) => candidate.executionKey === draft.key,
        ) ?? null);
    const ungranted =
      authority?.kind === "created-ungranted" ? authority : null;
    let holding: "assignment" | "report" | null = null;
    let sinceAt: number | null = null;
    for (const row of input.transactions) {
      const pubkey = draft.founder ? input.founderPubkey : draft.actorPubkey;
      if (pubkey === null) continue;
      if (
        row.type === "assignment" &&
        sameKey(row.counterparty?.pubkey ?? null, pubkey) &&
        !answeredAssignments.has(row.meta.sourceEventId)
      ) {
        if (sinceAt === null || row.createdAt > sinceAt) {
          holding = "assignment";
          sinceAt = row.createdAt;
        }
      }
      if (
        row.type === "report" &&
        sameKey(row.actor.pubkey, pubkey) &&
        !answeredReports.has(row.meta.sourceEventId)
      ) {
        if (sinceAt === null || row.createdAt > sinceAt) {
          holding = "report";
          sinceAt = row.createdAt;
        }
      }
    }
    heads.set(draft.key, {
      holding,
      sinceAt,
      sinceMs: sinceAt === null ? null : input.nowMs - sinceAt * 1_000,
      word: draft.word,
      seatAuthorityDetail: ungranted?.detail ?? null,
      seatAuthorityRemedy: ungranted?.remedy ?? null,
    });
  }
  return heads;
}

/** The `You are here` band from the rows currently on screen. */
function buildRouteHere(input: {
  visibleAt: readonly number[];
  offsetAt: ReadonlyMap<number, number>;
  nowAt: number;
  nowOffsetPx: number;
}): CodingSessionRouteHere | null {
  const seconds = input.visibleAt.filter((value) => Number.isFinite(value));
  if (seconds.length === 0) return null;
  const fromAt = Math.min(...seconds);
  const toAt = Math.max(...seconds);
  const fromOffset =
    input.offsetAt.get(fromAt) ?? nearestOffset(input.offsetAt, fromAt);
  const toOffset =
    input.offsetAt.get(toAt) ?? nearestOffset(input.offsetAt, toAt);
  const toNowMs = Math.max(0, (input.nowAt - toAt) * 1_000);
  return {
    fromAt,
    toAt,
    offsetPx: fromOffset,
    heightPx: Math.max(ROUTE_BAND_MIN_PX, toOffset - fromOffset),
    toNowMs,
    toNowLabel: (() => {
      const label = formatCodingSessionRouteDuration(toNowMs);
      return label === null ? null : `${label} to Now`;
    })(),
  };
}
