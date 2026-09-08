import type * as React from "react";

import { cn } from "@/shared/lib/cn";

import { roleAgentPackState } from "../lib/rolesViewModel";
import type { RoleAgentChip, RoleRow, SeatRow } from "../lib/rolesViewModel";
import type { RoleReportSummary } from "../lib/roleVersionSummary";
import { roleReportSentence } from "../lib/roleVersionSummary";
import {
  AGENT_CHIP_NO_PACK_TITLE,
  AGENT_CHIP_SHARED_HOME_TITLE,
  ROLE_AGENTS_EMPTY,
  ROLE_AGENTS_TITLE,
  ROLE_REPORT_VERSION_LIMIT,
  ROLE_REPORTS_EMPTY,
  ROLE_REPORTS_TITLE,
  ROLE_SEATS_EMPTY,
  ROLE_SEATS_TITLE,
  ROLE_SKILLS_EMPTY,
  roleAvailabilitySentence,
  roleReportsMoreText,
  roleReportVersionLine,
  roleSkillsSummary,
  SHARED_SKILL_MARK,
} from "./rolesCopy";
import { SeatRowButton } from "./SeatRowButton";

function CardSection({
  children,
  empty,
  isEmpty,
  testId,
  title,
}: {
  children: React.ReactNode;
  empty: string;
  isEmpty: boolean;
  testId: string;
  title: string;
}) {
  return (
    <div className="flex flex-col gap-1" data-testid={testId}>
      <h4 className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
        {title}
      </h4>
      {isEmpty ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid={`${testId}-empty`}
        >
          {empty}
        </p>
      ) : (
        children
      )}
    </div>
  );
}

/**
 * A managed agent whose home role is this card's role.
 *
 * The chip's disclosure comes from the *role row*, not the agent's own probe
 * (Fix 2, `roleAgentPackState`): a role whose pack resolved shows its agents
 * as plain chips, even for an agent whose own `hasRolePack` was never asked.
 * A refused shared home is the one claim that is about this agent
 * specifically, so it outranks the role's own state. Every state but
 * "present" is muted and dashed, with the tooltip naming why.
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
  const state = roleAgentPackState({
    roleHasPack,
    roleRefusal,
    agentPackRefusedSharedHome: agent.packRefusedSharedHome,
    isManagedHere: agent.isManagedHere,
  });
  const title =
    state === "unknown"
      ? "Shared agent · role availability on its computer is not reported here"
      : state === "refused"
        ? AGENT_CHIP_SHARED_HOME_TITLE
        : state === "missing"
          ? AGENT_CHIP_NO_PACK_TITLE
          : state === "blocked"
            ? (roleRefusal ?? undefined)
            : undefined;
  return (
    <li
      className={cn(
        "rounded-full border border-border px-2 py-0.5 text-xs text-foreground",
        state !== "present" && "border-dashed text-muted-foreground",
      )}
      data-agent-pack={state}
      data-agent-pubkey={agent.pubkey}
      data-testid="role-agent-chip"
      title={title}
    >
      {agent.name}
    </li>
  );
}

/**
 * What running agents said they were on, for this role only.
 *
 * Three cases stay distinct and none is inferred from the other: no report
 * at all, a report that named a version, and a report that named this role
 * without a version (`sha: null`, which the line words as "version not
 * reported"). At most five version lines are drawn — the rest are counted
 * and left to Report history, so a busy project cannot grow this card
 * without bound.
 */
function RoleReports({
  slug,
  summary,
}: {
  slug: string;
  summary: RoleReportSummary | null;
}) {
  const versions = summary?.versions ?? [];
  const shown = versions.slice(0, ROLE_REPORT_VERSION_LIMIT);
  const hidden = versions.length - shown.length;
  return (
    <div
      className="flex flex-col gap-1"
      data-reports={summary === null || summary.total === 0 ? "none" : "some"}
      data-testid={`role-reports-${slug}`}
    >
      <h4 className="text-2xs font-medium uppercase tracking-wide text-muted-foreground">
        {ROLE_REPORTS_TITLE}
      </h4>
      {summary === null || summary.total === 0 ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid={`role-reports-${slug}-empty`}
        >
          {ROLE_REPORTS_EMPTY}
        </p>
      ) : (
        <>
          <p className="text-xs text-foreground">
            {roleReportSentence(summary)}
          </p>
          <ul className="flex flex-col gap-0.5">
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
          </ul>
          {hidden > 0 ? (
            <p
              className="text-2xs text-muted-foreground"
              data-testid={`role-reports-${slug}-more`}
            >
              {roleReportsMoreText(hidden)}
            </p>
          ) : null}
        </>
      )}
    </div>
  );
}

/**
 * One role: what it is for, which agents can take it, which version of its
 * instructions is on this computer, and what running agents reported.
 *
 * The version facts are the card's own (`packRef`, `version`, `origin`,
 * `refusal`) and the report facts are the ones the reports themselves named
 * for this role — neither side fills in for the other, and a role with no
 * instructions here says so in the same place the version would have been.
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
  const available = roleAvailabilitySentence({
    hasPack: role.hasPack,
    version: role.version,
    origin: role.origin,
    packRef: role.packRef,
    refusal: role.refusal,
  });
  return (
    <article
      className="flex min-w-0 flex-col gap-2 rounded-lg border border-border bg-card p-3"
      data-role-has-pack={role.hasPack ? "true" : "false"}
      data-testid={`role-card-${slug}`}
    >
      <header className="flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-medium text-foreground">
          {role.displayName}
        </h3>
        <span
          className="text-2xs text-muted-foreground"
          data-testid={`role-slug-${slug}`}
        >
          {slug}
        </span>
      </header>
      {role.description ? (
        <p
          className="line-clamp-2 text-sm text-foreground"
          data-testid={`role-description-${slug}`}
          title={role.description}
        >
          {role.description}
        </p>
      ) : null}
      {role.summary ? (
        <p
          className="line-clamp-3 text-sm text-muted-foreground"
          data-testid={`role-summary-${slug}`}
        >
          {role.summary}
        </p>
      ) : null}
      <p
        className={cn(
          "text-xs",
          available.availability === "available"
            ? "text-foreground"
            : "text-amber-600 dark:text-amber-400",
        )}
        data-availability={available.availability}
        data-testid={`role-available-${slug}`}
        title={available.title ?? undefined}
      >
        {available.text}
      </p>
      <CardSection
        empty={ROLE_AGENTS_EMPTY}
        isEmpty={role.agents.length === 0}
        testId={`role-agents-${slug}`}
        title={ROLE_AGENTS_TITLE}
      >
        <ul className="flex flex-wrap gap-1">
          {role.agents.map((agent) => (
            <AgentChip
              agent={agent}
              key={agent.pubkey}
              roleHasPack={role.hasPack}
              roleRefusal={role.refusal}
            />
          ))}
        </ul>
      </CardSection>
      <RoleReports slug={slug} summary={reports} />
      <CardSection
        empty={ROLE_SEATS_EMPTY}
        isEmpty={role.seats.length === 0}
        testId={`role-seats-${slug}`}
        title={ROLE_SEATS_TITLE}
      >
        <ul className="flex flex-col">
          {role.seats.map((seat) => (
            <li key={seat.key}>
              <SeatRowButton
                columns="role-card"
                onOpen={onOpenSeat}
                seat={seat}
              />
            </li>
          ))}
        </ul>
      </CardSection>
      <details data-testid={`role-skills-${slug}`}>
        <summary className="cursor-pointer text-2xs font-medium uppercase tracking-wide text-muted-foreground">
          {roleSkillsSummary(role.skills.length)}
        </summary>
        {role.skills.length === 0 ? (
          <p
            className="mt-1 text-xs text-muted-foreground"
            data-testid={`role-skills-${slug}-empty`}
          >
            {ROLE_SKILLS_EMPTY}
          </p>
        ) : (
          <ul className="mt-1 flex flex-col gap-0.5">
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
      </details>
    </article>
  );
}
