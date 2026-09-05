/**
 * The goals you have already launched sessions with, newest first.
 *
 * The session composer has had ⌘↑/⌘↓ recall since it had a transcript to
 * read it out of. The create dialog has no transcript — the session it is
 * about does not exist yet — so the list has to be kept as prompts are
 * launched, which is what this module does.
 *
 * Scoped by the same `scopeId` as the draft, for the same reason: a channel
 * id and a project id each belong to exactly one relay, so one community's
 * prompts can never be recalled inside another's dialog. It also means a
 * brand-new project starts with an empty list, which is honest — there is
 * nothing of yours to recall there yet.
 *
 * Pure functions over an injectable storage so the ordering, de-duplication
 * and caps are unit-testable; the hook at the bottom is the only React.
 */

import * as React from "react";

import { matchCodingSessionHistoryKey } from "./codingSessionComposerModel";
import {
  IDLE_PROMPT_RECALL,
  type PromptRecallState,
  stepPromptRecall,
} from "./codingSessionPromptHistory";

const HISTORY_SCHEMA = "buzz-new-coding-session-prompts/v1";
const HISTORY_KEY_PREFIX = "buzz.coding-session-prompts.v1:";

/** Ceiling on remembered prompts; recall is for the recent past, not an archive. */
export const MAX_NEW_CODING_SESSION_PROMPT_HISTORY = 50;

/**
 * Ceiling on the stored record, matching the draft's.
 *
 * A goal may be as long as the field allows, so fifty of them can be large
 * enough to fill a storage quota on their own. Over the cap the oldest
 * prompts are dropped until it fits, rather than the write failing and the
 * whole list going with it.
 */
export const MAX_NEW_CODING_SESSION_PROMPT_HISTORY_BYTES = 512 * 1024;

type StoredNewCodingSessionPromptHistory = {
  schema: typeof HISTORY_SCHEMA;
  scopeId: string;
  /** Newest first, which is the order recall walks. */
  prompts: string[];
};

type HistoryStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export function newCodingSessionPromptHistoryKey(scopeId: string): string {
  return `${HISTORY_KEY_PREFIX}${scopeId}`;
}

function resolveDefaultHistoryStorage(): HistoryStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    // Storage can be denied outright (private mode, hardened webview). Recall
    // is then simply empty; nothing else about the dialog changes.
    return undefined;
  }
}

/**
 * Read one scope's prompts, newest first.
 *
 * Fails closed to an empty list on anything unexpected — a record written by
 * a newer build, a scope binding that does not match, a value that is not a
 * list of strings. An empty recall is a feature that does nothing; a
 * half-decoded one hands back text nobody wrote.
 */
export function readNewCodingSessionPromptHistory(
  scopeId: string,
  storage?: HistoryStorage,
): string[] {
  const targetStorage = storage ?? resolveDefaultHistoryStorage();
  if (!targetStorage) return [];
  try {
    const stored = targetStorage.getItem(
      newCodingSessionPromptHistoryKey(scopeId),
    );
    if (stored === null) return [];
    const parsed: unknown = JSON.parse(stored);
    if (
      typeof parsed !== "object" ||
      parsed === null ||
      Array.isArray(parsed) ||
      Object.keys(parsed).join(",") !== "schema,scopeId,prompts" ||
      !("schema" in parsed) ||
      parsed.schema !== HISTORY_SCHEMA ||
      !("scopeId" in parsed) ||
      parsed.scopeId !== scopeId ||
      !("prompts" in parsed) ||
      !Array.isArray(parsed.prompts) ||
      !parsed.prompts.every((entry) => typeof entry === "string")
    ) {
      return [];
    }
    return (parsed.prompts as string[]).slice(
      0,
      MAX_NEW_CODING_SESSION_PROMPT_HISTORY,
    );
  } catch {
    return [];
  }
}

function encodedBytes(value: string): number {
  return new TextEncoder().encode(value).byteLength;
}

/**
 * Add one launched prompt to the front, and return the list as it now stands.
 *
 * De-duplicated outright, not only against the entry before it: the composer
 * collapses consecutive repeats because it reads a transcript it does not
 * own, whereas this list is ours to keep. Launching the same goal twice
 * should move it back to the top, not occupy two rungs of the ladder the
 * person is climbing.
 *
 * Returns the new list even when the write fails, so the dialog's in-memory
 * recall still works in a session where storage is denied.
 */
export function rememberNewCodingSessionPrompt(
  scopeId: string,
  text: string,
  storage?: HistoryStorage,
): string[] {
  const trimmed = text.trim();
  const targetStorage = storage ?? resolveDefaultHistoryStorage();
  const current = readNewCodingSessionPromptHistory(scopeId, targetStorage);
  if (trimmed.length === 0) return current;

  const prompts = [
    trimmed,
    ...current.filter((entry) => entry !== trimmed),
  ].slice(0, MAX_NEW_CODING_SESSION_PROMPT_HISTORY);

  if (!targetStorage) return prompts;

  // Drop from the oldest end until the record fits. The newest prompt is the
  // one just launched, so it is the last thing that should be sacrificed —
  // and a single prompt larger than the whole budget stores nothing rather
  // than throwing the existing list away for it.
  const kept = [...prompts];
  while (kept.length > 0) {
    const record: StoredNewCodingSessionPromptHistory = {
      schema: HISTORY_SCHEMA,
      scopeId,
      prompts: kept,
    };
    const serialized = JSON.stringify(record);
    if (
      encodedBytes(serialized) <= MAX_NEW_CODING_SESSION_PROMPT_HISTORY_BYTES
    ) {
      try {
        targetStorage.setItem(
          newCodingSessionPromptHistoryKey(scopeId),
          serialized,
        );
      } catch {
        // A full or read-only store is not worth failing a launch over.
      }
      return prompts;
    }
    kept.pop();
  }
  return prompts;
}

/** Forget one scope's prompts entirely. */
export function clearNewCodingSessionPromptHistory(
  scopeId: string,
  storage?: HistoryStorage,
): void {
  const targetStorage = storage ?? resolveDefaultHistoryStorage();
  if (!targetStorage) return;
  try {
    targetStorage.removeItem(newCodingSessionPromptHistoryKey(scopeId));
  } catch {
    // Nothing to report: the list is advisory either way.
  }
}

/**
 * ⌘↑/⌘↓ recall for the create dialog's goal field.
 *
 * The keystroke, the stepping and the "not recalling any more" rule are the
 * composer's — `matchCodingSessionHistoryKey` and `stepPromptRecall` are
 * shared, so the two surfaces cannot drift into behaving differently under
 * the same shortcut.
 */
export function useNewCodingSessionPromptRecall({
  scopeId,
  text,
  setText,
}: {
  scopeId: string;
  text: string;
  setText: (next: string) => void;
}): {
  history: readonly string[];
  onKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  remember: (prompt: string) => void;
} {
  const [history, setHistory] = React.useState<string[]>(() =>
    readNewCodingSessionPromptHistory(scopeId),
  );
  React.useEffect(() => {
    setHistory(readNewCodingSessionPromptHistory(scopeId));
  }, [scopeId]);

  const [recall, setRecall] =
    React.useState<PromptRecallState>(IDLE_PROMPT_RECALL);
  const recalledTextRef = React.useRef<string | null>(null);

  React.useEffect(() => {
    // Any edit of the person's own ends recall: the cursor's place in the
    // list only means something while the field still holds what recall put
    // there. Watching the text rather than the keystroke catches a paste, an
    // IME commit and an undo, none of which arrive as a plain keydown.
    if (recalledTextRef.current === null) return;
    if (text === recalledTextRef.current) return;
    recalledTextRef.current = null;
    setRecall(IDLE_PROMPT_RECALL);
  }, [text]);

  const onKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      const direction = matchCodingSessionHistoryKey(event);
      if (!direction) return;
      // Claimed whether or not there is anything to recall: on macOS an
      // unclaimed ⌘↑ jumps the caret to the top of the textarea, which is a
      // different thing happening under the shortcut the person pressed.
      event.preventDefault();
      const step = stepPromptRecall(direction, recall, history, text);
      setRecall(step.state);
      if (step.text === null) return;
      recalledTextRef.current = step.state.cursor === -1 ? null : step.text;
      setText(step.text);
    },
    [history, recall, setText, text],
  );

  const remember = React.useCallback(
    (prompt: string) => {
      setHistory(rememberNewCodingSessionPrompt(scopeId, prompt));
      setRecall(IDLE_PROMPT_RECALL);
      recalledTextRef.current = null;
    },
    [scopeId],
  );

  return { history, onKeyDown, remember };
}
