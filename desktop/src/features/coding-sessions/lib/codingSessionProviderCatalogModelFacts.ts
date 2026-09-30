/**
 * The runtime-reported tail of a catalog model row (NIP-CSPC § Per-model rows).
 *
 * Split out of `codingSessionProviderCatalog.ts` to keep that parser under the
 * file-size ceiling; this is the same strict reader, one concern deeper.
 */
/** NIP-CSPC bounds on a model row's runtime-reported facts. */
export const MAX_PROVIDER_CATALOG_MODEL_NAME_BYTES = 128;
export const MAX_PROVIDER_CATALOG_MODEL_DESCRIPTION_BYTES = 512;
export const MAX_PROVIDER_CATALOG_MODEL_EFFORTS = 16;
export const MAX_PROVIDER_CATALOG_EFFORT_BYTES = 32;
/** `rank` is a `u32` on the Rust side; a larger number is not canonical. */
export const MAX_PROVIDER_CATALOG_MODEL_RANK = 0xffff_ffff;

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/**
 * The runtime's own words about a model — name, description, efforts, fast
 * mode, rank — checked exactly as `buzz-core`'s `check_models` checks them.
 * An empty `efforts` or a `false` fast mode is refused rather than tolerated:
 * the producer omits both, so either would be a second byte form of one fact.
 */
export function runtimeModelFactsAreCanonical(
  raw: Record<string, unknown>,
): boolean {
  const bounded = (value: unknown, max: number) =>
    typeof value === "string" &&
    value.trim().length > 0 &&
    utf8Length(value) <= max;
  if (
    Object.hasOwn(raw, "name") &&
    !bounded(raw.name, MAX_PROVIDER_CATALOG_MODEL_NAME_BYTES)
  ) {
    return false;
  }
  if (
    Object.hasOwn(raw, "description") &&
    !bounded(raw.description, MAX_PROVIDER_CATALOG_MODEL_DESCRIPTION_BYTES)
  ) {
    return false;
  }
  if (Object.hasOwn(raw, "efforts")) {
    const efforts = raw.efforts;
    if (
      !Array.isArray(efforts) ||
      efforts.length < 1 ||
      efforts.length > MAX_PROVIDER_CATALOG_MODEL_EFFORTS ||
      new Set(efforts).size !== efforts.length ||
      !efforts.every((effort) =>
        bounded(effort, MAX_PROVIDER_CATALOG_EFFORT_BYTES),
      )
    ) {
      return false;
    }
  }
  if (Object.hasOwn(raw, "fastMode") && raw.fastMode !== true) return false;
  if (
    Object.hasOwn(raw, "rank") &&
    (!Number.isSafeInteger(raw.rank) ||
      Number(raw.rank) < 0 ||
      Number(raw.rank) > MAX_PROVIDER_CATALOG_MODEL_RANK)
  ) {
    return false;
  }
  return true;
}
