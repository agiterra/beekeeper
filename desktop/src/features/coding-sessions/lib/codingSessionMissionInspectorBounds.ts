/**
 * Bounding primitives and the file/context-fact half of the Mission Inspector
 * model.
 *
 * Split out of `codingSessionMissionInspectorModel.ts` when that file reached
 * the repository's 1,000-line ceiling. Nothing here decides truth: `cap` and
 * `addTruncation` exist so that every omission the Inspector makes is a
 * *visible* number rather than a silently shorter list, and the two file
 * derivations only join already-verified observations to already-verified
 * report attributions.
 */
import type { CodingSessionObservedChanges } from "./codingSessionTranscriptModel";
import type {
  CodingSessionMissionAssignmentInput,
  CodingSessionMissionReportInput,
} from "./codingSessionMissionInspectorModel";

export const MISSION_INSPECTOR_LIMITS = {
  assignments: 100,
  acceptedPlanSteps: 100,
  contextFacts: 400,
  disclosureEventIds: 20,
  disclosures: 100,
  fileAttributions: 20,
  fileSourceEventIds: 20,
  files: 500,
  filesPerReport: 200,
  ownershipFilesPerAssignment: 100,
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
  label:
    | "Assignment"
    | "Assignment role"
    | "Objective"
    | "Brief"
    | "Ownership"
    | "Branch"
    | "Base"
    | "Head";
  value: string;
  sourceEventId: string;
  authorLabel: string;
};

/**
 * Join observed edits to report-claimed files into one bounded file list.
 *
 * `observed` and `reportedBy` stay separate fields rather than being merged:
 * a path Beekeeper watched change and a path a seat *said* it changed are
 * different claims, and a file with only the second must not read as verified.
 * Every cap taken here records a truncation, so a short list is never mistaken
 * for a complete one.
 */
export function deriveFiles(
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

/**
 * The signed provenance fields of one report, as individually-attributed facts.
 *
 * Only fields the report actually carries become facts — an absent branch or
 * base SHA yields no row rather than an empty one, because "unknown" and
 * "empty" are different answers.
 */
export function deriveContextFacts(
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

/**
 * The signed fields of one assignment, as individually-attributed facts.
 *
 * File ownership is capped and the omission recorded, so a long ownership list
 * is visibly truncated rather than silently shortened.
 */
export function deriveAssignmentContextFacts(
  assignment: CodingSessionMissionAssignmentInput,
  truncations: CodingSessionMissionTruncation[],
): CodingSessionMissionContextFact[] {
  const facts: CodingSessionMissionContextFact[] = [
    {
      id: `${assignment.sourceEventId}:Assignment role`,
      label: "Assignment role",
      value: assignment.assigneeRole,
      sourceEventId: assignment.sourceEventId,
      authorLabel: assignment.authorLabel,
    },
    {
      id: `${assignment.sourceEventId}:Objective`,
      label: "Objective",
      value: assignment.objective,
      sourceEventId: assignment.sourceEventId,
      authorLabel: assignment.authorLabel,
    },
    {
      id: `${assignment.sourceEventId}:Brief`,
      label: "Brief",
      value: assignment.brief,
      sourceEventId: assignment.sourceEventId,
      authorLabel: assignment.authorLabel,
    },
  ];
  const ownership = cap(
    assignment.fileOwnership,
    MISSION_INSPECTOR_LIMITS.ownershipFilesPerAssignment,
    "context",
    "assignment ownership paths",
    truncations,
  );
  for (const [index, path] of ownership.entries()) {
    facts.push({
      id: `${assignment.sourceEventId}:Ownership:${index}`,
      label: "Ownership",
      value: path,
      sourceEventId: assignment.sourceEventId,
      authorLabel: assignment.authorLabel,
    });
  }
  return facts;
}

/**
 * Take at most `limit` items and record what was left out.
 *
 * The recording is the point. A bare `slice` would render a shorter list that
 * looks complete, which is the failure mode this whole module exists to
 * prevent.
 */
export function cap<T>(
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

/**
 * Record, or extend, one section's "showing N of M" notice.
 *
 * Repeated calls for the same section and label accumulate rather than
 * overwrite, so a bound hit once per report still reports one honest total
 * across all of them.
 */
export function addTruncation(
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
