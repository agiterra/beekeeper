/**
 * Human-signed 44221 create observations, joined to executions by receipt.
 *
 * Creates are signed by *people*, so they can never enter the trusted ingress
 * store: that store's whole contract is "provider-authority-signed facts
 * only", and a human create is not a provider fact. Operator authority is
 * nevertheless a fact *about humans* — the design's rule is that the signer of
 * a create is that execution's operator, and the signer of the earliest create
 * bearing a `sessionRef` is the umbrella founder — so it is collected here, on
 * its own subscription, and deliberately kept out of the trusted store.
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
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
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
 * The two kinds this collector reads: the human create and the provider
 * receipt that joins it to a minted execution target.
 */
export const CODING_SESSION_CREATE_OBSERVATION_KINDS = [
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
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
    }
  | { kind: "irrelevant" }
  | { kind: "malformed" }
  | { kind: "invalid-signature" };

/**
 * Read the three authority facts off a signed 44221: who signed it, which
 * command it is, and which umbrella it claimed.
 *
 * The envelope is checked exactly — kind, the producer's three tags in order,
 * the schema, and the commandId agreeing with its tag — but the *action* is
 * read leniently past `type` and `sessionRef`. This observer needs three
 * fields, not the whole command: the strict 8-or-9-key decoders are the
 * sidecar's and the relay's, and a create that reached a receipt already
 * passed the sidecar's. Going blind on a create form this client does not
 * fully understand would drop authority the provider itself honoured.
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
  return {
    kind: "create",
    channelId: tags[0],
    commandId: payload.commandId,
    signerPubkey,
    sessionRef: claimed ?? null,
  };
}

type StoredCreate = {
  eventId: string;
  channelId: string;
  commandId: string;
  signerPubkey: string;
  sessionRef: string | null;
  createdAt: number;
};

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
        new Set(records.map((record) => record.sessionRef)).size > 1;
      if (disputed) continue;
      const target = this.resolveJoinedTarget(key);
      if (!target) continue;
      observations.push({
        sessionRef: first.sessionRef,
        signerPubkey: first.signerPubkey,
        createdAt: first.createdAt,
        eventId: first.eventId,
        target,
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
