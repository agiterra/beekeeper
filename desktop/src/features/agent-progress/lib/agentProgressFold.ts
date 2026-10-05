/**
 * The Agent Progress fold: every durable coding session this viewer can read,
 * as one lane each.
 *
 * **Two axes, never one.**
 *
 * 1. *Coordination* — `Reachable` / `Unverified` / `Closed` — comes from
 *    `shared/coordination`, which decides it from authority-proven generations
 *    and current kind-24223 leases. Nothing in this file may compute it, widen
 *    it, or substitute a proxy for it.
 * 2. *Reported status* — what the provider's newest signed 44223 said, and how
 *    long ago it said it. This is history. `Running` here means "it reported
 *    running", not "it is running".
 *
 * A row therefore reads `Reachable · Running` or `Unverified · last reported
 * running`. It may never collapse the two into one word: the old fold had a
 * 30-minute freshness window over `statusAt` deciding between `working` and
 * `stale`, which is precisely the claim metadata recency cannot support — a
 * machine that dies mid-turn keeps its last fact saying `running` forever, so
 * the window only chose how long to repeat it.
 *
 * **Counting.** One lane is one durable session. Distinct provider executions
 * are nested inside it and counted by `executionKey`; resumed generations of
 * one execution never inflate either the session or execution count.
 *
 * **Floors, not censuses.** When the read was incomplete the counts are
 * disclosed as "at least N", and an empty list says what it actually knows —
 * that this read returned no sessions — rather than claiming there are no
 * agents.
 */
import { coordinationDisplayGeneration } from "@/shared/coordination/sessionCoordinationFormat";
import type { CoordinatedSessionNameOrigin } from "@/shared/coordination/sessionCoordinationNames";
import type {
  CoordinatedGeneration,
  CoordinatedSession,
  SessionCoordinationAmbiguity,
  SessionCoordinationState,
} from "@/shared/coordination/sessionCoordinationTypes";

/** Longest activity line the fold will emit; longer text is elided visibly. */
export const AGENT_PROGRESS_ACTIVITY_MAX_CHARS = 120;

/** The transcript fields this projection needs, independent of its producer. */
export type AgentProgressTranscriptItem =
  | { type: "tool"; toolName: string; title: string; isError: boolean }
  | { type: "message"; text: string; title: string }
  | { type: "thought" }
  | { type: "plan" }
  | {
      type: "lifecycle";
      renderClass: "status" | "permission" | "error";
      text: string;
      title: string;
    }
  | { type: "metadata"; title: string };

/** Where a lane can be opened, when the local catalog knows the coordinates. */
export type AgentProgressOpenTarget = {
  channelId: string;
  generationId: string;
};

/**
 * Locally-held presentation for one session, joined in by `sessionRef`.
 *
 * Enrichment only. The lane exists because signed coordination facts prove it
 * exists; the transcript makes it legible. A lane with no local transcript is
 * still a real lane and still renders — it just has nothing to say about what
 * the agent is doing.
 */
export type AgentProgressLocalDetail = {
  sessionRef: string;
  label: string | null;
  runtimeLabel: string | null;
  transcript: readonly AgentProgressTranscriptItem[];
  openTarget: AgentProgressOpenTarget | null;
};

/** One folded lane. Every field is a fact; nothing here is humanized. */
export type AgentProgressLane = {
  /** `channelId` + `sessionKey`, escaped — the row's stable React identity. */
  laneId: string;
  sessionKey: string;
  sessionRef: string | null;
  /** The channel that proved this session, or null when its proof spans several. */
  channelId: string | null;
  label: string;
  /**
   * Where {@link label} came from when it is the session's signed name, so a
   * generated title (44252) is marked "Auto-named". Null when the label is a
   * local catalog row or a fallback, or the name is a person's with no origin
   * recorded.
   */
  labelOrigin: CoordinatedSessionNameOrigin | null;
  goal: string | null;
  runtimeLabel: string | null;
  /** The only liveness claim on this row. Never derived from `statusAt`. */
  coordination: SessionCoordinationState;
  /** The provider's newest signed status, or null when none was observed. */
  reportedStatus: string | null;
  /** Age in seconds of that report, or null when nothing was observed. */
  reportedAgeSeconds: number | null;
  /** Newest activity, or null when the lane has no local transcript at all. */
  activity: string | null;
  /** True when {@link activity} is an error the lane is currently explaining. */
  activityIsError: boolean;
  /** Distinct provider executions this one durable session stands for. */
  executionCount: number;
  /** When the current generation's live lease lapses, in epoch seconds. */
  leaseExpiresAt: number | null;
  openTarget: AgentProgressOpenTarget | null;
};

/**
 * Footer counts.
 *
 * Sessions, never executions — `executions` is carried separately so a surface
 * that wants it has to say the word rather than inflate `sessions`. `atLeast`
 * is the honesty flag: when the read was partial these are floors.
 */
export type AgentProgressAggregate = {
  sessions: number;
  reachable: number;
  unverified: number;
  closed: number;
  executions: number;
  atLeast: boolean;
};

export type AgentProgressModel = {
  lanes: AgentProgressLane[];
  aggregate: AgentProgressAggregate;
};

function isErrorItem(item: AgentProgressTranscriptItem): boolean {
  if (item.type === "tool") return item.isError;
  return item.type === "lifecycle" && item.renderClass === "error";
}

/** Collapse whitespace and bound the length, disclosing the truncation. */
function bounded(text: string): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > AGENT_PROGRESS_ACTIVITY_MAX_CHARS
    ? `${flat.slice(0, AGENT_PROGRESS_ACTIVITY_MAX_CHARS - 1)}…`
    : flat;
}

function summarizeItem(item: AgentProgressTranscriptItem): string | null {
  switch (item.type) {
    case "tool":
      return bounded(`▸ ${item.toolName || item.title}`) || null;
    case "message":
      return bounded(item.text) || bounded(item.title) || null;
    case "thought":
      return "Thinking";
    case "plan":
      return "Plan updated";
    case "lifecycle":
      return bounded(item.text) || bounded(item.title) || null;
    case "metadata":
      return bounded(item.title) || null;
  }
}

/**
 * The one activity line a lane shows.
 *
 * `preferError` is the failed-lane rule: a row whose provider reported a
 * failure leads with what went wrong, so it explains itself at a glance. Every
 * other row leads with its newest activity, because an old error on a lane
 * that recovered is not what the lane is doing now.
 */
export function summarizeAgentLaneActivity(
  transcript: readonly AgentProgressTranscriptItem[],
  preferError: boolean,
): { text: string; isError: boolean } | null {
  if (preferError) {
    for (let index = transcript.length - 1; index >= 0; index -= 1) {
      const item = transcript[index];
      if (!isErrorItem(item)) continue;
      const text = summarizeItem(item);
      if (text) return { text, isError: true };
    }
  }
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    const text = summarizeItem(item);
    if (text) return { text, isError: isErrorItem(item) };
  }
  return null;
}

/**
 * Stable lane identity.
 *
 * The delimiter is written as an escape rather than a raw control byte: a
 * literal NUL in a source file survives almost every editor and diff viewer as
 * nothing at all, so the key looks like plain concatenation and collides
 * silently the first time a channel id ends where a session key begins.
 */
export function agentProgressLaneId(
  channelId: string | null,
  sessionKey: string,
): string {
  return `${channelId ?? ""}\u0000${sessionKey}`;
}

/** Which statuses make a row lead with its error, if it has one. */
function reportedFailure(status: string | null): boolean {
  return status === "failed" || status === "interrupted";
}

/** Sort order: reachable work first, then unverified, then settled history. */
function coordinationPriority(state: SessionCoordinationState): number {
  switch (state) {
    case "provider_reachable":
      return 0;
    case "open_unverified":
      return 1;
    case "closed":
      return 2;
  }
}

function byteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

/**
 * A lane's fallback name when no local catalog row and no signed name exist —
 * neither the founder's 44229 nor a standing provider's generated 44252, which
 * the shared fold resolves into `session.name`
 * (`shared/coordination/sessionCoordinationNames.ts`). Never a bare 64-char
 * hash: a label the reader cannot match to anything on screen is worse than
 * admitting the session is unnamed.
 */
function fallbackLabel(session: CoordinatedSession): string {
  if (session.sessionRef) return `Session ${session.sessionRef.slice(0, 8)}`;
  const generation = coordinationDisplayGeneration(session);
  return generation
    ? `Execution ${generation.executionKey.slice(-8)}`
    : "Unnamed session";
}

/**
 * Fold the coordination read, plus whatever local detail is available, into
 * the panel's model.
 *
 * `complete` and `ambiguities` come from the read, not from this fold: whether
 * the list is a census or a floor is a fact about the queries, and the fold
 * must not launder it.
 */
export function foldAgentProgress(input: {
  sessions: readonly CoordinatedSession[];
  channelsBySession: ReadonlyMap<string, string[]>;
  /** The shared fold's name origins; absent means "unknown", never a marker. */
  nameOriginsBySession?: ReadonlyMap<string, CoordinatedSessionNameOrigin>;
  detailBySessionRef: ReadonlyMap<string, AgentProgressLocalDetail>;
  nowSeconds: number;
  complete: boolean;
  ambiguities?: readonly SessionCoordinationAmbiguity[];
}): AgentProgressModel {
  const lanes = input.sessions.map((session): AgentProgressLane => {
    const channels = input.channelsBySession.get(session.sessionKey) ?? [];
    const channelId = channels.length === 1 ? channels[0] : null;
    const detail = session.sessionRef
      ? (input.detailBySessionRef.get(session.sessionRef) ?? null)
      : null;
    const generation: CoordinatedGeneration | null =
      coordinationDisplayGeneration(session);
    const reportedStatus = generation?.status ?? null;
    const activity = detail
      ? summarizeAgentLaneActivity(
          detail.transcript,
          reportedFailure(reportedStatus),
        )
      : null;
    return {
      laneId: agentProgressLaneId(channelId, session.sessionKey),
      sessionKey: session.sessionKey,
      sessionRef: session.sessionRef,
      channelId,
      label: session.name ?? detail?.label ?? fallbackLabel(session),
      labelOrigin: session.name
        ? (input.nameOriginsBySession?.get(session.sessionKey) ?? null)
        : null,
      goal: session.goal,
      runtimeLabel: detail?.runtimeLabel ?? null,
      coordination: session.coordinationState,
      reportedStatus,
      reportedAgeSeconds:
        generation?.statusAt === null || generation?.statusAt === undefined
          ? null
          : Math.max(0, input.nowSeconds - generation.statusAt),
      activity: activity?.text ?? null,
      activityIsError: activity?.isError ?? false,
      executionCount: new Set(
        session.generations.map((candidate) => candidate.executionKey),
      ).size,
      leaseExpiresAt:
        generation && generation.reachability === "provider_reachable"
          ? generation.leaseExpiresAt
          : null,
      openTarget: detail?.openTarget ?? null,
    };
  });

  lanes.sort((left, right) => {
    const byCoordination =
      coordinationPriority(left.coordination) -
      coordinationPriority(right.coordination);
    if (byCoordination !== 0) return byCoordination;
    const leftAge = left.reportedAgeSeconds ?? Number.MAX_SAFE_INTEGER;
    const rightAge = right.reportedAgeSeconds ?? Number.MAX_SAFE_INTEGER;
    if (leftAge !== rightAge) return leftAge - rightAge;
    return byteOrder(left.laneId, right.laneId);
  });

  const aggregate: AgentProgressAggregate = {
    sessions: lanes.length,
    reachable: 0,
    unverified: 0,
    closed: 0,
    executions: 0,
    // An ambiguity the fold refused to resolve is a session it may have
    // dropped, so it makes the counts a floor exactly as a failed query does.
    atLeast: !input.complete || (input.ambiguities?.length ?? 0) > 0,
  };
  for (const lane of lanes) {
    if (lane.coordination === "provider_reachable") aggregate.reachable += 1;
    else if (lane.coordination === "open_unverified") aggregate.unverified += 1;
    else aggregate.closed += 1;
    aggregate.executions += lane.executionCount;
  }
  return { lanes, aggregate };
}
