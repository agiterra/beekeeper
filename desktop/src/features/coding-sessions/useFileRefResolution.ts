import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import {
  type CodingSessionFileRef,
  type CodingSessionFileRefScope,
  type CodingSessionFileRefsAnswer,
  MAX_FILE_REF_CANDIDATES,
  openCodingSessionFileRef,
  resolveCodingSessionFileRefs,
  revealCodingSessionFileRef,
} from "@/shared/api/tauriCodingSessionFileRefs";
import { getCodingSessionProviderStatus } from "@/shared/api/tauriSessionProvider";
import {
  FileRefContext,
  type FileRefScopeState,
} from "@/shared/ui/markdown/fileRefContext";

import {
  inlineCodeFilePathCandidate,
  markdownHrefFilePathCandidate,
} from "./lib/filePathCandidate";

/**
 * Resolving the file paths in coding-session answers against this computer
 * (SV-32), one execution at a time.
 *
 * Modelled on `useRedactionDictionary.ts`: a module cache plus a shared
 * in-flight lookup, **not** React Query, so the transcript stays renderable
 * without a `QueryClientProvider`. Unlike a redaction, an answer here can
 * change (a folder can be deleted), but nothing acts on it: open and reveal
 * are resolved again by the host at click time. The cache only decides what
 * is drawn.
 *
 * Every message of one execution asks in the same tick, so lookups are
 * batched per scope into one IPC call (per 256 candidates) rather than one
 * per message.
 */

/** What is known about one execution's scope on this computer. */
type ScopeEntry = {
  answer: Omit<CodingSessionFileRefsAnswer, "refs"> | null;
  refs: Map<string, CodingSessionFileRef>;
  queued: Set<string>;
  inFlight: Set<string>;
  scheduled: boolean;
  listeners: Set<() => void>;
};

const scopes = new Map<string, ScopeEntry>();
let providerPubkeyLookup: Promise<string | null> | null = null;

/**
 * Forget everything learned about file refs. Registered in
 * `resetCommunityState()`: a provider identity is minted per relay, so a
 * scope's locality means nothing after a community switch.
 */
export function resetFileRefResolution() {
  scopes.clear();
  providerPubkeyLookup = null;
}

/** How a lookup is performed; injected in tests, IPC everywhere else. */
export type FileRefResolver = (
  scope: CodingSessionFileRefScope,
  candidates: readonly string[],
) => Promise<CodingSessionFileRefsAnswer>;

function scopeKey(scope: CodingSessionFileRefScope): string {
  return JSON.stringify([
    scope.channelId,
    scope.providerSessionId,
    scope.projectRef,
    scope.isHiredSeat,
    scope.isLocalProvider,
  ]);
}

function entryFor(key: string): ScopeEntry {
  let entry = scopes.get(key);
  if (!entry) {
    entry = {
      answer: null,
      refs: new Map(),
      queued: new Set(),
      inFlight: new Set(),
      scheduled: false,
      listeners: new Set(),
    };
    scopes.set(key, entry);
  }
  return entry;
}

function notify(entry: ScopeEntry) {
  for (const listener of entry.listeners) listener();
}

function flush(
  entry: ScopeEntry,
  scope: CodingSessionFileRefScope,
  resolver: FileRefResolver,
) {
  entry.scheduled = false;
  const batch = [...entry.queued];
  entry.queued.clear();
  for (let start = 0; start < batch.length; start += MAX_FILE_REF_CANDIDATES) {
    const chunk = batch.slice(start, start + MAX_FILE_REF_CANDIDATES);
    for (const candidate of chunk) entry.inFlight.add(candidate);
    resolver(scope, chunk)
      .then((answer) => {
        const { refs, ...meta } = answer;
        entry.answer = meta;
        for (const [candidate, ref] of Object.entries(refs)) {
          entry.refs.set(candidate, ref);
        }
      })
      .catch((error) => {
        // A host that cannot answer leaves the paths as they were: plain code,
        // no claim. Logged, because "failed" and "nothing to say" otherwise
        // render the same.
        console.warn("file ref lookup failed", error);
      })
      .finally(() => {
        for (const candidate of chunk) entry.inFlight.delete(candidate);
        notify(entry);
      });
  }
}

/**
 * Ask about `candidates` in `scope`, calling `onChange` whenever the scope's
 * answer grows. Returns the unsubscribe.
 */
export function subscribeFileRefResolution(
  scope: CodingSessionFileRefScope,
  candidates: readonly string[],
  onChange: () => void,
  resolver: FileRefResolver = resolveCodingSessionFileRefs,
): () => void {
  const entry = entryFor(scopeKey(scope));
  entry.listeners.add(onChange);
  // A scope another computer ran answers the same for every path, so once it
  // has said so there is nothing further to ask.
  const settledElsewhere =
    entry.answer !== null && entry.answer.where !== "thisComputer";
  for (const candidate of candidates) {
    if (settledElsewhere) break;
    if (
      entry.refs.has(candidate) ||
      entry.inFlight.has(candidate) ||
      entry.queued.has(candidate)
    ) {
      continue;
    }
    entry.queued.add(candidate);
  }
  if (entry.queued.size > 0 && !entry.scheduled) {
    entry.scheduled = true;
    queueMicrotask(() => flush(entry, scope, resolver));
  }
  return () => {
    entry.listeners.delete(onChange);
  };
}

/** The scope's state as drawn, or `pending` before the host has answered. */
export function readFileRefScope(
  scope: CodingSessionFileRefScope,
): Pick<FileRefScopeState, "where" | "reason" | "refs"> {
  const entry = scopes.get(scopeKey(scope));
  if (!entry?.answer) return { where: "pending", reason: null, refs: {} };
  return {
    where: entry.answer.where,
    reason: entry.answer.reason,
    refs: Object.fromEntries(entry.refs),
  };
}

const FENCE_PATTERN =
  /(^|\n)[ \t]*(```|~~~)[^\n]*\n[\s\S]*?(\n[ \t]*\2[ \t]*(?=\n|$)|$)/g;
const INLINE_CODE_PATTERN = /(`+)([^`\n]+?)\1(?!`)/g;
const LINK_HREF_PATTERN = /\]\(\s*(<[^>\n]+>|[^)\s]+)(?:\s+"[^"\n]*")?\s*\)/g;

/**
 * Every candidate an answer's markdown contains: inline code spans and link
 * hrefs, never fenced code (T3 does not link it either). Approximate on
 * purpose: a candidate missed here is simply never asked about, and an
 * unasked candidate renders as it always did — it is never called missing.
 */
export function collectFileRefCandidates(markdown: string): string[] {
  const prose = markdown.replace(FENCE_PATTERN, "$1");
  const found = new Set<string>();
  for (const match of prose.matchAll(INLINE_CODE_PATTERN)) {
    const candidate = inlineCodeFilePathCandidate(match[2] ?? "");
    if (candidate) found.add(candidate);
  }
  for (const match of prose.matchAll(LINK_HREF_PATTERN)) {
    const candidate = markdownHrefFilePathCandidate(match[1]);
    if (candidate) found.add(candidate);
  }
  return [...found];
}

/** The execution facts the scope needs, from that execution's record. */
export type FileRefExecution = {
  projectRef: string | null;
  /** An agent is seated on it (`agentRef` present): a hire. */
  isHiredSeat: boolean;
};

/**
 * Optional, for a surface that knows each execution's record: given an
 * item's `providerSessionId`, its project and whether it is a hire. Absent,
 * only the worktree this host cut for the session can answer — the project
 * and channel defaults are never guessed at.
 */
export const FileRefExecutionContext = React.createContext<
  ((providerSessionId: string) => FileRefExecution | null) | null
>(null);

const UNKNOWN_LOCALITY_REASON =
  "This computer could not say whether it ran this agent, so the path is not opened here";
const UNRECORDED_REASON =
  "No worktree for this session is recorded on this computer";

function localProviderPubkey(): Promise<string | null> {
  providerPubkeyLookup ??= getCodingSessionProviderStatus()
    .then((status) => status.providerPubkey?.trim().toLowerCase() || "")
    .catch(() => null);
  return providerPubkeyLookup;
}

/**
 * Which of this machine's facts apply to an item: `true`/`false` when the
 * item's signer is or is not this computer's provider, `null` when that is
 * not known (no signer on the item, or the status could not be read).
 */
export function itemIsLocal(
  signerPubkey: string | null | undefined,
  localPubkey: string | null | undefined,
): boolean | null {
  const signer = signerPubkey?.trim().toLowerCase() ?? "";
  if (!signer || localPubkey === null || localPubkey === undefined) return null;
  return localPubkey.length > 0 && signer === localPubkey;
}

/**
 * Build the renderer-side scope for one item. `isLocalProvider` is sent as
 * `true` only when both the signer and the execution's record are known;
 * otherwise only the session's own worktree may answer.
 */
export function fileRefScopeForItem(
  item: Pick<TranscriptItem, "channelId" | "providerSessionId">,
  isLocal: boolean | null,
  execution: FileRefExecution | null,
): CodingSessionFileRefScope | null {
  const channelId = item.channelId?.trim();
  if (!channelId) return null;
  return {
    channelId,
    providerSessionId: item.providerSessionId?.trim() || null,
    projectRef: execution?.projectRef ?? null,
    isHiredSeat: execution ? execution.isHiredSeat : true,
    isLocalProvider: isLocal === true && execution !== null,
  };
}

/**
 * Reword a host answer for what the renderer did not know. A `notLocal` for a
 * query that withheld `isLocalProvider` is not "another computer": either
 * locality is unknown, or this computer runs it and only the session's own
 * worktree was asked about.
 */
export function presentScopeAnswer(
  state: Pick<FileRefScopeState, "where" | "reason" | "refs">,
  isLocal: boolean | null,
  queriedAsLocal: boolean,
): Pick<FileRefScopeState, "where" | "reason" | "refs"> {
  if (state.where !== "notLocal" || queriedAsLocal || isLocal === false) {
    return state;
  }
  return isLocal === null
    ? { where: "unknown", reason: UNKNOWN_LOCALITY_REASON, refs: {} }
    : { where: "notRecorded", reason: UNRECORDED_REASON, refs: {} };
}

/**
 * The `FileRefContext` value for one assistant message, or `null` when the
 * item names no channel (then nothing is chipped).
 */
export function useFileRefScopeState(
  item: TranscriptItem,
  text: string,
  executionOverride?: FileRefExecution | null,
): FileRefScopeState | null {
  const lookupExecution = React.useContext(FileRefExecutionContext);
  const providerSessionId = item.providerSessionId?.trim() || null;
  const execution =
    executionOverride !== undefined
      ? executionOverride
      : providerSessionId && lookupExecution
        ? lookupExecution(providerSessionId)
        : null;
  const [localPubkey, setLocalPubkey] = React.useState<
    string | null | undefined
  >(undefined);
  React.useEffect(() => {
    let live = true;
    void localProviderPubkey().then((pubkey) => {
      if (live) setLocalPubkey(pubkey);
    });
    return () => {
      live = false;
    };
  }, []);
  const isLocal = itemIsLocal(item.bridgeSource?.pubkey, localPubkey);
  const channelId = item.channelId ?? null;
  const projectRef = execution?.projectRef ?? null;
  const isHiredSeat = execution ? execution.isHiredSeat : null;
  const scope = React.useMemo(
    () =>
      fileRefScopeForItem(
        { channelId, providerSessionId },
        isLocal,
        isHiredSeat === null ? null : { projectRef, isHiredSeat },
      ),
    [channelId, providerSessionId, isLocal, projectRef, isHiredSeat],
  );
  const candidates = React.useMemo(
    () => collectFileRefCandidates(text),
    [text],
  );
  // Tagged with the scope it was read for, so a scope change never draws the
  // previous execution's answer for even one render.
  const [snapshot, setSnapshot] = React.useState<{
    scope: CodingSessionFileRefScope;
    state: ReturnType<typeof readFileRefScope>;
  } | null>(null);

  React.useEffect(() => {
    if (!scope || localPubkey === undefined || candidates.length === 0) return;
    const read = () => setSnapshot({ scope, state: readFileRefScope(scope) });
    read();
    return subscribeFileRefResolution(scope, candidates, read);
  }, [scope, candidates, localPubkey]);

  return React.useMemo(() => {
    if (!scope || candidates.length === 0) return null;
    if (localPubkey === undefined || snapshot?.scope !== scope) {
      return pendingState(scope);
    }
    const drawn = presentScopeAnswer(
      snapshot.state,
      isLocal,
      scope.isLocalProvider,
    );
    return {
      ...drawn,
      open: (candidate: string) => openCodingSessionFileRef(scope, candidate),
      reveal: (candidate: string) =>
        revealCodingSessionFileRef(scope, candidate),
    };
  }, [scope, candidates.length, localPubkey, isLocal, snapshot]);
}

function pendingState(scope: CodingSessionFileRefScope): FileRefScopeState {
  return {
    where: "pending",
    reason: null,
    refs: {},
    open: (candidate) => openCodingSessionFileRef(scope, candidate),
    reveal: (candidate) => revealCodingSessionFileRef(scope, candidate),
  };
}

/**
 * Scope one assistant message's markdown to the execution that wrote it.
 *
 * Per item, not per transcript: an umbrella session whose executions ran on
 * two computers chips one and leaves the other plain. `execution` overrides
 * `FileRefExecutionContext` when the caller already has the record.
 */
export function FileRefProvider({
  children,
  execution,
  item,
  text,
}: {
  children: React.ReactNode;
  execution?: FileRefExecution | null;
  item: TranscriptItem;
  text: string;
}) {
  const value = useFileRefScopeState(item, text, execution);
  return React.createElement(FileRefContext.Provider, { value }, children);
}
