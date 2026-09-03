/**
 * The Route's vocabulary: its constants, its shapes, and the two decisions a
 * caller makes before it draws anything — how long a span reads as, and
 * whether the gutter can carry the rail at all.
 *
 * Split out of `codingSessionRouteModel.ts` to keep both files inside the
 * repository's 1,000-line ceiling. The model imports from here and re-exports,
 * so every consumer still has one import site
 * (`@/features/coding-sessions/lib/codingSessionRouteModel`).
 */
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionMissionTransactionType,
} from "./codingSessionMissionContracts";
import { CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT } from "./codingSessionMissionContracts";
import type { CodingSessionMissionTransactionRow } from "./codingSessionMissionTransactionRows";
import type { CodingSessionParticipantAccent } from "./codingSessionParticipantAccent";

/** Pixels per minute between two signs less than {@link ROUTE_MAX_GAP_SECONDS} apart. */
export const ROUTE_PX_PER_MINUTE = 12;
/** A gap longer than this becomes a dashed stretch instead of scaled distance. */
export const ROUTE_MAX_GAP_SECONDS = 300;
/** Height of one dashed stretch, silence or queued alike. */
export const ROUTE_STRETCH_PX = 48;
/** Vertical slot one sign occupies; simultaneous signs stack in signed order. */
export const ROUTE_SIGN_SLOT_PX = 20;
/** Lane pitch for a route of four roads or fewer. */
export const ROUTE_LANE_PITCH_PX = 16;
/** Lane pitch from five roads up; more than {@link ROUTE_MAX_ROADS} is not drawn. */
export const ROUTE_LANE_PITCH_TIGHT_PX = 10;
/** Roads drawn at all. Beyond this the extras are disclosed as a count. */
export const ROUTE_MAX_ROADS = 7;
/** Signs kept per road; older ones collapse into `+N earlier` (§9.5). */
export const ROUTE_SIGNS_PER_ROAD_LIMIT =
  CODING_SESSION_MISSION_TRANSACTION_ROW_LIMIT;
/**
 * Ceiling for a road whose `+N earlier` marker the reader has clicked.
 *
 * Expanding lifts the bound; it does not remove it. A road with more signs
 * than this still shows a `+N earlier` count, so I10's "bounded with visible
 * truncation" holds in both states rather than only in the folded one.
 */
export const ROUTE_SIGNS_PER_ROAD_EXPANDED_LIMIT =
  ROUTE_SIGNS_PER_ROAD_LIMIT * 5;
/** The `You are here` band is never shorter than this, however brief its span. */
export const ROUTE_BAND_MIN_PX = 24;
/** The rail's width when the viewer has not chosen one. */
export const ROUTE_RAIL_WIDTH_PX = 224;
/** The collapsed rail: the scrubber's own 40 px track. */
export const ROUTE_SCRUBBER_WIDTH_PX = 40;
/**
 * The stream's floor — the same 420 px the Inspector's clamp already enforces.
 *
 * This is now the *only* automatic opinion about the rail. §9.2 used to also
 * demand a 1,280 px body and an 816 px reading reserve, which folded the rail
 * away at a ~1,590 px window (SURFACES §20e) and took the one sentence about
 * who is waiting on whom with it. Folding may hide detail; it may not be the
 * whole opinion about a panel the viewer is now able to size and collapse for
 * themselves.
 */
export const ROUTE_STREAM_MIN_WIDTH_PX = 420;

/** What one sign stands for. Every member maps to a signed artifact. */
export type CodingSessionRouteSignKind =
  | CodingSessionMissionTransactionType
  | "hire"
  | "delivery"
  | "seat-ungranted";

/**
 * The founder's own lane.
 *
 * A real key rather than `null`, because `null` already means "this sign
 * belongs to no drawn road" — an author the umbrella cannot place. Collapsing
 * the two put every unattributable sign in the founder's lane, which is the
 * single most misleading thing this map could do.
 */
export const CODING_SESSION_ROUTE_FOUNDER_ROAD = "route-road:founder";

/** A road key: an execution key, the founder sentinel, or null for off-road. */
export type CodingSessionRouteRoadKey = string | null;

/** One accepted create, as the route needs it. */
export type CodingSessionRouteHire = {
  /** Execution key of the seat this create minted. */
  executionKey: string;
  /**
   * Actor pubkey that signed the 44221, or null when no create observation
   * resolved one. A hire nobody can attribute draws no junction.
   */
  hiredByPubkey: string | null;
  /**
   * Signed `created_at` of the accepted create, in unix seconds, or null when
   * the create's own time is not reachable. A null time draws no sign: the
   * road still exists, it simply has no dated junction.
   */
  at: number | null;
  /** Signed event id of the create, when known. */
  sourceEventId: string | null;
};

/**
 * A stream row plus the one signed field the row shape does not carry: the
 * parent it references. The road head needs it to say what a participant is
 * *holding* — an assignment nobody reported on, a report nobody ruled on.
 */
export type CodingSessionRouteTransaction =
  CodingSessionMissionTransactionRow & {
    /** `assignmentRef` / `reportRef` from the signed body; null for a root. */
    parentEventId: string | null;
  };

/**
 * Zip the rows the stream renders with the signed inputs they were built from.
 *
 * The row builder drops `parentEventId` because no stream row shows it; the
 * route needs it and must not re-derive it, so the two are matched by signed
 * event id and a row whose input is missing is carried through with a null
 * parent rather than dropped.
 */
export function buildCodingSessionRouteTransactions(
  rows: readonly CodingSessionMissionTransactionRow[],
  inputs: readonly CodingSessionMissionTransactionInput[],
): CodingSessionRouteTransaction[] {
  const parents = new Map(
    inputs.map((input) => [input.sourceEventId, input.parentEventId] as const),
  );
  return rows.map((row) => ({
    ...row,
    parentEventId: parents.get(row.meta.sourceEventId) ?? null,
  }));
}

/**
 * One participant, resolved by the surface.
 *
 * `label`, `word` and `live` come from exactly the functions the roster chips
 * use, so a seat cannot read `live` on a chip and `idle` on its road head
 * (§9.9 "Road heads use the same W1 function as the chips").
 */
export type CodingSessionRouteParticipant = {
  executionKey: string;
  label: string;
  /** The seat's actor pubkey; how a transaction author finds its road. */
  actorPubkey: string | null;
  /** Role slug, so the lead takes the second lane. */
  role: string | null;
  /** `buildCodingSessionTargetKey` of the active generation, when known. */
  targetKey: string | null;
  /** W1's own answer for this seat, or null when the surface has none. */
  word: string | null;
  /** True only when W1 answers working — the chip's own test, verbatim. */
  live: boolean;
  /**
   * Earliest signed moment observed for this seat, in unix seconds. Used as
   * the road's start only when no hire dates it, and labelled as such.
   */
  firstSignedAt: number | null;
  /** When this seat was released, in unix seconds; null while it runs. */
  releasedAt: number | null;
};

/** One road: a participant's lane down the map. */
export type CodingSessionRouteRoad = {
  key: CodingSessionRouteRoadKey;
  founder: boolean;
  label: string;
  accent: CodingSessionParticipantAccent;
  /** Lane index from the left, 0-based. */
  lane: number;
  /** Unix seconds the road starts, or null when nothing dates it. */
  startedAt: number | null;
  /** How `startedAt` was established; null when the road has no start. */
  startedAtSource: "hire" | "first-signed" | null;
  /** Unix seconds the road ends (a released seat); null while it runs. */
  endedAt: number | null;
  /** W1's `working` answer. The founder is not a seat and is never live. */
  live: boolean;
  /** Pixel offsets of the road's own span within the map. */
  startOffsetPx: number;
  endOffsetPx: number;
  head: CodingSessionRouteRoadHead;
  /** Signs dropped by the per-road bound; 0 when none were. */
  hiddenSignCount: number;
  /** True when the reader lifted this road's bound via `+N earlier`. */
  expanded: boolean;
  /**
   * Facts this road owns that carry no usable time, so nothing could be
   * placed for them — today only a delivery whose local observation is
   * unknown. Disclosed as `N undated` rather than dropped in silence
   * (REVIEW-A4 F7).
   */
  undatedSignCount: number;
};

/** What the road head says under the Now rule (R8). */
export type CodingSessionRouteRoadHead = {
  /**
   * The unanswered fact this participant is holding: an assignment with no
   * report, or a report with no verdict. Null when it holds neither.
   */
  holding: "assignment" | "report" | null;
  /** How long it has been holding it, in ms; null when nothing dates it. */
  sinceMs: number | null;
  /** Unix seconds the held fact was signed; null when nothing dates it. */
  sinceAt: number | null;
  /** W1's word, or null. The founder is not a seat and carries none. */
  word: string | null;
  /** Seat authority copy when the seat is created-but-ungranted; else null. */
  seatAuthorityDetail: string | null;
  /** The exact repair command for an ungranted seat; else null. */
  seatAuthorityRemedy: string | null;
};

/** One sign in the sign column, anchored to a road and a moment. */
export type CodingSessionRouteSign = {
  key: string;
  road: CodingSessionRouteRoadKey;
  /** Unix seconds. */
  at: number;
  kind: CodingSessionRouteSignKind;
  /** The frozen word, verbatim — never re-worded for the gutter. */
  word: string;
  /** The row's own title plus its time; the sign's `aria-label` and tooltip. */
  title: string;
  sourceEventId: string | null;
  weight: "attention" | "standard";
  tone: "critical" | "caution" | null;
  /**
   * Whether this sign's moment is covered by a signature.
   *
   * `signed` is a signed `created_at`. `local` is Desktop's own observation
   * (`CodingSessionTeamWakeDelivery.observedAtMs`), which is the only time a
   * delivery carries — the rail draws those hollow, prefixes the clock with
   * `~`, and says "local time, not signed" to a screen reader, because a mixed
   * axis that looks uniform is the map lying about its own units
   * (REVIEW-A4 F7).
   */
  timeSource: "signed" | "local";
  /**
   * True when the signed row names a `requiredAction` — a decision held on a
   * person. Rendered as an extra amber mark beside the word, never as a second
   * sign for the same fact.
   */
  requiresDecision: boolean;
  /**
   * The stream row key this sign reveals, or null when the stream renders no
   * row for it (a seat authority, a delivery observed with no row of its own).
   */
  revealKey: string | null;
  offsetPx: number;
};

/** One signed handoff, drawn from the author's lane to the counterparty's. */
export type CodingSessionRouteBridge = {
  key: string;
  /**
   * The sign this bridge belongs to. A bridge is drawn only when its sign
   * survived the per-road bound: an arrow for a handoff the rail's own
   * `+N earlier` says is not shown is a mark with nothing behind it
   * (REVIEW-A4 F8).
   */
  ownerSignKey: string;
  from: CodingSessionRouteRoadKey;
  to: CodingSessionRouteRoadKey;
  at: number;
  kind: CodingSessionMissionTransactionType | "hire";
  sourceEventId: string | null;
  offsetPx: number;
  /** Lane index of each end, so the component draws without re-resolving. */
  fromLane: number;
  toLane: number;
};

/** A compressed span: a silence longer than five minutes, or a queued wake. */
export type CodingSessionRouteStretch = {
  key: string;
  /** Null means every road alive across the span (a silence). */
  road: CodingSessionRouteRoadKey;
  kind: "silence" | "queued";
  fromAt: number;
  toAt: number;
  durationMs: number;
  /** `7m`, `4m 20s`, `4h 37m`. Never `0s`. */
  label: string;
  offsetPx: number;
  heightPx: number;
};

/** The band showing which slice of the map the reader is currently looking at. */
export type CodingSessionRouteHere = {
  fromAt: number;
  toAt: number;
  offsetPx: number;
  heightPx: number;
  /** Distance from the band's newest edge to Now, in ms. */
  toNowMs: number;
  /** `7m to Now`, or null when the band already reaches Now. */
  toNowLabel: string | null;
};

/** The whole map. */
export type CodingSessionRoute = {
  roads: CodingSessionRouteRoad[];
  signs: CodingSessionRouteSign[];
  bridges: CodingSessionRouteBridge[];
  stretches: CodingSessionRouteStretch[];
  here: CodingSessionRouteHere | null;
  /** Unix seconds of the Now rule. */
  nowAt: number;
  nowOffsetPx: number;
  /** Height of the map body, Now rule included. */
  heightPx: number;
  /** Lane pitch this many roads earned. */
  lanePitchPx: number;
  /** Participants past {@link ROUTE_MAX_ROADS}; disclosed, never dropped silently. */
  hiddenRoadCount: number;
  /** Signs dropped by the per-road bound across every road. */
  hiddenSignCount: number;
  /** Undated facts across every road, including those on no road at all. */
  undatedSignCount: number;
};

/**
 * Format a span the way the rail says it: `Nh Nm` above an hour, `Nm Ss`
 * below one, `Ns` below a minute.
 *
 * Returns null under a second, because §9.4.4 forbids `0s`: a distance nobody
 * can measure is not printed as no distance at all.
 */
export function formatCodingSessionRouteDuration(
  durationMs: number,
): string | null {
  if (!Number.isFinite(durationMs) || durationMs < 1_000) return null;
  const totalSeconds = Math.floor(durationMs / 1_000);
  const hours = Math.floor(totalSeconds / 3_600);
  const minutes = Math.floor((totalSeconds % 3_600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
  if (minutes > 0)
    return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
  return `${seconds}s`;
}

/**
 * Is there room for the expanded rail, or must it fold to the scrubber?
 *
 * One question, and it is the stream's: would showing the rail at the
 * viewer's width push the stream under {@link ROUTE_STREAM_MIN_WIDTH_PX}? The
 * body-width and reading-reserve gates are gone — they were an opinion about
 * where a rail belongs, held against a viewer who now has a handle and a
 * collapse control of their own.
 *
 * `railShown` is fed back in for the same reason it always was: the section's
 * measured width already has the rail subtracted when the rail is drawn, so
 * the decision has to be asked about the *same* layout either way or it
 * oscillates on the exact pixel where it flips.
 *
 * This returns whether the rail *fits*. Whether it is *shown* is that answer
 * and the viewer's own collapse choice, and the viewer's choice is never
 * rewritten by a fold — see `useCodingSessionRoute`.
 */
export function codingSessionRouteFits(input: {
  /** Width of the narrative section as currently laid out. */
  sectionWidthPx: number;
  railShown: boolean;
  /** The viewer's rail width; the scrubber's 40 px is the cost either way. */
  railWidthPx: number;
}): boolean {
  if (input.sectionWidthPx <= 0) return false;
  const streamIfExpanded = input.railShown
    ? input.sectionWidthPx
    : input.sectionWidthPx - (input.railWidthPx - ROUTE_SCRUBBER_WIDTH_PX);
  return streamIfExpanded >= ROUTE_STREAM_MIN_WIDTH_PX;
}

/**
 * The scrubber's attention set: the signs a 40 px track still has to carry.
 *
 * R10 names them — a blocker, a decision held on a person, a failed delivery,
 * an ungranted seat. Everything else folds away with the rail.
 */
export function codingSessionRouteAttentionSigns(
  route: CodingSessionRoute,
): CodingSessionRouteSign[] {
  return route.signs.filter(
    (sign) =>
      sign.kind === "mission.blocked" ||
      sign.kind === "seat-ungranted" ||
      sign.requiresDecision ||
      (sign.kind === "delivery" && sign.tone === "critical"),
  );
}
