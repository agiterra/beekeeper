import * as React from "react";

/**
 * The compose draft for a not-yet-created session.
 *
 * Scoped by whatever owns the create screen — a channel id for a standalone
 * session, a project route id under the glue patch — so two create screens
 * never overwrite each other's text.
 */
const DRAFT_SCHEMA = "buzz-new-coding-session-draft/v1";
const DRAFT_KEY_PREFIX = "buzz.coding-session-draft.v1:";
export const MAX_NEW_CODING_SESSION_DRAFT_STORAGE_BYTES = 512 * 1024;

type StoredNewCodingSessionDraft = {
  schema: typeof DRAFT_SCHEMA;
  scopeId: string;
  text: string;
};

type DraftStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export type NewCodingSessionDraftPersistence =
  | { state: "persisted"; message: null }
  | { state: "over-cap"; message: string }
  | { state: "unavailable"; message: string };

export type NewCodingSessionDraftRead = {
  text: string;
  persistence: NewCodingSessionDraftPersistence;
};

const PERSISTED: NewCodingSessionDraftPersistence = {
  state: "persisted",
  message: null,
};

export function newCodingSessionDraftStorageKey(scopeId: string): string {
  return `${DRAFT_KEY_PREFIX}${scopeId}`;
}

export function readNewCodingSessionDraft(
  scopeId: string,
  storage?: DraftStorage,
): string {
  return readNewCodingSessionDraftState(scopeId, storage).text;
}

export function readNewCodingSessionDraftState(
  scopeId: string,
  storage?: DraftStorage,
): NewCodingSessionDraftRead {
  const targetStorage = storage ?? resolveDefaultDraftStorage();
  if (!targetStorage) {
    return {
      text: "",
      persistence: unavailableDraftPersistence(),
    };
  }
  try {
    const stored = targetStorage.getItem(
      newCodingSessionDraftStorageKey(scopeId),
    );
    if (stored === null) return { text: "", persistence: PERSISTED };
    const parsed: unknown = JSON.parse(stored);
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed) ||
      Object.keys(parsed).join(",") !== "schema,scopeId,text" ||
      !("schema" in parsed) ||
      parsed.schema !== DRAFT_SCHEMA ||
      !("scopeId" in parsed) ||
      parsed.scopeId !== scopeId ||
      !("text" in parsed) ||
      typeof parsed.text !== "string" ||
      new TextEncoder().encode(parsed.text).byteLength >
        MAX_NEW_CODING_SESSION_DRAFT_STORAGE_BYTES
    ) {
      return { text: "", persistence: PERSISTED };
    }
    return { text: parsed.text, persistence: PERSISTED };
  } catch {
    return {
      text: "",
      persistence: unavailableDraftPersistence(),
    };
  }
}

export function writeNewCodingSessionDraft(
  scopeId: string,
  text: string,
  storage?: DraftStorage,
): NewCodingSessionDraftPersistence {
  const targetStorage = storage ?? resolveDefaultDraftStorage();
  if (!targetStorage) return unavailableDraftPersistence();
  if (
    new TextEncoder().encode(text).byteLength >
    MAX_NEW_CODING_SESSION_DRAFT_STORAGE_BYTES
  ) {
    return {
      state: "over-cap",
      message:
        "This draft is too large for local persistence and will not survive an app restart.",
    };
  }
  const record: StoredNewCodingSessionDraft = {
    schema: DRAFT_SCHEMA,
    scopeId,
    text,
  };
  try {
    targetStorage.setItem(
      newCodingSessionDraftStorageKey(scopeId),
      JSON.stringify(record),
    );
    return PERSISTED;
  } catch {
    return unavailableDraftPersistence();
  }
}

export function clearNewCodingSessionDraft(
  scopeId: string,
  storage?: DraftStorage,
): NewCodingSessionDraftPersistence {
  const targetStorage = storage ?? resolveDefaultDraftStorage();
  if (!targetStorage) return unavailableDraftPersistence();
  try {
    targetStorage.removeItem(newCodingSessionDraftStorageKey(scopeId));
    return PERSISTED;
  } catch {
    return unavailableDraftPersistence();
  }
}

function resolveDefaultDraftStorage(): DraftStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

export function useNewCodingSessionDraft(scopeId: string): {
  text: string;
  setText: (text: string) => void;
  clear: () => void;
  persistence: NewCodingSessionDraftPersistence;
} {
  const [draft, setDraft] = React.useState(() =>
    readNewCodingSessionDraftState(scopeId),
  );
  React.useEffect(() => {
    setDraft(readNewCodingSessionDraftState(scopeId));
  }, [scopeId]);
  const setText = React.useCallback(
    (next: string) => {
      setDraft({
        text: next,
        persistence: writeNewCodingSessionDraft(scopeId, next),
      });
    },
    [scopeId],
  );
  const clear = React.useCallback(() => {
    setDraft({
      text: "",
      persistence: clearNewCodingSessionDraft(scopeId),
    });
  }, [scopeId]);
  return {
    text: draft.text,
    setText,
    clear,
    persistence: draft.persistence,
  };
}

function unavailableDraftPersistence(): NewCodingSessionDraftPersistence {
  return {
    state: "unavailable",
    message:
      "Draft storage is unavailable. This text is only in memory and will not survive an app restart.",
  };
}
