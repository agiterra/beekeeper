/**
 * Editing model for the coding-session provider allowlist.
 *
 * The consumer is fail-closed: a coding-session event is rendered only when its
 * signer appears in `GlobalAgentConfig["allowed-bridge-pubkeys"]`. That list is
 * therefore a trust decision, and this module is the pure part of editing it —
 * row identity, normalization, and the validation that Rust's
 * `validate_global_config` (`managed_agents/global_config/mod.rs`) would
 * otherwise reject at save time.
 *
 * Validating here rather than only in Rust is not duplication for its own sake:
 * the save writes the WHOLE global config, so one malformed key would also
 * block an unrelated model/env edit, with a backend error string that names a
 * wire field the user never sees. These messages name what the user is actually
 * looking at.
 *
 * The wire field name (`allowed-bridge-pubkeys`) is donor-compatible and stays
 * as-is; everything user-visible says "provider", because the signer of a
 * coding session is a provider, not a bridge.
 */
import type { GlobalAgentConfig } from "@/shared/api/types";

/** One persisted allowlist entry, as it appears on the wire. */
export type TrustedProviderKeyEntry =
  GlobalAgentConfig["allowed-bridge-pubkeys"][number];

/** One editable row. `id` is presentation-only and never persisted. */
export type TrustedProviderKeyRow = {
  id: string;
  pubkey: string;
  label: string;
};

/**
 * Label the desktop's own provisioning writes (`session_provider/trust.rs`).
 * Matching on it is how the UI knows which row is this computer's provider.
 */
export const LOCAL_PROVIDER_TRUST_LABEL = "This computer (coding sessions)";

/** Mirrors `MAX_BRIDGE_LABEL_BYTES` in `managed_agents/global_config/mod.rs`. */
export const MAX_TRUST_LABEL_BYTES = 80;

const HEX_PUBKEY_LENGTH = 64;
const LOWERCASE_HEX = /^[0-9a-f]+$/;
const NUL = String.fromCharCode(0);

/**
 * Normalize a pubkey the same way the backend does before persisting.
 *
 * Trust comparison against event pubkeys is byte-for-byte, so a mixed-case
 * entry would silently never match. Accepting it in the input and lowering it
 * here is the difference between "pasted from a hex dump" and "silently
 * untrusted".
 */
export function normalizeTrustPubkey(value: string): string {
  return value.trim().toLowerCase();
}

/** Normalize a label the same way the backend does before persisting. */
export function normalizeTrustLabel(value: string): string {
  return value.trim();
}

/** True when `value` is already a 64-character lowercase hex pubkey. */
export function isLowercaseHexPubkey(value: string): boolean {
  return value.length === HEX_PUBKEY_LENGTH && LOWERCASE_HEX.test(value);
}

/** UTF-8 byte length, which is the unit the Rust label cap is expressed in. */
export function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

/**
 * True when `entry` is the row this computer's provisioning added for itself.
 *
 * Used only to attach the "removing this stops session ingest" warning — never
 * to make the row unremovable. Revoking trust in your own provider is a
 * legitimate thing to want to do.
 */
export function isLocalProviderTrustEntry(entry: { label: string }): boolean {
  return normalizeTrustLabel(entry.label) === LOCAL_PROVIDER_TRUST_LABEL;
}

/**
 * Seed editable rows from persisted entries.
 *
 * Ids are positional and stable for the lifetime of the seeded list; rows added
 * afterwards get a UUID, so the two id spaces can never collide.
 */
export function trustRowsFromEntries(
  entries: readonly TrustedProviderKeyEntry[],
): TrustedProviderKeyRow[] {
  return entries.map((entry, index) => ({
    id: `seeded-${index}`,
    pubkey: entry.pubkey,
    label: entry.label,
  }));
}

/**
 * Project rows back to the persisted shape.
 *
 * Rows with a blank pubkey are dropped rather than rejected — an empty row is
 * how "I clicked Add and changed my mind" looks, and it must not block a save.
 */
export function trustEntriesFromRows(
  rows: readonly TrustedProviderKeyRow[],
): TrustedProviderKeyEntry[] {
  const entries: TrustedProviderKeyEntry[] = [];
  for (const row of rows) {
    const pubkey = normalizeTrustPubkey(row.pubkey);
    if (pubkey.length === 0) continue;
    entries.push({ pubkey, label: normalizeTrustLabel(row.label) });
  }
  return entries;
}

/** Row-keyed validation errors plus the aggregate save gate. */
export type TrustRowValidation = {
  /** Row id -> first error for that row. Empty when everything validates. */
  rowErrors: Record<string, string>;
  /** False when at least one row would be rejected by the backend. */
  isValid: boolean;
};

/**
 * Validate rows against the same rules `validate_global_config` enforces:
 * 64-char lowercase hex after normalization, no duplicates, no NUL in labels,
 * and labels bounded to 80 UTF-8 bytes.
 */
export function validateTrustRows(
  rows: readonly TrustedProviderKeyRow[],
): TrustRowValidation {
  const rowErrors: Record<string, string> = {};
  const seen = new Set<string>();
  for (const row of rows) {
    const pubkey = normalizeTrustPubkey(row.pubkey);
    const label = normalizeTrustLabel(row.label);
    if (pubkey.length === 0) {
      // A blank row is dropped on save; only a stranded name is a mistake worth
      // naming, because that name is what is about to disappear.
      if (label.length > 0) {
        rowErrors[row.id] =
          "Add the provider's public key, or remove this row.";
      }
      continue;
    }
    if (!isLowercaseHexPubkey(pubkey)) {
      rowErrors[row.id] =
        "Public keys are 64 hexadecimal characters (0-9, a-f).";
      continue;
    }
    if (seen.has(pubkey)) {
      rowErrors[row.id] = "This provider key is already trusted.";
      continue;
    }
    seen.add(pubkey);
    if (label.includes(NUL)) {
      rowErrors[row.id] = "Names can't contain NUL characters.";
      continue;
    }
    if (utf8ByteLength(label) > MAX_TRUST_LABEL_BYTES) {
      rowErrors[row.id] =
        `Names are limited to ${MAX_TRUST_LABEL_BYTES} bytes.`;
    }
  }
  return { rowErrors, isValid: Object.keys(rowErrors).length === 0 };
}

/**
 * Content signature used to decide whether an incoming `entries` prop is the
 * same list this editor last emitted. Compared instead of the array reference
 * because a save round-trip returns a fresh, normalized array that is
 * semantically identical to the local rows.
 */
export function trustEntriesSignature(
  entries: readonly TrustedProviderKeyEntry[],
): string {
  return JSON.stringify(
    entries.map((entry) => [
      normalizeTrustPubkey(entry.pubkey),
      normalizeTrustLabel(entry.label),
    ]),
  );
}
