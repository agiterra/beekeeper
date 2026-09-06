/**
 * Who may **commission** an execution — the authority half of the lifecycle
 * rule, and the envelope primitives it shares with the fold.
 *
 * A generation is accepted when one kind 44221 command and one kind 44224
 * receipt agree, the receipt is signed by the command's declared
 * `providerAuthorityPubkey`, and the status is a success. That checks who
 * **answered** the command. Until the 2026-09-05 refuter, nothing checked who
 * was entitled to **issue** one — so a seat could publish a `session.create`
 * naming *itself* as the provider (both refs are public), answer it with its
 * own receipt, and appear as a proven generation of somebody else's session.
 * `buzz-core`'s `mission_provider_pubkeys_from_lifecycle` closes the same hole
 * on the relay's side; this is its mirror.
 *
 * Split out of {@link ./sessionCoordinationFold.ts} so neither file passes
 * 1,000 lines. The four primitives below moved with it rather than being
 * copied: two definitions of "this envelope is exactly these tags" is two
 * answers to whether an event is readable at all.
 *
 * Constraints inherited from the fold (load-bearing): no runtime imports
 * outside this directory, and erasable TypeScript syntax only.
 */

import type { CoordinationEvent } from "./sessionCoordinationTypes.ts";

/** The facts the commissioning rule reads off one lifecycle command. */
export type CommissionableCommand = {
  event: CoordinationEvent;
  channelId: string;
  commandId: string;
  providerAuthorityPubkey: string;
  /** The kind 44221 `session.hire` a create answers, or null. */
  hireRef: string | null;
};

/** The value of the first tag named `key`. */
export function tagValue(event: CoordinationEvent, key: string): string | null {
  const tag = event.tags.find((candidate) => candidate[0] === key);
  return tag?.[1] ?? null;
}

/** Whether this event's tags are exactly `expected`, in order. */
export function hasExactOrderedTwoFieldTags(
  event: CoordinationEvent,
  expected: ReadonlyArray<readonly [string, string | null]>,
): boolean {
  return (
    event.tags.length === expected.length &&
    event.tags.every((tag, index) => {
      const [key, value] = expected[index];
      return (
        tag.length === 2 &&
        tag[0] === key &&
        (value === null ? tag[1].length > 0 : tag[1] === value)
      );
    })
  );
}

/** Whether this is a plain JSON object. */
export function isPlainObject(
  value: unknown,
): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** This event's content as a plain object, or null when it is not one. */
export function parseObjectContent(
  event: CoordinationEvent,
): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(event.content);
    return isPlainObject(value) ? value : null;
  } catch {
    return null;
  }
}

/** One kind 44221 `session.hire` — a request for a seat, naming no provider. */
export type LifecycleHireFacts = {
  event: CoordinationEvent;
  channelId: string;
  sessionRef: string;
};

/**
 * One kind 44221 `session.hire`, for the commissioning rule below.
 *
 * A hire names no `providerAuthorityPubkey` — it asks a host to choose one —
 * so the fold's `readLifecycleCommand` refuses it and always will. It is kept
 * as a separate attribution fact for the fold's stable wire model; admission
 * deliberately does not derive authority from it.
 */
export function readLifecycleHire(
  event: CoordinationEvent,
): LifecycleHireFacts | null {
  if (event.kind !== 44221) return null;
  const content = parseObjectContent(event);
  // Deliberately not `hasStrictLifecycleCommandValues`: that validator
  // requires `action.providerAuthorityPubkey`, which a hire has none of — a
  // hire asks a host to *choose* the provider. The envelope is checked here
  // instead. These attribution facts do not authorize the signer of a create
  // that cites the hire.
  if (
    !isPlainObject(content) ||
    content.schema !== "buzz-coding-session-lifecycle-command/v1" ||
    typeof content.commandId !== "string" ||
    !hasExactOrderedTwoFieldTags(event, [
      ["h", null],
      ["csl-v", "csl1-1"],
      ["csl-command", content.commandId],
    ]) ||
    !isPlainObject(content.action) ||
    content.action.type !== "session.hire" ||
    typeof content.action.sessionRef !== "string"
  ) {
    return null;
  }
  const channelId = tagValue(event, "h");
  if (!channelId) return null;
  return { event, channelId, sessionRef: content.action.sessionRef };
}

/**
 * Whether the key that signed this command was entitled to issue it — the
 * 2026-09-05 refuter's B1, mirrored from `buzz-core`'s
 * `mission_provider_pubkeys_from_lifecycle`.
 *
 * A receipt signed by the command's named provider proves who *answered* the
 * command. It says nothing about who was entitled to *issue* one, and until
 * this check existed nothing asked: a seat could publish a `session.create`
 * naming **itself** as `providerAuthorityPubkey`, answer it with its own
 * receipt, and appear here as a proven generation of somebody else's mission.
 *
 * Two evidence levels remain, and the caller decides which by supplying
 * `commissioners` or not:
 *
 * - **with** a steering set (founder ∪ accepted `operator` grants) the rule is
 *   the relay's: the command signer is in it. A public `hireRef` attributes
 *   why the create exists and grants the signer nothing;
 * - **without** one — currently Project Pulse and Agent Progress — the weaker
 *   rule remains signer ≠ provider. This closes the one-key self-certification
 *   case but is still defeatable by two keys (R4). Their visible `Unverified`
 *   wording describes liveness only; the authority weakness is not yet
 *   disclosed. R4 remains: supply the accepted authority projection to both
 *   callers, or add a separate authority-evidence label.
 */
export function commissioned(
  command: CommissionableCommand,
  commissioners: readonly string[] | null,
  _hires: readonly LifecycleHireFacts[],
): boolean {
  const signer = command.event.pubkey;
  return commissioners === null
    ? signer !== command.providerAuthorityPubkey
    : commissioners.includes(signer);
}
