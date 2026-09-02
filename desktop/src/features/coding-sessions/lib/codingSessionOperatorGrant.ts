import {
  codingSessionGrantRetryDelayMs,
  isCodingSessionGrantRateLimited,
} from "./codingSessionGrantRetry";
import {
  fetchCodingSessionRosterFold,
  publishCodingSessionAuthorityTransition,
  type CodingSessionRosterFold,
} from "./codingSessionRoster";

const RECEIPT_POLL_ATTEMPTS = 25;
const RECEIPT_POLL_DELAY_MS = 200;
/**
 * The longest one rate-limited confirmation read may wait before re-reading.
 *
 * The relay's `retry in Ns` hint can be as large as 300 s, and this loop runs
 * up to 25 times; without a clamp a single back-pressured confirmation could
 * hold a grant for hours.
 */
const RECEIPT_BACKPRESSURE_MAX_DELAY_MS = 2_000;

/**
 * Read the authority fold once, distinguishing "the relay is busy" from "the
 * relay answered".
 *
 * Returns null when the read was rate-limited. Every other failure is the
 * caller's to propagate: a malformed filter or a dropped socket is not a
 * reason to keep polling.
 */
async function readFoldThroughBackPressure(
  fetchFold: (
    channelId: string,
    genesisRef: string,
  ) => Promise<CodingSessionRosterFold>,
  channelId: string,
  genesisRef: string,
): Promise<{ fold: CodingSessionRosterFold } | { rateLimited: string }> {
  try {
    return { fold: await fetchFold(channelId, genesisRef) };
  } catch (error) {
    if (!isCodingSessionGrantRateLimited(error)) throw error;
    return {
      rateLimited: error instanceof Error ? error.message : String(error),
    };
  }
}

/** How long a rate-limited confirmation read waits before re-reading. */
function receiptBackPressureDelayMs(reason: string, attempt: number): number {
  return Math.min(
    codingSessionGrantRetryDelayMs(reason, attempt),
    RECEIPT_BACKPRESSURE_MAX_DELAY_MS,
  );
}

/**
 * What a grant says when the transition went out but no read could confirm it.
 *
 * Deliberately carries no `rate-limited:` token: the write already landed, so
 * this must never be retried by a caller that would republish it.
 */
const RECEIPT_BACKPRESSURE_MESSAGE =
  "The relay accepted the transition, but it answered every confirmation read with back-pressure, so this host could not verify the receipt.";

/** Whether this call appended a grant or reused the accepted active grant. */
export type CodingSessionOperatorGrantResult = {
  status: "already-active" | "granted";
  eventId: string | null;
};

export type CodingSessionCreateOperatorGrantResult =
  | { ok: true }
  | { ok: false; failed: "provider" | "actor"; reason: string };

type CodingSessionCreateOperatorGrantDependencies = {
  ensureGrant?: typeof ensureCodingSessionOperatorGrant;
};

type CodingSessionOperatorGrantDependencies = {
  fetchFold?: (
    channelId: string,
    genesisRef: string,
  ) => Promise<CodingSessionRosterFold>;
  publishTransition?: typeof publishCodingSessionAuthorityTransition;
  wait?: (milliseconds: number) => Promise<void>;
  receiptPollAttempts?: number;
};

/**
 * Ensure one pubkey holds a receipt-backed operator grant on the canonical
 * session authority chain.
 *
 * The accepted fold is checked before signing, so replaying a recovered
 * create is a no-op. After publication, the relay-signed acceptance receipt
 * must appear in the verified fold before this resolves. That postcondition
 * serializes callers which need to grant a provider authority and then its
 * actor without guessing that the relay's asynchronous receipt already ran.
 */
export async function ensureCodingSessionOperatorGrant(
  input: {
    channelId: string;
    genesisRef: string;
    granteePubkey: string;
  },
  dependencies: CodingSessionOperatorGrantDependencies = {},
): Promise<CodingSessionOperatorGrantResult> {
  const fetchFold = dependencies.fetchFold ?? fetchCodingSessionRosterFold;
  const publishTransition =
    dependencies.publishTransition ?? publishCodingSessionAuthorityTransition;
  const wait =
    dependencies.wait ??
    ((milliseconds: number) =>
      new Promise<void>((resolve) =>
        globalThis.setTimeout(resolve, milliseconds),
      ));
  const attempts = Math.max(
    1,
    dependencies.receiptPollAttempts ?? RECEIPT_POLL_ATTEMPTS,
  );

  const before = await fetchFold(input.channelId, input.genesisRef);
  if (before.accepted.get(input.granteePubkey.toLowerCase()) === "operator") {
    return { status: "already-active", eventId: null };
  }

  const event = await publishTransition({
    ...input,
    type: "grant-operator",
  });
  // Past this line the 44228 exists. A rate-limited read is waited out and
  // re-read here rather than thrown, because a caller that retries the whole
  // grant would publish a second transition for a write that already landed
  // (item 103 review, F2).
  let backPressured: string | null = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const read = await readFoldThroughBackPressure(
      fetchFold,
      input.channelId,
      input.genesisRef,
    );
    if ("fold" in read) {
      backPressured = null;
      if (
        read.fold.accepted.get(input.granteePubkey.toLowerCase()) === "operator"
      ) {
        return { status: "granted", eventId: event.id };
      }
    } else {
      backPressured = read.rateLimited;
    }
    if (attempt + 1 < attempts) {
      await wait(
        backPressured === null
          ? RECEIPT_POLL_DELAY_MS
          : receiptBackPressureDelayMs(backPressured, attempt + 1),
      );
    }
  }
  throw new Error(
    backPressured === null
      ? "The relay accepted the operator transition, but its signed receipt did not appear in the session authority chain."
      : RECEIPT_BACKPRESSURE_MESSAGE,
  );
}

/** Ensure one actor holds a receipt-backed governed seat and exact role. */
export async function ensureCodingSessionSeatGrant(
  input: {
    channelId: string;
    genesisRef: string;
    actorPubkey: string;
    role: string;
  },
  dependencies: CodingSessionOperatorGrantDependencies = {},
): Promise<CodingSessionOperatorGrantResult> {
  const fetchFold = dependencies.fetchFold ?? fetchCodingSessionRosterFold;
  const publishTransition =
    dependencies.publishTransition ?? publishCodingSessionAuthorityTransition;
  const wait =
    dependencies.wait ??
    ((milliseconds: number) =>
      new Promise<void>((resolve) =>
        globalThis.setTimeout(resolve, milliseconds),
      ));
  const attempts = Math.max(
    1,
    dependencies.receiptPollAttempts ?? RECEIPT_POLL_ATTEMPTS,
  );
  const actorPubkey = input.actorPubkey.toLowerCase();
  const before = await fetchFold(input.channelId, input.genesisRef);
  if (before.activeSeats.get(actorPubkey) === input.role) {
    return { status: "already-active", eventId: null };
  }
  if (before.activeSeats.has(actorPubkey)) {
    throw new Error("The actor already holds a different governed seat role.");
  }

  const event = await publishTransition({
    channelId: input.channelId,
    genesisRef: input.genesisRef,
    type: "grant-seat",
    granteePubkey: actorPubkey,
    role: input.role,
  });
  // Same rule as the operator grant above: the write has landed, so a
  // rate-limited read is re-read rather than turned into a republish.
  let backPressured: string | null = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const read = await readFoldThroughBackPressure(
      fetchFold,
      input.channelId,
      input.genesisRef,
    );
    if ("fold" in read) {
      backPressured = null;
      if (read.fold.activeSeats.get(actorPubkey) === input.role) {
        return { status: "granted", eventId: event.id };
      }
    } else {
      backPressured = read.rateLimited;
    }
    if (attempt + 1 < attempts) {
      await wait(
        backPressured === null
          ? RECEIPT_POLL_DELAY_MS
          : receiptBackPressureDelayMs(backPressured, attempt + 1),
      );
    }
  }
  throw new Error(
    backPressured === null
      ? "The relay accepted the seat transition, but its signed receipt did not appear in the session authority chain."
      : RECEIPT_BACKPRESSURE_MESSAGE,
  );
}

/**
 * Grant a newly created session's provider before its optional actor.
 *
 * Provider authority is the prerequisite for durable provider-owned wake. If
 * that first receipt-backed grant fails, the actor grant is never attempted:
 * advancing would leave an apparently steerable actor behind a provider that
 * cannot deliver its reports to the lead.
 */
export async function ensureCodingSessionCreateOperatorGrants(
  input: {
    channelId: string;
    genesisRef: string;
    providerAuthorityPubkey: string;
    actorPubkey: string | null;
  },
  dependencies: CodingSessionCreateOperatorGrantDependencies = {},
): Promise<CodingSessionCreateOperatorGrantResult> {
  const ensureGrant =
    dependencies.ensureGrant ?? ensureCodingSessionOperatorGrant;
  try {
    await ensureGrant({
      channelId: input.channelId,
      genesisRef: input.genesisRef,
      granteePubkey: input.providerAuthorityPubkey,
    });
  } catch (error) {
    return { ok: false, failed: "provider", reason: grantErrorReason(error) };
  }

  if (input.actorPubkey === null) return { ok: true };
  try {
    await ensureGrant({
      channelId: input.channelId,
      genesisRef: input.genesisRef,
      granteePubkey: input.actorPubkey,
    });
  } catch (error) {
    return { ok: false, failed: "actor", reason: grantErrorReason(error) };
  }
  return { ok: true };
}

function grantErrorReason(error: unknown): string {
  const reason = error instanceof Error ? error.message.trim() : String(error);
  return reason.length > 0 ? reason : "the grant did not go out";
}
