import {
  codingSessionProvidersForProject,
  type TrustedCodingSessionProviderCatalog,
} from "./codingSessionProviderCatalog";

/** The one signed catalog fact a routed hire is allowed to cite. */
export type CodingSessionHireCatalogSource = {
  modelCatalogs: ReadonlyMap<string, readonly string[]>;
  catalogRevision: number;
  eventId: string;
};

/**
 * Resolve the exact local-provider catalog behind one hire.
 *
 * Revisions are per signer and channel, so neither a catalog from another
 * provider nor a revision observed in another channel can explain this
 * create. The returned model map and revision come from the same signed event;
 * ambiguity refuses the source instead of combining facts nobody signed.
 */
export function resolveCodingSessionHireCatalogSource(input: {
  entries: readonly TrustedCodingSessionProviderCatalog[];
  channelId: string;
  signerPubkey: string;
  projectRef: string | null;
  repoRef?: string | null;
}): CodingSessionHireCatalogSource | null {
  const signer = input.signerPubkey.trim().toLowerCase();
  const matches = input.entries.filter(
    (entry) =>
      entry.channelId === input.channelId &&
      entry.signerPubkey.trim().toLowerCase() === signer,
  );
  if (matches.length === 0) return null;

  const sourceIds = new Set(matches.map((entry) => entry.eventId));
  const revisions = new Set(matches.map((entry) => entry.catalog.revision));
  if (sourceIds.size !== 1 || revisions.size !== 1) return null;

  const modelCatalogs = new Map<string, readonly string[]>();
  for (const entry of matches) {
    for (const provider of codingSessionProvidersForProject(
      entry.catalog,
      input.projectRef,
      input.repoRef ?? null,
    )) {
      const previous = modelCatalogs.get(provider.providerInstanceRef);
      if (
        previous !== undefined &&
        JSON.stringify(previous) !== JSON.stringify(provider.allowedModels)
      ) {
        return null;
      }
      modelCatalogs.set(provider.providerInstanceRef, provider.allowedModels);
    }
  }

  return {
    modelCatalogs,
    catalogRevision: matches[0].catalog.revision,
    eventId: matches[0].eventId,
  };
}
