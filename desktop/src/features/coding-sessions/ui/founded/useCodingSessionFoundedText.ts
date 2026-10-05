import * as React from "react";

import {
  codingSessionGoalOverflow,
  publishCodingSessionGoal,
} from "../../lib/codingSessionGoal";
import type { CodingSessionLaunchGoalReader } from "../../lib/codingSessionLaunchForm";
import {
  buildCodingSessionNameEvent,
  publishCodingSessionName,
} from "../../lib/codingSessionName";
import { useNewCodingSessionDraft } from "../../lib/newCodingSessionDraft";
import { useNewCodingSessionPromptRecall } from "../../lib/newCodingSessionPromptHistory";
import { codingSessionAutoGoalSentence } from "../../lib/codingSessionAutoGoal";
import { codingSessionBlankNameSentence } from "../../lib/codingSessionBlankNameSentence";
import { useNewCodingSessionTitleSuggestion } from "../useNewCodingSessionTitleSuggestion";
import type { CodingSessionFoundedGoal } from "./CodingSessionFoundedWorkspace";

/** The two relay writes the fields make; injected in tests. */
export type CodingSessionFoundedTextDeps = {
  publishName: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
  publishGoal: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
};

const DEFAULT_TEXT_DEPS: CodingSessionFoundedTextDeps = {
  publishName: (input) => publishCodingSessionName(input),
  publishGoal: (input) => publishCodingSessionGoal(input),
};

/** A record this page published, and the wire as it stood at that moment. */
type LastPublished = { content: string; wireBefore: string | null };

/** What a commit — or a flush — came to. */
export type CodingSessionFoundedTextOutcome =
  | { ok: true }
  | { ok: false; field: "name" | "prompt"; reason: string };

/** The scope the prompt draft and its ⌘↑/⌘↓ history live under. */
export function codingSessionFoundedTextScope(sessionRef: string): string {
  return `founded:${sessionRef}`;
}

/** The founded page's remembered Name text, host-local like the draft. */
export type CodingSessionFoundedNameDraft = {
  name: string | null;
  setName: (name: string | null) => void;
};

/**
 * The commit predicate, in one place: a field publishes only when it holds
 * something and that something differs from the wire.
 */
export function codingSessionFoundedTextDirty(
  value: string,
  wire: string | null,
): boolean {
  const trimmed = value.trim();
  return trimmed.length > 0 && trimmed !== (wire ?? "");
}

/**
 * Name and Initial prompt on the founded page, for one founder-owned
 * `(channelId, sessionRef)`.
 *
 * The honesty core of the page (plan, shared contract 6):
 *
 * - A field never publishes while its reader is unsettled. The name waits
 *   for `namesResolved`; the prompt waits for the goal reader. A 44229 or
 *   44227 signed over a record this computer has not read yet would silently
 *   replace what another device published.
 * - A field publishes only when `trimmed.length > 0 && trimmed !== wire`.
 *   A blank prompt is the `goal` attempt blocker, never a call into the goal
 *   builder (which throws on empty). A name the builder refuses is a field
 *   error under the input, not a relay refusal.
 * - The title suggestion is suppressed until names have settled and whenever
 *   a wire name exists — otherwise: type a prompt → suggestion writes the
 *   draft → blur → a 44229 overwrites a name set from the phone.
 * - A record this page just published counts as the wire until the wire
 *   echoes it (`lastPublished*`). Otherwise the field blanks or reverts to
 *   the old record for a beat, the namer fills the blank, and the next
 *   blur publishes a generated name over the typed one; or a Start sends
 *   the old goal as the first turn.
 * - `flush()` publishes name then goal, only the dirty ones, and stops at the
 *   first refusal, so Start never creates against a prompt the relay refused.
 */
export function useCodingSessionFoundedText(input: {
  channelId: string;
  sessionRef: string;
  /** The founder-keyed 44229, or null when none is on the wire. */
  wireName: string | null;
  /** Whether `useCodingSessionNames` has settled once for this scope. */
  namesResolved: boolean;
  /** A settled failed or partial read cannot authorize replacing a name. */
  nameReadError?: string | null;
  /** Explicitly retry the name reader without losing the local draft. */
  refreshNames?: () => void;
  /** The goal as the page could read it — `available` is the wire goal. */
  goal: CodingSessionFoundedGoal;
  /** The founded draft's Name text and its writer. */
  nameDraft: CodingSessionFoundedNameDraft;
  deps?: CodingSessionFoundedTextDeps;
}) {
  const {
    channelId,
    sessionRef,
    wireName,
    namesResolved,
    nameReadError = null,
    refreshNames,
    goal,
    nameDraft,
    deps = DEFAULT_TEXT_DEPS,
  } = input;
  const scopeId = codingSessionFoundedTextScope(sessionRef);
  const wireGoal = goal.kind === "available" ? goal.text : null;
  const goalReader: CodingSessionLaunchGoalReader =
    goal.kind === "unresolved"
      ? "unresolved"
      : goal.kind === "errored"
        ? "errored"
        : "resolved";

  // ── Initial prompt ──────────────────────────────────────────────────────
  const draft = useNewCodingSessionDraft(scopeId);
  // What this page last published, standing in for the wire until the wire
  // moves — to the echo, or to a newer record from another device.
  const [lastPublishedPrompt, setLastPublishedPrompt] =
    React.useState<LastPublished | null>(null);
  React.useEffect(() => {
    if (
      lastPublishedPrompt !== null &&
      wireGoal !== lastPublishedPrompt.wireBefore
    ) {
      setLastPublishedPrompt(null);
    }
  }, [lastPublishedPrompt, wireGoal]);
  const knownGoal = lastPublishedPrompt?.content ?? wireGoal;
  // The field shows the draft when one exists, else the wire goal.
  const prompt = draft.text.length > 0 ? draft.text : (knownGoal ?? "");
  const [promptError, setPromptError] = React.useState<string | null>(null);
  const [promptPublishing, setPromptPublishing] = React.useState(false);
  const [promptAttempted, setPromptAttempted] = React.useState(false);
  const promptDirty = codingSessionFoundedTextDirty(prompt, knownGoal);
  const goalOverflow = codingSessionGoalOverflow(prompt);
  const setPrompt = React.useCallback(
    (next: string) => {
      setPromptError(null);
      setPromptAttempted(false);
      draft.setText(next);
    },
    [draft],
  );
  const recall = useNewCodingSessionPromptRecall({
    scopeId,
    text: prompt,
    setText: setPrompt,
  });

  // ── Name ────────────────────────────────────────────────────────────────
  const [lastPublishedName, setLastPublishedName] =
    React.useState<LastPublished | null>(null);
  React.useEffect(() => {
    if (
      lastPublishedName !== null &&
      wireName !== lastPublishedName.wireBefore
    ) {
      setLastPublishedName(null);
    }
  }, [lastPublishedName, wireName]);
  const knownName = lastPublishedName?.content ?? wireName;
  const name = nameDraft.name ?? knownName ?? "";
  const [nameError, setNameError] = React.useState<string | null>(null);
  const [namePublishing, setNamePublishing] = React.useState(false);
  const nameDirty = codingSessionFoundedTextDirty(name, knownName);
  const writeName = React.useCallback(
    (next: string) => {
      setNameError(null);
      nameDraft.setName(next);
    },
    [nameDraft],
  );
  // The suggestion may fill a blank field, and a generated name may replace
  // a generated one — never a typed one. It is fed a blank message until
  // names have settled and whenever a wire name exists, so it can never fire
  // over either: the namer refuses a blank message, on its tick and on
  // `requestNow` alike.
  const suggestionAllowed =
    namesResolved && !nameReadError && knownName === null;
  const naming = useNewCodingSessionTitleSuggestion({
    firstMessage: suggestionAllowed ? prompt : "",
    title: name,
    setTitle: writeName,
  });
  const setName = naming.setTitleByHand;

  const commitNameNow =
    React.useCallback(async (): Promise<CodingSessionFoundedTextOutcome> => {
      if (!codingSessionFoundedTextDirty(name, knownName)) {
        // A blank field over a published name publishes nothing (a name cannot
        // be unset on the wire) and shows the wire name again.
        if (name.trim().length === 0 && knownName !== null)
          nameDraft.setName(null);
        return { ok: true };
      }
      if (!namesResolved || nameReadError) {
        return {
          ok: false,
          field: "name",
          reason: nameReadError
            ? `Name not saved: ${nameReadError} Your draft is preserved; retry the name read.`
            : "Name not saved: the existing name is still being read. Your draft is preserved.",
        };
      }
      const content = name.trim();
      try {
        // The builder's own refusal (empty, newline, over 256 B) is a field
        // error; it never becomes a relay round trip.
        buildCodingSessionNameEvent({ channelId, content, sessionRef });
      } catch (error) {
        const reason =
          error instanceof Error ? error.message : "The name was refused.";
        setNameError(reason);
        return { ok: false, field: "name", reason };
      }
      setNamePublishing(true);
      try {
        await deps.publishName({ channelId, content, sessionRef });
        setNameError(null);
        // The wire now carries it; the draft would only shadow the record.
        // Until the wire echoes, the published text stands in for it.
        setLastPublishedName({ content, wireBefore: wireName });
        nameDraft.setName(null);
        return { ok: true };
      } catch (error) {
        const reason =
          error instanceof Error
            ? error.message
            : "Failed to rename the session.";
        setNameError(reason);
        return { ok: false, field: "name", reason };
      } finally {
        setNamePublishing(false);
      }
    }, [
      channelId,
      deps,
      knownName,
      name,
      nameDraft,
      namesResolved,
      nameReadError,
      sessionRef,
      wireName,
    ]);

  const commitPromptNow =
    React.useCallback(async (): Promise<CodingSessionFoundedTextOutcome> => {
      if (goalReader !== "resolved") return { ok: true };
      if (!codingSessionFoundedTextDirty(prompt, knownGoal))
        return { ok: true };
      const overflow = codingSessionGoalOverflow(prompt);
      if (overflow) {
        // Inline blocker already; said here too so a flush reports it.
        const reason = `This goal is ${overflow.bytes.toLocaleString()} UTF-8 bytes; the signed record holds ${overflow.cap.toLocaleString()}.`;
        setPromptError(reason);
        return { ok: false, field: "prompt", reason };
      }
      setPromptPublishing(true);
      try {
        await deps.publishGoal({
          channelId,
          content: prompt.trim(),
          sessionRef,
        });
        setPromptError(null);
        setLastPublishedPrompt({
          content: prompt.trim(),
          wireBefore: wireGoal,
        });
        draft.clear();
        return { ok: true };
      } catch (error) {
        const reason =
          error instanceof Error
            ? error.message
            : "Failed to update the session goal.";
        setPromptError(reason);
        return { ok: false, field: "prompt", reason };
      } finally {
        setPromptPublishing(false);
      }
    }, [
      channelId,
      deps,
      draft,
      goalReader,
      knownGoal,
      prompt,
      sessionRef,
      wireGoal,
    ]);

  // One publish per field at a time. Pressing Start right after typing the
  // prompt fires the field's blur first, so its publish is already in flight
  // when Start's flush runs; the flush must ride that publish, not start a
  // second 44227 beside it (Lane C finding 4, 2026-09-10). Refs, not state:
  // the same render that handles the blur handles the click.
  const nameInFlight =
    React.useRef<Promise<CodingSessionFoundedTextOutcome> | null>(null);
  const promptInFlight =
    React.useRef<Promise<CodingSessionFoundedTextOutcome> | null>(null);
  const commitName = React.useCallback(() => {
    if (nameInFlight.current) return nameInFlight.current;
    const pending = commitNameNow().finally(() => {
      nameInFlight.current = null;
    });
    nameInFlight.current = pending;
    return pending;
  }, [commitNameNow]);
  const commitPrompt = React.useCallback(() => {
    if (promptInFlight.current) return promptInFlight.current;
    const pending = commitPromptNow().finally(() => {
      promptInFlight.current = null;
    });
    promptInFlight.current = pending;
    return pending;
  }, [commitPromptNow]);

  // Name, then goal; each only when dirty; stop at the first refusal.
  const flush =
    React.useCallback(async (): Promise<CodingSessionFoundedTextOutcome> => {
      const named = await commitName();
      if (!named.ok) return named;
      return await commitPrompt();
    }, [commitName, commitPrompt]);

  // Said under the field, never as a blocker: a blur-publish that disabled
  // Start under the cursor would swallow the click that caused the blur.
  const busySentence = namePublishing
    ? "Publishing the name…"
    : promptPublishing
      ? "Publishing the initial prompt…"
      : null;

  return {
    name,
    setName,
    nameError,
    nameReadPending: !namesResolved,
    nameReadError,
    refreshNames,
    nameDirty,
    commitName,
    suggestion: suggestionAllowed ? naming.status : null,
    /**
     * What a blank Name means at Start: it stays untitled unless the agent's
     * computer titles it from the first message (a provider-signed 44252) —
     * not guaranteed, and not decided by this desktop's draft-time namer.
     */
    autoNameSentence: codingSessionBlankNameSentence(),
    /** What a Solo Start does to the goal, per this computer's namer. */
    autoGoalSentence: codingSessionAutoGoalSentence(naming.settings),
    /** Ask the namer now — the prompt's blur; a no-op while suppressed. */
    requestSuggestionNow: naming.requestNow,
    prompt,
    setPrompt,
    promptError,
    promptDirty,
    promptAttempted,
    markPromptAttempted: () => setPromptAttempted(true),
    goalOverflow,
    goalReader,
    persistence: draft.persistence,
    onPromptKeyDown: recall.onKeyDown,
    commitPrompt,
    /** Remember the prompt in ⌘↑/⌘↓ history and forget the draft — on Start. */
    remember: () => {
      const trimmed = prompt.trim();
      if (trimmed.length > 0) recall.remember(trimmed);
      draft.clear();
    },
    flush,
    busySentence,
  };
}

export type CodingSessionFoundedTextModel = ReturnType<
  typeof useCodingSessionFoundedText
>;
