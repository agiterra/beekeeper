/**
 * The one call behind the Mission panel's work coverage.
 *
 * `project_work_coverage` is a native command: this module fetches the signed
 * events, hands them over whole, and returns the projection **verbatim**. It
 * computes nothing a person reads and it contains no fold — `buzz-core`'s
 * `assemble_fold_inputs` + `fold_work` are the only implementation, shared
 * with `bee sessions work status`, so the app and the CLI cannot disagree
 * about a criterion while both sound confident.
 *
 * Three failure modes are kept apart on purpose, because collapsing any of
 * them into "nothing is covered" is the lie this surface exists to prevent:
 *
 * - the transport failed (the error propagates),
 * - the native side refused the input (it names which fact was missing),
 * - a plan could not be read at its commit (`unreadablePlans` says which, and
 *   the fold reports those criteria as `unknown`, never `open`).
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";

/** The Tauri command name. Owned by the Rust implementation. */
export const PROJECT_WORK_COVERAGE_COMMAND = "project_work_coverage";

/** The wire-schema string the native command accepts. */
export const PROJECT_WORK_REQUEST_SCHEMA = "buzz-project-work-request/v1";

/** The wire-schema string of the native command's envelope. */
export const PROJECT_WORK_RESPONSE_SCHEMA = "buzz-project-work-response/v1";

/** The frozen contract's own schema string (`conformance/project-work`). */
export const PROJECT_WORK_COVERAGE_SCHEMA = "buzz-project-work-coverage/v1";

/**
 * One signed event, exactly as it was signed.
 *
 * The same seven fields the team-transaction fold already sends
 * (`ImmutableCodingSessionTeamWireEvent`): nothing is reshaped, and nothing
 * that was not signed is carried.
 */
export type SignedWireEvent = {
  id: string;
  pubkey: string;
  created_at: number;
  kind: number;
  tags: readonly (readonly string[])[];
  content: string;
  sig: string;
};

/** Narrow a fetched event to the fields its signature covers. */
export function toSignedWireEvent(event: RelayEvent): SignedWireEvent {
  return {
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.created_at,
    kind: event.kind,
    tags: event.tags,
    content: event.content,
    sig: event.sig,
  };
}

/**
 * The keys the native command requires on every request.
 *
 * Named here so a drift like lane 213's — the Rust struct gaining two
 * required fields while every TypeScript unit test stayed green — fails a
 * test instead of failing at runtime in front of a person.
 */
export const PROJECT_WORK_REQUIRED_KEYS = [
  "schema",
  "sessionRef",
  "projectRef",
  "founderPubkey",
  "channelRef",
  "genesisRef",
  "workEvents",
  "teamEvents",
] as const;

/**
 * Refuse a request that is missing a key the native side requires.
 *
 * Called before the invoke, so the failure names the missing field rather
 * than arriving as a serde error about a struct the caller cannot see.
 */
export function assertProjectWorkRequestComplete(
  request: Record<string, unknown>,
): void {
  const missing = PROJECT_WORK_REQUIRED_KEYS.filter(
    (key) => request[key] === undefined || request[key] === null,
  );
  if (missing.length > 0) {
    throw new Error(
      `the project-work request is missing ${missing.join(", ")}, which the native fold requires`,
    );
  }
}

/** One active seat, from the accepted kind:44228 projection. */
export type ProjectWorkSeatInput = {
  actorPubkey: string;
  role: string;
};

/** One active operator grant, from the same accepted projection. */
export type ProjectWorkGrantInput = {
  actorPubkey: string;
  grantEventRef: string;
  maySteer: boolean;
};

/**
 * Everything the frontend fetched.
 *
 * The event lists carry **whole signed events**, unmodified: `buzz-core`
 * reads them itself, and an event this layer reshaped is an event it cannot
 * establish a fact from.
 */
export type ProjectWorkRequest = {
  schema: typeof PROJECT_WORK_REQUEST_SCHEMA;
  sessionRef: string;
  projectRef: string;
  founderPubkey: string;
  /** `null` means unread: ref observations then read `unknown`. */
  relaySelfKey: string | null;
  activeSeats: readonly ProjectWorkSeatInput[];
  activeGrants: readonly ProjectWorkGrantInput[];
  workEvents: readonly RelayEvent[];
  /**
   * The session's kind:44244 team transactions **with their signatures**.
   *
   * Lane 213 made the assembler fold these with the canonical 44244 fold,
   * which verifies what it judges, so they cross the boundary as full signed
   * events rather than the plain shape the other lists use. Exactly the seven
   * signed fields are sent: the local-only render keys `localKey` and
   * `pending` were never part of a signature and a strict Rust decoder is
   * entitled to refuse them.
   */
  teamEvents: readonly SignedWireEvent[];
  /** The session's channel uuid, which scopes that fold. */
  channelRef: string;
  /** The session genesis event id, for the same reason. */
  genesisRef: string;
  goalEvents: readonly RelayEvent[];
  /** kind 46023 / 46014 / 46013, in one list; the native side splits them. */
  hostEvents: readonly RelayEvent[];
  refStates: readonly RelayEvent[];
};

/** How a criterion stands, in the contract's words. */
export type ProjectWorkCriterionStatus =
  | "open"
  | "covered"
  | "stale"
  | "unknown";

/** A declaration's state; `conformance/project-work/README.md` § (c). */
export type ProjectWorkDeclarationState =
  | "head"
  | "superseded"
  | "stale"
  | "conflict";

export type ProjectWorkProof =
  | { kind: "review" }
  | { kind: "action"; name: string; step: string }
  | { kind: "git-ref" };

export type ProjectWorkEvidenceRef = {
  kind: string;
  eventId: string;
};

export type ProjectWorkCriterion = {
  criterionId: string;
  proof: ProjectWorkProof;
  status: ProjectWorkCriterionStatus;
  assignmentRefs: readonly string[];
  evidence: readonly ProjectWorkEvidenceRef[];
  artifactCommit: string | null;
  reasonCode: string | null;
  reason: string | null;
};

export type ProjectWorkPlanRef = {
  repository: string;
  commit: string;
  path: string;
};

export type ProjectWorkDeclaration = {
  workId: string;
  declarationRef: string;
  planRef: ProjectWorkPlanRef;
  state: ProjectWorkDeclarationState;
  supersedes: readonly string[];
  supersededBy: readonly string[];
  stateReasonCode: string | null;
  stateReason: string | null;
  planResolved: boolean;
  candidateArtifact: string | null;
  artifactCommits: readonly string[];
  criteria: readonly ProjectWorkCriterion[];
  coverageComplete: boolean;
  coverageReasonCode: string | null;
  coverageReason: string | null;
};

/** The fold's output, exactly as `expected-fold.json` records it. */
export type ProjectWorkCoverage = {
  schema: string;
  sessionRef: string;
  projectRef: string;
  declarations: readonly ProjectWorkDeclaration[];
  excluded: readonly { eventId: string; code: string; message: string }[];
  conflicts: readonly {
    workId: string;
    heads: readonly string[];
    message: string;
  }[];
};

/** A plan this host could not read at its commit, and why. */
export type ProjectWorkUnreadablePlan = {
  repository: string;
  commit: string;
  path: string;
  /** `plan_unreadable` or `actions_uncompilable`. */
  reasonCode: string;
  reason: string;
};

export type ProjectWorkResponse = {
  schema: string;
  implementation: string;
  coverage: ProjectWorkCoverage;
  unreadablePlans: readonly ProjectWorkUnreadablePlan[];
  agentsRepoRead: boolean;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Check the envelope, not the fold.
 *
 * The projection inside is rendered verbatim; re-validating its every field
 * here would be a second opinion about a contract `buzz-core` owns. What this
 * checks is that the answer is the *shape of answer* asked for, and that it
 * is about the session that asked — a late answer from a previous scope
 * overwriting the current one is the bug this binding prevents.
 */
export function decodeProjectWorkResponse(
  value: unknown,
  expected: { sessionRef: string; projectRef: string },
): ProjectWorkResponse {
  if (
    !isRecord(value) ||
    value.schema !== PROJECT_WORK_RESPONSE_SCHEMA ||
    value.implementation !== "buzz-core" ||
    !isRecord(value.coverage) ||
    value.coverage.schema !== PROJECT_WORK_COVERAGE_SCHEMA ||
    !Array.isArray(value.coverage.declarations) ||
    !Array.isArray(value.unreadablePlans) ||
    typeof value.agentsRepoRead !== "boolean"
  ) {
    throw new Error(
      "the native project-work fold returned a response this build does not recognise",
    );
  }
  if (
    value.coverage.sessionRef !== expected.sessionRef ||
    value.coverage.projectRef !== expected.projectRef
  ) {
    throw new Error(
      "the native project-work fold answered about a different session or project than this read asked about",
    );
  }
  return value as unknown as ProjectWorkResponse;
}

/** Fold one session's work coverage natively. */
export async function invokeProjectWorkCoverage(
  request: ProjectWorkRequest,
): Promise<ProjectWorkResponse> {
  assertProjectWorkRequestComplete(
    request as unknown as Record<string, unknown>,
  );
  const raw = await invokeTauri<unknown>(PROJECT_WORK_COVERAGE_COMMAND, {
    request,
  });
  return decodeProjectWorkResponse(raw, {
    sessionRef: request.sessionRef,
    projectRef: request.projectRef,
  });
}
