import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Mock Tauri commands for surface contents (SV-24), owned by lane B2.
 *
 * Tried before the bridge's built-in `switch` (`e2eBridge.ts`). Return
 * `{ handled: true, value }` to answer a command, or `null` to pass it on.
 *
 * A spec opts in by setting `window.__BUZZ_E2E_WAVE_B_SURFACES__` in an init
 * script; with nothing set every command passes through untouched, so no
 * other spec sees a different bridge. Paths never appear here: the tree mock
 * answers in relative entries exactly as the host does.
 */
export type WaveBSurfacesMock = {
  /** `resolve_coding_session_tree`'s answer. */
  treeResolution?: {
    available: boolean;
    source: "session" | "project" | "channel" | null;
    label: string;
    reason: string | null;
    refusal: "notLocal" | "notRecorded" | "storeUnreadable" | null;
  };
  /** `list_coding_session_tree_entries` answers, by relative folder. */
  treeListings?: Record<
    string,
    {
      entries: { name: string; relPath: string; kind: string }[];
      truncated: boolean;
    }
  >;
  /** `coding_session_land`'s whole answer (the adapter's v1 shape). */
  land?: Record<string, unknown>;
  /** Full commit ids `main` holds, newest first, for the Landed read. */
  mainCommits?: string[];
  /** Make the `main` read fail with this message instead. */
  mainError?: string;
  /**
   * Leave `fold_coding_session_team_transactions` to the bridge's own
   * configured response instead of the empty-mission echo below.
   */
  teamFoldPassThrough?: boolean;
  /**
   * Leave `pulse_mission_rows` unanswered (the bridge has no arm for it, so
   * the Pulse surface shows its "Unsupported mocked Tauri command" failure)
   * instead of the no-missions answer below.
   */
  pulseMissionsPassThrough?: boolean;
};

declare global {
  interface Window {
    __BUZZ_E2E_WAVE_B_SURFACES__?: WaveBSurfacesMock;
  }
}

function commit(hash: string, index: number) {
  return {
    hash,
    short_hash: hash.slice(0, 7),
    author_name: "Mock author",
    author_email: "mock@example.com",
    timestamp: 1_800_000_000 - index * 60,
    subject: `Mock commit ${index + 1}`,
  };
}

/**
 * The native team fold's answer for a mission with no team transactions:
 * the request's own context echoed back, nothing included or excluded.
 *
 * This fixture signs no team transactions, so this is exactly what
 * the buzz-core fold returns for it. Without it the Mission evidence read
 * fails ("mock Mission fold response is not configured"), the land rule is
 * never asked, and Verdict and Land read that failure instead of the
 * fixture. A request that does carry transactions is passed on unanswered:
 * this mock does not fold them.
 */
function emptyTeamFold(payload: unknown): Record<string, unknown> | null {
  const request = (payload as { request?: Record<string, unknown> } | null)
    ?.request;
  const context = request?.context as Record<string, unknown> | undefined;
  const inputEventIds = request?.inputEventIds;
  if (!context || !Array.isArray(inputEventIds) || inputEventIds.length !== 0) {
    return null;
  }
  return {
    schema: "buzz-coding-session-team-fold-adapter/v1",
    implementation: "buzz-core",
    inputEventIds: [],
    context: {
      channelRef: context.channelRef,
      sessionRef: context.sessionRef,
      genesisRef: context.genesisRef,
      founderPubkey: context.founderPubkey,
      authorityHeadEventId: context.authorityHeadEventId,
      authorityHeadSeq: context.authorityHeadSeq,
      verifierRequired: context.verifierRequired,
    },
    includedEventIds: [],
    excluded: [],
    conflicts: [],
    assignments: [],
    unseatedReports: [],
    notes: [],
    decisions: [],
    waitingOnDecision: null,
    pendingCompletion: null,
    canonicalTerminal: null,
  };
}

/**
 * `pulse_mission_rows`' answer for this fixture: no mission rows, with the
 * caller's own read errors passed through as the native command passes them.
 *
 * This fixture signs no team transactions, so there is no mission fold to
 * show, and the Pulse surface's acceptance (SV-24) is the digest's session
 * card leading the panel, not mission content. Unanswered, the bridge refuses
 * the command and the panel shows that failure beside the digest; Pulse
 * missions have their own spec (`project-pulse-missions.spec.ts`) answered
 * with the Rust-generated fixture.
 */
function noPulseMissions(payload: unknown): Record<string, unknown> {
  const request = (payload as { request?: Record<string, unknown> } | null)
    ?.request;
  const viewer = request?.viewerPubkey;
  return {
    missionsSchema: "buzz-pulse-mission-rows/v1",
    missionScope:
      "project channels · the newest 8 open sessions by observation time",
    missions: [],
    missionErrors: Array.isArray(request?.readErrors) ? request.readErrors : [],
    openRulings: [],
    rulingsWaitingOnViewer: [],
    overlaps: [],
    viewerPubkey: typeof viewer === "string" ? viewer : null,
  };
}

export async function handleWaveBSurfacesMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  const mock =
    typeof window === "undefined"
      ? undefined
      : window.__BUZZ_E2E_WAVE_B_SURFACES__;
  if (!mock) return null;
  switch (command) {
    case "resolve_coding_session_tree":
      return mock.treeResolution
        ? { handled: true, value: mock.treeResolution }
        : null;
    case "list_coding_session_tree_entries": {
      if (!mock.treeListings) return null;
      const relPath = (payload as { relPath?: unknown } | null)?.relPath ?? "";
      const listing = mock.treeListings[String(relPath)];
      if (!listing)
        throw new Error("This folder is not in the session's tree.");
      return { handled: true, value: listing };
    }
    case "fold_coding_session_team_transactions": {
      if (mock.teamFoldPassThrough) return null;
      const fold = emptyTeamFold(payload);
      return fold ? { handled: true, value: fold } : null;
    }
    case "pulse_mission_rows":
      return mock.pulseMissionsPassThrough
        ? null
        : { handled: true, value: noPulseMissions(payload) };
    case "coding_session_land":
      return mock.land ? { handled: true, value: mock.land } : null;
    case "get_project_repo_snapshot": {
      const targetRef = (payload as { targetRef?: unknown } | null)?.targetRef;
      if (targetRef !== "refs/heads/main") return null;
      if (mock.mainError) throw new Error(mock.mainError);
      if (!mock.mainCommits) return null;
      const commits = mock.mainCommits.map(commit);
      return {
        handled: true,
        value: {
          latest_commit: commits[0] ?? null,
          commits,
          files: [],
          contributors: [],
        },
      };
    }
    default:
      return null;
  }
}
