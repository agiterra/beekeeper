/**
 * The planning providers this build knows, and which one a project uses.
 *
 * Native is built in and on by default. Linear is listed so a person can see
 * it is coming and what it needs, but this build has no way to connect a
 * project to Linear: no installation record, no credential custody and no
 * outbound API client exist yet. So it is reported unavailable and is never
 * selectable — a Linear choice that silently read native data, or an empty
 * list that looked like an empty Linear team, would be a lie.
 *
 * There is no stored per-project choice yet either, because there is only
 * one choice that works. When a second provider can be connected, the
 * choice has to be a shared project fact (every member and agent must see
 * the same provider), and switching must never move or copy records.
 */
import type {
  PlanningProviderDescriptor,
  PlanningProviderId,
} from "./planningTypes.ts";

export const NATIVE_PLANNING_PROVIDER: PlanningProviderDescriptor = {
  id: "native",
  label: "Beekeeper",
  description:
    "Issues live on this community's relay as the project's shared lists. Every member and agent reads the same records; `bee todos` operates them headless.",
  availability: { kind: "available" },
};

export const LINEAR_PLANNING_PROVIDER: PlanningProviderDescriptor = {
  id: "linear",
  label: "Linear",
  description:
    "Issues would stay in Linear, owned by Linear, and Beekeeper would operate them through Linear's API.",
  availability: {
    kind: "unavailable",
    reason:
      "Not available in this build: Beekeeper cannot connect a project to a Linear workspace yet.",
  },
};

export const PLANNING_PROVIDERS: readonly PlanningProviderDescriptor[] = [
  NATIVE_PLANNING_PROVIDER,
  LINEAR_PLANNING_PROVIDER,
];

/**
 * The provider a project's Planning reads. Always native today: it is the
 * only available provider and there is no stored choice to honour.
 */
export function resolvePlanningProvider(): PlanningProviderId {
  return "native";
}
