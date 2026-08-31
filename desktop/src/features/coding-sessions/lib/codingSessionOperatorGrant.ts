import {
  fetchCodingSessionRosterFold,
  publishCodingSessionAuthorityTransition,
  type CodingSessionRosterFold,
} from "./codingSessionRoster";

const RECEIPT_POLL_ATTEMPTS = 25;
const RECEIPT_POLL_DELAY_MS = 200;

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
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const confirmed = await fetchFold(input.channelId, input.genesisRef);
    if (
      confirmed.accepted.get(input.granteePubkey.toLowerCase()) === "operator"
    ) {
      return { status: "granted", eventId: event.id };
    }
    if (attempt + 1 < attempts) await wait(RECEIPT_POLL_DELAY_MS);
  }
  throw new Error(
    "The relay accepted the operator transition, but its signed receipt did not appear in the session authority chain.",
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
