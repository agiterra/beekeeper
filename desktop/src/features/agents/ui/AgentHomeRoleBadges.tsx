import { AlertTriangle } from "lucide-react";

import { Badge } from "@/shared/ui/badge";

/** As much of a managed agent as the role badges read. */
export type AgentHomeRoleBadgeAgent = {
  /**
   * The role this agent *is*, from its pack persona. `null`/`undefined` when
   * it declares none — then neither badge renders, because there is nothing
   * to claim.
   */
  homeRole?: string | null;
  /**
   * Whether this computer can stage the pack behind that role. `false` is the
   * disclosure; `undefined` means this build never asked, and an unanswered
   * field must not be rendered as a missing pack.
   */
  hasRolePack?: boolean;
};

/**
 * The fact: this computer holds no pack behind the agent's role.
 *
 * The first wording ("No role pack on this computer") read as if roles were
 * something a machine either had or lacked. A pack is something the operator
 * installs, so the label says "not installed here" and the remedy below says
 * where from.
 */
export const AGENT_NO_ROLE_PACK_LABEL = "Role pack not installed here";

/** What to do about it. Together: `{LABEL} — {REMEDY}`. */
export const AGENT_NO_ROLE_PACK_REMEDY =
  "install team roles from the project's personas/roles";

/**
 * The role this agent *is*, and whether this computer can stage the pack
 * behind it.
 *
 * The second badge is the disclosure the whole `hasRolePack` field exists for:
 * an agent carrying a home role with no pack on this computer must never
 * render as if it carried the role's craft — a seat on it runs on the persona
 * prompt alone. An agent with no home role shows neither badge, and an agent
 * whose pack was never looked for shows only the first.
 *
 * `withRemedy` is the only thing a surface gets to vary, and it only ever
 * *adds*: a card in a five-column grid states the fact, a detail panel states
 * the fact and what to do about it. Neither drops the fact, because that is
 * the claim; the remedy is advice, and leaving advice off a thumbnail is not a
 * lie about the agent.
 */
export function AgentHomeRoleBadges({
  agent,
  withRemedy = true,
}: {
  agent: AgentHomeRoleBadgeAgent | null | undefined;
  withRemedy?: boolean;
}) {
  const homeRole = agent?.homeRole?.trim();
  if (!homeRole) return null;
  const label = homeRole.charAt(0).toUpperCase() + homeRole.slice(1);
  return (
    <>
      <Badge data-testid="agent-home-role" variant="secondary">
        Home role: {label}
      </Badge>
      {agent?.hasRolePack === false ? (
        <Badge
          className="gap-1 whitespace-normal text-left normal-case leading-snug tracking-normal"
          data-testid="agent-no-role-pack"
          variant="warning"
        >
          <AlertTriangle className="mt-0.5 size-3 shrink-0 self-start" />
          {withRemedy
            ? `${AGENT_NO_ROLE_PACK_LABEL} — ${AGENT_NO_ROLE_PACK_REMEDY}`
            : AGENT_NO_ROLE_PACK_LABEL}
        </Badge>
      ) : null}
    </>
  );
}
