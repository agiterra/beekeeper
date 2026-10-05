/**
 * The digest's `errors[]`, said in English.
 *
 * `errors[]` is a **wire contract**: `{scope, message}`, byte-pinned by
 * `conformance/project-pulse-fold/fixtures/fold-vectors.json` and shared with
 * the Rust fold in `buzz-cli`, so its strings are written for a machine and
 * carry raw 64-hex event ids. Printing them verbatim on the Pulse screen — as
 * `invalid-entry: entry 0000…71 failed validation and was excluded` — hands a
 * reader an id they cannot look up and a scope that names an internal branch
 * of the fold, in the one place on this screen that exists to tell them what
 * they are missing. `VISION_ACTIVITY.md` is explicit that a coordination
 * surface shows a person and a filename, never a raw pubkey or event id.
 *
 * So this module is the seam: the fold keeps its machine strings, and every
 * error the screen shows becomes a sentence that names the entry by its own
 * words (via {@link pulseEntryReference}) and its author by name (via
 * {@link pulseAuthorLabel}). The raw `scope: message` survives verbatim in a
 * `title`, so the audit trail is one hover away and nothing is hidden.
 *
 * Nothing here changes what is reported — only how it reads. An error the
 * fold recorded always produces exactly one visible note.
 */
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_NAME,
  KIND_PULSE_ENTRY,
} from "@/shared/constants/kinds";

import { pulseAuthorLabel, type PulseAuthorNames } from "./pulseAuthors";
import { pulseEntryReference } from "./pulseFormat";
import type { PulseDigestEntry, PulseDigestError } from "./pulseFold.ts";

/** One rendered note: a plain sentence, plus the machine text for its `title`. */
export type PulseErrorNote = {
  /** Stable React key. */
  key: string;
  /** What the reader sees — no ids, no scopes. */
  sentence: string;
  /** The verbatim `scope: message` line(s) this note stands for. */
  title: string;
};

/** What the screen already knows, so a named entry can be described by name. */
export type PulseErrorContext = {
  entriesById?: ReadonlyMap<string, PulseDigestEntry>;
  authorNames?: PulseAuthorNames;
  nowSeconds: number;
};

/** A 64-hex event id or pubkey as the wire strings spell it. */
const HEX_ID = /\b[0-9a-f]{64}\b/gi;

/**
 * Ids never reach the reader's eye. A message this module does not recognise
 * still gets shown — suppressing an unknown error would be the exact dishonesty
 * these cards exist to prevent — but with its hashes replaced by a word.
 */
function withoutIds(message: string): string {
  return message
    .replace(/\bevent\s+[0-9a-f]{64}\b/gi, "an event")
    .replace(/\bentry\s+[0-9a-f]{64}\b/gi, "an entry")
    .replace(HEX_ID, "an event");
}

/** Capitalised, punctuated — a sentence rather than a log line. */
function asSentence(text: string): string {
  const trimmed = text.trim();
  if (trimmed.length === 0) return "";
  const capitalised = `${trimmed.slice(0, 1).toUpperCase()}${trimmed.slice(1)}`;
  return /[.!?…]$/.test(capitalised) ? capitalised : `${capitalised}.`;
}

/** What kind of thing was dropped, in the reader's vocabulary. */
function excludedNoun(kind: number): { one: string; many: string } {
  if (
    kind === KIND_CODING_SESSION_METADATA ||
    kind === KIND_CODING_SESSION_GOAL ||
    kind === KIND_CODING_SESSION_NAME ||
    kind === KIND_CODING_SESSION_GENERATED_TITLE ||
    kind === KIND_CODING_SESSION_CLOSURE
  ) {
    return { one: "A coding-session update", many: "coding-session updates" };
  }
  if (kind === KIND_PULSE_ENTRY) {
    return { one: "An entry", many: "entries" };
  }
  return { one: "An event", many: "events" };
}

/** Why it was dropped, in the reader's vocabulary. */
function excludedReason(why: string): { one: string; many: string } {
  if (why === "failed signature validation") {
    return {
      one: "its signature did not check out",
      many: "their signatures did not check out",
    };
  }
  if (why === "carried undecodable coding-session metadata") {
    return {
      one: "its session details could not be read",
      many: "their session details could not be read",
    };
  }
  const plain = withoutIds(why);
  return { one: plain, many: plain };
}

/**
 * A note before grouping. `groupKey` decides which notes are the same fact
 * said twice: three entries that each failed validation are one sentence with
 * a count, not three identical bullets.
 */
type PulseErrorDraft = {
  groupKey: string;
  sentence: string;
  plural?: (count: number) => string;
};

const INVALID_EVENT = /^event (\S+) \(kind (\d+)\) (.+) and was excluded$/;
const UNRESOLVED_SUPERSEDES =
  /^entry ([0-9a-f]{64}) supersedes ([0-9a-f]{64}), which is not in the visible result set$/i;

function draftFor(
  error: PulseDigestError,
  context: PulseErrorContext,
): PulseErrorDraft {
  const { scope, message } = error;

  if (scope === "entries") {
    if (message.startsWith("entry read truncated")) {
      return {
        groupKey: "entries:truncated",
        sentence:
          "There are more entries than one read returns, so the oldest ones are not shown.",
      };
    }
    return {
      groupKey: "entries:unread",
      sentence: asSentence(
        `this project's entries could not be read: ${withoutIds(message)}`,
      ),
    };
  }

  if (scope === "sessions") {
    if (message.startsWith("session read truncated")) {
      return {
        groupKey: "sessions:truncated",
        sentence:
          "There is more coding-session activity than one read returns, so some sessions are not shown.",
      };
    }
    return {
      groupKey: "sessions:unread",
      sentence: asSentence(
        `coding-session activity could not be read: ${withoutIds(message)}`,
      ),
    };
  }

  if (scope === "channels") {
    return {
      groupKey: "channels:unread",
      sentence:
        "This project's channel list could not be read, so no coding-session activity was looked for.",
    };
  }

  if (scope === "invalid-entry") {
    return {
      groupKey: "invalid-entry",
      sentence: "An entry was left out — it did not pass validation.",
      plural: (count) =>
        `${count} entries were left out — they did not pass validation.`,
    };
  }

  if (scope === "invalid-event") {
    const match = INVALID_EVENT.exec(message);
    if (match) {
      const noun = excludedNoun(Number(match[2]));
      const reason = excludedReason(match[3]);
      return {
        groupKey: `invalid-event:${match[2]}:${match[3]}`,
        sentence: `${noun.one} was left out — ${reason.one}.`,
        plural: (count) =>
          `${count} ${noun.many} were left out — ${reason.many}.`,
      };
    }
    return {
      groupKey: `invalid-event:${message}`,
      sentence: asSentence(withoutIds(message)),
    };
  }

  if (scope === "unresolved-supersedes") {
    // The same sentence the entry's own row prints for this claim
    // (`PulseEntryRow`), so one relationship reads in one vocabulary whether
    // the reader meets it on the row or on the card above it.
    const claimantId = UNRESOLVED_SUPERSEDES.exec(message)?.[1] ?? null;
    const claimant = claimantId
      ? (context.entriesById?.get(claimantId) ?? null)
      : null;
    if (claimant) {
      const author = pulseAuthorLabel(claimant.pubkey, context.authorNames);
      return {
        groupKey: `unresolved-supersedes:${claimant.eventId}`,
        sentence: `${author}'s ${pulseEntryReference(
          claimant,
          context.nowSeconds,
        )} says an entry that is not visible in this read is resolved — nothing was replaced.`,
      };
    }
    return {
      groupKey: "unresolved-supersedes:unknown",
      sentence:
        "An entry says another entry, not visible in this read, is resolved — nothing was replaced.",
      plural: (count) =>
        `${count} entries say entries that are not visible in this read are resolved — nothing was replaced.`,
    };
  }

  return {
    groupKey: `${scope}:${message}`,
    sentence: asSentence(withoutIds(message)),
  };
}

/**
 * The digest's errors as sentences a reader can act on, in the order the fold
 * reported them, with repeats of one fact folded into a single counted line.
 *
 * Never lossy: every input error contributes to exactly one note, and each
 * note's `title` carries the verbatim `scope: message` lines behind it.
 */
export function summarizePulseErrors(
  errors: readonly PulseDigestError[],
  context: PulseErrorContext,
): PulseErrorNote[] {
  const order: string[] = [];
  const groups = new Map<
    string,
    { draft: PulseErrorDraft; count: number; titles: string[] }
  >();

  for (const error of errors) {
    const draft = draftFor(error, context);
    const existing = groups.get(draft.groupKey);
    const title = `${error.scope}: ${error.message}`;
    if (existing) {
      existing.count += 1;
      existing.titles.push(title);
      continue;
    }
    order.push(draft.groupKey);
    groups.set(draft.groupKey, { draft, count: 1, titles: [title] });
  }

  return order.map((key) => {
    const group = groups.get(key) as {
      draft: PulseErrorDraft;
      count: number;
      titles: string[];
    };
    const sentence =
      group.count > 1 && group.draft.plural
        ? group.draft.plural(group.count)
        : group.draft.sentence;
    return { key, sentence, title: group.titles.join("\n") };
  });
}
