/**
 * Fractional ranks for ordered, concurrently edited lists — the TypeScript
 * twin of `crates/beekeeper-core/src/fractional_rank.rs`, pinned to the same
 * vectors in `conformance/project-todo-fold/fixtures/rank-vectors.json`.
 *
 * A rank is a base-62 string compared bytewise: an integer part whose first
 * letter encodes its length (`a`+1 digit … `z`+26 for non-negative, `Z`+1 …
 * `A`+26 for negative), then an optional fraction that never ends in `0`.
 * Appending increments the integer (`a0`, `a1`, …); inserting between two
 * neighbours takes the string midpoint of their fractions. The midpoint
 * digit between `a` and `b` is `⌈(a + b) / 2⌉` in integer arithmetic so every
 * implementation agrees bit for bit.
 *
 * This module has no imports: it is loaded by the conformance binder under
 * plain `node --test` with type stripping only.
 */

export const RANK_DIGITS =
  "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/** Maximum rank length; deeper than this the client must re-rank. */
export const MAX_RANK_LEN = 64;

/** The first rank of an empty list. */
export const FIRST_RANK = "a0";

const SMALLEST_INTEGER = `A${"0".repeat(26)}`;

function digitValue(ch: string): number {
  return RANK_DIGITS.indexOf(ch);
}

function integerLength(head: string): number | null {
  if (head >= "a" && head <= "z") {
    return head.charCodeAt(0) - "a".charCodeAt(0) + 2;
  }
  if (head >= "A" && head <= "Z") {
    return "Z".charCodeAt(0) - head.charCodeAt(0) + 2;
  }
  return null;
}

function splitRank(rank: string): [string, string] | string {
  if (rank.length === 0) return "rank must not be empty";
  const len = integerLength(rank[0] ?? "");
  if (len === null) {
    return `rank must start with a letter (got ${JSON.stringify(rank[0])})`;
  }
  if (rank.length < len) {
    return "rank integer part is shorter than its head declares";
  }
  return [rank.slice(0, len), rank.slice(len)];
}

/** Why `rank` is malformed, or `null` when it is well formed. */
export function rankError(rank: string): string | null {
  if (rank.length > MAX_RANK_LEN) {
    return `rank exceeds ${MAX_RANK_LEN} characters`;
  }
  const split = splitRank(rank);
  if (typeof split === "string") return split;
  const [integer, fraction] = split;
  for (const ch of integer.slice(1) + fraction) {
    if (digitValue(ch) < 0) {
      return `rank contains a character outside 0-9A-Za-z (${JSON.stringify(ch)})`;
    }
  }
  if (fraction.endsWith("0")) return "rank fraction must not end in 0";
  if (integer === SMALLEST_INTEGER && fraction === "") {
    return "rank is below the smallest representable order";
  }
  return null;
}

/** `true` when `rank` passes {@link rankError}. */
export function isValidRank(rank: string): boolean {
  return rankError(rank) === null;
}

function midpoint(a: string, b: string | null): string {
  if (b !== null) {
    let n = 0;
    while (n < b.length && (a[n] ?? "0") === b[n]) n++;
    if (n > 0) return b.slice(0, n) + midpoint(a.slice(n), b.slice(n));
  }
  const digitA = a.length > 0 ? digitValue(a[0] ?? "") : 0;
  const digitB =
    b !== null && b.length > 0 ? digitValue(b[0] ?? "") : RANK_DIGITS.length;
  if (digitB - digitA > 1) {
    const mid = Math.floor((digitA + digitB + 1) / 2);
    return RANK_DIGITS[mid] ?? "";
  }
  if (b !== null && b.length > 1) return b[0] ?? "";
  return (RANK_DIGITS[digitA] ?? "") + midpoint(a.slice(1), null);
}

function incrementInteger(x: string): string | null {
  const head = x[0] ?? "";
  const digits = x.slice(1).split("");
  let carry = true;
  for (let i = digits.length - 1; carry && i >= 0; i--) {
    const v = digitValue(digits[i] ?? "") + 1;
    if (v === RANK_DIGITS.length) {
      digits[i] = "0";
    } else {
      digits[i] = RANK_DIGITS[v] ?? "";
      carry = false;
    }
  }
  if (!carry) return head + digits.join("");
  if (head === "Z") return "a0";
  if (head === "z") return null;
  const next = String.fromCharCode(head.charCodeAt(0) + 1);
  if (next > "a") digits.push("0");
  else digits.pop();
  return next + digits.join("");
}

function decrementInteger(x: string): string | null {
  const head = x[0] ?? "";
  const digits = x.slice(1).split("");
  const last = RANK_DIGITS[RANK_DIGITS.length - 1] ?? "";
  let borrow = true;
  for (let i = digits.length - 1; borrow && i >= 0; i--) {
    const v = digitValue(digits[i] ?? "") - 1;
    if (v < 0) {
      digits[i] = last;
    } else {
      digits[i] = RANK_DIGITS[v] ?? "";
      borrow = false;
    }
  }
  if (!borrow) return head + digits.join("");
  if (head === "a") return `Z${last}`;
  if (head === "A") return null;
  const prev = String.fromCharCode(head.charCodeAt(0) - 1);
  if (prev < "Z") digits.push(last);
  else digits.pop();
  return prev + digits.join("");
}

/**
 * Mint a rank strictly between `after` and `before` (`null` on either side
 * means unbounded). Throws when a bound is malformed or the bounds are not
 * ordered — a caller working from a stale fold is told, not handed a rank
 * that sorts somewhere else.
 */
export function rankBetween(
  after: string | null,
  before: string | null,
): string {
  if (after !== null) {
    const err = rankError(after);
    if (err) throw new Error(err);
  }
  if (before !== null) {
    const err = rankError(before);
    if (err) throw new Error(err);
  }
  if (after !== null && before !== null && after >= before) {
    throw new Error(
      `rank bounds are not ordered (${JSON.stringify(after)} >= ${JSON.stringify(before)})`,
    );
  }
  let minted: string;
  if (after === null && before === null) {
    minted = FIRST_RANK;
  } else if (after === null && before !== null) {
    const [ib, fb] = splitRank(before) as [string, string];
    if (ib === SMALLEST_INTEGER) {
      minted = ib + midpoint("", fb);
    } else if (fb.length > 0) {
      minted = ib;
    } else {
      const dec = decrementInteger(ib);
      if (dec === null)
        throw new Error("rank cannot go below the smallest order");
      minted = dec;
    }
  } else if (after !== null && before === null) {
    const [ia, fa] = splitRank(after) as [string, string];
    const inc = incrementInteger(ia);
    minted = inc === null ? ia + midpoint(fa, null) : inc;
  } else {
    const [ia, fa] = splitRank(after as string) as [string, string];
    const [ib, fb] = splitRank(before as string) as [string, string];
    if (ia === ib) {
      minted = ia + midpoint(fa, fb);
    } else {
      const inc = incrementInteger(ia);
      if (inc === null)
        throw new Error("rank cannot go above the largest order");
      minted = inc < (before as string) ? inc : ia + midpoint(fa, null);
    }
  }
  if (minted.length > MAX_RANK_LEN) {
    throw new Error(
      `rank between ${JSON.stringify(after)} and ${JSON.stringify(before)} would exceed ${MAX_RANK_LEN} characters; re-rank the list`,
    );
  }
  return minted;
}
