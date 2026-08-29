/**
 * How one turn block names the seat that produced it (SURFACES.md C1a).
 *
 * A coding-session umbrella is signed by **one** provider key on behalf of
 * every seat in it, so that key is byte-identical on all of them: it is
 * provenance, never identity. The 2026-08-29 walk (finding 4) read three
 * seats' blocks and found the same `1958c6c4…` in the identity slot on each,
 * while the seats' real actors were on the wire as `agentRef` all along.
 *
 * This module decides the byline from the actor, falling back down a ladder
 * that never lands on a key: actor and role, then role and runtime, then
 * runtime and model, and finally {@link CODING_SESSION_UNKNOWN_ACTOR}. The
 * provider instance stays reachable as hover text and in the screen-reader
 * sentence, under the word `via`.
 */

import { formatCodingSessionExecutionLabel } from "./codingSessionLabels";

/**
 * What a byline says when nothing on the wire names the actor.
 *
 * Preferred over a truncated signer key, which reads like an identity and is
 * the same on every seat, and over silence, which reads like a resolved name
 * the reader simply cannot see.
 */
export const CODING_SESSION_UNKNOWN_ACTOR = "unknown actor";

/** One turn block's byline: identity, optional detail, and provenance. */
export type CodingSessionTurnByline = {
  /** The identity slot. Never a pubkey, truncated or otherwise. */
  name: string;
  /**
   * Runtime and model, rendered only when the identity line does not already
   * carry them — a seat whose profile has not been read yet shows its role
   * plus what it is running on, per C1a.
   */
  detail: string | null;
  /**
   * `via <providerInstanceRef>`, or null when the metadata named no provider.
   * Hover and screen-reader only: the provider key itself lives in the
   * provenance popover's `Verified source` row.
   */
  via: string | null;
  /** The block's screen-reader sentence, e.g. `Response from X, Role, generation 1.` */
  screenReader: string;
};

/**
 * Build a turn block's byline from one generation's signed metadata.
 *
 * `label` is the participant label the surrounding surface already resolved
 * (it carries collision disambiguation the record alone cannot reproduce);
 * pass null when the surface has none. A label that is really a pubkey — full
 * hex or the canonical `abcd1234…wxyz` truncation — is refused, so an older
 * caller cannot smuggle the signer back into the identity slot.
 */
export function buildCodingSessionTurnByline(input: {
  /** The `agent_ref` the provider signed into 44223, when this is a seat. */
  agentRef: string | null | undefined;
  /** The seat's role slug. Non-null exactly when `agentRef` is. */
  role: string | null | undefined;
  /** Whatever kind-0 lookup resolved for `agentRef`, or null when unread. */
  agentDisplayName?: string | null;
  runtime: string | null | undefined;
  model: string | null | undefined;
  /** The advertised provider instance ref (44223 `provider`). */
  providerInstanceRef?: string | null;
  generation: number;
  /** The surface's own resolved participant label, when it has one. */
  label?: string | null;
}): CodingSessionTurnByline {
  const identity = formatCodingSessionExecutionLabel({
    agentRef: input.agentRef,
    role: input.role,
    agentDisplayName: input.agentDisplayName,
    runtime: input.runtime,
    model: input.model,
  });
  const seated =
    nonEmpty(input.agentRef) !== null && nonEmpty(input.role) !== null;
  const named = seated && nonEmpty(input.agentDisplayName) !== null;
  const describesSomething =
    seated ||
    nonEmpty(input.runtime) !== null ||
    nonEmpty(input.model) !== null;

  const label = namelike(input.label);
  const name = describesSomething
    ? (label ?? identity.primary)
    : (label ?? CODING_SESSION_UNKNOWN_ACTOR);
  // A named seat's line is already `Actor · Role`; the runtime it happens to
  // run on is not what tells two seats apart, so it stays out. A seat with no
  // profile read shows role plus runtime, which is C1a's honest half.
  const detail = seated && !named ? identity.secondary : null;
  const via = nonEmpty(input.providerInstanceRef);

  const spoken = name.split(" · ").join(", ");
  const screenReader = via
    ? `Response from ${spoken}, generation ${input.generation}, via ${via}.`
    : `Response from ${spoken}, generation ${input.generation}.`;

  return {
    name,
    detail,
    via: via === null ? null : `via ${via}`,
    screenReader,
  };
}

function nonEmpty(value: string | null | undefined): string | null {
  return typeof value === "string" && value.trim().length > 0
    ? value.trim()
    : null;
}

/** Full hex, or the canonical `abcd1234…wxyz` truncation `truncatePubkey` emits. */
const PUBKEY_SHAPED = /^(?:[0-9a-f]{16,}|[0-9a-f]{4,}…[0-9a-f]{4,})$/i;

function namelike(value: string | null | undefined): string | null {
  const trimmed = nonEmpty(value);
  if (trimmed === null) return null;
  return PUBKEY_SHAPED.test(trimmed) ? null : trimmed;
}
