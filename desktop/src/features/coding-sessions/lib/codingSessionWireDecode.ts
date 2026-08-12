/**
 * Shared strict decoders for the 442xx wire payloads.
 *
 * Every one of these is a rejection primitive, not a coercion: an input that
 * is not exactly the declared shape decodes to `null`/`false` rather than to a
 * best guess. The producer signs canonical JSON with a fixed key set, so
 * anything looser than exact-key matching would admit an envelope this client
 * cannot claim to have verified.
 */
import type { CodingSessionCommandTarget } from "./codingSessionCommand";

export const MAX_TARGET_IDENTITY_BYTES = 512;

export function parseBoundedJson(content: unknown, maxBytes: number): unknown {
  if (
    typeof content !== "string" ||
    new TextEncoder().encode(content).byteLength > maxBytes
  ) {
    return null;
  }
  try {
    return JSON.parse(content);
  } catch {
    return null;
  }
}

export function decodeTarget(
  value: unknown,
  maxIdentityBytes: number = MAX_TARGET_IDENTITY_BYTES,
): CodingSessionCommandTarget | null {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, ["driver", "instanceId", "sessionId", "generation"]) ||
    !boundedNonempty(value.driver, maxIdentityBytes) ||
    !boundedNonempty(value.instanceId, maxIdentityBytes) ||
    !boundedNonempty(value.sessionId, maxIdentityBytes) ||
    !Number.isSafeInteger(value.generation) ||
    (value.generation as number) <= 0
  ) {
    return null;
  }
  return Object.freeze({
    driver: value.driver,
    instanceId: value.instanceId,
    sessionId: value.sessionId,
    generation: value.generation as number,
  });
}

export function boundedNonempty(
  value: unknown,
  maxBytes: number,
): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    new TextEncoder().encode(value).byteLength <= maxBytes
  );
}

export function boundedNullable(
  value: unknown,
  maxBytes: number,
): value is string | null {
  return value === null || boundedNonempty(value, maxBytes);
}

export function isPlainRecord(
  value: unknown,
): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function hasExactKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && actual.every((key) => keys.includes(key))
  );
}

export function hasRequiredAndOptionalKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    required.every((key) => Object.hasOwn(value, key)) &&
    actual.every((key) => required.includes(key) || optional.includes(key))
  );
}

/** Depth- and cycle-bounded structural probe for untrusted nested payloads. */
export function isWithinDepth(value: unknown, maxDepth: number): boolean {
  const stack: Array<{ value: unknown; depth: number }> = [{ value, depth: 0 }];
  const seen = new Set<object>();
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;
    if (current.depth > maxDepth) return false;
    if (typeof current.value !== "object" || current.value === null) continue;
    if (seen.has(current.value)) return false;
    seen.add(current.value);
    const nested = Array.isArray(current.value)
      ? current.value
      : Object.values(current.value);
    for (const item of nested) {
      stack.push({ value: item, depth: current.depth + 1 });
    }
  }
  return true;
}

/**
 * Read the event's tags as an exact, ordered list of single-value tags.
 *
 * Order and count are part of the contract: the producer emits these tags in
 * one fixed order, so an event with the right tags in the wrong order, or with
 * an extra tag appended, is not one this client signed off on.
 */
export function parseExactTags(
  tags: string[][],
  names: readonly string[],
): string[] | null {
  if (tags.length !== names.length) return null;
  const values: string[] = [];
  for (let index = 0; index < names.length; index += 1) {
    const tag = tags[index];
    if (
      !Array.isArray(tag) ||
      tag.length !== 2 ||
      tag[0] !== names[index] ||
      typeof tag[1] !== "string"
    ) {
      return null;
    }
    values.push(tag[1]);
  }
  return values;
}

export function hasTagNamed(tags: string[][], name: string): boolean {
  return tags.some((tag) => Array.isArray(tag) && tag[0] === name);
}

export function normalizePubkey(value: unknown): string {
  if (typeof value !== "string") return "";
  const normalized = value.trim().toLowerCase();
  return isExactProviderAuthorityPubkey(normalized) ? normalized : "";
}

export function isExactProviderAuthorityPubkey(
  value: unknown,
): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}
