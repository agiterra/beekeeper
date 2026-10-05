import {
  Check,
  ChevronDown,
  Lock,
  LockOpen,
  ShieldQuestion,
} from "lucide-react";

import {
  codingSessionSandboxChip,
  type CodingSessionLocalSandboxGrant,
  type CodingSessionSandboxPeriod,
  type CodingSessionSandboxReport,
  type CodingSessionSandboxState,
} from "@/features/coding-sessions/lib/codingSessionBoundaryStatus";
import {
  CODING_SESSION_FULL_ACCESS_BADGE,
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
          // T3's composer control: borderless, a hover fill, label and caret
          // (ChatComposer's access picker). The warning keeps its colour and
          // a tint on the chip itself, so full access is never only in the
          // menu.
          className={cn(
            "inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md px-2 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
            chip.tone === "warning"
              ? "bg-amber-500/10 font-medium text-amber-700 hover:bg-amber-500/20 dark:text-amber-300"
              : "hover:bg-muted/60 hover:text-foreground",
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
      <PopoverContent align="start" className="w-80 p-1" side="top">
        <CodingSessionSandboxModeRows
          granted={local?.granted ?? null}
          pending={local?.pending ?? null}
          state={sandbox.report.state}
          toggle={toggle}
        />
        <div className="mx-1 my-1 border-t border-border/60" />
        <div className="px-2 pt-1 pb-2">
          <p className="text-sm font-medium">{chip.label}</p>
          <p
            className="mt-1 text-xs leading-relaxed text-muted-foreground"
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
            <p
              className={cn(
                "mt-3 text-xs",
                local.error ? "text-destructive" : "text-muted-foreground",
              )}
              data-testid="coding-session-sandbox-control-detail"
            >
              {codingSessionFullAccessDetail(local)}
            </p>
          ) : (
            <p
              className="mt-3 text-xs text-muted-foreground"
              data-testid="coding-session-sandbox-control-note"
            >
              {readOnlyNote ??
                "Full access is granted on the computer running this agent, from that computer."}
            </p>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** The two modes a person can choose, as T3's access picker lists its own. */
const SANDBOX_MODES = [
  {
    mode: "sandboxed",
    label: "Sandboxed",
    description: "Commands and edits stay inside this project's boundary.",
    Icon: Lock,
  },
  {
    mode: "full-access",
    label: CODING_SESSION_FULL_ACCESS_BADGE,
    description:
      "Commands and edits can reach anything the account running the agent can.",
    Icon: LockOpen,
  },
] as const;

/**
 * The menu half of the dropdown (SV-17; ref `sv17-t3-access-picker-open`):
 * one row per mode, an icon and its name over a one-line description, and a
 * check on the mode in force — the boundary the host reports for the running
 * agent (`state`), never this computer's grant. A boundary the host did not
 * enforce, or never reported, is neither mode, so it gets its own checked row
 * rather than a check on a mode that is not true.
 *
 * A grant the running agent has not restarted under is a different fact: the
 * row it chose says "applies when the agent next starts", with no check, so
 * the check never claims a boundary the agent is not running under.
 *
 * Only this computer, when it runs the agent, can change the mode: then the
 * row of the mode its grant would flip to is the control (`toggle`), and a
 * change in flight disables it. Anywhere else the rows are a statement, not
 * buttons.
 */
export function CodingSessionSandboxModeRows({
  granted,
  pending,
  state,
  toggle,
}: {
  /** This computer's grant, or null when it does not run the agent. */
  granted: boolean | null;
  pending: boolean | null;
  state: CodingSessionSandboxState;
  toggle?: () => void;
}) {
  const current =
    state === "full-access" || state === "sandboxed" ? state : null;
  // The mode this computer's grant chose but the running agent is not under
  // yet. Revoking a grant under a boundary the host never enforced does not
  // promise a sandbox at the next start, so only a reported mode can be
  // superseded by one.
  const nextStart =
    granted === true && state !== "full-access"
      ? "full-access"
      : granted === false && state === "full-access"
        ? "sandboxed"
        : null;
  return (
    <ul aria-label="Sandbox mode" data-testid="coding-session-sandbox-modes">
      {current === null ? (
        <SandboxModeRow
          checked
          description={
            state === "not-sandboxed"
              ? "The host did not enforce a boundary for this agent."
              : "This agent has not reported a boundary."
          }
          Icon={ShieldQuestion}
          label={PERIOD_STATE_LABELS[state]}
          mode={state}
        />
      ) : null}
      {SANDBOX_MODES.map((entry) => {
        const checked = entry.mode === current;
        // The toggle flips this computer's grant, so only the mode it would
        // flip *to* is a control — never the one already granted.
        const control =
          toggle !== undefined &&
          granted !== null &&
          granted !== (entry.mode === "full-access");
        return (
          <SandboxModeRow
            checked={checked}
            description={entry.description}
            disabled={pending !== null}
            Icon={entry.Icon}
            key={entry.mode}
            label={entry.label}
            mode={entry.mode}
            nextStart={entry.mode === nextStart}
            onSelect={control ? toggle : undefined}
          />
        );
      })}
    </ul>
  );
}

function SandboxModeRow({
  checked,
  description,
  disabled = false,
  Icon,
  label,
  mode,
  nextStart = false,
  onSelect,
}: {
  checked: boolean;
  description: string;
  disabled?: boolean;
  Icon: typeof Lock;
  label: string;
  mode: string;
  /** Granted on this computer; the running agent is not under it yet. */
  nextStart?: boolean;
  onSelect?: () => void;
}) {
  const body = (
    <>
      <span className="grid min-w-0 flex-1 gap-0.5">
        <span className="inline-flex items-center gap-1.5 font-medium text-foreground">
          <Icon
            aria-hidden
            className="size-3.5 shrink-0 text-muted-foreground"
          />
          {label}
        </span>
        <span className="text-xs leading-4 text-muted-foreground">
          {description}
        </span>
        {nextStart ? (
          <span
            className="text-xs leading-4 font-medium text-amber-700 dark:text-amber-300"
            data-testid="coding-session-sandbox-mode-next-start"
          >
            {mode === "full-access"
              ? "Granted · applies when the agent next starts"
              : "Full access revoked · applies when the agent next starts"}
          </span>
        ) : null}
      </span>
      {checked ? (
        <Check
          aria-hidden
          className="mt-0.5 size-3.5 shrink-0 text-foreground"
        />
      ) : null}
    </>
  );
  const rowClass =
    "flex w-full items-start gap-3 rounded-md px-2 py-1.5 text-left text-sm";
  return (
    <li
      aria-current={checked ? "true" : undefined}
      data-mode={mode}
      data-next-start={nextStart ? "true" : undefined}
      data-testid="coding-session-sandbox-mode"
    >
      {onSelect ? (
        <button
          className={cn(
            rowClass,
            "transition-colors hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
          )}
          data-testid="coding-session-sandbox-full-access-toggle"
          disabled={disabled}
          onClick={onSelect}
          type="button"
        >
          {body}
        </button>
      ) : (
        <div className={cn(rowClass, checked && "bg-muted/60")}>{body}</div>
      )}
    </li>
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
