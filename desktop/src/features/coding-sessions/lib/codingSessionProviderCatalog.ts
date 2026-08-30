/**
 * The 44222 provider catalog: what a provider offers, canonically.
 *
 * The catalog is how the create flow learns a provider exists, which models it
 * accepts, and what it can be asked to do. `cspc-key` digests the exact signed
 * bytes, so this decoder re-serializes its parse and compares byte for byte —
 * the serialization is part of the contract, not an implementation detail.
 *
 * `providers[]` is the top-level offer and is required. `projects[]` is an
 * optional narrowing that names provider refs per project; it is omitted
 * entirely when empty, which is the standalone case — every listed provider
 * serves every session.
 */
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";

import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_PROVIDER_CATALOG } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import type { CodingSessionIngressAuthority } from "./codingSessionIngressAuthority";
import { encodeStructuredKey } from "./codingSessionKeys";

export const CODING_SESSION_PROVIDER_CATALOG_SCHEMA =
  "buzz-coding-session-provider-catalog/v1" as const;
export const CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION = "cspc1-1" as const;

export const MAX_PROVIDER_CATALOG_CONTENT_BYTES = 256 * 1024;
export const MAX_PROVIDER_CATALOG_REFERENCE_BYTES = 2 * 1024;
export const MAX_PROVIDER_CATALOG_PROJECTS = 512;
export const MAX_PROVIDER_CATALOG_PROVIDERS = 32;
export const MAX_PROVIDER_CATALOG_MODELS = 64;

export type CodingSessionProviderCapabilities = {
  threadTurnStart: boolean;
  threadTurnInterrupt: boolean;
  threadSteer: boolean;
  context: boolean;
  diff: boolean;
  plan: boolean;
};

/**
 * What the publisher knows about one offered model id.
 *
 * Every field beyond `id` is optional and absent means **nobody said** — never
 * a default. A window that was never measured is omitted rather than guessed,
 * because a percentage computed against a guessed denominator renders exactly
 * like a measured one.
 */
export type CodingSessionProviderCatalogModel = {
  /** The id, exactly as it appears in `allowedModels`. */
  id: string;
  contextWindow?: number;
  family?: string;
  vendor?: string;
  deprecated?: boolean;
};

export type CodingSessionProviderCatalogProvider = {
  providerInstanceRef: string;
  driver: string;
  runtime: string;
  defaultModel: string;
  allowedModels: string[];
  capabilities: CodingSessionProviderCapabilities;
  /**
   * Per-model description of ids `allowedModels` already offers.
   *
   * Sparse and optional: rows appear in `allowedModels` order, name only ids
   * on that list, and each carries at least one fact beyond its id. It
   * **describes** the offer and never extends it — `allowedModels` is the
   * whole list of models on offer, and a row outside it would make an
   * unoffered model look offered.
   */
  models?: CodingSessionProviderCatalogModel[];
};

/** A project-scoped narrowing: which offered providers serve this project. */
export type CodingSessionProviderCatalogProject = {
  projectRef: string;
  repoRef: string | null;
  providers: string[];
};

export type CodingSessionProviderCatalogV1 = {
  schema: typeof CODING_SESSION_PROVIDER_CATALOG_SCHEMA;
  revision: number;
  providers: CodingSessionProviderCatalogProvider[];
  projects?: CodingSessionProviderCatalogProject[];
};

export type TrustedCodingSessionProviderCatalog = {
  channelId: string;
  signerPubkey: string;
  eventId: string;
  createdAt: number;
  catalog: Readonly<CodingSessionProviderCatalogV1>;
};

export type CodingSessionProviderCatalogSnapshot = {
  entries: TrustedCodingSessionProviderCatalog[];
  malformedCount: number;
  rejectedAuthorCount: number;
  invalidSignatureCount: number;
  conflicts: CodingSessionProviderCatalogConflict[];
  conflictCount: number;
};

export type CodingSessionProviderCatalogConflict = {
  channelId: string;
  providerInstanceRef: string;
};

export type CodingSessionProviderCatalogClassification =
  | { kind: "catalog"; entry: TrustedCodingSessionProviderCatalog }
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "rejected-author" }
  | { kind: "invalid-signature" };

export function codingSessionProviderCatalogSemanticKey(
  channelId: string,
  revision: number,
  content: string,
): string {
  return encodeStructuredKey(
    "coding-session-provider-catalog/v1",
    channelId,
    String(revision),
    bytesToHex(sha256(new TextEncoder().encode(content))),
  );
}

/** Every provider a catalog offers, whether or not it narrows by project. */
export function codingSessionProviderCatalogProviders(
  catalog: Readonly<CodingSessionProviderCatalogV1>,
): CodingSessionProviderCatalogProvider[] {
  return catalog.providers;
}

/**
 * The providers offered for one project coordinate.
 *
 * With no `projects[]` narrowing, every provider serves every project — that
 * is what makes a standalone session possible against a catalog that never
 * heard of the project it is being created under.
 */
export function codingSessionProvidersForProject(
  catalog: Readonly<CodingSessionProviderCatalogV1>,
  projectRef: string | null,
  repoRef: string | null = null,
): CodingSessionProviderCatalogProvider[] {
  if (!catalog.projects || projectRef === null) return catalog.providers;
  const narrowing = catalog.projects.find(
    (project) =>
      project.projectRef === projectRef && project.repoRef === repoRef,
  );
  if (!narrowing) return [];
  return catalog.providers.filter((provider) =>
    narrowing.providers.includes(provider.providerInstanceRef),
  );
}

export function parseCodingSessionProviderCatalog(
  content: unknown,
): Readonly<CodingSessionProviderCatalogV1> | null {
  if (
    typeof content !== "string" ||
    utf8Length(content) > MAX_PROVIDER_CATALOG_CONTENT_BYTES
  ) {
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(content);
  } catch {
    return null;
  }
  if (
    !isPlainRecord(parsed) ||
    !hasOrderedKeys(
      parsed,
      ["schema", "revision", "providers"],
      ["projects"],
    ) ||
    parsed.schema !== CODING_SESSION_PROVIDER_CATALOG_SCHEMA ||
    !Number.isSafeInteger(parsed.revision) ||
    Number(parsed.revision) <= 0
  ) {
    return null;
  }

  const providers = parseProviders(parsed.providers);
  if (!providers) return null;
  const projects = Object.hasOwn(parsed, "projects")
    ? parseProjects(parsed.projects, providers)
    : undefined;
  if (Object.hasOwn(parsed, "projects") && !projects) return null;

  const catalog: CodingSessionProviderCatalogV1 = {
    schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
    revision: parsed.revision as number,
    providers,
    ...(projects ? { projects } : {}),
  };
  return JSON.stringify(catalog) === content ? Object.freeze(catalog) : null;
}

export function classifyCodingSessionProviderCatalogEvent(
  event: RelayEvent,
  allowedChannelIds: ReadonlySet<string>,
  authority: CodingSessionIngressAuthority,
): CodingSessionProviderCatalogClassification {
  if (
    event.kind !== KIND_CODING_SESSION_PROVIDER_CATALOG ||
    !Array.isArray(event.tags)
  ) {
    return { kind: "irrelevant" };
  }
  if (authority.state !== "valid") return { kind: "malformed" };
  const source = authority.byPubkey.get(event.pubkey.trim().toLowerCase());
  if (!source) return { kind: "rejected-author" };
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };
  const tags = parseExactCatalogTags(event.tags);
  if (
    !tags ||
    !allowedChannelIds.has(tags.channelId) ||
    tags.version !== CODING_SESSION_PROVIDER_CATALOG_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const catalog = parseCodingSessionProviderCatalog(event.content);
  if (
    !catalog ||
    tags.revision !== String(catalog.revision) ||
    tags.semanticKey !==
      codingSessionProviderCatalogSemanticKey(
        tags.channelId,
        catalog.revision,
        event.content,
      )
  ) {
    return { kind: "malformed" };
  }
  return {
    kind: "catalog",
    entry: {
      channelId: tags.channelId,
      signerPubkey: event.pubkey.trim().toLowerCase(),
      eventId: event.id,
      createdAt: event.created_at,
      catalog,
    },
  };
}

/** Latest trusted canonical snapshot per exact channel and signing authority. */
export class CodingSessionProviderCatalogStore {
  private readonly bySource = new Map<
    string,
    {
      revision: number;
      candidates: Map<string, TrustedCodingSessionProviderCatalog>;
    }
  >();
  private readonly dispositions = new Map<
    string,
    CodingSessionProviderCatalogClassification["kind"]
  >();
  private malformedCount = 0;
  private rejectedAuthorCount = 0;
  private invalidSignatureCount = 0;

  ingestRelayEvents(
    events: readonly RelayEvent[],
    channelIds: readonly string[],
    authority: CodingSessionIngressAuthority,
  ): void {
    const allowed = new Set(channelIds);
    for (const event of events) {
      if (this.dispositions.has(event.id)) continue;
      const classification = classifyCodingSessionProviderCatalogEvent(
        event,
        allowed,
        authority,
      );
      this.dispositions.set(event.id, classification.kind);
      if (classification.kind === "catalog") {
        const sourceKey = `${classification.entry.channelId}\u0000${classification.entry.signerPubkey}`;
        const previous = this.bySource.get(sourceKey);
        if (
          !previous ||
          classification.entry.catalog.revision > previous.revision
        ) {
          this.bySource.set(sourceKey, {
            revision: classification.entry.catalog.revision,
            candidates: new Map([[event.content, classification.entry]]),
          });
        } else if (
          classification.entry.catalog.revision === previous.revision
        ) {
          const samePayload = previous.candidates.get(event.content);
          if (
            !samePayload ||
            compareCatalogEntryFreshness(classification.entry, samePayload) < 0
          ) {
            previous.candidates.set(event.content, classification.entry);
          }
        }
      } else if (classification.kind === "malformed") {
        this.malformedCount += 1;
      } else if (classification.kind === "rejected-author") {
        this.rejectedAuthorCount += 1;
      } else if (classification.kind === "invalid-signature") {
        this.invalidSignatureCount += 1;
      }
    }
  }

  snapshot(
    channelIds: readonly string[],
  ): CodingSessionProviderCatalogSnapshot {
    const allowed = new Set(channelIds);
    const byChannel = new Map<string, TrustedCodingSessionProviderCatalog[]>();
    for (const record of this.bySource.values()) {
      for (const entry of record.candidates.values()) {
        if (!allowed.has(entry.channelId)) continue;
        const channel = byChannel.get(entry.channelId) ?? [];
        channel.push(entry);
        byChannel.set(entry.channelId, channel);
      }
    }

    const conflicts: CodingSessionProviderCatalogConflict[] = [];
    const entries: TrustedCodingSessionProviderCatalog[] = [];
    for (const [channelId, candidates] of byChannel) {
      const reconciled = reconcileChannelCatalogs(channelId, candidates);
      conflicts.push(...reconciled.conflicts);
      entries.push(...reconciled.entries);
    }
    return {
      entries: entries.sort(compareCatalogEntries),
      malformedCount: this.malformedCount,
      rejectedAuthorCount: this.rejectedAuthorCount,
      invalidSignatureCount: this.invalidSignatureCount,
      conflicts: conflicts.sort(compareCatalogConflicts),
      conflictCount: conflicts.length,
    };
  }
}

type ProviderDeclaration = {
  entry: TrustedCodingSessionProviderCatalog;
  provider: CodingSessionProviderCatalogProvider;
};

/**
 * Resolve one channel's catalogs into a per-provider offer.
 *
 * A `providerInstanceRef` is the coordinate a create command names, so two
 * catalogs describing the same ref differently is an ambiguity the picker must
 * not silently pick a side of. Within one signer that is a producer bug;
 * across signers at the same timestamp there is no shared clock to break the
 * tie. Either way the ref is withheld and reported as a conflict.
 */
function reconcileChannelCatalogs(
  channelId: string,
  candidates: TrustedCodingSessionProviderCatalog[],
): {
  entries: TrustedCodingSessionProviderCatalog[];
  conflicts: CodingSessionProviderCatalogConflict[];
} {
  const declarations = new Map<string, ProviderDeclaration[]>();
  for (const entry of candidates) {
    for (const provider of entry.catalog.providers) {
      const grouped = declarations.get(provider.providerInstanceRef) ?? [];
      grouped.push({ entry, provider });
      declarations.set(provider.providerInstanceRef, grouped);
    }
  }

  const winners = new Map<
    string,
    {
      origin: TrustedCodingSessionProviderCatalog;
      providers: CodingSessionProviderCatalogProvider[];
    }
  >();
  const conflicts: CodingSessionProviderCatalogConflict[] = [];
  for (const [providerInstanceRef, grouped] of declarations) {
    // `bySource` has already selected each signer's highest local revision.
    // Revisions from different signing authorities have no shared clock.
    const bySigner = new Map<string, ProviderDeclaration[]>();
    for (const declaration of grouped) {
      const signerDeclarations =
        bySigner.get(declaration.entry.signerPubkey) ?? [];
      signerDeclarations.push(declaration);
      bySigner.set(declaration.entry.signerPubkey, signerDeclarations);
    }

    const signerWinners: ProviderDeclaration[] = [];
    let signerConflict = false;
    for (const signerDeclarations of bySigner.values()) {
      const descriptors = new Set(
        signerDeclarations.map((declaration) =>
          JSON.stringify(declaration.provider),
        ),
      );
      if (descriptors.size > 1) {
        signerConflict = true;
        break;
      }
      signerWinners.push(
        [...signerDeclarations].sort((left, right) =>
          compareCatalogEntryFreshness(left.entry, right.entry),
        )[0],
      );
    }
    if (signerConflict) {
      conflicts.push({ channelId, providerInstanceRef });
      continue;
    }

    const freshestCreatedAt = Math.max(
      ...signerWinners.map((declaration) => declaration.entry.createdAt),
    );
    const freshest = signerWinners.filter(
      (declaration) => declaration.entry.createdAt === freshestCreatedAt,
    );
    const freshestDescriptors = new Set(
      freshest.map((declaration) => JSON.stringify(declaration.provider)),
    );
    if (freshestDescriptors.size > 1) {
      conflicts.push({ channelId, providerInstanceRef });
      continue;
    }

    const selected = [...freshest].sort((left, right) =>
      compareCatalogEntryFreshness(left.entry, right.entry),
    )[0];
    const originKey = `${selected.entry.signerPubkey}\u0000${selected.entry.eventId}`;
    const originGroup = winners.get(originKey) ?? {
      origin: selected.entry,
      providers: [],
    };
    originGroup.providers.push(selected.provider);
    winners.set(originKey, originGroup);
  }

  const entries = [...winners.values()].map(({ origin, providers }) => {
    const kept = providers.sort((left, right) =>
      left.providerInstanceRef.localeCompare(right.providerInstanceRef),
    );
    const keptRefs = new Set(
      kept.map((provider) => provider.providerInstanceRef),
    );
    // A narrowing that now names only withheld providers describes nothing.
    const projects = (origin.catalog.projects ?? [])
      .map((project) => ({
        ...project,
        providers: project.providers.filter((ref) => keptRefs.has(ref)),
      }))
      .filter((project) => project.providers.length > 0);
    return {
      ...origin,
      catalog: Object.freeze({
        schema: CODING_SESSION_PROVIDER_CATALOG_SCHEMA,
        revision: origin.catalog.revision,
        providers: kept,
        ...(projects.length > 0 ? { projects } : {}),
      } satisfies CodingSessionProviderCatalogV1),
    };
  });

  return { entries, conflicts };
}

function compareCatalogEntryFreshness(
  left: TrustedCodingSessionProviderCatalog,
  right: TrustedCodingSessionProviderCatalog,
): number {
  const byCreatedAt = right.createdAt - left.createdAt;
  if (byCreatedAt !== 0) return byCreatedAt;
  const bySigner = left.signerPubkey.localeCompare(right.signerPubkey);
  return bySigner !== 0 ? bySigner : left.eventId.localeCompare(right.eventId);
}

function compareCatalogEntries(
  left: TrustedCodingSessionProviderCatalog,
  right: TrustedCodingSessionProviderCatalog,
): number {
  const byChannel = left.channelId.localeCompare(right.channelId);
  if (byChannel !== 0) return byChannel;
  const byFreshness = compareCatalogEntryFreshness(left, right);
  if (byFreshness !== 0) return byFreshness;
  return JSON.stringify(left.catalog.providers).localeCompare(
    JSON.stringify(right.catalog.providers),
  );
}

function compareCatalogConflicts(
  left: CodingSessionProviderCatalogConflict,
  right: CodingSessionProviderCatalogConflict,
): number {
  const byChannel = left.channelId.localeCompare(right.channelId);
  return byChannel !== 0
    ? byChannel
    : left.providerInstanceRef.localeCompare(right.providerInstanceRef);
}

function parseProviders(
  value: unknown,
): CodingSessionProviderCatalogProvider[] | null {
  if (
    !Array.isArray(value) ||
    value.length < 1 ||
    value.length > MAX_PROVIDER_CATALOG_PROVIDERS
  ) {
    return null;
  }
  const refs = new Set<string>();
  const providers: CodingSessionProviderCatalogProvider[] = [];
  for (const raw of value) {
    const provider = parseProvider(raw);
    if (
      !provider ||
      refs.has(provider.providerInstanceRef) ||
      !modelsAreCanonical(provider.allowedModels, provider.defaultModel)
    ) {
      return null;
    }
    refs.add(provider.providerInstanceRef);
    providers.push(provider);
  }
  return isSorted(providers, (provider) => provider.providerInstanceRef)
    ? providers
    : null;
}

function parseProjects(
  value: unknown,
  providers: readonly CodingSessionProviderCatalogProvider[],
): CodingSessionProviderCatalogProject[] | null {
  // An empty narrowing is never canonical: the producer omits the key.
  if (
    !Array.isArray(value) ||
    value.length < 1 ||
    value.length > MAX_PROVIDER_CATALOG_PROJECTS
  ) {
    return null;
  }
  const declaredRefs = new Set(
    providers.map((provider) => provider.providerInstanceRef),
  );
  const coordinates = new Set<string>();
  const projects: CodingSessionProviderCatalogProject[] = [];
  for (const raw of value) {
    if (
      !isPlainRecord(raw) ||
      !hasOrderedKeys(raw, ["projectRef", "repoRef", "providers"], []) ||
      !boundedNonblank(raw.projectRef) ||
      !boundedNullable(raw.repoRef) ||
      !Array.isArray(raw.providers) ||
      raw.providers.length < 1 ||
      raw.providers.length > MAX_PROVIDER_CATALOG_PROVIDERS
    ) {
      return null;
    }
    const coordinate = `${raw.projectRef}\u0000${raw.repoRef ?? ""}`;
    if (coordinates.has(coordinate)) return null;
    coordinates.add(coordinate);

    const seen = new Set<string>();
    for (const ref of raw.providers) {
      // A narrowing that names a provider this catalog never offered is not a
      // narrowing — it is a claim about something the signer did not declare.
      if (!boundedNonblank(ref) || seen.has(ref) || !declaredRefs.has(ref)) {
        return null;
      }
      seen.add(ref);
    }
    if (!isSorted(raw.providers as string[], (ref) => ref)) return null;
    projects.push({
      projectRef: raw.projectRef,
      repoRef: raw.repoRef,
      providers: raw.providers as string[],
    });
  }
  return projectsAreCanonical(projects) ? projects : null;
}

function parseProvider(
  value: unknown,
): CodingSessionProviderCatalogProvider | null {
  if (
    !isPlainRecord(value) ||
    !hasOrderedKeys(
      value,
      [
        "providerInstanceRef",
        "driver",
        "runtime",
        "defaultModel",
        "allowedModels",
        "capabilities",
      ],
      ["models"],
    ) ||
    !boundedNonblank(value.providerInstanceRef) ||
    !boundedNonblank(value.driver) ||
    !boundedNonblank(value.runtime) ||
    !boundedNonblank(value.defaultModel) ||
    !Array.isArray(value.allowedModels) ||
    value.allowedModels.length < 1 ||
    value.allowedModels.length > MAX_PROVIDER_CATALOG_MODELS ||
    !isPlainRecord(value.capabilities) ||
    !hasOrderedKeys(
      value.capabilities,
      [
        "threadTurnStart",
        "threadTurnInterrupt",
        "threadSteer",
        "context",
        "diff",
        "plan",
      ],
      [],
    )
  ) {
    return null;
  }
  const models: string[] = [];
  const modelSet = new Set<string>();
  for (const model of value.allowedModels) {
    if (!boundedNonblank(model) || modelSet.has(model)) return null;
    modelSet.add(model);
    models.push(model);
  }
  const capabilities = value.capabilities;
  if (
    typeof capabilities.threadTurnStart !== "boolean" ||
    typeof capabilities.threadTurnInterrupt !== "boolean" ||
    typeof capabilities.threadSteer !== "boolean" ||
    typeof capabilities.context !== "boolean" ||
    typeof capabilities.diff !== "boolean" ||
    typeof capabilities.plan !== "boolean"
  ) {
    return null;
  }
  const described = Object.hasOwn(value, "models")
    ? parseCatalogModels(value.models, models)
    : undefined;
  if (Object.hasOwn(value, "models") && !described) return null;
  return {
    providerInstanceRef: value.providerInstanceRef,
    driver: value.driver,
    runtime: value.runtime,
    defaultModel: value.defaultModel,
    allowedModels: models,
    capabilities: {
      threadTurnStart: capabilities.threadTurnStart,
      threadTurnInterrupt: capabilities.threadTurnInterrupt,
      threadSteer: capabilities.threadSteer,
      context: capabilities.context,
      diff: capabilities.diff,
      plan: capabilities.plan,
    },
    ...(described ? { models: described } : {}),
  };
}

/**
 * Parse `models[]` against the offer it describes.
 *
 * One cursor walks `allowedModels`, so order, membership and uniqueness are
 * all decided by the same pass: a row naming an id the offer does not carry —
 * or carrying one out of order, or twice — has no place to land and the whole
 * catalog is refused. That is the rule that keeps this table a description of
 * the offer rather than a second, quieter offer of its own.
 */
function parseCatalogModels(
  value: unknown,
  allowedModels: readonly string[],
): CodingSessionProviderCatalogModel[] | null {
  // An empty list is never canonical: the producer omits the key.
  if (
    !Array.isArray(value) ||
    value.length < 1 ||
    value.length > allowedModels.length
  ) {
    return null;
  }
  const models: CodingSessionProviderCatalogModel[] = [];
  let cursor = 0;
  for (const raw of value) {
    if (
      !isPlainRecord(raw) ||
      !hasOrderedKeySubsequence(
        raw,
        ["id"],
        ["contextWindow", "family", "vendor", "deprecated"],
      ) ||
      // A row that adds no fact beyond the id is a second way to encode one
      // offer; `allowedModels` already named it.
      Object.keys(raw).length < 2 ||
      !boundedNonblank(raw.id)
    ) {
      return null;
    }
    if (
      Object.hasOwn(raw, "contextWindow") &&
      (!Number.isSafeInteger(raw.contextWindow) ||
        Number(raw.contextWindow) <= 0)
    ) {
      return null;
    }
    if (Object.hasOwn(raw, "family") && !boundedNonblank(raw.family))
      return null;
    if (Object.hasOwn(raw, "vendor") && !boundedNonblank(raw.vendor))
      return null;
    if (
      Object.hasOwn(raw, "deprecated") &&
      typeof raw.deprecated !== "boolean"
    ) {
      return null;
    }
    const offset = allowedModels.indexOf(raw.id, cursor);
    if (offset < 0) return null;
    cursor = offset + 1;
    models.push({
      id: raw.id,
      ...(Object.hasOwn(raw, "contextWindow")
        ? { contextWindow: raw.contextWindow as number }
        : {}),
      ...(Object.hasOwn(raw, "family") ? { family: raw.family as string } : {}),
      ...(Object.hasOwn(raw, "vendor") ? { vendor: raw.vendor as string } : {}),
      ...(Object.hasOwn(raw, "deprecated")
        ? { deprecated: raw.deprecated as boolean }
        : {}),
    });
  }
  return models;
}

function parseExactCatalogTags(tags: string[][]): {
  channelId: string;
  version: string;
  revision: string;
  semanticKey: string;
} | null {
  const names = ["h", "cspc-v", "cspc-revision", "cspc-key"];
  if (tags.length !== names.length) return null;
  const values: string[] = [];
  for (let index = 0; index < names.length; index += 1) {
    const tag = tags[index];
    if (
      !Array.isArray(tag) ||
      tag.length !== 2 ||
      tag[0] !== names[index] ||
      !boundedNonblank(tag[1])
    ) {
      return null;
    }
    values.push(tag[1]);
  }
  return {
    channelId: values[0],
    version: values[1],
    revision: values[2],
    semanticKey: values[3],
  };
}

function modelsAreCanonical(models: string[], defaultModel: string): boolean {
  if (models[0] !== defaultModel) return false;
  return isSorted(models.slice(1), (model) => model);
}

function projectsAreCanonical(
  projects: CodingSessionProviderCatalogProject[],
): boolean {
  for (let index = 1; index < projects.length; index += 1) {
    const previous = projects[index - 1];
    const current = projects[index];
    const byProject = previous.projectRef.localeCompare(current.projectRef);
    if (byProject > 0) return false;
    if (byProject === 0) {
      if (previous.repoRef === null) continue;
      if (
        current.repoRef === null ||
        previous.repoRef.localeCompare(current.repoRef) > 0
      ) {
        return false;
      }
    }
  }
  return true;
}

function isSorted<T>(values: T[], key: (value: T) => string): boolean {
  return values.every(
    (value, index) =>
      index === 0 || key(values[index - 1]).localeCompare(key(value)) <= 0,
  );
}

function boundedNonblank(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    utf8Length(value) <= MAX_PROVIDER_CATALOG_REFERENCE_BYTES
  );
}

function boundedNullable(value: unknown): value is string | null {
  return value === null || boundedNonblank(value);
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

/**
 * Keys present in exactly the declared order, with the optional tail allowed
 * only after the required head. Key order is load-bearing here: the canonical
 * bytes are re-serialized from this parse, so a reordered payload that decoded
 * to the same object would fail the digest check with no explanation.
 */
/**
 * Like {@link hasOrderedKeys}, but the optional keys may be any *subsequence*
 * of the declared tail rather than a prefix of it.
 *
 * A model row carries whichever facts are known, so `{ id, vendor }` is as
 * canonical as `{ id, contextWindow, vendor }`. Relative order still binds:
 * one set of facts has exactly one byte form.
 */
function hasOrderedKeySubsequence(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[],
): boolean {
  const actual = Object.keys(value);
  if (actual.length < required.length) return false;
  for (let index = 0; index < required.length; index += 1) {
    if (actual[index] !== required[index]) return false;
  }
  let cursor = 0;
  for (const key of actual.slice(required.length)) {
    const offset = optional.indexOf(key, cursor);
    if (offset < 0) return false;
    cursor = offset + 1;
  }
  return true;
}

function hasOrderedKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[],
): boolean {
  const actual = Object.keys(value);
  if (
    actual.length < required.length ||
    actual.length > required.length + optional.length
  ) {
    return false;
  }
  const expected = [
    ...required,
    ...optional.slice(0, actual.length - required.length),
  ];
  return actual.every((key, index) => key === expected[index]);
}
