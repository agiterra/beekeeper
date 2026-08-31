import type { CodingSessionContextLoad } from "./codingSessionContextLoad";
import type { CodingSessionParticipantPresence } from "./codingSessionStreamPresence";
import type {
  CodingSessionTaskModel,
  CodingSessionTaskStatus,
} from "./codingSessionTaskModel";
import type { CodingSessionObservedChanges } from "./codingSessionTranscriptModel";

export const MISSION_INSPECTOR_LIMITS = {
  acceptedPlanSteps: 100,
  contextFacts: 400,
  disclosureEventIds: 20,
  disclosures: 100,
  fileAttributions: 20,
  fileSourceEventIds: 20,
  files: 500,
  filesPerReport: 200,
  goalSourceEventIds: 20,
  missionStateItems: 100,
  participants: 64,
  reports: 100,
  seatPlanSteps: 100,
  seatPlans: 32,
  tests: 500,
  testsPerReport: 100,
} as const;

export type CodingSessionMissionInspectorSection =
  | "goal"
  | "mission-state"
  | "accepted-plan"
  | "seat-plans"
  | "changes"
  | "files"
  | "tests"
  | "team"
  | "context"
  | "reports"
  | "integrity";

export type CodingSessionMissionTruncation = {
  id: string;
  section: CodingSessionMissionInspectorSection;
  shown: number;
  total: number;
  omitted: number;
  notice: string;
};

export type CodingSessionMissionGoalInput =
  | { kind: "absent" }
  | {
      kind: "available";
      sourceEventId: string;
      authorLabel: string;
      text: string;
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionAcceptedPlanInput =
  | { kind: "absent" }
  | {
      kind: "available";
      sourceEventId: string;
      authorLabel: string;
      steps: readonly string[];
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionSeatPlanInput = {
  executionKey: string;
  ownerLabel: string;
  model: CodingSessionTaskModel | null;
};

export type CodingSessionStructuredTestInput = {
  name: string;
  command: string;
  outcome: "passed" | "failed" | "not-run";
  evidence: string | null;
};

export type CodingSessionMissionReportInput = {
  sourceEventId: string;
  authorLabel: string;
  summary: string;
  assignmentRef: string;
  branch: string | null;
  baseSha: string | null;
  headSha: string | null;
  files: readonly string[];
  tests: readonly CodingSessionStructuredTestInput[];
};

export type CodingSessionMissionCanonicalStep = {
  type:
    | "assignment"
    | "report"
    | "refutation"
    | "disposition"
    | "acknowledgement";
  sourceEventId: string;
  authorPubkey: string;
  createdAt: number;
  summary: string;
};

export type CodingSessionMissionStateInput =
  | { kind: "unknown"; detail: string | null }
  | {
      kind: "running";
      sourceEventId: string;
      phase: "assigned" | "reported" | "ruled" | "acknowledged";
      detail: string;
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | {
      kind: "acknowledgement-required";
      sourceEventId: string;
      assignmentRef: string;
      requiredAction: string;
      heldOn: string | null;
      canonicalChain: readonly CodingSessionMissionCanonicalStep[];
    }
  | {
      kind: "waiting-on-person";
      sourceEventId: string;
      requiredAction: string;
      heldOn: string | null;
      canonicalChain?: readonly CodingSessionMissionCanonicalStep[];
    }
  | { kind: "stalled"; sourceEventId: string; detail: string }
  | {
      kind: "blocked";
      sourceEventId: string;
      summary: string;
      blockers: readonly string[];
      requiredAction: string;
    }
  | {
      kind: "completed";
      sourceEventId: string;
      summary: string;
      landedShas: readonly string[];
      followUps: readonly string[];
    }
  | { kind: "conflict"; eventIds: readonly string[] };

export type CodingSessionMissionUsageInput = {
  sourceEventId: string;
  inputTokens: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  toolCalls: number | null;
  costUsd: number | null;
};

export type CodingSessionMissionDisclosureInput = {
  code: string;
  summary: string;
  eventIds: readonly string[];
};

export type CodingSessionMissionInspectorInput = {
  goal: CodingSessionMissionGoalInput;
  acceptedPlan: CodingSessionAcceptedPlanInput;
  seatPlans: readonly CodingSessionSeatPlanInput[];
  reports: readonly CodingSessionMissionReportInput[];
  observedChanges: CodingSessionObservedChanges;
  /** Trusted projected source items for observed paths, when ingress retains them. */
  observedFileSources?: ReadonlyMap<string, readonly string[]>;
  participants: readonly CodingSessionParticipantPresence[];
  contextLoads: ReadonlyMap<string, CodingSessionContextLoad | null>;
  missionState: CodingSessionMissionStateInput;
  usage: CodingSessionMissionUsageInput | null;
  rejectedEventCount: number | null;
  rejectionsTruncated: boolean;
  rejectedReasons: readonly CodingSessionMissionDisclosureInput[];
  conflicts: readonly CodingSessionMissionDisclosureInput[];
};

export type CodingSessionMissionGoalModel =
  | { kind: "absent" }
  | {
      kind: "available";
      sourceEventId: string;
      authorLabel: string;
      text: string;
    }
  | { kind: "conflict"; eventIds: string[] };

export type CodingSessionMissionPlanStep = {
  id: string;
  text: string;
  /** null means the producer published no progress state; it is not pending. */
  status: CodingSessionTaskStatus | null;
};

export type CodingSessionMissionPlanModel = {
  kind: "absent" | "available" | "conflict";
  label: string;
  sourceEventIds: string[];
  authorLabel: string | null;
  steps: CodingSessionMissionPlanStep[];
};

export type CodingSessionMissionSeatPlanModel = {
  executionKey: string;
  ownerLabel: string;
  sourceEventId: string | null;
  explanation: string | null;
  state: CodingSessionTaskModel["state"] | "absent";
  completedCount: number;
  steps: CodingSessionMissionPlanStep[];
};

export type CodingSessionMissionTestModel = CodingSessionStructuredTestInput & {
  id: string;
  sourceEventId: string;
  authorLabel: string;
};

export type CodingSessionMissionFileAttribution = {
  authorLabel: string;
  sourceEventId: string;
};

export type CodingSessionMissionFileModel = {
  path: string;
  observed: boolean;
  observedSourceEventIds: string[];
  observedSourceKnown: boolean;
  reportedBy: CodingSessionMissionFileAttribution[];
  additions: number | null;
  deletions: number | null;
  editCount: number | null;
};

export type CodingSessionMissionContextFact = {
  id: string;
  label: "Assignment" | "Branch" | "Base" | "Head";
  value: string;
  sourceEventId: string;
  authorLabel: string;
};

export type CodingSessionMissionInspectorModel = {
  goal: CodingSessionMissionGoalModel;
  acceptedPlan: CodingSessionMissionPlanModel;
  seatPlans: CodingSessionMissionSeatPlanModel[];
  reports: Array<{
    sourceEventId: string;
    authorLabel: string;
    summary: string;
  }>;
  tests: CodingSessionMissionTestModel[];
  changes: {
    state: "none" | "named" | "unnamed" | "mixed";
    namedEditCount: number;
    unreportedEditCount: number;
    additions: number | null;
    deletions: number | null;
  };
  files: CodingSessionMissionFileModel[];
  participants: Array<
    CodingSessionParticipantPresence & {
      contextLoad: CodingSessionContextLoad | null;
    }
  >;
  contextFacts: CodingSessionMissionContextFact[];
  missionState: CodingSessionMissionStateInput;
  usage: CodingSessionMissionUsageInput | null;
  integrity: {
    rejectedEventCount: number | null;
    rejectionsTruncated: boolean;
    rejectedReasons: CodingSessionMissionDisclosureInput[];
    conflicts: CodingSessionMissionDisclosureInput[];
  };
  truncations: CodingSessionMissionTruncation[];
};

/** Bound and copy trusted projections; every omission becomes visible metadata. */
export function deriveCodingSessionMissionInspectorModel(
  input: CodingSessionMissionInspectorInput,
): CodingSessionMissionInspectorModel {
  const truncations: CodingSessionMissionTruncation[] = [];
  const reports = cap(
    input.reports,
    MISSION_INSPECTOR_LIMITS.reports,
    "reports",
    "structured reports",
    truncations,
  );
  if (reports.length < input.reports.length) {
    for (const section of ["tests", "files", "context"] as const) {
      addTruncation(
        section,
        reports.length,
        input.reports.length,
        "reports contributing to this section",
        truncations,
      );
    }
  }
  const observedFiles = cap(
    input.observedChanges.files,
    MISSION_INSPECTOR_LIMITS.files,
    "changes",
    "observed files",
    truncations,
  );
  if (observedFiles.length < input.observedChanges.files.length) {
    addTruncation(
      "files",
      observedFiles.length,
      input.observedChanges.files.length,
      "observed files",
      truncations,
    );
  }
  const seatPlans = cap(
    input.seatPlans,
    MISSION_INSPECTOR_LIMITS.seatPlans,
    "seat-plans",
    "seat plans",
    truncations,
  ).map((plan) => deriveSeatPlan(plan, truncations));
  const participants = cap(
    input.participants,
    MISSION_INSPECTOR_LIMITS.participants,
    "team",
    "participants",
    truncations,
  ).map((participant) => ({
    ...participant,
    status: { ...participant.status },
    contextLoad: copyContextLoad(
      input.contextLoads.get(participant.executionKey) ?? null,
    ),
  }));
  const boundedChanges = {
    files: observedFiles,
    unreportedEditCount: input.observedChanges.unreportedEditCount,
  };
  return {
    goal: deriveGoal(input.goal, truncations),
    acceptedPlan: deriveAcceptedPlan(input.acceptedPlan, truncations),
    seatPlans,
    reports: reports.map((report) => ({
      sourceEventId: report.sourceEventId,
      authorLabel: report.authorLabel,
      summary: report.summary,
    })),
    tests: deriveTests(reports, truncations),
    changes: deriveChanges(boundedChanges),
    files: deriveFiles(
      boundedChanges,
      input.observedFileSources,
      reports,
      truncations,
    ),
    participants,
    contextFacts: cap(
      reports.flatMap(deriveContextFacts),
      MISSION_INSPECTOR_LIMITS.contextFacts,
      "context",
      "context facts",
      truncations,
    ),
    missionState: deriveMissionState(input.missionState, truncations),
    usage: hasUsage(input.usage) ? { ...input.usage } : null,
    integrity: {
      rejectedEventCount:
        input.rejectedEventCount === null
          ? null
          : Math.max(0, input.rejectedEventCount),
      rejectionsTruncated: input.rejectionsTruncated,
      rejectedReasons: deriveDisclosures(
        input.rejectedReasons,
        "rejected reasons",
        truncations,
      ),
      conflicts: deriveDisclosures(input.conflicts, "conflicts", truncations),
    },
    truncations,
  };
}

function deriveGoal(
  input: CodingSessionMissionGoalInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionGoalModel {
  if (input.kind !== "conflict") return { ...input };
  return {
    kind: "conflict",
    eventIds: cap(
      input.eventIds,
      MISSION_INSPECTOR_LIMITS.goalSourceEventIds,
      "goal",
      "conflicting goal sources",
      truncations,
    ),
  };
}

function deriveAcceptedPlan(
  input: CodingSessionAcceptedPlanInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionPlanModel {
  if (input.kind === "absent") {
    return {
      kind: "absent",
      label: "No accepted plan published",
      sourceEventIds: [],
      authorLabel: null,
      steps: [],
    };
  }
  if (input.kind === "conflict") {
    return {
      kind: "conflict",
      label: "Accepted plan unavailable — conflicting signed records",
      sourceEventIds: cap(
        input.eventIds,
        MISSION_INSPECTOR_LIMITS.disclosureEventIds,
        "accepted-plan",
        "conflicting accepted-plan sources",
        truncations,
      ),
      authorLabel: null,
      steps: [],
    };
  }
  const steps = cap(
    input.steps,
    MISSION_INSPECTOR_LIMITS.acceptedPlanSteps,
    "accepted-plan",
    "accepted-plan steps",
    truncations,
  ).map((text, index) => ({
    id: `${input.sourceEventId}:accepted:${index}`,
    text,
    status: null,
  }));
  return {
    kind: "available",
    label: steps.length > 0 ? "Accepted plan" : "Accepted plan has no steps",
    sourceEventIds: [input.sourceEventId],
    authorLabel: input.authorLabel,
    steps,
  };
}

function deriveSeatPlan(
  input: CodingSessionSeatPlanInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionSeatPlanModel {
  const model = input.model;
  const tasks = model
    ? cap(
        model.tasks,
        MISSION_INSPECTOR_LIMITS.seatPlanSteps,
        "seat-plans",
        "seat plan steps",
        truncations,
      )
    : [];
  return {
    executionKey: input.executionKey,
    ownerLabel: input.ownerLabel,
    sourceEventId: model?.sourceItemId ?? null,
    explanation: model?.explanation ?? null,
    state: model?.state ?? "absent",
    completedCount: model?.completedCount ?? 0,
    steps: tasks.map((task) => ({
      id: task.id,
      text: task.text,
      status: task.status,
    })),
  };
}

function deriveTests(
  reports: readonly CodingSessionMissionReportInput[],
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionTestModel[] {
  const tests: CodingSessionMissionTestModel[] = [];
  let totalAfterNestedCaps = 0;
  for (const report of reports) {
    const reportTests = cap(
      report.tests,
      MISSION_INSPECTOR_LIMITS.testsPerReport,
      "tests",
      "report tests",
      truncations,
    );
    totalAfterNestedCaps += reportTests.length;
    for (const [index, result] of reportTests.entries()) {
      if (tests.length >= MISSION_INSPECTOR_LIMITS.tests) continue;
      tests.push({
        ...result,
        id: `${report.sourceEventId}:test:${index}`,
        sourceEventId: report.sourceEventId,
        authorLabel: report.authorLabel,
      });
    }
  }
  if (tests.length < totalAfterNestedCaps) {
    addTruncation(
      "tests",
      tests.length,
      totalAfterNestedCaps,
      "tests after the section limit",
      truncations,
    );
  }
  return tests;
}

function deriveChanges(changes: CodingSessionObservedChanges) {
  const namedEditCount = changes.files.reduce(
    (total, file) => total + file.editCount,
    0,
  );
  const allCountsKnown = changes.files.every(
    (file) => file.additions !== null && file.deletions !== null,
  );
  return {
    state:
      namedEditCount === 0 && changes.unreportedEditCount === 0
        ? ("none" as const)
        : namedEditCount === 0
          ? ("unnamed" as const)
          : changes.unreportedEditCount > 0
            ? ("mixed" as const)
            : ("named" as const),
    namedEditCount,
    unreportedEditCount: changes.unreportedEditCount,
    additions:
      changes.files.length > 0 && allCountsKnown
        ? changes.files.reduce((sum, file) => sum + (file.additions ?? 0), 0)
        : null,
    deletions:
      changes.files.length > 0 && allCountsKnown
        ? changes.files.reduce((sum, file) => sum + (file.deletions ?? 0), 0)
        : null,
  };
}

function deriveFiles(
  changes: CodingSessionObservedChanges,
  observedSources: ReadonlyMap<string, readonly string[]> | undefined,
  reports: readonly CodingSessionMissionReportInput[],
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionFileModel[] {
  const files = new Map<string, CodingSessionMissionFileModel>();
  const attributionSources = new Map<string, Set<string>>();
  for (const file of changes.files) {
    const sourceInput = observedSources?.get(file.path);
    files.set(file.path, {
      path: file.path,
      observed: true,
      observedSourceEventIds: sourceInput
        ? cap(
            sourceInput,
            MISSION_INSPECTOR_LIMITS.fileSourceEventIds,
            "files",
            "observed file source events",
            truncations,
          )
        : [],
      observedSourceKnown: sourceInput !== undefined && sourceInput.length > 0,
      reportedBy: [],
      additions: file.additions,
      deletions: file.deletions,
      editCount: file.editCount,
    });
  }
  let omittedFiles = 0;
  for (const report of reports) {
    const reportFiles = cap(
      report.files,
      MISSION_INSPECTOR_LIMITS.filesPerReport,
      "files",
      "report files",
      truncations,
    );
    for (const path of reportFiles) {
      const existing = files.get(path);
      if (existing) {
        const seenSources = attributionSources.get(path) ?? new Set<string>();
        attributionSources.set(path, seenSources);
        if (!seenSources.has(report.sourceEventId)) {
          seenSources.add(report.sourceEventId);
          if (
            existing.reportedBy.length <
            MISSION_INSPECTOR_LIMITS.fileAttributions
          ) {
            existing.reportedBy.push({
              authorLabel: report.authorLabel,
              sourceEventId: report.sourceEventId,
            });
          }
        }
      } else if (files.size < MISSION_INSPECTOR_LIMITS.files) {
        attributionSources.set(path, new Set([report.sourceEventId]));
        files.set(path, {
          path,
          observed: false,
          observedSourceEventIds: [],
          observedSourceKnown: false,
          reportedBy: [
            {
              authorLabel: report.authorLabel,
              sourceEventId: report.sourceEventId,
            },
          ],
          additions: null,
          deletions: null,
          editCount: null,
        });
      } else {
        omittedFiles += 1;
      }
    }
  }
  for (const sources of attributionSources.values()) {
    if (sources.size > MISSION_INSPECTOR_LIMITS.fileAttributions) {
      addTruncation(
        "files",
        MISSION_INSPECTOR_LIMITS.fileAttributions,
        sources.size,
        "file report attributions",
        truncations,
      );
    }
  }
  if (omittedFiles > 0) {
    addTruncation(
      "files",
      files.size,
      files.size + omittedFiles,
      "files after the section limit",
      truncations,
    );
  }
  return [...files.values()];
}

function deriveContextFacts(
  report: CodingSessionMissionReportInput,
): CodingSessionMissionContextFact[] {
  const candidates: Array<
    [CodingSessionMissionContextFact["label"], string | null]
  > = [
    ["Assignment", report.assignmentRef],
    ["Branch", report.branch],
    ["Base", report.baseSha],
    ["Head", report.headSha],
  ];
  return candidates.flatMap(([label, value]) =>
    value
      ? [
          {
            id: `${report.sourceEventId}:${label}:${value}`,
            label,
            value,
            sourceEventId: report.sourceEventId,
            authorLabel: report.authorLabel,
          },
        ]
      : [],
  );
}

function deriveMissionState(
  input: CodingSessionMissionStateInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionStateInput {
  if (input.kind === "running" || input.kind === "acknowledgement-required") {
    return {
      ...input,
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "waiting-on-person" && input.canonicalChain) {
    return {
      ...input,
      canonicalChain: cap(
        input.canonicalChain,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "canonical team transaction steps",
        truncations,
      ).map((step) => ({ ...step })),
    };
  }
  if (input.kind === "blocked") {
    return {
      ...input,
      blockers: cap(
        input.blockers,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "mission blockers",
        truncations,
      ),
    };
  }
  if (input.kind === "completed") {
    return {
      ...input,
      landedShas: cap(
        input.landedShas,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "landed SHAs",
        truncations,
      ),
      followUps: cap(
        input.followUps,
        MISSION_INSPECTOR_LIMITS.missionStateItems,
        "mission-state",
        "mission follow-ups",
        truncations,
      ),
    };
  }
  if (input.kind === "conflict") {
    return {
      kind: "conflict",
      eventIds: cap(
        input.eventIds,
        MISSION_INSPECTOR_LIMITS.disclosureEventIds,
        "mission-state",
        "conflicting mission-state sources",
        truncations,
      ),
    };
  }
  return { ...input };
}

function deriveDisclosures(
  input: readonly CodingSessionMissionDisclosureInput[],
  label: string,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionDisclosureInput[] {
  return cap(
    input,
    MISSION_INSPECTOR_LIMITS.disclosures,
    "integrity",
    label,
    truncations,
  ).map((item) => ({
    code: item.code,
    summary: item.summary,
    eventIds: cap(
      item.eventIds,
      MISSION_INSPECTOR_LIMITS.disclosureEventIds,
      "integrity",
      "disclosure source events",
      truncations,
    ),
  }));
}

function cap<T>(
  input: readonly T[],
  limit: number,
  section: CodingSessionMissionInspectorSection,
  label: string,
  truncations: CodingSessionMissionTruncation[],
): T[] {
  const output = input.slice(0, limit);
  if (output.length < input.length) {
    addTruncation(section, output.length, input.length, label, truncations);
  }
  return output;
}

function addTruncation(
  section: CodingSessionMissionInspectorSection,
  shown: number,
  total: number,
  label: string,
  truncations: CodingSessionMissionTruncation[],
) {
  const omitted = Math.max(0, total - shown);
  if (omitted === 0) return;
  const existing = truncations.find(
    (item) => item.section === section && item.id === `${section}:${label}`,
  );
  if (existing) {
    existing.shown += shown;
    existing.total += total;
    existing.omitted += omitted;
    existing.notice = `Showing ${existing.shown} of ${existing.total} ${label}; ${existing.omitted} omitted.`;
    return;
  }
  truncations.push({
    id: `${section}:${label}`,
    section,
    shown,
    total,
    omitted,
    notice: `Showing ${shown} of ${total} ${label}; ${omitted} omitted.`,
  });
}

function copyContextLoad(
  value: CodingSessionContextLoad | null,
): CodingSessionContextLoad | null {
  return value ? { ...value } : null;
}

function hasUsage(
  usage: CodingSessionMissionUsageInput | null,
): usage is CodingSessionMissionUsageInput {
  return (
    usage !== null &&
    [
      usage.inputTokens,
      usage.outputTokens,
      usage.totalTokens,
      usage.toolCalls,
      usage.costUsd,
    ].some((value) => value !== null)
  );
}
