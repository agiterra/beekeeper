/**
 * The Planning providers a project can use, and which one this view shows.
 *
 * A provider is a relay plugin that implements `beekeeper.planning/1`.
 * Beekeeper's own (`planning`) is registered the same way as any other — it
 * ships in the relay image and is on by default, but nothing here treats it
 * specially: the list is whatever the relay says it runs, labelled by what
 * each plugin says about itself.
 *
 * Linear is listed so a person can see it is coming and what it needs, but
 * no Linear plugin exists: no installation record, no credential custody and
 * no outbound client. It is never selectable — a Linear choice that silently
 * read native data, or an empty list that looked like an empty Linear team,
 * would be a lie.
 *
 * Beekeeper's own Planning (`planning`) is the default whenever the relay
 * runs it, however the others sort: an operator adding a provider must not
 * quietly move everyone off native Planning. There is no stored per-project
 * choice. When a relay runs more than one provider and none is `planning`,
 * this view picks the first by name and says so; the person can
 * switch what *they* are looking at, but that is not a project fact — every
 * member and agent must eventually see the same provider, and switching must
 * never move or copy records.
 */
import type { PluginInfo } from "@/shared/plugins/pluginClient";

import type {
  PlanningCapabilities,
  PlanningProviderDescriptor,
  PlanningProviderId,
} from "./planningTypes";

export const LINEAR_PLANNING_PROVIDER: PlanningProviderDescriptor = {
  id: "linear",
  label: "Linear",
  description:
    "Issues would stay in Linear, owned by Linear, and Beekeeper would operate them through Linear's API.",
  availability: {
    kind: "unavailable",
    reason:
      "Not available: no Linear plugin exists yet, so Beekeeper cannot connect a project to a Linear workspace.",
  },
};

/** The descriptor for a registered provider plugin. */
export function pluginProviderDescriptor(
  plugin: PluginInfo,
  capabilities: PlanningCapabilities | null,
): PlanningProviderDescriptor {
  const described = capabilities?.provider;
  return {
    id: plugin.name,
    label: described?.label ?? plugin.name,
    description: `${described?.description ?? "A Planning provider registered on this relay."} Plugin ${plugin.name} ${plugin.version}, sha256 ${plugin.sha256.slice(0, 12)}…`,
    availability: { kind: "available" },
  };
}

/**
 * Every provider the control lists: the relay's, then Linear (unless a
 * plugin by that name is registered, in which case it is just another
 * provider).
 */
export function planningProviderList(
  registered: readonly PlanningProviderDescriptor[],
): PlanningProviderDescriptor[] {
  return registered.some((p) => p.id === LINEAR_PLANNING_PROVIDER.id)
    ? [...registered]
    : [...registered, LINEAR_PLANNING_PROVIDER];
}

/** The name of Beekeeper's own Planning plugin, the built-in a relay ships. */
export const NATIVE_PLANNING_PLUGIN = "planning";

/**
 * The provider this view shows: the viewer's pick when it is still
 * registered, else native Planning when registered, else the first
 * registered by name, else none.
 */
export function resolvePlanningProvider(
  registered: readonly PluginInfo[],
  picked: PlanningProviderId | null,
): PluginInfo | null {
  return (
    registered.find((plugin) => plugin.name === picked) ??
    registered.find((plugin) => plugin.name === NATIVE_PLANNING_PLUGIN) ??
    registered[0] ??
    null
  );
}
