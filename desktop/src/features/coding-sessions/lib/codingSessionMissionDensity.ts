export type CodingSessionMissionDensity = "brief" | "live" | "trace";

export type CodingSessionMissionDensityCoordinates = {
  communityScope: string;
  channelId: string;
  sessionKey: string;
};

const STORAGE_PREFIX = "buzz:coding-session-mission-density:v1";

function storageKey(
  coordinates: CodingSessionMissionDensityCoordinates,
): string {
  return [
    STORAGE_PREFIX,
    encodeURIComponent(coordinates.communityScope),
    encodeURIComponent(coordinates.channelId),
    encodeURIComponent(coordinates.sessionKey),
  ].join(":");
}

/** Read a presentation-only Mission density. Missing/invalid state is Live. */
export function readCodingSessionMissionDensity(input: {
  coordinates: CodingSessionMissionDensityCoordinates;
  storage: Pick<Storage, "getItem">;
}): CodingSessionMissionDensity {
  const value = input.storage.getItem(storageKey(input.coordinates));
  return value === "brief" || value === "trace" ? value : "live";
}

/** Persist a density for the session, independent of any execution target. */
export function writeCodingSessionMissionDensity(input: {
  coordinates: CodingSessionMissionDensityCoordinates;
  density: CodingSessionMissionDensity;
  storage: Pick<Storage, "setItem">;
}): void {
  input.storage.setItem(storageKey(input.coordinates), input.density);
}
