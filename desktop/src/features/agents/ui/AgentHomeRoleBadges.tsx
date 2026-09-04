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
  /**
   * This agent has a pack and the home it runs in refuses to take it. `false`
   * means it does not; `undefined` means this build never asked, and an
   * unanswered field must not be rendered as a refusal.
   */
  packRefusedSharedHome?: boolean;
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
 * The fact: this agent's pack exists and is being refused at every spawn.
 *
 * A different claim from {@link AGENT_NO_ROLE_PACK_LABEL}, and the two never
 * appear together: that one says this computer has no pack, this one says the
 * pack is here and the shared working directory will not take it. An operator
 * told "not installed here" would go and install a pack they already have.
 */
export const AGENT_SHARED_HOME_LABEL = "Shared home — packs refused";

/** What to do about it. Together: `{LABEL} — {REMEDY}`. */
export const AGENT_SHARED_HOME_REMEDY =
  "give this agent its own nest, then restart it";

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
 * The shared-home badge is a *different* claim and displaces it: that pack is
 * on this computer and the working directory every agent shares refuses to
 * take it, so the agent runs with no role skills anyway (finding 68). Showing
 * both would tell an operator to install a pack they already have. It carries
 * the remedy this one cannot: the agent needs a nest of its own.
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
  const packRefused = agent?.packRefusedSharedHome === true;
  if (!homeRole) return null;
  const label = homeRole.charAt(0).toUpperCase() + homeRole.slice(1);
  return (
    <>
      <Badge data-testid="agent-home-role" variant="secondary">
        Home role: {label}
      </Badge>
      {packRefused ? (
        <Badge
          className="gap-1 whitespace-normal text-left normal-case leading-snug tracking-normal"
          data-testid="agent-shared-home"
          variant="warning"
        >
          <AlertTriangle className="mt-0.5 size-3 shrink-0 self-start" />
          {withRemedy
            ? `${AGENT_SHARED_HOME_LABEL} — ${AGENT_SHARED_HOME_REMEDY}`
            : AGENT_SHARED_HOME_LABEL}
        </Badge>
      ) : null}
      {!packRefused && agent?.hasRolePack === false ? (
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
