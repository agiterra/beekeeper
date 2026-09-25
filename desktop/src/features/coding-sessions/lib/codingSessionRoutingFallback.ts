/**
 * The catalog-fallback path: what runs when the registry-ranked pool is
 * empty and no override was given.
 *
 * Split out of `codingSessionRouting.ts` to keep that file under the
 * repository's file-size ratchet — this is one self-contained step of the
 * pipeline (spec §7's empty-pool case), not a second router. See ledger 267,
 * control run 8: a live catalog offer with no registry row is DORMANT
 * knowledge, not an unauthorized target, so an ordinary session may still be
 * served by it; a genuine hard requirement (this class's `requires`, or the
 * task's own `requirements`) still refuses every such candidate, named.
 */
import type {
  ModelRegistry,
  RegistryClass,
} from "./codingSessionModelRegistry";
import type { RoutedExecutionTarget } from "./codingSessionRoutingRecord";
import type {
  RoutingCatalogEntry,
  RoutingEffort,
  RoutingExclusion,
  RoutingRequirements,
} from "./codingSessionRouting";

/** The bare catalog id, stripping a trailing `[effort]` variant if present. */
export function bareModelId(model: string): string {
  const bracket = model.indexOf("[");
  return bracket === -1 ? model : model.slice(0, bracket);
}

/**
 * The first catalog offer, in the catalog's own order, that no registry row
 * covers at all and that no hard requirement rules out.
 *
 * Only reached when the registry-ranked pool is empty and no override was
 * given (spec's minimums and cost ranking never run here — there is nothing
 * to rank a target the registry says nothing about against). What still
 * applies, because it does not depend on a registry row to check: the
 * class's own `requires` and the task's own `requirements` — modality, tool
 * support, minimum context window, intolerable failure modes. Any of those
 * refuses every fallback candidate uniformly, since none of them is
 * something an unregistered catalog offer can be vouched for; that is the
 * "explicitly chosen hard policy" the ruling preserves. An ordinary session
 * with none of those set is not blocked by class trait minimums here — the
 * ruling's "registry ranking is a preference, not a gate" for exactly this
 * path.
 */
export function fallbackCatalogTarget(input: {
  registry: ModelRegistry;
  catalog: readonly RoutingCatalogEntry[];
  classGate: RegistryClass;
  requirements: RoutingRequirements | undefined;
  effort: RoutingEffort;
  excluded: RoutingExclusion[];
}):
  | { ok: true; target: RoutedExecutionTarget }
  | { ok: false; why: string | null } {
  const { registry, catalog, classGate, requirements, effort, excluded } =
    input;
  const registered = new Set(
    registry.targets.map((target) => `${target.provider}\u0000${target.model}`),
  );
  const block = fallbackHardBlock(classGate, requirements);
  let anyUnregistered = false;
  for (const entry of catalog) {
    const key = `${entry.providerInstanceRef}\u0000${bareModelId(entry.model)}`;
    if (registered.has(key)) continue; // already considered with real facts
    anyUnregistered = true;
    if (block !== null) {
      excluded.push({
        provider: entry.providerInstanceRef,
        model: entry.model,
        why: `no registry row, and ${block}`,
      });
      continue;
    }
    if (requirements?.minContextWindow !== undefined) {
      const window = entry.contextWindow ?? null;
      if (window === null || window < requirements.minContextWindow) {
        excluded.push({
          provider: entry.providerInstanceRef,
          model: entry.model,
          why:
            window === null
              ? `no registry row and no recorded context window, so ${requirements.minContextWindow} tokens is not established for it`
              : `no registry row; holds ${window} tokens, and the task needs ${requirements.minContextWindow}`,
        });
        continue;
      }
    }
    return {
      ok: true,
      target: {
        provider: entry.providerInstanceRef,
        model: entry.model,
        effort,
      },
    };
  }
  return { ok: false, why: anyUnregistered ? block : null };
}

/**
 * Why every unregistered catalog offer is blocked, or null when none is.
 *
 * Only the requirements an unknown target can never be vouched for: the class
 * or task's `requires`/`requirements`. `minContextWindow` is checked per
 * catalog entry instead (the live catalog publishes its own context window
 * per model, so that one fact does not need a registry row).
 */
export function fallbackHardBlock(
  classGate: RegistryClass,
  requirements: RoutingRequirements | undefined,
): string | null {
  const multimodalRequired =
    requirements?.multimodal === true ||
    classGate.requires?.multimodal === true;
  if (multimodalRequired) {
    return (
      "this class requires multimodal, and an unregistered catalog offer " +
      "carries no recorded modality to vouch for it"
    );
  }
  const tools = [
    ...(classGate.requires?.tools ?? []),
    ...(requirements?.tools ?? []),
  ];
  if (tools.length > 0) {
    return (
      `this class requires the ${tools.join(", ")} tool(s), and an ` +
      "unregistered catalog offer carries no recorded tool support"
    );
  }
  if ((requirements?.incompatibleFailureModes ?? []).length > 0) {
    return (
      "this task cannot tolerate a known failure mode, and an unregistered " +
      "catalog offer carries no recorded failure-mode history to clear it"
    );
  }
  return null;
}
