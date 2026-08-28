import type { AgentPersona, ManagedAgent } from "@/shared/api/types";

/**
 * What an Agents-grid card is called.
 *
 * A persona card in the library stands for one running identity — the card's
 * avatar, model, status and click target all resolve to that instance — so it
 * is titled with the instance's own name whenever there is one. The persona's
 * display name is the fallback for a card that has no instance behind it yet.
 *
 * This exists because the two can disagree: the team-role installer mints an
 * identity the operator names (`Keystone`) onto a persona card that came from
 * a pack (`Lead`), and a grid that showed the pack's name named an agent the
 * operator could not find anywhere else in the app (item 79a).
 */
export function resolveAgentCardTitle(input: {
  persona: Pick<AgentPersona, "displayName">;
  agent: Pick<ManagedAgent, "name"> | undefined;
}): string {
  const instanceName = input.agent?.name?.trim() ?? "";
  return instanceName.length > 0 ? instanceName : input.persona.displayName;
}
