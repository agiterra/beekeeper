import { Bot, Terminal } from "lucide-react";

import { relativeTime } from "@/shared/lib/relative-time";
import {
  type CodingSessionExecution,
  type CodingSessionUmbrella,
  codingSessionFounderLabel,
  codingSessionReachabilityLine,
  codingSessionStatusChipLabel,
  generationExecutionLabel,
} from "../domain/index.ts";
import { CodingSessionConnectionLine } from "./CodingSessionConnectionLine.tsx";
import {
  CodingSessionAuthorityBadge,
  CodingSessionReadOnlyFooter,
  CodingSessionTrustNotes,
} from "./CodingSessionNotices.tsx";
import {
  CodingSessionClosedBadge,
  CodingSessionStatusChip,
} from "./CodingSessionStatusChip.tsx";
import { CodingSessionTitleOrigin } from "./CodingSessionTitleOrigin.tsx";
import { CodingSessionTranscript } from "./CodingSessionTranscript.tsx";
import {
  type CodingSessionObserverView,
  selectCodingSessionTranscriptBlocks,
  selectHeadlineExecution,
  selectReachability,
} from "./observer-contract.ts";

function ExecutionRow({
  execution,
  view,
}: {
  execution: CodingSessionExecution;
  view: CodingSessionObserverView;
}) {
  const generation = execution.activeGeneration;
  const report = selectReachability(view, generation.generationId);
  return (
    <li
      className="flex items-start gap-2 border-b border-black/10 px-3 py-2 last:border-b-0 dark:border-white/10"
      data-testid="coding-session-execution"
    >
      <Bot className="mt-0.5 h-4 w-4 shrink-0 text-black/50 dark:text-white/50" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-sm text-black dark:text-white">
            {generationExecutionLabel(generation)}
          </span>
          <CodingSessionStatusChip status={generation.status} />
          {execution.authoritySource === "disclosed-fallback" && (
            <CodingSessionAuthorityBadge />
          )}
        </div>
        <p className="mt-0.5 text-xs text-black/50 dark:text-white/50">
          {codingSessionReachabilityLine(
            report,
            codingSessionStatusChipLabel(generation.status),
          )}
        </p>
        <p className="mt-0.5 text-xs text-black/50 dark:text-white/50">
          Generation {generation.target.generation} ·{" "}
          {execution.priorGenerations.length > 0 &&
            `${execution.priorGenerations.length} earlier generation${
              execution.priorGenerations.length === 1 ? "" : "s"
            } · `}
          last activity{" "}
          {relativeTime(Math.floor(generation.lastEventAt / 1000))}
        </p>
      </div>
    </li>
  );
}

/**
 * Name records the resolver refused, disclosed rather than dropped: a title
 * from a provider with no execution in this session, or a 44229 from someone
 * other than the founder. Neither may name the session; both happened.
 */
function CodingSessionNameSetAside({
  umbrella,
}: {
  umbrella: CodingSessionUmbrella;
}) {
  const { foreignNames, foreignTitles } = umbrella.nameDiagnostics;
  if (foreignNames === 0 && foreignTitles === 0) return null;
  const parts = [
    foreignTitles > 0 &&
      `${foreignTitles} generated title${foreignTitles === 1 ? "" : "s"} from a provider outside this session`,
    foreignNames > 0 &&
      `${foreignNames} name${foreignNames === 1 ? "" : "s"} not signed by the founder`,
  ].filter((part): part is string => typeof part === "string");
  return (
    <p
      className="mt-1 text-xs text-black/50 dark:text-white/50"
      data-testid="coding-session-name-set-aside"
    >
      Ignored {parts.join(" and ")}.
    </p>
  );
}

/**
 * One session, read-only.
 *
 * Every claim on this screen traces to a signed fact: the status comes from
 * the D8 fold, the reachability line from a lease (or from the absence of
 * one), and the transcript from the projection. Nothing here is inferred to
 * make the page look complete.
 */
export function CodingSessionDetail({
  umbrella,
  view,
}: {
  umbrella: CodingSessionUmbrella;
  view: CodingSessionObserverView;
}) {
  const headline = selectHeadlineExecution(umbrella);
  const blocks = selectCodingSessionTranscriptBlocks(view, umbrella);
  const headlineReport =
    headline === null
      ? null
      : selectReachability(view, headline.activeGeneration.generationId);

  return (
    <div data-testid="coding-session-detail">
      <div data-testid="coding-session-detail-summary">
        <div
          className="flex flex-wrap items-center gap-3"
          data-testid="coding-session-detail-header"
        >
          <Terminal className="h-5 w-5 shrink-0 text-black/50 dark:text-white/50" />
          <h1
            className="text-xl font-semibold tracking-tight text-black dark:text-white"
            data-testid="coding-session-detail-name"
          >
            {umbrella.name}
          </h1>
          <CodingSessionTitleOrigin umbrella={umbrella} />
          <CodingSessionStatusChip status={umbrella.status} />
          {umbrella.closed && <CodingSessionClosedBadge />}
        </div>

        {headlineReport !== null && (
          <p
            className="mt-2 text-sm text-black/60 dark:text-white/60"
            data-testid="coding-session-reachability"
          >
            {codingSessionReachabilityLine(
              headlineReport,
              codingSessionStatusChipLabel(umbrella.status),
            )}
          </p>
        )}

        <p className="mt-1 text-xs text-black/50 dark:text-white/50">
          Founder {codingSessionFounderLabel(umbrella)}
          {umbrella.genesisRef !== null && " · governed by a genesis record"}
        </p>
        <CodingSessionNameSetAside umbrella={umbrella} />
        {umbrella.foreignAttachmentCount > 0 && (
          <p className="mt-1 text-xs text-amber-700 dark:text-amber-300">
            {umbrella.foreignAttachmentCount} execution
            {umbrella.foreignAttachmentCount === 1 ? "" : "s"} attached by
            someone other than the founder — shown, never merged.
          </p>
        )}
      </div>

      <div className="mt-4">
        <CodingSessionConnectionLine view={view} />
      </div>

      <h2 className="mt-6 text-sm font-semibold text-black dark:text-white">
        Executions
      </h2>
      <ul className="mt-2 overflow-hidden rounded-lg border border-black/10 bg-white/50 dark:border-white/10 dark:bg-white/5">
        {umbrella.executions.map((execution) => (
          <ExecutionRow
            key={execution.executionKey}
            execution={execution}
            view={view}
          />
        ))}
      </ul>

      <h2 className="mt-6 text-sm font-semibold text-black dark:text-white">
        Transcript
      </h2>
      <CodingSessionTranscript blocks={blocks} />

      <CodingSessionTrustNotes view={view} />
      <CodingSessionReadOnlyFooter />
    </div>
  );
}
