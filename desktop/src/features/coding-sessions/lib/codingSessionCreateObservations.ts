/**
 * Human-signed 44221 create observations, joined to executions by receipt.
 *
 * Creates are signed by *people*, so they can never enter the trusted ingress
 * store: that store's whole contract is "provider-authority-signed facts
 * only", and a human create is not a provider fact. Operator authority is
 * nevertheless a fact *about humans*: a create signer is that execution's
 * operator, while a genesis signer becomes founder only when reached through
 * a receipt-joined create's explicit event-id reference. Both human-signed
 * lanes live on this subscription and stay out of the trusted provider store.
 *
 * An observation asserts exactly one thing: *this pubkey signed a create with
 * this commandId claiming this sessionRef, and the provider-signed 44224
 * receipt for that same commandId minted this target.* Nothing else is
 * inferred. In particular:
 *
 * - **Only receipt-joined creates are observed.** A create nobody's provider
 *   ever acted on mints no execution, so it is not evidence of operating one —
 *   and requiring the join is what stops a member from backdating a create
 *   bearing someone else's `sessionRef` to claim foundership over a session
 *   they never ran. The receipt is verified against the same configured
 *   provider authority the trusted ingress uses.
 * - **Ambiguity yields no observation, never a guess.** Two different signers
 *   (or two different `sessionRef` claims) on one commandId, or two receipts
 *   naming different targets for it, resolve to nothing. Downstream that means
 *   `founderPubkey`/`operatorPubkey` stay null, which is the permissive
 *   fallback — an unresolved claim must never lock a legitimate operator out.
 */
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  CODING_SESSION_GENESIS_SCHEMA_VERSION,
  CODING_SESSION_GENESIS_TAG_VERSION,
  MAX_CODING_SESSION_GENESIS_CONTENT_BYTES,
} from "./codingSessionGenesis";
import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import type { CodingSessionIngressAuthority } from "./codingSessionIngressAuthority";
import { encodeStructuredKey } from "./codingSessionKeys";
import {
  CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
  CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION,
  MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
} from "./codingSessionLifecycleCommand";
import { classifyTrustedCodingSessionIngressEvent } from "./codingSessionTrustedIngress";
import type { CodingSessionUmbrellaCreateObservation } from "./codingSessionUmbrellaModel";
import {
  boundedNonempty,
  hasExactKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./codingSessionWireDecode";

export type { CodingSessionUmbrellaCreateObservation };

/**
 * Human create + genesis, and the provider receipt that joins a create to a
 * minted execution target.
 */
export const CODING_SESSION_CREATE_OBSERVATION_KINDS = [
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_GENESIS,
] as const;

/**
 * No `authors` narrowing, deliberately: any channel member may found a session
 * (that is today's create rule), so there is no allowlist to scope creates by.
 * The receipts that arrive on the same filter are still re-verified against the
 * configured provider authority by the classifier before any join is made, so
 * widening the subscription widens what is *read*, never what is *trusted*.
 */
export function buildCodingSessionCreateObservationFilter(
  channelIds: readonly string[],
  limit: number,
): RelaySubscriptionFilter {
  return {
    kinds: [...CODING_SESSION_CREATE_OBSERVATION_KINDS],
    "#h": [...channelIds],
    limit,
  };
}

export type CodingSessionCreateClassification =
  | {
      kind: "create";
      channelId: string;
      commandId: string;
      signerPubkey: string;
      sessionRef: string | null;
      genesisRef: string | null;
    }
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "invalid-signature" };

/**
 * Read the three authority facts off a signed 44221: who signed it, which
 * command it is, and which umbrella it claimed.
 *
 * The envelope and action are checked exactly — kind, the producer's three
 * tags in order, the schema, the commandId agreeing with its tag, and one of
 * R9's three accepted create forms. Unknown action keys bind no authority:
 * the desktop must not give a smuggled payload a more permissive meaning than
 * the provider that acts on it.
 */
export function classifyCodingSessionCreateEvent(
  event: RelayEvent,
  allowedChannelIds: ReadonlySet<string>,
): CodingSessionCreateClassification {
  if (
    event.kind !== KIND_CODING_SESSION_LIFECYCLE_COMMAND ||
    !Array.isArray(event.tags)
  ) {
    return { kind: "irrelevant" };
  }
  const tags = parseExactTags(event.tags, ["h", "csl-v", "csl-command"]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!signerPubkey) return { kind: "malformed" };
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };

  const payload = parseBoundedJson(
    event.content,
    MAX_CODING_SESSION_LIFECYCLE_CONTENT_BYTES,
  );
  if (
    !isPlainRecord(payload) ||
    !hasExactKeys(payload, ["schema", "commandId", "action"]) ||
    payload.schema !== CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA ||
    !boundedNonempty(
      payload.commandId,
      MAX_CODING_SESSION_LIFECYCLE_IDENTIFIER_BYTES,
    ) ||
    payload.commandId !== tags[2] ||
    !isPlainRecord(payload.action) ||
    payload.action.type !== "session.create"
  ) {
    return { kind: "malformed" };
  }
  const hasSessionRef = Object.hasOwn(payload.action, "sessionRef");
  const hasGenesisRef = Object.hasOwn(payload.action, "genesisRef");
  const createKeys = [
    "type",
    "projectRef",
    "repoRef",
    ...(hasSessionRef ? ["sessionRef"] : []),
    ...(hasGenesisRef ? ["genesisRef"] : []),
    "providerInstanceRef",
    "providerAuthorityPubkey",
    "model",
    "title",
    "initialTurn",
  ];
  if (
    !hasExactKeys(payload.action, createKeys) ||
    (hasGenesisRef && !hasSessionRef)
  ) {
    return { kind: "malformed" };
  }
  // Absent is the historical 8-key form ("no umbrella claimed"); an explicit
  // null means the same thing on a new create. Anything present must be a
  // canonical UUID — a malformed claim is refused rather than coerced.
  const claimed = payload.action.sessionRef;
  if (
    claimed !== undefined &&
    claimed !== null &&
    !isCodingSessionSessionRef(claimed)
  ) {
    return { kind: "malformed" };
  }
  const genesisRef = payload.action.genesisRef;
  if (
    genesisRef !== undefined &&
    (typeof genesisRef !== "string" ||
      !/^[0-9a-f]{64}$/.test(genesisRef) ||
      !hasSessionRef ||
      claimed === null)
  ) {
    return { kind: "malformed" };
  }
  return {
    kind: "create",
    channelId: tags[0],
    commandId: payload.commandId,
    signerPubkey,
    sessionRef: claimed ?? null,
    genesisRef: genesisRef ?? null,
  };
}

export type CodingSessionGenesisClassification =
  | {
      kind: "genesis";
      eventId: string;
      channelId: string;
      sessionRef: string;
      founderPubkey: string;
    }
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "invalid-signature" };

/** Validate a genesis for storage by id; its session tag is never a selector. */
export function classifyCodingSessionGenesisEvent(
  event: RelayEvent,
  allowedChannelIds: ReadonlySet<string>,
): CodingSessionGenesisClassification {
  if (
    event.kind !== KIND_CODING_SESSION_GENESIS ||
    !Array.isArray(event.tags)
  ) {
    return { kind: "irrelevant" };
  }
  const tags = parseExactTags(event.tags, ["h", "csg-v", "csg-session"]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_GENESIS_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const founderPubkey = normalizePubkey(event.pubkey);
  if (!founderPubkey || !/^[0-9a-f]{64}$/.test(event.id)) {
    return { kind: "malformed" };
  }
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };
  const payload = parseBoundedJson(
    event.content,
    MAX_CODING_SESSION_GENESIS_CONTENT_BYTES,
  );
  if (!isPlainRecord(payload)) return { kind: "malformed" };
  const hasAdopts = Object.hasOwn(payload, "adopts");
  if (
    !hasExactKeys(
      payload,
      hasAdopts ? ["sessionRef", "v", "adopts"] : ["sessionRef", "v"],
    ) ||
    payload.v !== CODING_SESSION_GENESIS_SCHEMA_VERSION ||
    !isCodingSessionSessionRef(payload.sessionRef) ||
    payload.sessionRef !== tags[2]
  ) {
    return { kind: "malformed" };
  }
  if (hasAdopts) {
    const adopts = payload.adopts;
    if (
      !isPlainRecord(adopts) ||
      !hasExactKeys(adopts, ["createEventId", "receiptEventId"]) ||
      typeof adopts.createEventId !== "string" ||
      !/^[0-9a-f]{64}$/.test(adopts.createEventId) ||
      typeof adopts.receiptEventId !== "string" ||
      !/^[0-9a-f]{64}$/.test(adopts.receiptEventId)
    ) {
      return { kind: "malformed" };
    }
  }
  return {
    kind: "genesis",
    eventId: event.id,
    channelId: tags[0],
    sessionRef: payload.sessionRef,
    founderPubkey,
  };
}

type StoredCreate = {
  eventId: string;
  channelId: string;
  commandId: string;
  signerPubkey: string;
  sessionRef: string | null;
  genesisRef: string | null;
  createdAt: number;
};

type StoredGenesis = Extract<
  CodingSessionGenesisClassification,
  { kind: "genesis" }
>;

type StoredReceiptTarget = {
  eventId: string;
  targetKey: string;
  target: CodingSessionCommandTarget;
};

/**
 * In-memory store for create observations and the receipts that join them.
 *
 * Community-scoped and owned by the hook that builds it, exactly like the
 * trusted ingress store: it holds no module-level state.
 */
export class CodingSessionCreateObservationStore {
  private readonly creates = new Map<string, Map<string, StoredCreate>>();
  private readonly receiptTargets = new Map<
    string,
    Map<string, StoredReceiptTarget>
  >();
  /** Keyed only by the explicit event id a receipt-joined create names. */
  private readonly geneses = new Map<string, StoredGenesis>();
  private readonly dispositions = new Map<string, string>();
  private malformedCount = 0;
  private invalidSignatureCount = 0;

  ingestRelayEvents(
    events: readonly RelayEvent[],
    channelIds: readonly string[],
    authority: CodingSessionIngressAuthority,
  ): void {
    const allowedChannels = new Set(channelIds);
    for (const event of events) {
      if (this.dispositions.has(event.id)) continue;
      if (event.kind === KIND_CODING_SESSION_GENESIS) {
        const classified = classifyCodingSessionGenesisEvent(
          event,
          allowedChannels,
        );
        this.dispositions.set(event.id, classified.kind);
        if (classified.kind === "malformed") this.malformedCount += 1;
        if (classified.kind === "invalid-signature") {
          this.invalidSignatureCount += 1;
        }
        if (classified.kind === "genesis") {
          this.geneses.set(classified.eventId, classified);
        }
        continue;
      }
      if (event.kind === KIND_CODING_SESSION_LIFECYCLE_COMMAND) {
        const classified = classifyCodingSessionCreateEvent(
          event,
          allowedChannels,
        );
        this.dispositions.set(event.id, classified.kind);
        if (classified.kind === "malformed") this.malformedCount += 1;
        if (classified.kind === "invalid-signature") {
          this.invalidSignatureCount += 1;
        }
        if (classified.kind !== "create") continue;
        const key = commandKey(classified.channelId, classified.commandId);
        const bucket = this.creates.get(key) ?? new Map<string, StoredCreate>();
        bucket.set(event.id, {
          eventId: event.id,
          channelId: classified.channelId,
          commandId: classified.commandId,
          signerPubkey: classified.signerPubkey,
          sessionRef: classified.sessionRef,
          genesisRef: classified.genesisRef,
          createdAt: event.created_at,
        });
        this.creates.set(key, bucket);
        continue;
      }
      if (event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT) continue;
      // Receipts are provider facts, so they run the trusted classifier —
      // same authority allowlist, same signature check, same exact-tag rules
      // the ingress store applies. Only the join is borrowed here.
      const classified = classifyTrustedCodingSessionIngressEvent(
        event,
        allowedChannels,
        authority,
      );
      this.dispositions.set(event.id, classified.kind);
      if (classified.kind !== "receipt") continue;
      const target = classified.receipt.session;
      if (!target) continue;
      const key = commandKey(
        classified.channelId,
        classified.receipt.commandId,
      );
      const bucket =
        this.receiptTargets.get(key) ?? new Map<string, StoredReceiptTarget>();
      bucket.set(event.id, {
        eventId: event.id,
        targetKey: buildCodingSessionTargetKey(target),
        target,
      });
      this.receiptTargets.set(key, bucket);
    }
  }

  /**
   * The joined observations for the visible channels, earliest first.
   *
   * Each surviving commandId contributes at most one observation; disputed
   * ones contribute none.
   */
  snapshot(
    channelIds: readonly string[],
  ): CodingSessionUmbrellaCreateObservation[] {
    const allowedChannels = new Set(channelIds);
    const observations: CodingSessionUmbrellaCreateObservation[] = [];
    for (const [key, bucket] of this.creates) {
      const records = [...bucket.values()].sort(compareCreateOrder);
      const first = records[0];
      if (!first || !allowedChannels.has(first.channelId)) continue;
      // One commandId, one signer, one claim — otherwise the create is
      // disputed and binds nothing.
      const disputed =
        new Set(records.map((record) => record.signerPubkey)).size > 1 ||
        new Set(records.map((record) => record.sessionRef)).size > 1 ||
        new Set(records.map((record) => record.genesisRef)).size > 1;
      if (disputed) continue;
      const target = this.resolveJoinedTarget(key);
      if (!target) continue;
      const genesis = first.genesisRef
        ? (this.geneses.get(first.genesisRef) ?? null)
        : null;
      observations.push({
        channelId: first.channelId,
        sessionRef: first.sessionRef,
        signerPubkey: first.signerPubkey,
        createdAt: first.createdAt,
        eventId: first.eventId,
        target,
        genesisRef: first.genesisRef,
        genesisFounderPubkey:
          genesis?.channelId === first.channelId &&
          genesis.sessionRef === first.sessionRef
            ? genesis.founderPubkey
            : null,
      });
    }
    observations.sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    );
    return observations;
  }

  /** Diagnostics only; refused creates never reach a projection. */
  counts(): { malformedCount: number; invalidSignatureCount: number } {
    return {
      malformedCount: this.malformedCount,
      invalidSignatureCount: this.invalidSignatureCount,
    };
  }

  /**
   * The execution target a command minted, per the provider's receipt.
   *
   * Receipts naming different targets for one commandId are a disagreement
   * between providers, and the design's discipline for a disputed claim is to
   * resolve nothing rather than pick a side.
   */
  private resolveJoinedTarget(key: string): CodingSessionCommandTarget | null {
    const bucket = this.receiptTargets.get(key);
    if (!bucket || bucket.size === 0) return null;
    const records = [...bucket.values()];
    const distinct = new Set(records.map((record) => record.targetKey));
    return distinct.size === 1 ? records[0].target : null;
  }
}

function compareCreateOrder(left: StoredCreate, right: StoredCreate): number {
  return (
    left.createdAt - right.createdAt ||
    left.eventId.localeCompare(right.eventId)
  );
}

function commandKey(channelId: string, commandId: string): string {
  return encodeStructuredKey(
    "coding-session-create-observation/v1",
    channelId,
    commandId,
  );
}
