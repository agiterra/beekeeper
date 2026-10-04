import { ChevronDown, Lock, LockOpen, ShieldQuestion } from "lucide-react";

import {
  codingSessionSandboxChip,
  type CodingSessionLocalSandboxGrant,
  type CodingSessionSandboxPeriod,
  type CodingSessionSandboxReport,
  type CodingSessionSandboxState,
} from "@/features/coding-sessions/lib/codingSessionBoundaryStatus";
import {
  CODING_SESSION_FULL_ACCESS_LABEL,
  codingSessionFullAccessDetail,
} from "@/features/coding-sessions/lib/codingSessionFullAccess";
import { cn } from "@/shared/lib/cn";
import { formatItemTimestamp } from "@/shared/lib/datetime";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

/**
 * The sandbox facts the composer chip reads (SV-17, decision D3).
 *
 * `report` is the running generation's boundary disclosure, read from the
 * transcript every viewer has, so a teammate on another machine sees the same
 * chip. `local` is this computer's own full-access grant — present only when
 * the host answered for this session — and carries the control that changes
 * it.
 */
export type CodingSessionComposerSandbox = {
  report: CodingSessionSandboxReport;
  local: (CodingSessionLocalSandboxGrant & { toggle?: () => void }) | null;
};

/**
 * "Sandboxed ▾" / "Full access ▾": the session's boundary as one composer
 * chip, with the host's own words one click away.
 *
 * Full access and an unenforced boundary wear the warning colour on the chip
 * itself, never only inside the dropdown: moving the boundary row out of the
 * transcript must not move the warning out of sight.
 */
export function CodingSessionComposerSandboxChip({
  sandbox,
  readOnlyNote,
}: {
  sandbox: CodingSessionComposerSandbox;
  /**
   * Shown in place of the full-access control when the chip is a record
   * rather than a control (a closed session, or one with no command target).
   * Setting it drops the toggle even when this computer could change it.
   */
  readOnlyNote?: string;
}) {
  const chip = codingSessionSandboxChip(sandbox.report, sandbox.local);
  const Icon =
    chip.tone === "safe"
      ? Lock
      : chip.tone === "warning"
        ? LockOpen
        : ShieldQuestion;
  const local = sandbox.local;
  const toggle = readOnlyNote === undefined ? local?.toggle : undefined;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          aria-label={`Sandbox: ${chip.label}. Show details`}
          className={cn(
            "inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full border px-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            chip.tone === "warning"
              ? "border-amber-500/50 bg-amber-500/10 text-amber-700 hover:bg-amber-500/20 dark:text-amber-300"
              : "border-border/60 bg-background/40 hover:bg-muted/50 hover:text-foreground",
          )}
          data-testid="coding-session-control-sandbox"
          data-tone={chip.tone}
          type="button"
        >
          <Icon aria-hidden className="size-3 shrink-0" />
          <span className="whitespace-nowrap">{chip.label}</span>
          <ChevronDown aria-hidden className="size-3 shrink-0 opacity-60" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80" side="top">
        <p className="text-sm font-medium">{chip.label}</p>
        <p
          className="mt-2 text-xs leading-relaxed text-muted-foreground"
          data-testid="coding-session-sandbox-summary"
        >
          {chip.summary}
        </p>
        {sandbox.report.boundaryText ? (
          <div className="mt-3 text-xs">
            <p className="text-muted-foreground">Project boundary</p>
            <p
              className="mt-0.5 leading-relaxed wrap-break-word"
              data-testid="coding-session-sandbox-boundary"
            >
              {sandbox.report.boundaryText}
            </p>
          </div>
        ) : null}
        {sandbox.report.isolation.length > 0 ? (
          <div className="mt-3 text-xs">
            <p className="text-muted-foreground">Session isolation</p>
            <ul className="mt-0.5 list-disc space-y-0.5 pl-4">
              {sandbox.report.isolation.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
          </div>
        ) : null}
        <CodingSessionSandboxHistory earlier={sandbox.report.earlier ?? []} />
        {local && toggle ? (
          <button
            aria-pressed={local.granted}
            className="-mx-1 mt-3 flex w-[calc(100%+0.5rem)] items-start gap-2 rounded-lg px-2 py-1.5 text-left text-sm transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60"
            data-testid="coding-session-sandbox-full-access-toggle"
            disabled={local.pending !== null}
            onClick={toggle}
            type="button"
          >
            <LockOpen aria-hidden className="mt-0.5 size-3.5 shrink-0" />
            <span className="min-w-0 flex-1">
              <span className="block font-medium">
                {CODING_SESSION_FULL_ACCESS_LABEL}
              </span>
              <span className="block text-xs text-muted-foreground">
                {codingSessionFullAccessDetail(local)}
              </span>
            </span>
          </button>
        ) : (
          <p
            className="mt-3 text-xs text-muted-foreground"
            data-testid="coding-session-sandbox-control-note"
          >
            {readOnlyNote ??
              "Full access is granted on the computer running this agent, from that computer."}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}

const PERIOD_STATE_LABELS: Record<CodingSessionSandboxState, string> = {
  sandboxed: "Sandboxed",
  "full-access": "Full access",
  "not-sandboxed": "Not sandboxed",
  unreported: "Unrecognised boundary report",
};

function periodTime(timestamp: string): string | null {
  const millis = Date.parse(timestamp);
  if (!Number.isFinite(millis)) return null;
  return formatItemTimestamp(millis / 1_000, { withTime: true });
}

/**
 * Every earlier boundary disclosure in this session, newest first (SV-17).
 *
 * Boundary rows left the transcript for the chip, so this list is their only
 * home: a period that ran with full access, or with no enforced boundary,
 * stays readable here — in the warning colour — after the agent restarts
 * sandboxed. Scrolled, never truncated.
 */
export function CodingSessionSandboxHistory({
  earlier,
}: {
  earlier: readonly CodingSessionSandboxPeriod[];
}) {
  if (earlier.length === 0) return null;
  return (
    <div className="mt-3 text-xs" data-testid="coding-session-sandbox-history">
      <p className="text-muted-foreground">Earlier in this session</p>
      <ul className="mt-0.5 max-h-40 space-y-1.5 overflow-y-auto">
        {earlier.map((period) => {
          const time = periodTime(period.timestamp);
          const unsandboxed =
            period.state === "full-access" || period.state === "not-sandboxed";
          return (
            <li
              className={cn(
                "leading-relaxed wrap-break-word",
                unsandboxed && "text-amber-700 dark:text-amber-300",
              )}
              data-state={period.state}
              data-testid="coding-session-sandbox-history-row"
              key={period.id}
            >
              <span className="font-medium">
                {PERIOD_STATE_LABELS[period.state]}
              </span>
              {time ? (
                <span className="tabular-nums text-muted-foreground">
                  {` · ${time}`}
                </span>
              ) : null}
              <span className="block">{period.boundaryText}</span>
              {period.isolation.map((line) => (
                <span className="block text-muted-foreground" key={line}>
                  {line}
                </span>
              ))}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
