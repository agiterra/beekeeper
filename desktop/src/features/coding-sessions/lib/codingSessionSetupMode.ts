/**
 * Which way this computer last set up a new coding session: Solo or Team.
 *
 * The founded page opens in the mode this computer used last (Andy,
 * 2026-09-10) — first ever, Solo. A preference, not a fact about any session:
 * it is keyed to the computer, not to a `sessionRef`, and it says nothing
 * about how a session already on the wire runs. Same storage discipline as
 * the drafts: unavailable storage reads as the default and writes as a no-op.
 */
export type CodingSessionSetupMode = "solo" | "team";

/** The `localStorage` key the remembered mode lives under. */
export const CODING_SESSION_SETUP_MODE_KEY =
  "buzz.coding-session-setup-mode.v1";

type ModeStorage = Pick<Storage, "getItem" | "setItem">;

/** The type guard, so a foreign value in storage reads as the default. */
export function isCodingSessionSetupMode(
  value: unknown,
): value is CodingSessionSetupMode {
  return value === "solo" || value === "team";
}

/** The mode this computer used last; `"solo"` when none is readable. */
export function readCodingSessionSetupMode(
  storage?: ModeStorage,
): CodingSessionSetupMode {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return "solo";
  try {
    const stored = targetStorage.getItem(CODING_SESSION_SETUP_MODE_KEY);
    return isCodingSessionSetupMode(stored) ? stored : "solo";
  } catch {
    return "solo";
  }
}

/** Remember the mode. Returns false when storage refused it. */
export function writeCodingSessionSetupMode(
  mode: CodingSessionSetupMode,
  storage?: ModeStorage,
): boolean {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return false;
  try {
    targetStorage.setItem(CODING_SESSION_SETUP_MODE_KEY, mode);
    return true;
  } catch {
    return false;
  }
}

function resolveDefaultStorage(): ModeStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}
