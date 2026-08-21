export const MAX_PROJECTION_DEPTH = 64;

export function isWithinProjectionDepth(value: unknown): boolean {
  const stack: Array<{ value: unknown; depth: number }> = [{ value, depth: 0 }];

  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;
    if (current.depth > MAX_PROJECTION_DEPTH) {
      return false;
    }

    if (Array.isArray(current.value)) {
      for (const item of current.value) {
        stack.push({ value: item, depth: current.depth + 1 });
      }
      continue;
    }

    if (isRecord(current.value)) {
      for (const item of Object.values(current.value)) {
        stack.push({ value: item, depth: current.depth + 1 });
      }
    }
  }

  return true;
}

export function canonicalizeProjectionPayload(value: unknown): string | null {
  try {
    return JSON.stringify(sortForStableStringify(value));
  } catch {
    return null;
  }
}

export function areCanonicalProjectionPayloadsDuplicate(
  left: string | null,
  right: string | null,
): boolean {
  return left !== null && right !== null && left === right;
}

function sortForStableStringify(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortForStableStringify);
  }
  if (!isRecord(value)) {
    return value;
  }
  return Object.fromEntries(
    Object.keys(value)
      .sort()
      .map((key) => [key, sortForStableStringify(value[key])]),
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
