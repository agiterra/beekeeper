export type CodingSessionLens = "conversation" | "mission";

const STORAGE_PREFIX = "buzz:coding-session-lens:v1";

/**
 * A lens is a user choice, never a lifecycle side effect. Conversation stays
 * the default; team evidence may earn a Mission suggestion but cannot switch
 * an already-open session underneath the reader.
 */
export function readCodingSessionLensPreference(input: {
  communityScope: string;
  channelId: string;
  sessionKey: string;
  storage: Pick<Storage, "getItem">;
}): CodingSessionLens {
  try {
    const value = input.storage.getItem(lensStorageKey(input));
    return value === "mission" ? "mission" : "conversation";
  } catch {
    return "conversation";
  }
}

export function writeCodingSessionLensPreference(input: {
  communityScope: string;
  channelId: string;
  sessionKey: string;
  lens: CodingSessionLens;
  storage: Pick<Storage, "setItem">;
}): boolean {
  try {
    input.storage.setItem(lensStorageKey(input), input.lens);
    return true;
  } catch {
    return false;
  }
}

export function shouldSuggestCodingSessionMission(input: {
  currentLens: CodingSessionLens;
  participantCount: number;
  hasTypedTeamTransaction: boolean;
}): boolean {
  return (
    input.currentLens === "conversation" &&
    (input.participantCount >= 2 || input.hasTypedTeamTransaction)
  );
}

function lensStorageKey(input: {
  communityScope: string;
  channelId: string;
  sessionKey: string;
}): string {
  return `${STORAGE_PREFIX}:${encodeURIComponent(input.communityScope)}:${encodeURIComponent(input.channelId)}:${encodeURIComponent(input.sessionKey)}`;
}
