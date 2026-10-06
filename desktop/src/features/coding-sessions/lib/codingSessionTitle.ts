/**
 * Desktop reader of a coding session's display name: a person's 44229 or a
 * provider's generated 44252 title (NIP-CSG § Generated title, SV-31).
 *
 * The parse and the ranking rule are the pure mirror of
 * `crates/beekeeper-core/src/coding_session_title.rs`, which lives in
 * `shared/coordination/sessionCoordinationNames.ts` so the coordination fold
 * (Pulse, Agent Progress) can share it without importing from a feature; this
 * module re-exports it and adds what only the desktop hook needs:
 *
 * - the relay filters (explicit kinds, always);
 * - **signer standing from confirmed executions** — the hook cannot see the
 *   umbrella's catalog, so a 44252 counts only when its signer also signed a
 *   44223 whose `cs-target` is the title's and whose `sessionRef` is its `d`,
 *   *and* a 44224 lifecycle receipt naming that same generation. That is the
 *   catalog's "confirmed" row (`bee sessions` `scope_from_rows`): a 44223
 *   alone is anyone's claim, and a channel member with no execution could
 *   otherwise sign one, then title the session;
 * - the effective-name map `useCodingSessionNames` returns.
 *
 * Conformance: `conformance/session-display-name/` — `codingSessionTitle.test.mjs`
 * loads its vectors byte for byte.
 */

import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_GENERATED_TITLE,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
} from "@/shared/constants/kinds";
import {
  isValidSessionNameParts,
  parseCodingSessionTitleParts,
  resolveSessionDisplayNameWithSource,
  type SessionDisplayNameDiagnostics,
  type SessionExecutionAuthority,
} from "@/shared/coordination/sessionCoordinationNames";
import { hasValidSignature } from "@/shared/lib/authors";
import { buildCodingSessionTargetKey } from "./codingSessionCommand";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  isCodingSessionTurnReceiptStatus,
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads";
import {
  type CodingSessionName,
  foldLatestCodingSessionNamesByFounder,
} from "./codingSessionName";
import { parseExactTags } from "./codingSessionWireDecode";

export {
  CODING_SESSION_TITLE_SCHEMA,
  CODING_SESSION_TITLE_TAG_VERSION,
  MAX_CODING_SESSION_TITLE_CONTENT_BYTES,
  MAX_CODING_SESSION_TITLE_MODEL_BYTES,
  MAX_SESSION_NAME_BYTES,
  UNTITLED_SESSION_NAME,
  isValidSessionNameParts,
  parseCodingSessionTitleContent,
  parseCodingSessionTitleParts,
  parseSessionTitleTargetKey,
  resolveSessionDisplayName,
  resolveSessionDisplayNameWithSource,
  type CodingSessionTitleEnvelope,
  type CodingSessionTitlePayload,
  type SessionDisplayName,
  type SessionDisplayNameDiagnostics,
  type SessionDisplayNameOrigin,
  type SessionDisplayNameScope,
  type SessionExecutionAuthority,
  type SessionNameRecord,
} from "@/shared/coordination/sessionCoordinationNames";

/**
 * The name a reader shows for a session, and whose words it is.
 *
 * `origin: "person"` — the founder's own 44229; `signerPubkey` is the founder
 * and `model` is null. `origin: "generated"` — a provider's 44252 title;
 * `content` is the title, `eventId`/`createdAt` are the 44252's, `model` is the
 * model that wrote it and `signerPubkey` the provider that signed it.
 * `founderPubkey` is always the founder the reader asked about.
 */
export type CodingSessionDisplayName = CodingSessionName & {
  origin: "person" | "generated";
  model: string | null;
  signerPubkey: string;
};

/** One generated title that won its session's generated tier. */
export type CodingSessionGeneratedTitle = {
  channelId: string;
  sessionRef: string;
  content: string;
  createdAt: number;
  eventId: string;
  model: string;
  signerPubkey: string;
};

/** 44229 names and 44252 titles in one bounded read: explicit kinds. */
export function buildCodingSessionDisplayNameFilter(
  channelIds: readonly string[],
  limit = 1000,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_NAME, KIND_CODING_SESSION_GENERATED_TITLE],
    "#h": [...new Set(channelIds)],
    limit,
  };
}

function sessionKey(channelId: string, sessionRef: string): string {
  return `${channelId}\u0000${sessionRef}`;
}

function confirmationKey(
  channelId: string,
  targetKey: string,
  signerPubkey: string,
): string {
  return `${channelId}\u0000${targetKey}\u0000${signerPubkey}`;
}

/**
 * The `(h, cs-target, signer)` a validly signed 44224 lifecycle receipt
 * confirms, or null: the exact envelope the trusted ingress checks, a
 * decodable payload whose status is a generation outcome (never a turn stage
 * — per NIP-CSL a turn receipt confirms nothing) and that names a session.
 * Mirrors `resolve_sessions`' `confirmed` set in `bee sessions`.
 */
function confirmedReceiptKey(event: RelayEvent): string | null {
  if (event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT) return null;
  const tags = parseExactTags(event.tags, [
    "h",
    "cslr-v",
    "csl-command",
    "csl-key",
  ]);
  if (!tags || tags[1] !== CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION) {
    return null;
  }
  const receipt = parseCodingSessionLifecycleReceipt(event.content);
  if (
    !receipt?.session ||
    isCodingSessionTurnReceiptStatus(receipt.status) ||
    tags[2] !== receipt.commandId ||
    tags[3] !==
      codingSessionReceiptSemanticKey(receipt.commandId, receipt.status) ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  return confirmationKey(
    tags[0],
    buildCodingSessionTargetKey(receipt.session),
    event.pubkey.toLowerCase(),
  );
}

function confirmedExecutions(events: Iterable<RelayEvent>): Set<string> {
  const confirmed = new Set<string>();
  for (const event of events) {
    const key = confirmedReceiptKey(event);
    if (key) confirmed.add(key);
  }
  return confirmed;
}

type StandingMetadata = {
  channelId: string;
  sessionRef: string;
  targetKey: string;
  providerAuthorityPubkey: string;
};

/**
 * What a signed 44223 would vouch for once confirmed, or null: the exact
 * envelope the trusted ingress checks, a decodable payload naming a
 * `sessionRef`, a `cs-target` equal to the payload's own session and a valid
 * signature.
 */
function standingMetadata(event: RelayEvent): StandingMetadata | null {
  if (event.kind !== KIND_CODING_SESSION_METADATA) return null;
  const tags = parseExactTags(event.tags, [
    "h",
    "csm-v",
    "cs-target",
    "csm-key",
  ]);
  if (!tags || tags[1] !== CODING_SESSION_METADATA_TAG_VERSION) return null;
  const metadata = parseBeekeeperCodingSessionMetadata(event.content);
  if (
    !metadata?.sessionRef ||
    tags[2] !== buildCodingSessionTargetKey(metadata.session) ||
    tags[3] !== codingSessionMetadataSemanticKey(metadata.session) ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  return {
    channelId: tags[0],
    sessionRef: metadata.sessionRef,
    targetKey: tags[2],
    providerAuthorityPubkey: event.pubkey.toLowerCase(),
  };
}

/**
 * Executions with standing to title each `(h, sessionRef)` — the confirmed
 * executions, as the Rust readers count them. A signed 44223 counts when
 * {@link standingMetadata} accepts it **and** the same signer published a
 * lifecycle receipt for that same `cs-target` in the same channel. The signer
 * is then the execution's provider authority: it signed that execution's
 * facts and its lifecycle. A 44223 with no such receipt is anyone's claim and
 * grants nothing, so its signer's 44252 is counted as a foreign title.
 *
 * `events` may mix 44223 and 44224; every other kind is ignored.
 */
export function codingSessionTitleStanding(
  events: Iterable<RelayEvent>,
): Map<string, SessionExecutionAuthority[]> {
  const all = [...events];
  const confirmed = confirmedExecutions(all);
  const standing = new Map<string, SessionExecutionAuthority[]>();
  const seen = new Set<string>();
  for (const event of all) {
    const entry = standingMetadata(event);
    if (
      !entry ||
      !confirmed.has(
        confirmationKey(
          entry.channelId,
          entry.targetKey,
          entry.providerAuthorityPubkey,
        ),
      )
    ) {
      continue;
    }
    const key = sessionKey(entry.channelId, entry.sessionRef);
    const dedupe = `${key}\u0000${entry.targetKey}\u0000${entry.providerAuthorityPubkey}`;
    if (seen.has(dedupe)) continue;
    seen.add(dedupe);
    const executions = standing.get(key) ?? [];
    executions.push({
      targetKey: entry.targetKey,
      providerAuthorityPubkey: entry.providerAuthorityPubkey,
    });
    standing.set(key, executions);
  }
  return standing;
}

/**
 * Seconds past a title's `created_at` the standing read still looks: the same
 * signer's clock wrote both, so this only absorbs same-second ordering.
 */
export const TITLE_STANDING_SLACK_SECONDS = 60;
/** Records per standing page. */
export const TITLE_STANDING_PAGE_LIMIT = 1000;
/** Pages per (channel, signer, kind) before the read stops and says so. */
export const TITLE_STANDING_MAX_PAGES = 20;

/**
 * One standing read: every `cs-target` one signer's titles claim in one
 * channel, and the newest title's `created_at` (plus slack) as the upper
 * bound. The execution a title names was created, and its first metadata
 * signed, before the title was generated during its first turn — so standing
 * is found at or before the title, never among the signer's later receipts.
 */
export type CodingSessionTitleStandingQuery = {
  channelId: string;
  signerPubkey: string;
  targetKeys: string[];
  until: number;
};

/**
 * The standing reads a names read needs, one per `(channel, signer)`, sorted
 * so the list is a stable dependency key.
 */
export function codingSessionTitleStandingQueries(
  events: Iterable<RelayEvent>,
): CodingSessionTitleStandingQuery[] {
  const groups = new Map<
    string,
    {
      channelId: string;
      signerPubkey: string;
      targets: Set<string>;
      newest: number;
    }
  >();
  for (const event of events) {
    if (event.kind !== KIND_CODING_SESSION_GENERATED_TITLE) continue;
    const envelope = parseCodingSessionTitleParts(event.tags, event.content);
    if (!envelope) continue;
    const signerPubkey = event.pubkey.toLowerCase();
    const key = `${envelope.channelId}\u0000${signerPubkey}`;
    const group = groups.get(key) ?? {
      channelId: envelope.channelId,
      signerPubkey,
      targets: new Set<string>(),
      newest: event.created_at,
    };
    group.targets.add(envelope.targetKey);
    group.newest = Math.max(group.newest, event.created_at);
    groups.set(key, group);
  }
  return [...groups.entries()]
    .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
    .map(([, group]) => ({
      channelId: group.channelId,
      signerPubkey: group.signerPubkey,
      targetKeys: [...group.targets].sort(),
      until: group.newest + TITLE_STANDING_SLACK_SECONDS,
    }));
}

/** The `cs-target`s of `query` that `events` already confirm as executions. */
export function confirmedTitleStandingTargets(
  query: CodingSessionTitleStandingQuery,
  events: Iterable<RelayEvent>,
): Set<string> {
  const all = [...events];
  const confirmed = confirmedExecutions(all);
  const covered = new Set<string>();
  for (const event of all) {
    const entry = standingMetadata(event);
    if (
      entry &&
      entry.channelId === query.channelId &&
      entry.providerAuthorityPubkey === query.signerPubkey &&
      query.targetKeys.includes(entry.targetKey) &&
      confirmed.has(
        confirmationKey(entry.channelId, entry.targetKey, query.signerPubkey),
      )
    ) {
      covered.add(entry.targetKey);
    }
  }
  return covered;
}

/** What one paged standing read produced. */
export type CodingSessionTitleStandingRead = {
  events: RelayEvent[];
  /**
   * True only when a claim is still unconfirmed and its history did **not**
   * run out — the page cap stopped the read. A claim whose history ran out
   * unconfirmed is a foreign title, not an incomplete read.
   */
  incomplete: boolean;
};

/**
 * Read one signer's 44223 metadata or 44224 receipts in one channel,
 * newest first, paging backwards from `query.until` until every target is
 * covered by `isCovered`, the history runs out, or the page cap is reached.
 */
async function pageStandingKind(
  query: CodingSessionTitleStandingQuery,
  kind: number,
  fetchPage: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>,
  isCovered: (events: RelayEvent[]) => boolean,
  options: { pageLimit: number; maxPages: number },
): Promise<{ events: RelayEvent[]; exhausted: boolean; covered: boolean }> {
  const events = new Map<string, RelayEvent>();
  let until = query.until;
  for (let page = 0; page < options.maxPages; page += 1) {
    const batch = await fetchPage({
      kinds: [kind],
      "#h": [query.channelId],
      authors: [query.signerPubkey],
      until,
      limit: options.pageLimit,
    });
    for (const event of batch) events.set(event.id, event);
    const all = [...events.values()];
    if (isCovered(all)) return { events: all, exhausted: false, covered: true };
    if (batch.length < options.pageLimit) {
      return { events: all, exhausted: true, covered: false };
    }
    const oldest = Math.min(...batch.map((event) => event.created_at));
    // `until` is inclusive: step past the oldest second once a full page
    // sits on it, so the walk always moves.
    until = oldest < until ? oldest : until - 1;
  }
  return { events: [...events.values()], exhausted: false, covered: false };
}

/**
 * Standing for every title claim, bounded by the claims rather than by the
 * signers' whole history (the Rust readers page the full history; a newest-N
 * read loses an old session's create receipt once its provider has signed N
 * later turn receipts). Each `(channel, signer)` pages its 44223s and its
 * 44224s backwards from the newest title it signed there, separately, until
 * every claimed target is confirmed or the history runs out.
 */
export async function readCodingSessionTitleStanding(
  queries: readonly CodingSessionTitleStandingQuery[],
  fetchPage: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>,
  options: { pageLimit?: number; maxPages?: number } = {},
): Promise<CodingSessionTitleStandingRead> {
  const paging = {
    pageLimit: options.pageLimit ?? TITLE_STANDING_PAGE_LIMIT,
    maxPages: options.maxPages ?? TITLE_STANDING_MAX_PAGES,
  };
  const results = await Promise.all(
    queries.map(async (query) => {
      const [metadata, receipts] = await Promise.all([
        pageStandingKind(
          query,
          KIND_CODING_SESSION_METADATA,
          fetchPage,
          (events) =>
            query.targetKeys.every((target) =>
              events.some((event) => {
                const entry = standingMetadata(event);
                return (
                  entry?.channelId === query.channelId &&
                  entry.providerAuthorityPubkey === query.signerPubkey &&
                  entry.targetKey === target
                );
              }),
            ),
          paging,
        ),
        pageStandingKind(
          query,
          KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
          fetchPage,
          (events) => {
            const confirmed = confirmedExecutions(events);
            return query.targetKeys.every((target) =>
              confirmed.has(
                confirmationKey(query.channelId, target, query.signerPubkey),
              ),
            );
          },
          paging,
        ),
      ]);
      const events = [...metadata.events, ...receipts.events];
      const unconfirmed =
        confirmedTitleStandingTargets(query, events).size <
        query.targetKeys.length;
      const cutShort =
        (!metadata.covered && !metadata.exhausted) ||
        (!receipts.covered && !receipts.exhausted);
      return { events, incomplete: unconfirmed && cutShort };
    }),
  );
  return {
    events: results.flatMap((result) => result.events),
    incomplete: results.some((result) => result.incomplete),
  };
}

function firstTagValue(event: RelayEvent, name: string): string | null {
  const tag = event.tags.find(
    (candidate) => Array.isArray(candidate) && candidate[0] === name,
  );
  return typeof tag?.[1] === "string" && tag[1] ? tag[1] : null;
}

function splitNameKey(key: string): [string, string, string] | null {
  const parts = key.split("\u0000");
  return parts.length === 3 ? [parts[0], parts[1], parts[2]] : null;
}

/**
 * The effective display name per {@link codingSessionNameKey}
 * `(channel, sessionRef, founder)`: the founder's 44229, else the session's
 * standing generated title.
 *
 * `get` and `has` resolve through the shared rule on demand, because the
 * generated tier is keyed by `(channel, sessionRef)` and the founder is only
 * known to the caller. Iteration and `size` cover the 44229 entries this
 * reader holds — every signer's latest, keyed by signer, exactly the map this
 * hook returned before titles existed — and never a generated title, which
 * has no founder of its own to be keyed by.
 */
export class CodingSessionDisplayNames extends Map<
  string,
  CodingSessionDisplayName
> {
  readonly #records: ReadonlyMap<string, RelayEvent[]>;
  readonly #standing: ReadonlyMap<string, SessionExecutionAuthority[]>;
  readonly #resolved = new Map<string, CodingSessionDisplayName | null>();

  constructor(
    records: ReadonlyMap<string, RelayEvent[]>,
    standing: ReadonlyMap<string, SessionExecutionAuthority[]>,
    personNames: ReadonlyMap<string, CodingSessionName>,
  ) {
    super();
    this.#records = records;
    this.#standing = standing;
    for (const [key, name] of personNames) {
      super.set(key, {
        ...name,
        origin: "person",
        model: null,
        signerPubkey: name.founderPubkey,
      });
    }
  }

  override get(key: string): CodingSessionDisplayName | undefined {
    if (this.#resolved.has(key)) return this.#resolved.get(key) ?? undefined;
    const resolved = this.#resolve(key);
    this.#resolved.set(key, resolved);
    return resolved ?? undefined;
  }

  override has(key: string): boolean {
    return this.get(key) !== undefined;
  }

  #resolve(key: string): CodingSessionDisplayName | null {
    const parts = splitNameKey(key);
    if (!parts) return null;
    const [channelId, sessionRef, founderPubkey] = parts;
    const resolved = resolveSessionDisplayNameWithSource(
      {
        channelId,
        sessionRef,
        founderPubkey,
        foundingExecutionTitle: null,
        executions: this.#standing.get(sessionKey(channelId, sessionRef)) ?? [],
      },
      this.#records.get(sessionKey(channelId, sessionRef)) ?? [],
    );
    const source = resolved.source;
    if (resolved.origin === "fallback" || !source) return null;
    if (resolved.origin === "person") {
      return super.get(key) ?? null;
    }
    return {
      channelId,
      sessionRef,
      founderPubkey,
      content: resolved.name,
      createdAt: source.created_at,
      eventId: source.id,
      origin: "generated",
      model: resolved.model,
      signerPubkey: resolved.signerPubkey ?? source.pubkey.toLowerCase(),
    };
  }
}

/** What one fold of the names read produced. */
export type CodingSessionNamesFold = {
  /** Effective name per founder key: person, else generated. */
  names: CodingSessionDisplayNames;
  /** Founder-signed (by key) 44229s only — the "a person named it" guard. */
  personNames: Map<string, CodingSessionName>;
  /** The winning standing generated title per `(channel, sessionRef)`. */
  generatedTitles: Map<string, CodingSessionGeneratedTitle>;
  /** 44252s set aside across the scope — counted, never shown. */
  titleDiagnostics: Pick<
    SessionDisplayNameDiagnostics,
    "foreignTitles" | "malformed"
  >;
};

/**
 * Fold a names read and a standing read (44223 metadata and 44224 lifecycle
 * receipts, in `metadata`) into the hook's maps.
 *
 * Every event is signature-checked first; the shared resolver judges shape
 * and standing only. A 44229 that the relay's envelope rule would refuse is
 * no person's name here either, so `personNames` and the person tier of
 * `names` agree.
 */
export function foldCodingSessionNames(input: {
  events: Iterable<RelayEvent>;
  metadata: Iterable<RelayEvent>;
}): CodingSessionNamesFold {
  const standing = codingSessionTitleStanding(input.metadata);
  const records = new Map<string, RelayEvent[]>();
  const validNames: RelayEvent[] = [];
  for (const event of input.events) {
    if (
      event.kind !== KIND_CODING_SESSION_NAME &&
      event.kind !== KIND_CODING_SESSION_GENERATED_TITLE
    ) {
      continue;
    }
    if (!hasValidSignature(event)) continue;
    // Grouped by the first `h` and `d` wherever they sit, so a misordered
    // envelope is still counted as this session's malformed record.
    const channelId = firstTagValue(event, "h");
    const sessionRef = firstTagValue(event, "d");
    if (!channelId || !sessionRef) continue;
    const key = sessionKey(channelId, sessionRef);
    const bucket = records.get(key) ?? [];
    bucket.push(event);
    records.set(key, bucket);
    if (
      event.kind === KIND_CODING_SESSION_NAME &&
      isValidSessionNameParts(event.tags, event.content)
    ) {
      validNames.push(event);
    }
  }
  const personNames = foldLatestCodingSessionNamesByFounder(validNames);

  const generatedTitles = new Map<string, CodingSessionGeneratedTitle>();
  const titleDiagnostics = { foreignTitles: 0, malformed: 0 };
  for (const [key, bucket] of records) {
    const [channelId, sessionRef] = key.split("\u0000");
    // Founder unknown: the generated tier does not depend on one, and the
    // person tier is the caller's lookup by key.
    const resolved = resolveSessionDisplayNameWithSource(
      {
        channelId,
        sessionRef,
        founderPubkey: null,
        foundingExecutionTitle: null,
        executions: standing.get(key) ?? [],
      },
      bucket.filter(
        (event) => event.kind === KIND_CODING_SESSION_GENERATED_TITLE,
      ),
    );
    titleDiagnostics.foreignTitles += resolved.diagnostics.foreignTitles;
    titleDiagnostics.malformed += resolved.diagnostics.malformed;
    if (resolved.origin === "generated" && resolved.source) {
      generatedTitles.set(key, {
        channelId,
        sessionRef,
        content: resolved.name,
        createdAt: resolved.source.created_at,
        eventId: resolved.source.id,
        model: resolved.model ?? "",
        signerPubkey: resolved.signerPubkey ?? "",
      });
    }
  }
  return {
    names: new CodingSessionDisplayNames(records, standing, personNames),
    personNames,
    generatedTitles,
    titleDiagnostics,
  };
}
