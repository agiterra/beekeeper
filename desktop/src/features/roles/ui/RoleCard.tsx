import { AlertTriangle } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { UserAvatar } from "@/shared/ui/UserAvatar";

import { roleAgentPackState } from "../lib/rolesViewModel";
import type { RoleAgentChip, RoleRow, SeatRow } from "../lib/rolesViewModel";
import type { RoleReportSummary } from "../lib/roleVersionSummary";
import { roleReportSentence } from "../lib/roleVersionSummary";
import {
  AGENT_CHIP_NO_PACK_TITLE,
  AGENT_CHIP_PACK_UNKNOWN_TITLE,
  AGENT_CHIP_SHARED_HOME_TITLE,
  AGENT_SHARED_BADGE,
  AGENT_SHARED_TITLE,
  agentStatusWord,
  DISPUTED_WORD,
  nonProjectAgentBadge,
  ROLE_AGENTS_EMPTY,
  ROLE_NON_PROJECT_AGENTS_LABEL,
  ROLE_QUIET,
  ROLE_REPORT_VERSION_LIMIT,
  ROLE_REPORTS_EMPTY,
  ROLE_SKILLS_EMPTY,
  ROLE_UNAVAILABLE_DETAILS_SUMMARY,
  roleAboutSummary,
  roleActivityLabel,
  roleDisputedReportCount,
  roleReportShortSentence,
  roleReportsMoreText,
  roleReportVersionLine,
  roleVersionChip,
  roleVersionsSummary,
  SHARED_SKILL_MARK,
} from "./rolesCopy";
import {
  agentStatusDotClass,
  roleActivity,
  roleActivityDotClass,
  seatStatusDotClass,
  StatusDot,
} from "./roleDots";
import { SeatRowButton } from "./SeatRowButton";

const DETAILS_SUMMARY_CLASS =
  "cursor-pointer text-xs text-muted-foreground marker:text-muted-foreground/60";

/**
 * An agent chip. Under `agents`, a project agent whose primary role is this
 * card's role; under the "Also in this project's sessions" line, an identity
 * that held a session here and is not this project's agent on this computer,
 * badged with why (`nonProjectAgentBadge`) so it never reads as the role's
 * agent.
 *
 * The chip's pack disclosure comes from the *role row*, not the agent's own
 * probe (Fix 2, `roleAgentPackState`): a role whose pack resolved shows its
 * agents as plain chips, even for an agent whose own `hasRolePack` was never
 * asked. A refused shared home is the one claim that is about this agent
 * specifically, so it outranks the role's own state. Every state but
 * "present" keeps the dashed outline, with the tooltip naming why.
 *
 * The run-state dot is a separate fact and is drawn only when something
 * reported one. An agent this view was never told the run state of gets no
 * dot and says so in the tooltip — a grey dot would read as "stopped", which
 * is a claim nobody made.
 *
 * An agent this computer does not manage (`isManagedHere === false`) is only
 * ever a sighting: it was seen in this project's sessions or channels. It
 * carries a "shared" badge, no status dot, and a pack state of "unknown" —
 * not "missing", which would be this computer answering a question about
 * another machine. "Available here" keeps meaning here.
 */
function AgentChip({
  agent,
  roleHasPack,
  roleRefusal,
}: {
  agent: RoleAgentChip;
  roleHasPack: boolean;
  roleRefusal: string | null;
}) {
  const isShared = agent.isManagedHere === false;
  const nonProject =
    agent.projectAgent && agent.projectAgent !== "project"
      ? nonProjectAgentBadge(agent.projectAgent)
      : null;
  const badge =
    nonProject ??
    (isShared ? { text: AGENT_SHARED_BADGE, title: AGENT_SHARED_TITLE } : null);
  const state = roleAgentPackState({
    roleHasPack,
    roleRefusal,
    agentPackRefusedSharedHome: agent.packRefusedSharedHome,
    isManagedHere: agent.isManagedHere,
  });
  const packTitle =
    state === "unknown"
      ? AGENT_CHIP_PACK_UNKNOWN_TITLE
      : state === "refused"
        ? AGENT_CHIP_SHARED_HOME_TITLE
        : state === "missing"
          ? AGENT_CHIP_NO_PACK_TITLE
          : state === "blocked"
            ? roleRefusal
            : null;
  const statusWord = agentStatusWord(agent.status);
  const dotClass = isShared ? null : agentStatusDotClass(agent.status);
  const title = isShared
    ? AGENT_CHIP_PACK_UNKNOWN_TITLE
    : packTitle === null
      ? statusWord
      : `${statusWord} · ${packTitle}`;
  return (
    <li
      className={cn(
        "flex min-w-0 items-center gap-1.5 rounded-full py-0.5 pl-0.5 pr-2 text-sm text-foreground",
        state !== "present" &&
          "border border-dashed border-border text-muted-foreground",
      )}
      data-agent-pack={state}
      data-agent-project={agent.projectAgent}
      data-agent-pubkey={agent.pubkey}
      data-agent-scope={isShared ? "shared" : "local"}
      data-testid="role-agent-chip"
      title={title}
    >
      <UserAvatar
        avatarUrl={agent.avatarUrl}
        displayName={agent.name}
        fallbackDelayMs={0}
        size="sm"
      />
      <span className="truncate">{agent.name}</span>
      {badge ? (
        <span
          className="min-w-0 truncate rounded-full border border-border/70 px-1.5 text-2xs text-muted-foreground"
          data-testid="role-agent-chip-badge"
          title={badge.title}
        >
          {badge.text}
        </span>
      ) : null}
      {dotClass === null ? null : (
        <StatusDot className={dotClass} title={statusWord} />
      )}
    </li>
  );
}

/**
 * What running agents said they were on, for this role only.
 *
 * The face carries one line — the counts, in the fewest words that stay true,
 * with the long sentence in its `title` — and the per-version detail lives in
 * a disclosure beside it. At most five version lines are drawn; the rest are
 * counted and left to Report history, so a busy project cannot grow this card
 * without bound. A contradiction is the one thing that is never folded into
 * the count: a disputed report leads the line, in words and in colour.
 */
function RoleReports({
  slug,
  summary,
}: {
  slug: string;
  summary: RoleReportSummary;
}) {
  const versions = summary.versions;
  const shown = versions.slice(0, ROLE_REPORT_VERSION_LIMIT);
  const hidden = versions.length - shown.length;
  const disputed = roleDisputedReportCount(summary);
  return (
    <>
      <p
        className="flex flex-wrap items-center gap-x-1.5 text-xs text-muted-foreground"
        title={roleReportSentence(summary)}
      >
        {disputed > 0 ? (
          <span className="flex items-center gap-1 text-destructive">
            <AlertTriangle aria-hidden className="size-3" />
            {DISPUTED_WORD}
          </span>
        ) : null}
        <span>{roleReportShortSentence(summary)}</span>
      </p>
      <details data-testid={`role-versions-${slug}`}>
        <summary className={DETAILS_SUMMARY_CLASS}>
          {roleVersionsSummary(versions.length)}
        </summary>
        <ul className="mt-1 flex flex-col gap-0.5">
          {shown.map((version) => {
            const line = roleReportVersionLine({
              sha: version.sha,
              relation: version.relation,
              behind: version.behind,
              ahead: version.ahead,
              count: version.count,
              latestAgeSeconds: version.latestAgeSeconds,
              provenance: version.provenance,
            });
            return (
              <li
                className="text-xs text-muted-foreground"
                data-relation={version.relation}
                data-testid="role-report-version"
                key={`${version.sha ?? "no-version"}:${version.relation}`}
                title={line.title}
              >
                {line.text}
              </li>
            );
          })}
          {hidden > 0 ? (
            <li
              className="text-xs text-muted-foreground"
              data-testid={`role-reports-${slug}-more`}
            >
              {roleReportsMoreText(hidden)}
            </li>
          ) : null}
        </ul>
      </details>
    </>
  );
}

/** The card's foot: the long description and the skills, behind one click. */
function RoleAbout({ role }: { role: RoleRow }) {
  const slug = role.role;
  return (
    <details className="mt-auto" data-testid={`role-about-${slug}`}>
      <summary className={DETAILS_SUMMARY_CLASS}>
        {roleAboutSummary(role.skills.length)}
      </summary>
      <div className="mt-1 flex flex-col gap-1">
        {role.summary ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid={`role-summary-${slug}`}
          >
            {role.summary}
          </p>
        ) : null}
        {role.skills.length === 0 ? (
          <p
            className="text-xs text-muted-foreground"
            data-testid={`role-skills-${slug}-empty`}
          >
            {ROLE_SKILLS_EMPTY}
          </p>
        ) : (
          <ul
            className="flex flex-col gap-0.5"
            data-testid={`role-skills-${slug}`}
          >
            {role.skills.map((skill) => (
              <li
                className="text-xs"
                data-testid="role-skill"
                key={`${skill.name}:${skill.shared ? "shared" : "own"}`}
              >
                <span className="font-medium text-foreground">
                  {skill.name}
                </span>
                {skill.description ? (
                  <span className="text-muted-foreground">
                    {" — "}
                    {skill.description}
                  </span>
                ) : null}
                {skill.shared ? (
                  <span className="text-muted-foreground">
                    {" "}
                    {SHARED_SKILL_MARK}
                  </span>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </div>
    </details>
  );
}

/**
 * One role, read top to bottom: whether anything is running it, what it is
 * called, which version of its instructions is here, what it is for, who can
 * take it, and what running agents reported.
 *
 * The version facts are the card's own (`packRef`, `version`, `origin`,
 * `refusal`) and the report facts are the ones the reports themselves named
 * for this role — neither side fills in for the other, and a role with no
 * instructions here says so in the same place the version would have been.
 *
 * A role nothing is using says that once, in one line, instead of three
 * separate absences. Everything long — the persona summary, the skills, the
 * per-version report lines, the reason a role is unavailable — is behind a
 * disclosure, so the card's height is set by what is actually happening.
 */
export function RoleCard({
  role,
  reports = null,
  onOpenSeat,
}: {
  role: RoleRow;
  reports?: RoleReportSummary | null;
  onOpenSeat?: (seat: SeatRow) => void;
}) {
  const slug = role.role;
  const version = roleVersionChip({
    hasPack: role.hasPack,
    version: role.version,
    origin: role.origin,
    packRef: role.packRef,
    refusal: role.refusal,
  });
  const activity = roleActivity(role.seats);
  const hasReports = reports !== null && reports.total > 0;
  const nonProjectAgents = role.nonProjectAgents ?? [];
  const quiet =
    role.agents.length === 0 &&
    nonProjectAgents.length === 0 &&
    role.seats.length === 0 &&
    !hasReports;
  return (
    <article
      className="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3"
      data-role-has-pack={role.hasPack ? "true" : "false"}
      data-testid={`role-card-${slug}`}
    >
      <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <StatusDot
          className={roleActivityDotClass(activity)}
          data-activity={activity}
          label={roleActivityLabel(activity)}
          testId={`role-activity-${slug}`}
          title={roleActivityLabel(activity)}
        />
        {/* Name and slug travel together, so when the version chip wraps to
            its own line the slug stays beside the name instead of drifting
            to the far edge. */}
        <span className="flex min-w-0 flex-1 basis-40 items-baseline gap-2">
          <h3
            className="min-w-0 truncate text-sm font-semibold text-foreground"
            title={role.displayName}
          >
            {role.displayName}
          </h3>
          <span
            className="min-w-0 truncate font-mono text-xs text-muted-foreground"
            data-testid={`role-slug-${slug}`}
          >
            {slug}
          </span>
        </span>
        <span
          className={cn(
            "ml-auto shrink-0 rounded-md border px-1.5 py-0.5 font-mono text-2xs",
            version.availability === "available"
              ? "border-border/70 bg-muted/40 text-muted-foreground"
              : "border-amber-500/30 text-amber-600 dark:text-amber-400",
          )}
          data-availability={version.availability}
          data-testid={`role-available-${slug}`}
          title={version.title}
        >
          {version.text}
        </span>
      </header>
      {role.description ? (
        <p
          className="line-clamp-2 text-sm text-muted-foreground"
          data-testid={`role-description-${slug}`}
          title={role.description}
        >
          {role.description}
        </p>
      ) : null}
      {version.availability === "unavailable" ? (
        <details data-testid={`role-unavailable-${slug}`}>
          <summary className={DETAILS_SUMMARY_CLASS}>
            {ROLE_UNAVAILABLE_DETAILS_SUMMARY}
          </summary>
          <p className="mt-1 text-xs text-muted-foreground">
            {version.sentence}
          </p>
        </details>
      ) : null}
      {quiet ? (
        <div
          data-reports="none"
          data-testid={`role-reports-${slug}`}
          className="flex flex-col gap-1"
        >
          <p
            className="text-xs text-muted-foreground"
            data-testid={`role-quiet-${slug}`}
          >
            {ROLE_QUIET}
          </p>
        </div>
      ) : (
        <>
          <ul
            className="flex flex-wrap items-center gap-x-3 gap-y-1"
            data-testid={`role-agents-${slug}`}
          >
            {role.agents.length === 0 ? (
              <li
                className="text-xs text-muted-foreground"
                data-testid={`role-agents-${slug}-empty`}
              >
                {ROLE_AGENTS_EMPTY}
              </li>
            ) : (
              role.agents.map((agent) => (
                <AgentChip
                  agent={agent}
                  key={agent.pubkey}
                  roleHasPack={role.hasPack}
                  roleRefusal={role.refusal}
                />
              ))
            )}
          </ul>
          {nonProjectAgents.length > 0 ? (
            <div
              className="flex min-w-0 flex-col gap-1"
              data-testid={`role-non-project-agents-${slug}`}
            >
              <p className="text-xs text-muted-foreground">
                {ROLE_NON_PROJECT_AGENTS_LABEL}
              </p>
              <ul className="flex flex-wrap items-center gap-x-3 gap-y-1">
                {nonProjectAgents.map((agent) => (
                  <AgentChip
                    agent={agent}
                    key={agent.pubkey}
                    roleHasPack={role.hasPack}
                    roleRefusal={role.refusal}
                  />
                ))}
              </ul>
            </div>
          ) : null}
          <div
            className="flex flex-col gap-1"
            data-reports={hasReports ? "some" : "none"}
            data-testid={`role-reports-${slug}`}
          >
            {reports !== null && hasReports ? (
              <RoleReports slug={slug} summary={reports} />
            ) : (
              <p
                className="text-xs text-muted-foreground"
                data-testid={`role-reports-${slug}-empty`}
              >
                {ROLE_REPORTS_EMPTY}
              </p>
            )}
          </div>
          {role.seats.length > 0 ? (
            <ul
              className="flex flex-col gap-0.5"
              data-testid={`role-seats-${slug}`}
            >
              {role.seats.map((seat) => (
                <li
                  className="flex min-w-0 items-center gap-1.5"
                  key={seat.key}
                >
                  <StatusDot
                    className={seatStatusDotClass(seat.status)}
                    title={seat.status}
                  />
                  <SeatRowButton
                    columns="role-card"
                    onOpen={onOpenSeat}
                    seat={seat}
                  />
                </li>
              ))}
            </ul>
          ) : null}
        </>
      )}
      <RoleAbout role={role} />
    </article>
  );
}
