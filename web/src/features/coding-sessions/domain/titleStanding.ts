/**
 * SV-31 standing back-fill: the reads that let an older generated title keep
 * its standing on a busy channel.
 *
 * A 44252 stands only when its signer is the verified provider authority of
 * the exact generation its `cs-target` names (`umbrella.ts`). The web reads
 * that proof — the 44221 create, the 44224 receipt answering it, and the
 * 44223 metadata that brings the generation into being — from the channel's
 * newest-1000 history pages. On a busy channel those pages stop reaching the
 * founding generation of an older session long before the session itself
 * leaves the list, and the title would silently drop back to the fallback.
 *
 * So the observer asks for exactly the missing proof, narrowed by what the
 * title itself carries rather than by "newest N for this signer":
 *
 * - the create **by id** (`createEventId` is in the title's payload);
 * - the signer's 44223s and 44224s in a window **around the title's own
 *   `created_at`**. The title is minted during the founder's first turn of
 *   generation 1, after the create's receipt and the generation's first
 *   metadata, so that window holds them however many records the signer
 *   published since.
 *
 * Only titles for a session the reader already shows are back-filled: the
 * read settles a name, it never brings a stale session back onto the list.
 */
import {
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "../../../shared/lib/kinds.ts";
import type { CodingSessionObserverFacts } from "./catalog.ts";
import {
  CODING_SESSION_HISTORY_LIMIT,
  type CodingSessionFilter,
} from "./filters.ts";
import { buildCodingSessionTargetKey } from "./keys.ts";

/** How far before a title its generation's proof is looked for. */
export const TITLE_STANDING_WINDOW_BEFORE_SECONDS = 3600;
/** Slack after the title, for clock skew between records of one host. */
export const TITLE_STANDING_WINDOW_AFTER_SECONDS = 60;
/** Titles settled per back-fill read; the rest go in the next read. */
export const MAX_TITLE_STANDING_GAPS_PER_READ = 10;

/** One held 44252 whose standing the reader cannot yet prove. */
export type CodingSessionTitleStandingGap = {
  titleEventId: string;
  channelId: string;
  sessionRef: string;
  signerPubkey: string;
  targetKey: string;
  createEventId: string;
  createdAt: number;
};

function standingKey(
  channelId: string,
  targetKey: string,
  signerPubkey: string,
): string {
  return `${channelId}\u0000${targetKey}\u0000${signerPubkey.toLowerCase()}`;
}

function sessionKey(channelId: string, sessionRef: string): string {
  return `${channelId}\u0000${sessionRef}`;
}

/**
 * Every held generated title that names a session the reader shows but whose
 * `(channel, cs-target, signer)` no verified generation confirms, oldest
 * first.
 */
export function codingSessionTitleStandingGaps(
  facts: Pick<CodingSessionObserverFacts, "generations" | "generatedTitles">,
): CodingSessionTitleStandingGap[] {
  const verified = new Set<string>();
  const shown = new Set<string>();
  for (const generation of facts.generations) {
    if (generation.sessionRef !== null) {
      shown.add(sessionKey(generation.channelId, generation.sessionRef));
    }
    if (generation.authoritySource === "create") {
      verified.add(
        standingKey(
          generation.channelId,
          buildCodingSessionTargetKey(generation.target),
          generation.providerAuthorityPubkey,
        ),
      );
    }
  }
  return facts.generatedTitles
    .filter(
      (title) =>
        shown.has(sessionKey(title.channelId, title.sessionRef)) &&
        !verified.has(
          standingKey(title.channelId, title.targetKey, title.signerPubkey),
        ),
    )
    .map((title) => ({
      titleEventId: title.eventId,
      channelId: title.channelId,
      sessionRef: title.sessionRef,
      signerPubkey: title.signerPubkey,
      targetKey: title.targetKey,
      createEventId: title.createEventId,
      createdAt: title.createdAt,
    }))
    .sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.titleEventId.localeCompare(right.titleEventId),
    );
}

/**
 * The back-fill read for these gaps: one by-id read of the creates per
 * channel, then a metadata and a receipt window per title.
 *
 * Each filter is its own REQ (the transport never shares one), so a busy
 * signer's receipts cannot crowd its metadata out, nor one title's window
 * another's. A by-id filter's `limit` is its id count, so a full page there
 * is completeness, not truncation — see {@link isTitleStandingPageTruncated}.
 */
export function codingSessionTitleStandingFilters(
  gaps: readonly CodingSessionTitleStandingGap[],
): CodingSessionFilter[] {
  const createIdsByChannel = new Map<string, Set<string>>();
  for (const gap of gaps) {
    const ids = createIdsByChannel.get(gap.channelId);
    if (ids) ids.add(gap.createEventId);
    else createIdsByChannel.set(gap.channelId, new Set([gap.createEventId]));
  }
  const filters: CodingSessionFilter[] = [];
  for (const [channelId, ids] of createIdsByChannel) {
    const sorted = [...ids].sort();
    filters.push({
      kinds: [KIND_CODING_SESSION_LIFECYCLE_COMMAND],
      "#h": [channelId],
      ids: sorted,
      limit: sorted.length,
    });
  }
  const windows = new Set<string>();
  for (const gap of gaps) {
    const since = Math.max(
      0,
      gap.createdAt - TITLE_STANDING_WINDOW_BEFORE_SECONDS,
    );
    const until = gap.createdAt + TITLE_STANDING_WINDOW_AFTER_SECONDS;
    const windowKey = `${gap.channelId}\u0000${gap.signerPubkey}\u0000${since}\u0000${until}`;
    if (windows.has(windowKey)) continue;
    windows.add(windowKey);
    for (const kind of [
      KIND_CODING_SESSION_METADATA,
      KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    ]) {
      filters.push({
        kinds: [kind],
        "#h": [gap.channelId],
        authors: [gap.signerPubkey],
        since,
        until,
        limit: CODING_SESSION_HISTORY_LIMIT,
      });
    }
  }
  return filters;
}

/**
 * Whether a back-fill page that came back with `eventCount` events may have
 * been cut short. A by-id page is complete when full; a window page at its
 * limit is evidence of truncation, as for every history page.
 */
export function isTitleStandingPageTruncated(
  filter: CodingSessionFilter,
  eventCount: number,
): boolean {
  if (filter.ids !== undefined) return false;
  return filter.limit > 0 && eventCount >= filter.limit;
}
