/**
 * The observer's store: signed events in, per-generation records out.
 *
 * Mirrors the read half of
 * `desktop/src/features/coding-sessions/lib/codingSessionTrustedIngress.ts`.
 * Nothing here reaches the network; the caller hands it whatever the relay
 * delivered and reads a snapshot back.
 *
 * Rules it enforces (D5, D6, D10):
 * - Byte-identical duplicates collapse by event id.
 * - Two distinct transcript payloads at one (target, signer, eventSeq) render
 *   NEITHER and count a conflict — a stream that disagrees with itself proves
 *   nothing about either version.
 * - Two distinct metadata payloads inside one second count a conflict but
 *   still render, resolved on event id: `created_at` has second granularity
 *   and a provider legitimately moves `starting -> idle -> running` inside
 *   one, so a burst must never wedge a session forever.
 * - A generation exists only when a *lifecycle* receipt names it. Turn
 *   receipts are decoded and then deliberately ignored for existence and
 *   status.
 * - Two creates under one `commandId` that disagree about signer, session,
 *   genesis or provider are a conflict: neither binds, and the generation
 *   falls through to the disclosed fallback rather than vanishing.
 * - Only creates a receipt from their own named provider answered are exported
 *   as facts, so an unanswered claim can never resolve a founder.
 * - At most 2000 raw events are retained per generation, oldest evicted.
 */
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_NAME,
} from "../../../shared/lib/kinds.ts";
import type { BuzzCodingSessionMetadataV1 } from "./ingressPayloads.ts";
import { isCodingSessionTurnReceiptStatus } from "./ingressPayloads.ts";
import {
  buildCodingSessionExecutionKey,
  buildCodingSessionGenerationId,
  buildCodingSessionTargetKey,
  encodeStructuredKey,
} from "./keys.ts";
import { type CodingSessionLease, parseCodingSessionLease } from "./lease.ts";
import { formatCodingSessionExecutionLabel } from "./labels.ts";
import {
  type CodingSessionLifecycleCommand,
  parseCodingSessionLifecycleCommand,
} from "./lifecycleCommand.ts";
import {
  type CodingSessionClosure,
  type CodingSessionGenesis,
  type CodingSessionGoal,
  type CodingSessionName,
  parseCodingSessionClosure,
  parseCodingSessionGenesis,
  parseCodingSessionGoal,
  parseCodingSessionName,
} from "./sessionRecords.ts";
import type { BuzzCodingSessionTranscriptV1 } from "./transcriptEnvelope.ts";
import { projectCodingSessionTranscript } from "./transcriptProjection.ts";
import { classifyCodingSessionEvent, isSignatureVerified } from "./trust.ts";
import type {
  CodingSessionAuthoritySource,
  CodingSessionGenerationRecord,
  CodingSessionTarget,
  ObservedEvent,
} from "./types.ts";

/** Raw events retained per generation before the oldest is evicted (D10). */
export const MAX_RETAINED_RAW_EVENTS_PER_GENERATION = 2_000;

/**
 * The four statuses that prove a generation exists (D6).
 *
 * Every other lifecycle status reports something about a generation that must
 * already exist — `stopped` ends one, `failed` never had a target at all — so
 * none of them may bring one into being.
 */
const GENERATION_EXISTENCE_STATUSES: ReadonlySet<string> = new Set([
  "created",
  "created_with_failed_initial_turn",
  "resumed",
  "resumed_without_context",
]);

type Stored<T> = {
  eventId: string;
  createdAt: number;
  signerPubkey: string;
  canonicalPayload: string;
  value: T;
};

type MetadataBucket = {
  channelId: string;
  targetKey: string;
  records: Map<string, Stored<Readonly<BuzzCodingSessionMetadataV1>>>;
};

type TranscriptBucket = {
  channelId: string;
  targetKey: string;
  signerPubkey: string;
  eventSeq: number;
  records: Map<string, Stored<Readonly<BuzzCodingSessionTranscriptV1>>>;
};

type LifecycleReceiptRecord = {
  channelId: string;
  commandId: string;
  status: string;
  signerPubkey: string;
  target: CodingSessionTarget;
  targetKey: string;
  createdAt: number;
  eventId: string;
};

/** Everything the store learned, ready to be grouped into umbrellas. */
export type CodingSessionObserverFacts = {
  generations: CodingSessionGenerationRecord[];
  /**
   * 44221 creates that a lifecycle receipt from their OWN named provider
   * answered — the only ones founder and operator resolution may consider.
   *
   * A create nobody's provider ever acted on mints no execution, so it is no
   * evidence of having operated one. Requiring the join is what stops any
   * channel member from backdating a create bearing someone else's
   * `sessionRef` to become that session's founder — and, since a close is an
   * owner-authority act, to close it (D7).
   */
  creates: CodingSessionLifecycleCommand[];
  genesisByEventId: Map<string, CodingSessionGenesis>;
  names: CodingSessionName[];
  closures: CodingSessionClosure[];
  goals: CodingSessionGoal[];
  /** Leases keyed by `(channelId, targetKey)`. */
  leasesByTarget: Map<string, CodingSessionLease[]>;
  /** The confirmed create commandId per generation id. */
  acceptedCommandIdByGenerationId: Map<string, string>;
  malformedCount: number;
  invalidSignatureCount: number;
  conflictCount: number;
};

/**
 * Accumulates verified facts across history pages and live events.
 *
 * Deliberately a class with an explicit `ingest`: the observer re-subscribes
 * on socket loss and replays from `lastSeen - 5s`, so the same event arrives
 * more than once and idempotence has to be structural, not incidental.
 */
export class CodingSessionObserverStore {
  private readonly dispositions = new Map<string, string>();
  private readonly metadata = new Map<string, MetadataBucket>();
  private readonly transcripts = new Map<string, TranscriptBucket>();
  private readonly lifecycleReceipts: LifecycleReceiptRecord[] = [];
  private readonly creates = new Map<string, CodingSessionLifecycleCommand>();
  private readonly genesis = new Map<string, CodingSessionGenesis>();
  private readonly names = new Map<string, CodingSessionName>();
  private readonly closures = new Map<string, CodingSessionClosure>();
  private readonly goals = new Map<string, CodingSessionGoal>();
  private readonly leases = new Map<string, CodingSessionLease>();
  private readonly rawEvents = new Map<string, Map<string, ObservedEvent>>();
  private readonly maxRetainedRawEventsPerGeneration: number;
  private malformedCount = 0;
  private invalidSignatureCount = 0;

  constructor(
    maxRetainedRawEventsPerGeneration = MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
  ) {
    this.maxRetainedRawEventsPerGeneration = maxRetainedRawEventsPerGeneration;
  }

  /** Ingest a page of events. Re-delivery of an already-seen id is a no-op. */
  ingest(
    events: readonly ObservedEvent[],
    channelIds: readonly string[],
  ): void {
    const allowed = new Set(channelIds);
    for (const event of events) {
      if (this.dispositions.has(event.id)) continue;
      this.dispositions.set(event.id, "seen");
      if (this.ingestSessionRecord(event, allowed)) continue;
      this.ingestFact(event, allowed);
    }
  }

  /** The raw signed bytes retained for one generation, oldest first. */
  retainedRawEvents(
    channelId: string,
    targetKey: string,
    signerPubkey: string,
  ): ObservedEvent[] {
    const key = generationRawKey(channelId, targetKey, signerPubkey);
    return [...(this.rawEvents.get(key)?.values() ?? [])].sort(
      (left, right) =>
        left.created_at - right.created_at || left.id.localeCompare(right.id),
    );
  }

  /** Everything the store can prove, projected for the umbrella fold. */
  facts(channelIds: readonly string[]): CodingSessionObserverFacts {
    const allowed = new Set(channelIds);
    const generations: CodingSessionGenerationRecord[] = [];
    const acceptedCommandIdByGenerationId = new Map<string, string>();
    let conflictCount = 0;

    const undisputed = this.undisputedCreatesByCommandId(allowed);
    const createsByCommandId = undisputed.creates;
    conflictCount += undisputed.conflictCount;

    // A generation exists iff a lifecycle receipt names it (D6) — and it is
    // scoped to the SIGNER of that receipt, never to the target alone. Two
    // providers can legitimately mint the same `(driver, instanceId,
    // sessionId, generation)` tuple, and merging them would put one provider's
    // status over another provider's transcript.
    const byGeneration = new Map<
      string,
      {
        channelId: string;
        target: CodingSessionTarget;
        targetKey: string;
        signerPubkey: string;
      }
    >();
    const receiptsByStream = new Map<string, LifecycleReceiptRecord[]>();
    for (const receipt of this.lifecycleReceipts) {
      if (!allowed.has(receipt.channelId)) continue;
      const key = factKey(
        receipt.channelId,
        encodeStructuredKey(
          "coding-session-generation-stream/v1",
          receipt.targetKey,
          receipt.signerPubkey,
        ),
      );
      // Only an existence status mints the stream. A `stopped` receipt names
      // an end, and an end is not a proof the generation was ever created —
      // when the create fell outside the history page and the stop did not,
      // minting one here would invent a generation D6 says does not exist and
      // bind its authority to the stop command, so no lease minted under the
      // create could ever join it.
      if (GENERATION_EXISTENCE_STATUSES.has(receipt.status)) {
        byGeneration.set(key, {
          channelId: receipt.channelId,
          target: receipt.target,
          targetKey: receipt.targetKey,
          signerPubkey: receipt.signerPubkey,
        });
      }
      // Every non-turn receipt still joins the stream: a stop contributes the
      // stream's last activity and its own receipt join once the generation
      // exists.
      const bucket = receiptsByStream.get(key);
      if (bucket) bucket.push(receipt);
      else receiptsByStream.set(key, [receipt]);
    }

    for (const [key, scope] of byGeneration) {
      const receipts = receiptsByStream.get(key) ?? [];
      const authority = this.resolveAuthority(
        scope,
        receipts,
        createsByCommandId,
      );
      if (authority === null) continue;
      const metadataBucket = this.metadata.get(
        factKey(scope.channelId, scope.targetKey),
      );
      const selected = metadataBucket
        ? resolveNewestMetadata(metadataBucket.records, authority.pubkey)
        : { value: null, conflictCount: 0 };
      conflictCount += selected.conflictCount;

      const generationId = buildCodingSessionGenerationId(
        scope.channelId,
        authority.pubkey,
        scope.target,
      );
      const transcriptEnvelopes = this.transcriptEnvelopesFor(
        scope.channelId,
        scope.targetKey,
        authority.pubkey,
      );
      conflictCount += transcriptEnvelopes.conflictCount;

      const metadata = selected.value?.value ?? null;
      const lastEventAt = Math.max(
        0,
        ...receipts
          .filter((receipt) => receipt.signerPubkey === authority.pubkey)
          .map((receipt) => receipt.createdAt * 1000),
        ...(selected.value ? [selected.value.createdAt * 1000] : []),
        ...transcriptEnvelopes.createdAtMs,
      );
      generations.push({
        generationId,
        channelId: scope.channelId,
        target: scope.target,
        providerAuthorityPubkey: authority.pubkey,
        authoritySource: authority.source,
        title: metadata?.title ?? null,
        projectRef: metadata?.projectRef ?? null,
        repoRef: metadata?.repoRef ?? null,
        sessionRef:
          metadata?.sessionRef ?? authority.create?.sessionRef ?? null,
        provider: metadata?.provider ?? null,
        runtime: metadata?.runtime ?? null,
        model: metadata?.model ?? authority.create?.model ?? null,
        agentRef: metadata?.agentRef ?? null,
        capabilities: metadata?.capabilities ?? null,
        status: metadata?.status ?? "unknown",
        statusAt: selected.value ? selected.value.createdAt * 1000 : null,
        lastEventAt,
        conflictCount:
          selected.conflictCount + transcriptEnvelopes.conflictCount,
        transcript: projectCodingSessionTranscript(transcriptEnvelopes.items, {
          generationId,
          blockKey: buildCodingSessionExecutionKey(
            authority.pubkey,
            scope.target,
          ),
        }),
      });
      if (authority.acceptedCommandId !== null) {
        acceptedCommandIdByGenerationId.set(
          generationId,
          authority.acceptedCommandId,
        );
      }
    }

    generations.sort(
      (left, right) =>
        right.lastEventAt - left.lastEventAt ||
        left.generationId.localeCompare(right.generationId),
    );

    const leasesByTarget = new Map<string, CodingSessionLease[]>();
    for (const lease of this.leases.values()) {
      if (!allowed.has(lease.channelId)) continue;
      const key = factKey(lease.channelId, lease.targetKey);
      const bucket = leasesByTarget.get(key);
      if (bucket) bucket.push(lease);
      else leasesByTarget.set(key, [lease]);
    }

    return {
      generations,
      creates: this.receiptJoinedCreates(createsByCommandId, allowed),
      genesisByEventId: new Map(this.genesis),
      names: [...this.names.values()].filter((name) =>
        allowed.has(name.channelId),
      ),
      closures: [...this.closures.values()].filter((closure) =>
        allowed.has(closure.channelId),
      ),
      goals: [...this.goals.values()].filter((goal) =>
        allowed.has(goal.channelId),
      ),
      leasesByTarget,
      acceptedCommandIdByGenerationId,
      malformedCount: this.malformedCount,
      invalidSignatureCount: this.invalidSignatureCount,
      conflictCount,
    };
  }

  /**
   * Creates indexed by `(channel, commandId)`, with the disputed ones removed.
   *
   * A `commandId` is public in the channel, so copying one into a second
   * create is trivial. Two creates under one id that disagree about who signed
   * them, which session they claim, which genesis anchors it, or which
   * provider may answer are irreconcilable, and picking a winner would let the
   * copy decide — so neither is believed and the pair is counted as a
   * conflict. The generation itself is NOT dropped: it falls through to D5's
   * disclosed fallback and renders as "authority unverified". Two creates
   * agreeing on all four are the same claim republished, not a dispute.
   */
  private undisputedCreatesByCommandId(allowed: ReadonlySet<string>): {
    creates: Map<string, CodingSessionLifecycleCommand>;
    conflictCount: number;
  } {
    const buckets = new Map<string, CodingSessionLifecycleCommand[]>();
    for (const create of this.creates.values()) {
      if (!allowed.has(create.channelId)) continue;
      const key = factKey(create.channelId, create.commandId);
      const bucket = buckets.get(key);
      if (bucket) bucket.push(create);
      else buckets.set(key, [create]);
    }
    const creates = new Map<string, CodingSessionLifecycleCommand>();
    let conflictCount = 0;
    for (const [key, bucket] of buckets) {
      const disputed =
        new Set(bucket.map((create) => create.signerPubkey)).size > 1 ||
        new Set(bucket.map((create) => create.sessionRef)).size > 1 ||
        new Set(bucket.map((create) => create.genesisRef)).size > 1 ||
        new Set(bucket.map((create) => create.providerAuthorityPubkey)).size >
          1;
      if (disputed) {
        conflictCount += bucket.length - 1;
        continue;
      }
      // Ingest order must never decide: the earliest signed claim wins, ties
      // broken on event id.
      creates.set(
        key,
        [...bucket].sort(
          (left, right) =>
            left.createdAt - right.createdAt ||
            left.eventId.localeCompare(right.eventId),
        )[0],
      );
    }
    return { creates, conflictCount };
  }

  /**
   * The subset of {@link undisputedCreatesByCommandId} that a lifecycle
   * receipt signed by the create's OWN named provider answered.
   *
   * The fence travels with the signed create, so every member of the channel
   * resolves the same operator and founder — not just the one whose machine
   * happens to run that provider.
   */
  private receiptJoinedCreates(
    createsByCommandId: ReadonlyMap<string, CodingSessionLifecycleCommand>,
    allowed: ReadonlySet<string>,
  ): CodingSessionLifecycleCommand[] {
    const answered = new Set<string>();
    for (const receipt of this.lifecycleReceipts) {
      if (!allowed.has(receipt.channelId)) continue;
      answered.add(
        commandAnswerKey(
          receipt.channelId,
          receipt.commandId,
          receipt.signerPubkey,
        ),
      );
    }
    return [...createsByCommandId.values()].filter((create) =>
      answered.has(
        commandAnswerKey(
          create.channelId,
          create.commandId,
          create.providerAuthorityPubkey,
        ),
      ),
    );
  }

  /**
   * Whose facts count for this generation.
   *
   * The create names the provider authority; a receipt signed by anybody else
   * proves nothing. Only when no create is readable does the first-seen
   * metadata signer stand in — and then the record says so, so the surface can
   * disclose it (D5).
   */
  private resolveAuthority(
    scope: { channelId: string; targetKey: string; signerPubkey: string },
    receipts: readonly LifecycleReceiptRecord[],
    createsByCommandId: ReadonlyMap<string, CodingSessionLifecycleCommand>,
  ): {
    pubkey: string;
    source: CodingSessionAuthoritySource;
    acceptedCommandId: string | null;
    create: CodingSessionLifecycleCommand | null;
  } | null {
    const readableCreates = receipts
      .map((receipt) => ({
        receipt,
        create:
          createsByCommandId.get(factKey(scope.channelId, receipt.commandId)) ??
          null,
      }))
      .filter(
        (
          entry,
        ): entry is {
          receipt: LifecycleReceiptRecord;
          create: CodingSessionLifecycleCommand;
        } => entry.create !== null,
      );
    const authorized = readableCreates
      .filter(
        (entry) =>
          entry.create.providerAuthorityPubkey === entry.receipt.signerPubkey,
      )
      .sort(
        (left, right) =>
          left.receipt.createdAt - right.receipt.createdAt ||
          left.receipt.eventId.localeCompare(right.receipt.eventId),
      );
    if (authorized.length > 0) {
      const first = authorized[0];
      return {
        pubkey: first.receipt.signerPubkey,
        source: "create",
        acceptedCommandId: first.receipt.commandId,
        create: first.create,
      };
    }
    // A create IS readable for this stream and names somebody else. That is a
    // refusal, not a gap: falling back here would let any signer answer a
    // command addressed to a named provider.
    if (readableCreates.length > 0) return null;
    const bucket = this.metadata.get(factKey(scope.channelId, scope.targetKey));
    const firstSeen = [...(bucket?.records.values() ?? [])].sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    )[0];
    // The disclosed fallback (D5): with no readable create, the first-seen
    // metadata signer for this target stands in — and only that signer. The
    // record says `disclosed-fallback` so the surface must say so too.
    if (!firstSeen || firstSeen.signerPubkey !== scope.signerPubkey) {
      return null;
    }
    return {
      pubkey: scope.signerPubkey,
      source: "disclosed-fallback",
      acceptedCommandId: null,
      create: null,
    };
  }

  private transcriptEnvelopesFor(
    channelId: string,
    targetKey: string,
    signerPubkey: string,
  ): {
    items: {
      target: CodingSessionTarget;
      eventSeq: number;
      timestamp: number;
      turnId: string | null;
      item: unknown;
      eventId: string;
    }[];
    conflictCount: number;
    createdAtMs: number[];
  } {
    const items: {
      target: CodingSessionTarget;
      eventSeq: number;
      timestamp: number;
      turnId: string | null;
      item: unknown;
      eventId: string;
    }[] = [];
    const createdAtMs: number[] = [];
    let conflicts = 0;
    for (const bucket of this.transcripts.values()) {
      if (
        bucket.channelId !== channelId ||
        bucket.targetKey !== targetKey ||
        bucket.signerPubkey !== signerPubkey
      ) {
        continue;
      }
      const records = [...bucket.records.values()];
      const payloads = new Set(
        records.map((record) => record.canonicalPayload),
      );
      if (payloads.size > 1) {
        // Distinct payloads at one (target, signer, eventSeq): neither renders.
        conflicts += payloads.size - 1;
        continue;
      }
      const record = records[0];
      if (!record) continue;
      createdAtMs.push(record.createdAt * 1000);
      items.push({
        target: record.value.session,
        eventSeq: record.value.eventSeq,
        timestamp: record.value.timestamp,
        turnId: record.value.turnId,
        item: record.value.item,
        eventId: record.eventId,
      });
    }
    return { items, conflictCount: conflicts, createdAtMs };
  }

  private ingestFact(event: ObservedEvent, allowed: ReadonlySet<string>): void {
    const classified = classifyCodingSessionEvent(event, allowed);
    switch (classified.kind) {
      case "metadata": {
        const key = factKey(classified.channelId, classified.targetKey);
        const bucket = this.metadata.get(key) ?? {
          channelId: classified.channelId,
          targetKey: classified.targetKey,
          records: new Map(),
        };
        bucket.records.set(event.id, {
          eventId: event.id,
          createdAt: event.created_at,
          signerPubkey: classified.signerPubkey,
          canonicalPayload: classified.canonicalPayload,
          value: classified.metadata,
        });
        this.metadata.set(key, bucket);
        this.retainRaw(
          event,
          classified.channelId,
          classified.targetKey,
          classified.signerPubkey,
        );
        break;
      }
      case "transcript": {
        const key = factKey(
          classified.channelId,
          encodeStructuredKey(
            "coding-session-transcript-store/v1",
            classified.targetKey,
            classified.signerPubkey,
            String(classified.transcript.eventSeq),
          ),
        );
        const bucket = this.transcripts.get(key) ?? {
          channelId: classified.channelId,
          targetKey: classified.targetKey,
          signerPubkey: classified.signerPubkey,
          eventSeq: classified.transcript.eventSeq,
          records: new Map(),
        };
        bucket.records.set(event.id, {
          eventId: event.id,
          createdAt: event.created_at,
          signerPubkey: classified.signerPubkey,
          canonicalPayload: classified.canonicalPayload,
          value: classified.transcript,
        });
        this.transcripts.set(key, bucket);
        this.retainRaw(
          event,
          classified.channelId,
          classified.targetKey,
          classified.signerPubkey,
        );
        break;
      }
      case "receipt": {
        // Turn receipts are decoded and then dropped: they report what
        // happened to one turn and never create, confirm, or end a generation.
        if (isCodingSessionTurnReceiptStatus(classified.receipt.status)) break;
        const target = classified.receipt.session;
        if (!target) break;
        const targetKey = buildCodingSessionTargetKey(target);
        this.lifecycleReceipts.push({
          channelId: classified.channelId,
          commandId: classified.receipt.commandId,
          status: classified.receipt.status,
          signerPubkey: classified.signerPubkey,
          target,
          targetKey,
          createdAt: event.created_at,
          eventId: event.id,
        });
        this.retainRaw(
          event,
          classified.channelId,
          targetKey,
          classified.signerPubkey,
        );
        break;
      }
      case "invalid-signature":
        this.invalidSignatureCount += 1;
        break;
      case "malformed":
        this.malformedCount += 1;
        break;
      case "irrelevant":
        break;
    }
  }

  /** The session-scoped records: 44221, 44226, 44229, 44230, 44227, 24223. */
  private ingestSessionRecord(
    event: ObservedEvent,
    allowed: ReadonlySet<string>,
  ): boolean {
    const sessionKinds = new Set([
      KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      KIND_CODING_SESSION_GENESIS,
      KIND_CODING_SESSION_NAME,
      KIND_CODING_SESSION_CLOSURE,
      KIND_CODING_SESSION_GOAL,
      KIND_CODING_SESSION_LEASE,
    ]);
    if (!sessionKinds.has(event.kind)) return false;
    if (!isSignatureVerified(event)) {
      this.invalidSignatureCount += 1;
      return true;
    }
    if (event.kind === KIND_CODING_SESSION_LIFECYCLE_COMMAND) {
      const create = parseCodingSessionLifecycleCommand(event);
      if (create && allowed.has(create.channelId)) {
        this.creates.set(event.id, create);
      } else {
        this.malformedCount += 1;
      }
      return true;
    }
    if (event.kind === KIND_CODING_SESSION_GENESIS) {
      const genesis = parseCodingSessionGenesis(event);
      if (genesis && allowed.has(genesis.channelId)) {
        this.genesis.set(event.id, genesis);
      } else {
        this.malformedCount += 1;
      }
      return true;
    }
    if (event.kind === KIND_CODING_SESSION_NAME) {
      const name = parseCodingSessionName(event);
      if (name && allowed.has(name.channelId)) this.names.set(event.id, name);
      else this.malformedCount += 1;
      return true;
    }
    if (event.kind === KIND_CODING_SESSION_CLOSURE) {
      const closure = parseCodingSessionClosure(event);
      if (closure && allowed.has(closure.channelId)) {
        this.closures.set(event.id, closure);
      } else {
        this.malformedCount += 1;
      }
      return true;
    }
    if (event.kind === KIND_CODING_SESSION_GOAL) {
      const goal = parseCodingSessionGoal(event);
      if (goal && allowed.has(goal.channelId)) this.goals.set(event.id, goal);
      else this.malformedCount += 1;
      return true;
    }
    const lease = parseCodingSessionLease(event);
    if (lease && allowed.has(lease.channelId)) this.leases.set(event.id, lease);
    else this.malformedCount += 1;
    return true;
  }

  private retainRaw(
    event: ObservedEvent,
    channelId: string,
    targetKey: string,
    signerPubkey: string,
  ): void {
    const key = generationRawKey(channelId, targetKey, signerPubkey);
    const retained =
      this.rawEvents.get(key) ?? new Map<string, ObservedEvent>();
    retained.set(event.id, event);
    // Map iteration is insertion-ordered, so the first key is the least
    // recently ingested — the one a refetch can most cheaply recover.
    while (retained.size > this.maxRetainedRawEventsPerGeneration) {
      const oldest = retained.keys().next();
      if (oldest.done) break;
      retained.delete(oldest.value);
    }
    this.rawEvents.set(key, retained);
  }
}

/** The execution label for one generation record. */
export function generationExecutionLabel(
  record: CodingSessionGenerationRecord,
): string {
  return formatCodingSessionExecutionLabel({
    runtime: record.runtime,
    model: record.model,
    agentRef: record.agentRef,
    driver: record.target.driver,
  });
}

/** The `(channel, target)` index key leases and metadata are bucketed under. */
export function codingSessionTargetFactKey(
  channelId: string,
  target: CodingSessionTarget,
): string {
  return factKey(channelId, buildCodingSessionTargetKey(target));
}

function resolveNewestMetadata(
  records: ReadonlyMap<string, Stored<Readonly<BuzzCodingSessionMetadataV1>>>,
  signerPubkey: string,
): {
  value: Stored<Readonly<BuzzCodingSessionMetadataV1>> | null;
  conflictCount: number;
} {
  const matching = [...records.values()].filter(
    (record) => record.signerPubkey === signerPubkey,
  );
  if (matching.length === 0) return { value: null, conflictCount: 0 };
  const newestAt = Math.max(...matching.map((record) => record.createdAt));
  const newest = matching.filter((record) => record.createdAt === newestAt);
  const payloads = new Set(newest.map((record) => record.canonicalPayload));
  return {
    value: newest.sort((left, right) =>
      left.eventId.localeCompare(right.eventId),
    )[0],
    conflictCount: Math.max(0, payloads.size - 1),
  };
}

function factKey(channelId: string, semanticKey: string): string {
  return encodeStructuredKey(
    "coding-session-ingress-index/v1",
    channelId,
    semanticKey,
  );
}

/** The `(channel, command, provider)` key a receipt-joined create is proven by. */
function commandAnswerKey(
  channelId: string,
  commandId: string,
  providerPubkey: string,
): string {
  return encodeStructuredKey(
    "coding-session-command-answer/v1",
    channelId,
    commandId,
    providerPubkey,
  );
}

function generationRawKey(
  channelId: string,
  targetKey: string,
  signerPubkey: string,
): string {
  return encodeStructuredKey(
    "coding-session-generation-raw/v1",
    channelId,
    targetKey,
    signerPubkey,
  );
}
