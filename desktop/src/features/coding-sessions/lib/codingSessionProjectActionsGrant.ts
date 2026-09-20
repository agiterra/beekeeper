import {
  codingSessionGrantRetryDelayMs,
  isCodingSessionGrantRateLimited,
} from "./codingSessionGrantRetry";
import {
  codingSessionProjectActionGrantKey,
  fetchCodingSessionRosterFold,
  publishCodingSessionAuthorityTransition,
  type CodingSessionRosterFold,
} from "./codingSessionRoster";

/**
 * The narrow delegation that lets a team session's lead finish its own work.
 *
 * ## The defect (ledger 186, finding 178(f))
 *
 * A team session was given "build kettle with tests and a verify action, land
 * it on main" and could not finish it. Publishing a project's kind:30620
 * action definition, and starting a manual kind:46020 run of one, is admitted
 * for the channel owner or admin plus the project's creator, a roster owner or
 * an endorsed repository's founder. A lead seat holds none of those and
 * nothing in setup could give it one, so the lead did the only honest thing
 * left and asked a person for a ruling.
 *
 * This publishes the `grant-project-actions` link in the session's own
 * kind:44228 authority chain, so the delegation is owner-signed, scoped to one
 * project, revocable by a later `revoke-project-actions`, and readable by
 * anyone who can read the chain. It delegates publishing that project's action
 * definitions and starting manual runs of them — never approving a host step
 * (kind:46030), never another project, and no steering, hiring or read
 * authority of any kind.
 */

const RECEIPT_POLL_ATTEMPTS = 25;
const RECEIPT_POLL_DELAY_MS = 200;
/** The same clamp the operator grant applies to a rate-limited re-read. */
const RECEIPT_BACKPRESSURE_MAX_DELAY_MS = 2_000;

/**
 * What one call did, and — when it could not finish the sentence — why.
 *
 * `published-unconfirmed` is a third status rather than a thrown error on
 * purpose. Past the publish the transition exists; reporting that as a failure
 * would tell a person the lead has no delegation when it may well have one,
 * and would invite a caller to republish a write that already landed. The
 * status says exactly what is known: the write went out, this host could not
 * read the acceptance back.
 */
export type CodingSessionProjectActionsGrantResult = {
  status: "already-active" | "granted" | "published-unconfirmed";
  /** The transition this call published, or null when it published none. */
  eventId: string | null;
  /** Why the acceptance could not be confirmed; null otherwise. */
  reason: string | null;
};

type CodingSessionProjectActionsGrantDependencies = {
  fetchFold?: (
    channelId: string,
    genesisRef: string,
  ) => Promise<CodingSessionRosterFold>;
  publishTransition?: typeof publishCodingSessionAuthorityTransition;
  wait?: (milliseconds: number) => Promise<void>;
  receiptPollAttempts?: number;
};

/**
 * Read the fold once, distinguishing "the relay is busy" from "the relay
 * answered". Returns the rate-limit sentence rather than throwing, because
 * past the publish a thrown read error becomes a republished grant.
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

/**
 * Ensure one pubkey holds a receipt-backed delegation over one project's
 * actions on this session's authority chain.
 *
 * The accepted fold is read before signing, so a replayed launch is a no-op
 * rather than a second identical link. After publication the relay's own
 * acceptance receipt must appear in the verified fold before this reports
 * `granted`: the fold is the only read on this side that proves the relay
 * accepted the link, and a grant nobody confirmed is exactly the comfortable
 * guess this codebase refuses to print.
 */
export async function ensureCodingSessionProjectActionsGrant(
  input: {
    channelId: string;
    genesisRef: string;
    actorPubkey: string;
    projectRef: string;
  },
  dependencies: CodingSessionProjectActionsGrantDependencies = {},
): Promise<CodingSessionProjectActionsGrantResult> {
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
  const key = codingSessionProjectActionGrantKey(actorPubkey, input.projectRef);

  const before = await fetchFold(input.channelId, input.genesisRef);
  if (before.projectActionGrants.has(key)) {
    return { status: "already-active", eventId: null, reason: null };
  }

  const event = await publishTransition({
    channelId: input.channelId,
    genesisRef: input.genesisRef,
    type: "grant-project-actions",
    granteePubkey: actorPubkey,
    projectRef: input.projectRef,
  });
  // Past this line the 44228 exists, so a rate-limited read is waited out and
  // re-read rather than thrown: a caller that retried the whole grant would
  // publish a second transition for a write that already landed (item 103
  // review, F2 — the same rule the operator and seat grants follow).
  let backPressured: string | null = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const read = await readFoldThroughBackPressure(
      fetchFold,
      input.channelId,
      input.genesisRef,
    );
    if ("fold" in read) {
      backPressured = null;
      if (read.fold.projectActionGrants.has(key)) {
        return { status: "granted", eventId: event.id, reason: null };
      }
    } else {
      backPressured = read.rateLimited;
    }
    if (attempt + 1 < attempts) {
      await wait(
        backPressured === null
          ? RECEIPT_POLL_DELAY_MS
          : Math.min(
              codingSessionGrantRetryDelayMs(backPressured, attempt + 1),
              RECEIPT_BACKPRESSURE_MAX_DELAY_MS,
            ),
      );
    }
  }
  return {
    status: "published-unconfirmed",
    eventId: event.id,
    reason:
      backPressured === null
        ? "The relay accepted the project-actions delegation, but its signed receipt did not appear in this session's authority chain."
        : "The relay accepted the project-actions delegation, but it answered every confirmation read with back-pressure, so this host could not verify the receipt.",
  };
}

/**
 * The one sentence a launch says when the delegation did not go out.
 *
 * Named here rather than in the launch sequence so the copy sits beside the
 * capability it describes, and so `codingSessionCrewLaunch.ts` stays under the
 * repository's 1000-line file gate. It names what the lead cannot do and who
 * can fix it, because a lead that silently lacks this standing is what made
 * ledger 178(f) look like an agent that would not finish its work.
 */
export function codingSessionProjectActionsGrantDisclosure(input: {
  leadLabel: string;
  detail: string | null;
}): string {
  const said = input.detail?.trim();
  return `${input.leadLabel} cannot publish or trigger this project's actions — a project owner must sign that delegation for this session's lead.${
    said ? ` ${said}` : ""
  }`;
}

/**
 * And the sentence for the middle case: the delegation was published and this
 * host could not read the acceptance back. Deliberately not the sentence
 * above — claiming the lead cannot act when it may well be able to is the
 * same class of lie as claiming it can.
 */
export function codingSessionProjectActionsGrantUnconfirmedDisclosure(input: {
  leadLabel: string;
  detail: string | null;
}): string {
  const said = input.detail?.trim();
  return `Whether ${input.leadLabel} may publish and trigger this project's actions is unconfirmed on this computer — check the session's access list.${
    said ? ` ${said}` : ""
  }`;
}
