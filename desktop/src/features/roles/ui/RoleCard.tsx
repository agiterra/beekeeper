import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";

import type { RoleAgentChip, RoleRow, SeatRow } from "../lib/rolesViewModel";
import {
  AGENT_CHIP_NO_PACK_TITLE,
  AGENT_CHIP_SHARED_HOME_TITLE,
  ROLE_AGENTS_EMPTY,
  ROLE_AGENTS_TITLE,
  ROLE_NO_PACK,
  ROLE_SEATS_EMPTY,
  ROLE_SEATS_TITLE,
  ROLE_SKILLS_EMPTY,
  ROLE_SKILLS_TITLE,
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
 * The muted, dashed chip is the disclosure `hasRolePack === false` exists
 * for: this computer holds no pack behind the role, so a seat on the agent
 * runs on its persona prompt alone. The tooltip is the Agents tab's exact
 * badge copy. A refused shared home is the other claim and displaces it, as
 * `AgentHomeRoleBadges` does. An agent whose pack was never looked for
 * (`undefined`) gets a plain chip — an unanswered question is not a
 * missing pack.
 */
function AgentChip({ agent }: { agent: RoleAgentChip }) {
  const refused = agent.packRefusedSharedHome === true;
  const missing = !refused && agent.hasRolePack === false;
  const title = refused
    ? AGENT_CHIP_SHARED_HOME_TITLE
    : missing
      ? AGENT_CHIP_NO_PACK_TITLE
      : undefined;
  return (
    <li
      className={cn(
        "rounded-full border border-border px-2 py-0.5 text-xs text-foreground",
        (missing || refused) && "border-dashed text-muted-foreground",
      )}
      data-agent-pack={
        refused
          ? "refused"
          : missing
            ? "missing"
            : agent.hasRolePack === true
              ? "present"
              : "unasked"
      }
      data-agent-pubkey={agent.pubkey}
      data-testid="role-agent-chip"
      title={title}
    >
      {agent.name}
    </li>
  );
}

/** One role: what it has, who carries it, who is seated in it. */
export function RoleCard({
  role,
  onOpenSeat,
}: {
  role: RoleRow;
  onOpenSeat?: (seat: SeatRow) => void;
}) {
  const slug = role.role;
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
        {role.origin ? (
          <Badge
            data-testid={`role-origin-${slug}`}
            variant={role.origin === "shipped" ? "outline" : "secondary"}
          >
            {role.origin}
          </Badge>
        ) : null}
        {role.version ? (
          <span
            className="text-2xs text-muted-foreground"
            data-testid={`role-version-${slug}`}
          >
            v{role.version}
          </span>
        ) : null}
      </header>
      {role.hasPack ? null : (
        <p
          className="text-xs text-muted-foreground"
          data-testid={`role-nopack-${slug}`}
        >
          {ROLE_NO_PACK}
        </p>
      )}
      {role.description ? (
        <p
          className="truncate text-xs text-foreground"
          data-testid={`role-description-${slug}`}
          title={role.description}
        >
          {role.description}
        </p>
      ) : null}
      {role.summary ? (
        <p
          className="line-clamp-3 text-xs text-muted-foreground"
          data-testid={`role-summary-${slug}`}
        >
          {role.summary}
        </p>
      ) : null}
      <CardSection
        empty={ROLE_SKILLS_EMPTY}
        isEmpty={role.skills.length === 0}
        testId={`role-skills-${slug}`}
        title={ROLE_SKILLS_TITLE}
      >
        <ul className="flex flex-col gap-0.5">
          {role.skills.map((skill) => (
            <li
              className="text-xs"
              data-testid="role-skill"
              key={`${skill.name}:${skill.shared ? "shared" : "own"}`}
            >
              <span className="font-medium text-foreground">{skill.name}</span>
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
      </CardSection>
      <CardSection
        empty={ROLE_AGENTS_EMPTY}
        isEmpty={role.agents.length === 0}
        testId={`role-agents-${slug}`}
        title={ROLE_AGENTS_TITLE}
      >
        <ul className="flex flex-wrap gap-1">
          {role.agents.map((agent) => (
            <AgentChip agent={agent} key={agent.pubkey} />
          ))}
        </ul>
      </CardSection>
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
      {role.refusal ? (
        <p
          className="text-xs text-amber-600 dark:text-amber-400"
          data-testid={`role-refusal-${slug}`}
        >
          {role.refusal}
        </p>
      ) : null}
    </article>
  );
}
