/**
 * Who drove a coding-session turn, and what to call them.
 *
 * A coding session is multi-operator: the founder plus any granted operators
 * can each send turns into the same execution. The provider stamps the
 * verified commanding signer onto every `user_prompt` item it publishes
 * (`operatorPubkey`), so the transcript can name the operator instead of
 * assuming every user message belongs to whoever happens to be reading it.
 *
 * Pure on purpose — the renderer supplies the local identity and the profile
 * lookup it already has, and nothing here touches React or the network.
 */

import {
  resolveUserLabel,
  type UserProfileLookup,
} from "@/features/profile/lib/identity";

const LOWERCASE_HEX_PUBKEY = /^[0-9a-f]{64}$/;

/**
 * Read an operator pubkey off an untrusted value, or `null`.
 *
 * Case is folded before the shape check so a provider that published
 * uppercase hex still attributes correctly; anything that is not 64 hex
 * characters is discarded rather than displayed as a mystery string.
 */
export function normalizeOperatorPubkey(value: unknown): string | null {
  if (typeof value !== "string") {
    return null;
  }
  const normalized = value.trim().toLowerCase();
  return LOWERCASE_HEX_PUBKEY.test(normalized) ? normalized : null;
}

/**
 * Command-id prefix every automatic team wake carries.
 *
 * Desktop's fallback mints `team-wake-v1:<source>:<digest>` (and `…:r1` for
 * its single re-arm); the provider's own wake mints `team-wake-<hex>`. Both
 * begin with this, and both are signed with a key that belongs to a person who
 * did not type them — which is exactly why the prefix has to be consulted
 * before the signer is.
 */
export const CODING_SESSION_TEAM_WAKE_COMMAND_PREFIX = "team-wake-";

/**
 * A signed vouch that one execution is a seat this computer hired.
 *
 * The only thing that may turn a prompt into a hire-host dispatch. It is
 * produced by joining this host's own hire record to the execution by **actor
 * and session**, and it carries the key that signed the create — so the branch
 * below can *require* the signer to be that host rather than overriding it.
 *
 * The first attempt derived the same fact from the `[From the lead] ` text
 * prefix, which is not a signed fact at all: it took a lane message signed by
 * the builder seat and rendered it as `Your Desktop (hire host)`, and it took a
 * prompt stamped with the reader's own key away from them on the strength of a
 * string (REVIEW-B3 N1). Text is never consulted here again.
 */
export type CodingSessionHireDispatchVouch = {
  /** What to call the lead that asked, already compared with the hire's signer. */
  label: string | null;
  /** The key that signed the seated create — this computer's own operator. */
  hostPubkey: string;
};

/** What a `team-wake-` prompt is called. Never `You`; nobody typed it. */
export const CODING_SESSION_TEAM_WAKE_AUTHOR_LABEL = "Beekeeper · team wake";

/**
 * What a hire-host dispatch is called when the hiring seat cannot be named.
 * The founder's Desktop signed it, but the words are a lead's brief.
 */
export const CODING_SESSION_HIRE_HOST_AUTHOR_LABEL = "Your Desktop (hire host)";

/** What a prompt carrying no operator stamp at all is called. */
export const CODING_SESSION_UNRECORDED_OPERATOR_LABEL = "Operator not recorded";

/** How a prompt's author was established. The renderer styles on this. */
export type CodingSessionPromptAuthorKind =
  /** Stamped with the viewer's own key, and nothing automatic about it. */
  | "you"
  /** Another human operator with a grant on this session. */
  | "operator"
  /** A seat in this umbrella sending a turn to another seat. */
  | "seat"
  /** An automatic `team-wake-` command, whoever signed it. */
  | "team-wake"
  /** The hire host dispatching a brief a lead wrote. */
  | "hire-host"
  /** No operator stamp exists. Unknown, stated as unknown. */
  | "unrecorded";

export type CodingSessionPromptAuthor = {
  /** The label to render. */
  label: string;
  kind: CodingSessionPromptAuthorKind;
  /**
   * The seat's execution key when the author is a seat this umbrella knows,
   * so the row can draw that seat's monogram and accent. `null` otherwise.
   */
  executionKey: string | null;
};

/**
 * Names a seat from its actor pubkey.
 *
 * `label` is the seat's whole display line — `buildCodingSessionTurnByline`
 * already renders a named seat as `Name · Role`, so this is not a bare name.
 * Return `null` for a pubkey that is not a seat in this umbrella.
 */
export type CodingSessionPromptSeatResolver = (pubkey: string) => {
  label: string | null;
  executionKey: string | null;
} | null;

/** True when `commandId` names an automatic team wake. */
export function isCodingSessionTeamWakeCommandId(
  commandId: string | null | undefined,
): boolean {
  return (
    typeof commandId === "string" &&
    commandId.startsWith(CODING_SESSION_TEAM_WAKE_COMMAND_PREFIX)
  );
}

/**
 * Who opened this turn, and what to call them.
 *
 * The rule this replaces labelled any prompt whose `operatorPubkey` matched
 * the viewer's key as `"You"`, and any prompt with **no** stamp as `"You"`
 * too. Both were wrong in the same direction — they put the reader's name on
 * words the reader never wrote:
 *
 * - Desktop signs its automatic commands with the founder's key. The
 *   `team-wake-v1:` fallback wake and the hire host's dispatch of a
 *   *lead-written* brief both came back reading `You` to the founder.
 * - A seat sending a turn to another seat is stamped with the seat's actor
 *   key, which is not the viewer's — but with no seat resolver it rendered as
 *   a truncated hash instead of the seat's name.
 * - An unstamped prompt is simply unknown. `"You"` was a guess, and a
 *   person's own typed turn always carries their stamp, so the guess was
 *   wrong more often than it was right.
 *
 * Resolution order, and why: the automatic prefixes come **first**, because
 * they are the cases where the signer is genuinely misleading. A seat comes
 * before the viewer check so a seat is never mistaken for the reader, and is
 * resolved without `currentPubkey` so it can never collapse to `"You"`.
 * `"You"` is left to a prompt stamped with the viewer's own key and no
 * automatic prefix — the one case where the reader really did type it.
 */
export function resolveCodingSessionPromptAuthor(input: {
  /** The 44220 command id the provider stamped on this echo, if any. */
  commandId?: string | null;
  currentUserPubkey?: string | null;
  /**
   * Signed evidence that this execution is a seat this computer hired, when
   * there is any. See {@link CodingSessionHireDispatchVouch}.
   */
  hireDispatch?: CodingSessionHireDispatchVouch | null;
  operatorPubkey?: string | null;
  profiles?: UserProfileLookup;
  resolveSeat?: CodingSessionPromptSeatResolver;
}): CodingSessionPromptAuthor {
  if (isCodingSessionTeamWakeCommandId(input.commandId)) {
    return {
      label: CODING_SESSION_TEAM_WAKE_AUTHOR_LABEL,
      kind: "team-wake",
      executionKey: null,
    };
  }
  const operatorPubkey = normalizeOperatorPubkey(input.operatorPubkey);
  // A hire-host dispatch is the initial turn carried *inside* a seated create,
  // and all three of these are facts about signed events:
  //
  //   1. a hire record vouches for this execution (actor **and** session),
  //   2. the prompt carries no 44220 command id — a later turn to the same
  //      seat is a command and does carry one,
  //   3. the stamped operator **is** the host that signed that create.
  //
  // (3) is what keeps this from outranking the signer: the branch does not
  // override `operatorPubkey`, it requires a particular value of it.
  if (
    input.hireDispatch &&
    !input.commandId &&
    operatorPubkey !== null &&
    operatorPubkey === normalizeOperatorPubkey(input.hireDispatch.hostPubkey)
  ) {
    const hiringSeatLabel = input.hireDispatch.label?.trim();
    return {
      label: hiringSeatLabel
        ? `${hiringSeatLabel} · via your Desktop`
        : CODING_SESSION_HIRE_HOST_AUTHOR_LABEL,
      kind: "hire-host",
      executionKey: null,
    };
  }
  if (!operatorPubkey) {
    return {
      label: CODING_SESSION_UNRECORDED_OPERATOR_LABEL,
      kind: "unrecorded",
      executionKey: null,
    };
  }
  const seat = input.resolveSeat?.(operatorPubkey) ?? null;
  if (seat) {
    return {
      // No `currentPubkey`: a seat is never the reader, and passing it would
      // let a seat collapse to `"You"` — the exact bug this exists to fix.
      label:
        seat.label?.trim() ||
        resolveUserLabel({ profiles: input.profiles, pubkey: operatorPubkey }),
      kind: "seat",
      executionKey: seat.executionKey,
    };
  }
  const currentUserPubkey = normalizeOperatorPubkey(input.currentUserPubkey);
  return {
    label: resolveUserLabel({
      currentPubkey: currentUserPubkey ?? undefined,
      profiles: input.profiles,
      pubkey: operatorPubkey,
    }),
    kind:
      currentUserPubkey !== null && currentUserPubkey === operatorPubkey
        ? "you"
        : "operator",
    executionKey: null,
  };
}

/**
 * The label to show above a user message in a coding-session transcript.
 *
 * Thin wrapper over {@link resolveCodingSessionPromptAuthor} for call sites
 * that need only the words.
 */
export function resolveCodingSessionPromptAuthorLabel(input: {
  commandId?: string | null;
  currentUserPubkey?: string | null;
  hireDispatch?: CodingSessionHireDispatchVouch | null;
  operatorPubkey?: string | null;
  profiles?: UserProfileLookup;
  resolveSeat?: CodingSessionPromptSeatResolver;
}): string {
  return resolveCodingSessionPromptAuthor(input).label;
}

/**
 * The distinct foreign operators appearing in `items`, sorted for a stable
 * reference across renders.
 *
 * Only foreign operators are returned: the local user's own profile never
 * needs resolving, because they render as `"You"`. The result feeds the batch
 * profile query, so a transcript driven by one operator issues one lookup no
 * matter how many turns it contains.
 */
export function collectForeignOperatorPubkeys(
  items: readonly unknown[],
  currentUserPubkey: string | null | undefined,
): string[] {
  const local = normalizeOperatorPubkey(currentUserPubkey);
  const found = new Set<string>();
  for (const item of items) {
    // Read positionally rather than by transcript-item type: only user
    // messages carry the field, and every other variant simply has no
    // `operatorPubkey` to find.
    const operatorPubkey =
      typeof item === "object" && item !== null
        ? normalizeOperatorPubkey(
            (item as { operatorPubkey?: unknown }).operatorPubkey,
          )
        : null;
    if (operatorPubkey && operatorPubkey !== local) {
      found.add(operatorPubkey);
    }
  }
  return [...found].sort();
}

/**
 * The requester's name for a seat this computer hired, or null.
 *
 * Reads the hire host's own outcome store — the record of what *this* Desktop
 * did — and joins it to an execution by the actor it seated **and the umbrella
 * it was seated into**. Only the founder's machine hosts hires, so elsewhere
 * this is null and the ordinary signer-based byline renders instead of a name
 * nobody here can check.
 */
export function codingSessionHireDispatchLabelForSeat(
  outcomes: readonly {
    sessionRef?: string | null;
    seatActor?: string | null;
    requesterLabel?: string | null;
    hostPubkey?: string | null;
  }[],
  scope: {
    actorPubkey: string | null | undefined;
    sessionRef: string | null | undefined;
  },
): CodingSessionHireDispatchVouch | null {
  const actor = scope.actorPubkey?.trim().toLowerCase();
  const sessionRef = scope.sessionRef?.trim();
  // Both, always. Joining on the actor alone showed the newer requester's name
  // on both sessions when one identity was hired twice by two different leads
  // (REVIEW-B3 N1).
  if (!actor || !sessionRef) return null;
  for (let index = outcomes.length - 1; index >= 0; index -= 1) {
    const outcome = outcomes[index];
    if (outcome.seatActor?.trim().toLowerCase() !== actor) continue;
    if (outcome.sessionRef?.trim() !== sessionRef) continue;
    const hostPubkey = outcome.hostPubkey?.trim().toLowerCase();
    // No host key, no vouch: without it the renderer could not require the
    // signer to be this computer's own operator, and the branch would be a
    // guess again.
    if (!hostPubkey) return null;
    return { label: outcome.requesterLabel?.trim() || null, hostPubkey };
  }
  return null;
}
