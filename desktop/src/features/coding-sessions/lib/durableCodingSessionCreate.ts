import {
  buildCodingSessionCreateEvent,
  type CodingSessionLifecycleCommandEventInput,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { hasValidSignature } from "@/shared/lib/authors";

/**
 * Creating a coding session is the one place in this feature where a crash
 * between two steps could produce a *second* session rather than a lost one.
 * The event is signed and written to storage before it is published, so a
 * reload finds the exact bytes already in flight and can either resume them or
 * report the outcome as unknown — never mint a fresh command id and try again.
 *
 * The donor pre-signed two events, the native kind and a kind-9 fallback, and
 * carried a `transport` field to record which one had actually gone out. We own
 * the relay, so there is one kind, one signed event, and no transport question
 * to answer.
 */

const DURABLE_CREATE_SCHEMA = "buzz-durable-coding-session-create/v1";
const DURABLE_CREATE_KEY_PREFIX = "buzz.coding-session-create.v1:";
const MAX_DURABLE_CREATE_STORAGE_BYTES = 256 * 1024;

export type DurableCodingSessionCreateInput = Parameters<
  typeof buildCodingSessionCreateEvent
>[0];

export type DurableCodingSessionCreateTransaction = {
  schema: typeof DURABLE_CREATE_SCHEMA;
  /**
   * The scope this transaction belongs to. Standalone sessions scope by
   * channel id; a project-scoped create flow (glue) scopes by its own route
   * id. One in-flight create per scope is the invariant.
   */
  scopeId: string;
  input: DurableCodingSessionCreateInput;
  event: RelayEvent;
  publishState: "prepared" | "publishing" | "ambiguous" | "published";
  createdAt: number;
};

type DurableCreateStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

type DurableCreatePublisher = {
  publishEvent: (
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) => Promise<RelayEvent>;
};

type DurableCreateSigner = (
  input: CodingSessionLifecycleCommandEventInput,
) => Promise<RelayEvent>;

export type DurableCodingSessionCreatePersistenceResult =
  | { ok: true }
  | { ok: false; errorMessage: string };

export type DurableCodingSessionCreateLoadResult = {
  transaction: DurableCodingSessionCreateTransaction | null;
  errorMessage: string | null;
};

export type DurableCodingSessionCreatePublishResult = {
  transaction: DurableCodingSessionCreateTransaction;
  publishError: string | null;
  persistenceError: string | null;
  accepted: boolean;
};

export function durableCodingSessionCreateStorageKey(scopeId: string): string {
  return `${DURABLE_CREATE_KEY_PREFIX}${scopeId}`;
}

/**
 * Sign the create command and persist it before anything is published.
 *
 * The signed bytes are re-verified locally before they are stored: a signer
 * that returned something other than what was asked for must fail here, not
 * after the relay has accepted it.
 */
export async function prepareDurableCodingSessionCreate(
  scopeId: string,
  input: DurableCodingSessionCreateInput,
  dependencies: {
    signer?: DurableCreateSigner;
    storage?: DurableCreateStorage;
    now?: () => number;
  } = {},
): Promise<
  | { ok: true; transaction: DurableCodingSessionCreateTransaction }
  | { ok: false; errorMessage: string }
> {
  const eventInput = buildCodingSessionCreateEvent(input);
  const signer = dependencies.signer ?? signRelayEvent;
  const event = normalizeRelayEvent(await signer(eventInput));
  const transaction: DurableCodingSessionCreateTransaction = {
    schema: DURABLE_CREATE_SCHEMA,
    scopeId,
    input: { ...input },
    event,
    publishState: "prepared",
    createdAt: (dependencies.now ?? Date.now)(),
  };
  if (!isValidDurableTransaction(transaction, scopeId)) {
    return {
      ok: false,
      errorMessage: "The signed session request failed local verification.",
    };
  }
  const persisted = persistDurableCodingSessionCreate(
    transaction,
    dependencies.storage,
  );
  return persisted.ok
    ? { ok: true, transaction }
    : { ok: false, errorMessage: persisted.errorMessage };
}

/**
 * Publish the exact prepared event.
 *
 * A publish that neither succeeds nor definitively fails lands in `ambiguous`
 * and stays there. That state is the honest one: the relay may hold the event,
 * so the resolution is to watch for the 44224 receipt, not to publish again.
 */
export async function publishDurableCodingSessionCreate(
  transaction: DurableCodingSessionCreateTransaction,
  dependencies: {
    publisher?: DurableCreatePublisher;
    storage?: DurableCreateStorage;
    onTransaction?: (
      transaction: DurableCodingSessionCreateTransaction,
    ) => void;
  } = {},
): Promise<DurableCodingSessionCreatePublishResult> {
  const publisher = dependencies.publisher ?? relayClient;
  const persistTransition = (
    next: DurableCodingSessionCreateTransaction,
  ): DurableCodingSessionCreatePersistenceResult => {
    const result = persistDurableCodingSessionCreate(
      next,
      dependencies.storage,
    );
    if (result.ok) dependencies.onTransaction?.(next);
    return result;
  };
  const publishing: DurableCodingSessionCreateTransaction = {
    ...transaction,
    publishState: "publishing",
  };
  let persisted = persistTransition(publishing);
  if (!persisted.ok) {
    return {
      transaction,
      publishError: null,
      persistenceError: persisted.errorMessage,
      accepted: false,
    };
  }

  try {
    await publisher.publishEvent(
      publishing.event,
      "Timed out while creating the coding session.",
      "Failed to create the coding session.",
    );
  } catch (error) {
    return persistAmbiguousFailure(publishing, error, persistTransition);
  }

  const published: DurableCodingSessionCreateTransaction = {
    ...publishing,
    publishState: "published",
  };
  persisted = persistTransition(published);
  return {
    transaction: persisted.ok ? published : publishing,
    publishError: null,
    persistenceError: persisted.ok ? null : persisted.errorMessage,
    accepted: true,
  };
}

/**
 * Read the in-flight transaction for a scope.
 *
 * Unreadable or unavailable storage is reported as an error rather than as
 * "nothing in flight": the whole point of the record is to stop a second
 * create, so a missing record has to block creation instead of permitting it.
 */
export function loadDurableCodingSessionCreate(
  scopeId: string,
  storage?: DurableCreateStorage,
): DurableCodingSessionCreateLoadResult {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) {
    return {
      transaction: null,
      errorMessage:
        "Durable session-request storage is unavailable. Creation is blocked to prevent duplicate sessions.",
    };
  }
  try {
    const raw = targetStorage.getItem(
      durableCodingSessionCreateStorageKey(scopeId),
    );
    if (raw === null) return { transaction: null, errorMessage: null };
    if (
      new TextEncoder().encode(raw).byteLength >
      MAX_DURABLE_CREATE_STORAGE_BYTES
    ) {
      return invalidStoredTransaction();
    }
    const parsed: unknown = JSON.parse(raw);
    return isValidDurableTransaction(parsed, scopeId)
      ? { transaction: parsed, errorMessage: null }
      : invalidStoredTransaction();
  } catch {
    return {
      transaction: null,
      errorMessage:
        "The saved session request could not be read. Creation is blocked to prevent duplicate sessions.",
    };
  }
}

export function persistDurableCodingSessionCreate(
  transaction: DurableCodingSessionCreateTransaction,
  storage?: DurableCreateStorage,
): DurableCodingSessionCreatePersistenceResult {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return unavailablePersistence();
  if (!isValidDurableTransaction(transaction, transaction.scopeId)) {
    return {
      ok: false,
      errorMessage: "The session request failed local durability validation.",
    };
  }
  try {
    const serialized = JSON.stringify(transaction);
    if (
      new TextEncoder().encode(serialized).byteLength >
      MAX_DURABLE_CREATE_STORAGE_BYTES
    ) {
      return {
        ok: false,
        errorMessage: "The session request is too large to persist safely.",
      };
    }
    targetStorage.setItem(
      durableCodingSessionCreateStorageKey(transaction.scopeId),
      serialized,
    );
    return { ok: true };
  } catch {
    return unavailablePersistence();
  }
}

export function clearDurableCodingSessionCreate(
  scopeId: string,
  storage?: DurableCreateStorage,
): DurableCodingSessionCreatePersistenceResult {
  const targetStorage = storage ?? resolveDefaultStorage();
  if (!targetStorage) return unavailablePersistence();
  try {
    targetStorage.removeItem(durableCodingSessionCreateStorageKey(scopeId));
    return { ok: true };
  } catch {
    return unavailablePersistence();
  }
}

function persistAmbiguousFailure(
  transaction: DurableCodingSessionCreateTransaction,
  error: unknown,
  persist: (
    transaction: DurableCodingSessionCreateTransaction,
  ) => DurableCodingSessionCreatePersistenceResult,
): DurableCodingSessionCreatePublishResult {
  const ambiguous: DurableCodingSessionCreateTransaction = {
    ...transaction,
    publishState: "ambiguous",
  };
  const persisted = persist(ambiguous);
  return {
    transaction: persisted.ok ? ambiguous : transaction,
    publishError:
      error instanceof Error
        ? error.message
        : "The signed session request has an unknown publish outcome.",
    persistenceError: persisted.ok ? null : persisted.errorMessage,
    accepted: false,
  };
}

function isValidDurableTransaction(
  value: unknown,
  scopeId: string,
): value is DurableCodingSessionCreateTransaction {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, [
      "schema",
      "scopeId",
      "input",
      "event",
      "publishState",
      "createdAt",
    ]) ||
    value.schema !== DURABLE_CREATE_SCHEMA ||
    value.scopeId !== scopeId ||
    !["prepared", "publishing", "ambiguous", "published"].includes(
      String(value.publishState),
    ) ||
    !Number.isFinite(value.createdAt) ||
    Number(value.createdAt) <= 0 ||
    !isCreateInput(value.input)
  ) {
    return false;
  }
  let eventInput: CodingSessionLifecycleCommandEventInput;
  try {
    eventInput = buildCodingSessionCreateEvent(value.input);
  } catch {
    return false;
  }
  return isExactSignedEvent(value.event, eventInput);
}

function isCreateInput(
  value: unknown,
): value is DurableCodingSessionCreateInput {
  return (
    isPlainRecord(value) &&
    hasExactKeys(value, [
      "channelId",
      "commandId",
      "projectRef",
      "repoRef",
      "providerInstanceRef",
      "providerAuthorityPubkey",
      "model",
      "title",
      "initialTurn",
    ])
  );
}

function isExactSignedEvent(
  value: unknown,
  expected: CodingSessionLifecycleCommandEventInput,
): value is RelayEvent {
  return (
    isPlainRecord(value) &&
    hasExactKeys(value, [
      "id",
      "pubkey",
      "created_at",
      "kind",
      "tags",
      "content",
      "sig",
    ]) &&
    value.kind === expected.kind &&
    value.content === expected.content &&
    JSON.stringify(value.tags) === JSON.stringify(expected.tags) &&
    hasValidSignature(value as RelayEvent)
  );
}

function normalizeRelayEvent(event: RelayEvent): RelayEvent {
  return {
    id: event.id,
    pubkey: event.pubkey,
    created_at: event.created_at,
    kind: event.kind,
    tags: event.tags.map((tag) => [...tag]),
    content: event.content,
    sig: event.sig,
  };
}

function invalidStoredTransaction(): DurableCodingSessionCreateLoadResult {
  return {
    transaction: null,
    errorMessage:
      "The saved session request is invalid. Creation is blocked to prevent duplicate sessions.",
  };
}

function unavailablePersistence(): DurableCodingSessionCreatePersistenceResult {
  return {
    ok: false,
    errorMessage:
      "Durable session-request storage is unavailable. Nothing was published.",
  };
}

function resolveDefaultStorage(): DurableCreateStorage | undefined {
  try {
    return globalThis.localStorage;
  } catch {
    return undefined;
  }
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

function hasExactKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length &&
    actual.every((key, index) => key === keys[index])
  );
}
