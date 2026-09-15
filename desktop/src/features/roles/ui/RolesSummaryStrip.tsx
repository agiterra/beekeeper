import type * as React from "react";
import { AlertTriangle } from "lucide-react";

import type { RolesPageSummary } from "../lib/rolesPageSummary";
import {
  agentsCountLabel,
  disputedCountText,
  openSessionsCountLabel,
  reportsCountLabel,
  ROLES_SCOPE_TITLE,
  rolesCountLabel,
  rolesScopeText,
  unconfirmedCountText,
} from "./rolesCopy";

function Tile({
  children,
  label,
  testId,
  value,
}: {
  children?: React.ReactNode;
  label: string;
  testId: string;
  value: number;
}) {
  return (
    <div
      className="min-w-0 rounded-lg border border-border/70 bg-card px-3 py-2"
      data-testid={testId}
    >
      <p className="text-xl font-semibold tabular-nums text-foreground">
        {value}
      </p>
      <p className="text-xs leading-snug text-muted-foreground">{label}</p>
      {children}
    </div>
  );
}

/**
 * The four counts a reader wants before reading anything: how many roles this
 * project has, how many agents can take one, how many sessions are open, and
 * how much of what the page says came from reports.
 *
 * The strip carries no colour of its own — a number is not a state — with one
 * exception the design keeps deliberately: a disputed report is a
 * contradiction the page must not let a reader walk past, so it gets the
 * warning icon *and* the word.
 *
 * The scope line under the agent count is the honest boundary: the count is
 * this project's agents on this computer, and identities that only held a
 * session here are named beside it as not project agents rather than added
 * to it.
 */
export function RolesSummaryStrip({ summary }: { summary: RolesPageSummary }) {
  return (
    <div
      className="grid grid-cols-2 gap-2 md:grid-cols-4"
      data-testid="roles-summary"
    >
      <Tile
        label={rolesCountLabel(summary.roles)}
        testId="roles-summary-roles"
        value={summary.roles}
      />
      <Tile
        label={agentsCountLabel(summary.agents)}
        testId="roles-summary-agents"
        value={summary.agents}
      >
        <p
          className="text-xs leading-snug text-muted-foreground"
          data-testid="roles-scope"
          title={ROLES_SCOPE_TITLE}
        >
          {rolesScopeText(
            summary.agentsLocal,
            summary.agentsShared,
            summary.nonProjectAgents,
          )}
        </p>
      </Tile>
      <Tile
        label={openSessionsCountLabel(summary.openSessions)}
        testId="roles-summary-sessions"
        value={summary.openSessions}
      />
      <Tile
        label={reportsCountLabel(summary.reports)}
        testId="roles-summary-reports"
        value={summary.reports}
      >
        {summary.unconfirmed > 0 || summary.disputed > 0 ? (
          <p
            className="flex flex-wrap items-center gap-x-2 text-xs text-muted-foreground"
            data-testid="roles-summary-reports-detail"
          >
            {summary.unconfirmed > 0 ? (
              <span>{unconfirmedCountText(summary.unconfirmed)}</span>
            ) : null}
            {summary.disputed > 0 ? (
              <span className="flex items-center gap-1 text-destructive">
                <AlertTriangle aria-hidden className="size-3" />
                {disputedCountText(summary.disputed)}
              </span>
            ) : null}
          </p>
        ) : null}
      </Tile>
    </div>
  );
}
