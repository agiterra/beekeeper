/**
 * When a create is settled, and where the work actually landed.
 *
 * Split out of `codingSessionHandoverPublish.ts` because these are the reads
 * that decide two irreversible-ish acts: whether the staged workdir hint may
 * be dropped, and whether a reconstruction may be published as "recovered".
 * Both turn on the same rule — **an exception is not proof that nothing was
 * accepted** — and keeping them together makes that rule readable in one
 * screen.
 */
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  hasStrictLifecycleReceiptJson,
  hasStrictLifecycleReceiptValues,
  hasStrictMetadataJson,
} from "@/shared/coordination/sessionCoordinationStrictJson";
import { buildCodingSessionTargetKey } from "./codingSessionCommand";
import type { CodingSessionHandoverTarget } from "./codingSessionHandoverWire";

/** How long a settle waits, in the grant path's own units. */
export const SETTLE_ATTEMPTS = 25;
export const SETTLE_DELAY_MS = 200;
const LIFECYCLE_RECEIPT_KIND = 44224;
const METADATA_KIND = 44223;

/** What `handover_prepare_checkout` answers. Mirrors the Rust report. */
export type CodingSessionHandoverCheckoutReport = {
  branch: string;
  checkedOutSha: string;
  recovered: string[];
  missing: string[];
};

/** The reads these helpers need, so a test can drive every one of them. */
export type CodingSessionHandoverSettlementDependencies = {
  fetchEvents?: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>;
  wait?: (milliseconds: number) => Promise<void>;
  settleAttempts?: number;
};

export function defaultWait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, milliseconds));
}

function reasonOf(error: unknown): string {
  const reason = error instanceof Error ? error.message.trim() : String(error);
  return reason.length > 0 ? reason : "the step gave no reason";
}

/**
 * The NIP-01 machine-readable prefixes a relay puts on an `OK: false`.
 *
 * Their presence is the only evidence this client has that an event was
 * **not** stored; a timeout or socket failure carries no such word, and is
 * never read as a refusal.
 */
const PROVEN_REFUSAL_PREFIXES = [
  "duplicate:",
  "pow:",
  "blocked:",
  "rate-limited:",
  "invalid:",
  "restricted:",
  "error:",
  "auth-required:",
] as const;

/** Whether this error proves the relay did not accept the event. */
export function isProvenRelayRefusal(error: unknown): boolean {
  if (!(error instanceof Error)) return false;
  const message = error.message.trim().toLowerCase();
  return PROVEN_REFUSAL_PREFIXES.some((prefix) => message.startsWith(prefix));
}

/** What a caller says while a create's outcome is unknown. */
export const CODING_SESSION_HANDOVER_UNKNOWN_CREATE =
  "The create was signed and sent, but nothing has answered it yet, so this app cannot say whether the relay accepted it. The folder hint stays staged for that command, so a late delivery still lands in the recovered checkout.";

/**
 * Wait, bounded, for **any** receipt that settles this command.
 *
 * Three answers, because they lead to three different acts. `created` is a
 * provider carrying the work. `failed` is a provider answering that it will
 * not — settled all the same, so the staged hint has nothing left to steer.
 * `unknown` is silence or a read that failed, which proves nothing at all.
 */
export async function awaitCodingSessionCreateSettlement(
  input: { channelId: string; commandId: string },
  dependencies: CodingSessionHandoverSettlementDependencies = {},
): Promise<
  | { settled: "created"; target: CodingSessionHandoverTarget }
  | { settled: "failed"; status: string }
  | { settled: "unknown"; reason: string }
> {
  const fetchEvents =
    dependencies.fetchEvents ??
    ((filter: RelaySubscriptionFilter) => relayClient.fetchEvents(filter));
  const wait = dependencies.wait ?? defaultWait;
  const attempts = Math.max(1, dependencies.settleAttempts ?? SETTLE_ATTEMPTS);
  let lastReadError: string | null = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    let events: RelayEvent[] = [];
    try {
      events = await fetchEvents({
        kinds: [LIFECYCLE_RECEIPT_KIND],
        "#h": [input.channelId],
        "#csl-command": [input.commandId],
        limit: 8,
      });
      lastReadError = null;
    } catch (error) {
      lastReadError = reasonOf(error);
    }
    for (const event of events) {
      const target = readCreatedTarget(event, input.commandId);
      if (target) return { settled: "created", target };
      const refusal = readSettledFailure(event, input.commandId);
      if (refusal !== null) return { settled: "failed", status: refusal };
    }
    if (attempt + 1 < attempts) await wait(SETTLE_DELAY_MS);
  }
  return {
    settled: "unknown",
    reason:
      lastReadError ??
      "the create went out, but no provider answered it with a receipt inside this window's budget",
  };
}

/** A receipt for this command that settles it as *not* created. */
function readSettledFailure(
  event: RelayEvent,
  commandId: string,
): string | null {
  if (event.kind !== LIFECYCLE_RECEIPT_KIND) return null;
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (
    !hasStrictLifecycleReceiptJson(event.content, content) ||
    !hasStrictLifecycleReceiptValues(content) ||
    content.commandId !== commandId ||
    content.status === "created"
  ) {
    return null;
  }
  return typeof content.status === "string" ? content.status : null;
}

/** Wait, bounded, for the `created` receipt naming this command's target. */
export async function awaitCodingSessionCreated(
  input: { channelId: string; commandId: string },
  dependencies: CodingSessionHandoverSettlementDependencies = {},
): Promise<CodingSessionHandoverTarget> {
  const fetchEvents =
    dependencies.fetchEvents ??
    ((filter: RelaySubscriptionFilter) => relayClient.fetchEvents(filter));
  const wait = dependencies.wait ?? defaultWait;
  const attempts = Math.max(1, dependencies.settleAttempts ?? SETTLE_ATTEMPTS);
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const events = await fetchEvents({
      kinds: [LIFECYCLE_RECEIPT_KIND],
      "#h": [input.channelId],
      "#csl-command": [input.commandId],
      limit: 8,
    });
    for (const event of events) {
      const target = readCreatedTarget(event, input.commandId);
      if (target) return target;
    }
    if (attempt + 1 < attempts) await wait(SETTLE_DELAY_MS);
  }
  throw new Error(
    "The create went out, but no provider answered it with a created receipt, so nothing carries this session's work yet.",
  );
}

function readCreatedTarget(
  event: RelayEvent,
  commandId: string,
): CodingSessionHandoverTarget | null {
  if (event.kind !== LIFECYCLE_RECEIPT_KIND) return null;
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (
    !hasStrictLifecycleReceiptJson(event.content, content) ||
    !hasStrictLifecycleReceiptValues(content) ||
    content.commandId !== commandId ||
    content.status !== "created"
  ) {
    return null;
  }
  const session = content.session as Record<string, unknown>;
  return {
    driver: session.driver as string,
    instanceId: session.instanceId as string,
    sessionId: session.sessionId as string,
    generation: session.generation as number,
  };
}

/**
 * Read the new execution's own first metadata and check it is where this host
 * put the work.
 *
 * The provider publishes its branch and observed commit; the patch is applied
 * uncommitted, so `HEAD` stays at the checkpoint's sha. A match is the only
 * thing that lets this flow call the reconstruction recovered — a mismatch
 * means the model is running in some other folder, which is exactly the state
 * a "recovered" label would hide.
 */
export async function confirmRecoveredCheckout(
  input: {
    channelId: string;
    target: CodingSessionHandoverTarget;
    branch: string | null;
    headSha: string;
  },
  dependencies: CodingSessionHandoverSettlementDependencies = {},
): Promise<{ confirmed: boolean; missing: string | null }> {
  const fetchEvents =
    dependencies.fetchEvents ??
    ((filter: RelaySubscriptionFilter) => relayClient.fetchEvents(filter));
  const wait = dependencies.wait ?? defaultWait;
  const attempts = Math.max(1, dependencies.settleAttempts ?? SETTLE_ATTEMPTS);
  const targetKey = buildCodingSessionTargetKey(input.target);
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    let events: RelayEvent[] = [];
    try {
      events = await fetchEvents({
        kinds: [METADATA_KIND],
        "#h": [input.channelId],
        "#cs-target": [targetKey],
        limit: 4,
      });
    } catch {
      // A failed read is not a mismatch; the loop tries again and the
      // absence sentence below is the honest answer if it never arrives.
    }
    for (const event of events) {
      const facts = readMetadataFacts(event);
      if (facts === null) continue;
      if (
        input.branch !== null &&
        facts.branch === input.branch &&
        facts.observedCommit?.toLowerCase() === input.headSha.toLowerCase()
      ) {
        return { confirmed: true, missing: null };
      }
      return {
        confirmed: false,
        missing: `the execution reports branch ${facts.branch ?? "none"} at ${facts.observedCommit ?? "no commit"}, not the recovered checkout — it is running somewhere else`,
      };
    }
    if (attempt + 1 < attempts) await wait(SETTLE_DELAY_MS);
  }
  return {
    confirmed: false,
    missing:
      "the execution published no metadata, so this app could not confirm it is running in the recovered checkout",
  };
}

/** The branch and commit one 44223 reports, or null when it is not one. */
function readMetadataFacts(
  event: RelayEvent,
): { branch: string | null; observedCommit: string | null } | null {
  if (event.kind !== METADATA_KIND) return null;
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (!hasStrictMetadataJson(event.content, content)) return null;
  return {
    branch: typeof content.branch === "string" ? content.branch : null,
    observedCommit:
      typeof content.observedCommit === "string"
        ? content.observedCommit
        : null,
  };
}
