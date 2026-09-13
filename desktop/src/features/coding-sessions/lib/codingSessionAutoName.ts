import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  type CodingSessionNamingSettings,
  generateCodingSessionName,
  getCodingSessionNamingSettings,
} from "@/shared/api/tauriCodingSessionNaming";
import {
  buildCodingSessionNameFilter,
  codingSessionNameKey,
  foldLatestCodingSessionNamesByFounder,
  publishCodingSessionName,
} from "./codingSessionName";
import { MIN_CODING_SESSION_NAME_SUGGEST_CHARS } from "./codingSessionNameSuggestion";

const NAME_HISTORY_LIMIT = 1000;

/**
 * Name a session that was started without one, from its first message.
 *
 * The founded page's Name field asks the configured namer while the prompt
 * is being written, but a Start pressed before the model answers used to
 * leave the session "Untitled session" for good (Andy, 2026-09-10: "after
 * the first turn, automatically set the name … like we did in the dialog").
 * So Start, when the field was blank, asks once more after the create is
 * accepted and publishes the answer as the 44229 — the same record a person
 * writes from the header's pencil, newest wins.
 *
 * Two rules keep it honest. It never names a session that has a name by the
 * time the model answers (someone may have typed one from the phone in the
 * seconds the request took: a fresh read of the wire, not a stale snapshot,
 * decides). And it never invents: no namer configured, a message too short
 * to name, an empty answer or a refusal each end as a stated outcome, never
 * as a made-up title.
 */
export type CodingSessionAutoNameOutcome =
  | { kind: "off" }
  | { kind: "too-short" }
  | { kind: "empty" }
  | { kind: "named-meanwhile"; name: string }
  | { kind: "published"; name: string }
  | { kind: "failed"; reason: string };

export type CodingSessionAutoNameInput = {
  channelId: string;
  sessionRef: string;
  /** The signer of the name — the founder, whose key names the record. */
  founderPubkey: string;
  firstMessage: string;
};

export type CodingSessionAutoNameDeps = {
  /** Resolves null where there is no host to ask (browser preview, mock). */
  getSettings: () => Promise<CodingSessionNamingSettings | null>;
  generate: (firstMessage: string) => Promise<string>;
  /** Read the exact founder/session's 44229 history at publish time. */
  readNames: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>;
  publishName: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
};

const DEFAULT_DEPS: CodingSessionAutoNameDeps = {
  getSettings: () => getCodingSessionNamingSettings().catch(() => null),
  generate: generateCodingSessionName,
  readNames: (filter) => relayClient.fetchEventsCoalesced(filter),
  publishName: publishCodingSessionName,
};

/** Whether the namer would be asked at all for this message. */
export function codingSessionAutoNameApplies(input: {
  settings: CodingSessionNamingSettings | null;
  firstMessage: string;
}): boolean {
  if (input.settings === null || input.settings.provider === "off") {
    return false;
  }
  return (
    input.firstMessage.trim().length >= MIN_CODING_SESSION_NAME_SUGGEST_CHARS
  );
}

/** Ask the namer and publish its answer, unless a name landed meanwhile. */
export async function autoNameCodingSession(
  input: CodingSessionAutoNameInput,
  deps: CodingSessionAutoNameDeps = DEFAULT_DEPS,
): Promise<CodingSessionAutoNameOutcome> {
  const settings = await deps.getSettings();
  if (settings === null || settings.provider === "off") return { kind: "off" };
  const firstMessage = input.firstMessage.trim();
  if (firstMessage.length < MIN_CODING_SESSION_NAME_SUGGEST_CHARS) {
    return { kind: "too-short" };
  }
  let name: string;
  try {
    name = (await deps.generate(firstMessage)).trim();
  } catch (error) {
    return {
      kind: "failed",
      reason: error instanceof Error ? error.message : String(error),
    };
  }
  if (name.length === 0) return { kind: "empty" };
  try {
    const history = await deps.readNames({
      ...buildCodingSessionNameFilter([input.channelId], NAME_HISTORY_LIMIT),
      "#d": [input.sessionRef],
      authors: [input.founderPubkey.toLowerCase()],
    });
    const onWire = foldLatestCodingSessionNamesByFounder(history).get(
      codingSessionNameKey(
        input.channelId,
        input.sessionRef,
        input.founderPubkey,
      ),
    );
    if (onWire && onWire.content.trim().length > 0) {
      return { kind: "named-meanwhile", name: onWire.content };
    }
    if (history.length >= NAME_HISTORY_LIMIT) {
      return {
        kind: "failed",
        reason:
          "Cannot confirm that this session is unnamed: name history is incomplete.",
      };
    }
    await deps.publishName({
      channelId: input.channelId,
      content: name,
      sessionRef: input.sessionRef,
    });
    return { kind: "published", name };
  } catch (error) {
    return {
      kind: "failed",
      reason: error instanceof Error ? error.message : String(error),
    };
  }
}

/** The sentence under the Name field about what a blank field means. */
export function codingSessionAutoNameSentence(
  settings: CodingSessionNamingSettings | null,
): string | null {
  if (settings === null) return null;
  if (settings.provider === "off") {
    return "Left blank, this session stays “Untitled session” — no naming model is set in Settings.";
  }
  const model = settings.model.trim();
  return model.length > 0
    ? `Left blank, it is named from the initial prompt after Start (${model}).`
    : "Left blank, it is named from the initial prompt after Start.";
}
