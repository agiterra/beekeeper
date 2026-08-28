/**
 * Closed-shape and duplicate-key checks for the signed coding-session facts.
 *
 * Shared, not Pulse's: every surface that reads a 44221/44223/44224/24223/44230
 * decodes it through these, so "this event is well-formed" has exactly one
 * answer in the app. Deliberately dependency-free and erasable-syntax-only —
 * the conformance binder loads this file under plain `node --test`.
 */

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const encoder = new TextEncoder();
const MAX_IDENTIFIER_BYTES = 256;
const MAX_REFERENCE_BYTES = 2 * 1024;
const SESSION_STATUSES = new Set([
  "starting",
  "idle",
  "running",
  "waiting_for_input",
  "completed",
  "stopped",
  "failed",
  "interrupted",
  "disconnected",
  "unknown",
]);

function boundedString(value: unknown, maxBytes: number): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    encoder.encode(value).length <= maxBytes
  );
}

function boundedNullable(value: unknown, maxBytes: number): boolean {
  return value === null || boundedString(value, maxBytes);
}

function isCanonicalSessionRef(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  );
}

/** Values of an exact target, including Rust's bounds and control rejection. */
export function hasStrictSessionTargetValues(
  value: unknown,
): value is Record<string, unknown> {
  return (
    hasExactSessionTargetFields(value) &&
    boundedString(value.driver, MAX_IDENTIFIER_BYTES) &&
    boundedString(value.instanceId, MAX_IDENTIFIER_BYTES) &&
    boundedString(value.sessionId, MAX_IDENTIFIER_BYTES) &&
    ![value.driver, value.instanceId, value.sessionId].some((identifier) =>
      [...(identifier as string)].some((character) =>
        /\p{Cc}/u.test(character),
      ),
    ) &&
    Number.isSafeInteger(value.generation) &&
    (value.generation as number) > 0
  );
}

/** Whether a decoded JSON object has exactly one of the accepted field sets. */
export function hasExactFields(
  value: unknown,
  forms: readonly (readonly string[])[],
): value is Record<string, unknown> {
  if (!isPlainObject(value)) return false;
  const keys = Object.keys(value);
  return forms.some(
    (form) =>
      keys.length === form.length &&
      form.every((key) => Object.hasOwn(value, key)),
  );
}

/** Exact four-field shape shared by command, receipt, and lease targets. */
export function hasExactSessionTargetFields(
  value: unknown,
): value is Record<string, unknown> {
  return hasExactFields(value, [
    ["driver", "instanceId", "sessionId", "generation"],
  ]);
}

/** Strict lifecycle-command object shapes, including additive create forms. */
export function hasStrictLifecycleCommandJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  if (
    hasDuplicateJsonKeys(source) ||
    !hasExactFields(content, [["schema", "commandId", "action"]]) ||
    !isPlainObject(content.action)
  ) {
    return false;
  }
  const action = content.action;
  if (action.type === "session.create") {
    const base = [
      "type",
      "projectRef",
      "repoRef",
      "providerInstanceRef",
      "providerAuthorityPubkey",
      "model",
      "title",
      "initialTurn",
    ];
    const unseated = [
      base,
      [...base.slice(0, 3), "sessionRef", ...base.slice(3)],
      [...base.slice(0, 3), "sessionRef", "genesisRef", ...base.slice(3)],
    ];
    // The seated forms buzz-core accepts: every unseated form plus the
    // `actor`/`role` pair, which travels together or not at all.
    return hasExactFields(action, [
      ...unseated,
      ...unseated.map((form) => [...form, "actor", "role"]),
    ]);
  }
  return (
    (action.type === "session.resume" || action.type === "session.stop") &&
    hasExactFields(action, [["type", "session", "providerAuthorityPubkey"]])
  );
}

/** Validate lifecycle values after the closed JSON shape has been established. */
export function hasStrictLifecycleCommandValues(
  content: Record<string, unknown>,
): boolean {
  if (
    content.schema !== "buzz-coding-session-lifecycle-command/v1" ||
    !boundedString(content.commandId, MAX_IDENTIFIER_BYTES) ||
    !isPlainObject(content.action) ||
    typeof content.action.providerAuthorityPubkey !== "string" ||
    !/^[0-9a-f]{64}$/.test(content.action.providerAuthorityPubkey)
  ) {
    return false;
  }
  const action = content.action;
  if (action.type === "session.create") {
    const sessionRefValid =
      !Object.hasOwn(action, "sessionRef") ||
      action.sessionRef === null ||
      isCanonicalSessionRef(action.sessionRef);
    const genesisValid =
      !Object.hasOwn(action, "genesisRef") ||
      (isCanonicalSessionRef(action.sessionRef) &&
        typeof action.genesisRef === "string" &&
        /^[0-9a-f]{64}$/.test(action.genesisRef));
    const seatValid =
      !Object.hasOwn(action, "actor") ||
      (typeof action.actor === "string" &&
        /^[0-9a-f]{64}$/.test(action.actor) &&
        typeof action.role === "string" &&
        isRoleSlug(action.role));
    return (
      boundedNullable(action.projectRef, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.repoRef, MAX_REFERENCE_BYTES) &&
      boundedString(action.providerInstanceRef, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.model, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.title, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.initialTurn, 12 * 1024) &&
      sessionRefValid &&
      genesisValid &&
      seatValid
    );
  }
  return hasStrictSessionTargetValues(action.session);
}

/** Strict lifecycle-receipt top-level and nested object shapes. */
export function hasStrictLifecycleReceiptJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [
      ["schema", "commandId", "status", "session", "error"],
    ]) &&
    (content.session === null ||
      hasExactSessionTargetFields(content.session)) &&
    (content.error === null ||
      hasExactFields(content.error, [["code", "message"]]))
  );
}

/** Validate receipt values and the status/session/error coupling. */
export function hasStrictLifecycleReceiptValues(
  content: Record<string, unknown>,
): boolean {
  if (
    content.schema !== "buzz-coding-session-lifecycle-receipt/v1" ||
    !boundedString(content.commandId, MAX_IDENTIFIER_BYTES) ||
    !hasStrictSessionTargetValues(content.session) ||
    typeof content.status !== "string"
  ) {
    return false;
  }
  if (content.status === "created" || content.status === "resumed") {
    return content.error === null;
  }
  if (
    content.status === "created_with_failed_initial_turn" ||
    content.status === "resumed_without_context"
  ) {
    if (!isPlainObject(content.error)) return false;
    const expectedCode =
      content.status === "created_with_failed_initial_turn"
        ? "INITIAL_TURN_FAILED"
        : "CONTEXT_NOT_RECOVERED";
    return (
      content.error.code === expectedCode &&
      boundedString(content.error.message, 1024 + 3)
    );
  }
  return false;
}

/** Strict lease top-level and nested target shapes. */
export function hasStrictLeaseJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [["schema", "target", "state", "leaseSequence"]]) &&
    hasExactSessionTargetFields(content.target)
  );
}

/** Validate lease values after its closed shape and duplicate-key checks. */
export function hasStrictLeaseValues(
  content: Record<string, unknown>,
): boolean {
  return (
    content.schema === "buzz-coding-session-lease/v1" &&
    hasStrictSessionTargetValues(content.target) &&
    (content.state === "live" || content.state === "released") &&
    Number.isSafeInteger(content.leaseSequence) &&
    (content.leaseSequence as number) > 0
  );
}

/**
 * Every field set `buzz-core`'s `decode_coding_session_metadata` accepts.
 *
 * Four independent additive amendments have landed on the metadata payload —
 * the `sessionRef` echo, the agent seat's `role`, D9's `turnBudget`, and B1's
 * four coordinate facts (which travel all-four-or-none) — and each is present
 * or absent on its own, so the base key set has **sixteen** valid shapes, not
 * four. Enumerating fewer silently drops every event carrying an amendment
 * this list forgot, which is a whole-surface outage rather than a strictness
 * nuance: the reader sees no sessions at all.
 */
function metadataFieldForms(): string[][] {
  const base = [
    "schema",
    "session",
    "projectRef",
    "repoRef",
    "title",
    "agentRef",
    "provider",
    "runtime",
    "model",
    "status",
    "branch",
    "capabilities",
  ];
  const amendments = [
    ["sessionRef"],
    ["role"],
    ["turnBudget"],
    METADATA_FACT_FIELDS,
  ];
  const forms: string[][] = [];
  for (let mask = 0; mask < 1 << amendments.length; mask += 1) {
    const form = [...base];
    for (const [index, keys] of amendments.entries()) {
      if (mask & (1 << index)) form.push(...keys);
    }
    forms.push(form);
  }
  return forms;
}

const METADATA_FACT_FIELDS = [
  "observedCommit",
  "dirty",
  "relayReachable",
  "verifiedAt",
];
const METADATA_FIELD_FORMS = metadataFieldForms();
const MAX_ROLE_SLUG_BYTES = 64;

/** A seat's role slug: `[a-z0-9-]`, 1..=64 bytes, exactly as Rust reads it. */
function isRoleSlug(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    encoder.encode(value).length <= MAX_ROLE_SLUG_BYTES &&
    /^[a-z0-9-]+$/.test(value)
  );
}

/**
 * A `turnBudget` object: exactly `{used, limit}`, and only beside an umbrella.
 *
 * A budget is a fact about an umbrella, so it cannot describe an execution
 * that claimed none, and a `limit` of zero would read as "no turn may ever
 * pass" rather than "unbudgeted" — the producer omits the key instead. Both
 * are the rejections `validate_session_metadata` makes.
 */
function isTurnBudget(value: unknown, sessionRef: unknown): boolean {
  return (
    typeof sessionRef === "string" &&
    hasExactFields(value, [["used", "limit"]]) &&
    Number.isSafeInteger(value.used) &&
    (value.used as number) >= 0 &&
    Number.isSafeInteger(value.limit) &&
    (value.limit as number) > 0
  );
}

/** Strict metadata shape/value check matching buzz-core's sixteen forms. */
export function hasStrictMetadataJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  if (
    encoder.encode(source).length > 32 * 1024 ||
    hasDuplicateJsonKeys(source) ||
    !hasExactFields(content, METADATA_FIELD_FORMS) ||
    content.schema !== "buzz-coding-session-metadata/v1" ||
    !hasStrictSessionTargetValues(content.session) ||
    !boundedNullable(content.projectRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.repoRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.title, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.agentRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.provider, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.runtime, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.model, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.branch, MAX_REFERENCE_BYTES) ||
    !SESSION_STATUSES.has(content.status as string)
  ) {
    return false;
  }
  if (
    Object.hasOwn(content, "sessionRef") &&
    !isCanonicalSessionRef(content.sessionRef)
  ) {
    return false;
  }
  // An explicit `null` is how serde's `Option` reads an absent value, and the
  // Rust decoder validates neither key when it holds one — so neither does
  // this. A non-null `role` labels a seat, so it needs the actor that holds
  // it; a non-null `turnBudget` needs the umbrella it bounds.
  if (
    Object.hasOwn(content, "role") &&
    content.role !== null &&
    (typeof content.agentRef !== "string" || !isRoleSlug(content.role))
  ) {
    return false;
  }
  if (
    Object.hasOwn(content, "turnBudget") &&
    content.turnBudget !== null &&
    !isTurnBudget(content.turnBudget, content.sessionRef)
  ) {
    return false;
  }
  const capabilityKeys = [
    "threadTurnStart",
    "threadTurnInterrupt",
    "threadSteer",
    "context",
    "diff",
    "plan",
  ];
  if (
    !hasExactFields(content.capabilities, [capabilityKeys]) ||
    !capabilityKeys.every(
      (key) =>
        isPlainObject(content.capabilities) &&
        typeof content.capabilities[key] === "boolean",
    )
  ) {
    return false;
  }
  if (!Object.hasOwn(content, "observedCommit")) return true;
  return (
    boundedNullable(content.observedCommit, MAX_REFERENCE_BYTES) &&
    (content.dirty === null || typeof content.dirty === "boolean") &&
    (content.relayReachable === null ||
      typeof content.relayReachable === "boolean") &&
    (content.verifiedAt === null || Number.isSafeInteger(content.verifiedAt)) &&
    (content.relayReachable === null) === (content.verifiedAt === null)
  );
}

/** Parse and strictly validate one metadata content string without throwing. */
export function isStrictMetadataContent(source: string): boolean {
  try {
    return hasStrictMetadataJson(source, JSON.parse(source));
  } catch {
    return false;
  }
}

/** Strict closure content check matching buzz-core's decoder. */
export function hasStrictClosureJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    encoder.encode(source).length <= 512 &&
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [["action", "genesisRef", "sessionRef", "v"]]) &&
    (content.action === "closed" || content.action === "open") &&
    typeof content.genesisRef === "string" &&
    /^[0-9a-f]{64}$/.test(content.genesisRef) &&
    isCanonicalSessionRef(content.sessionRef) &&
    content.v === 1
  );
}

/** Detect duplicate object keys at any depth before `JSON.parse` overwrites. */
export function hasDuplicateJsonKeys(source: string): boolean {
  const stack: Array<{ keys: Set<string> | null; wantsKey: boolean }> = [];
  for (let index = 0; index < source.length; index += 1) {
    const token = source[index];
    if (token === '"') {
      let end = index + 1;
      while (end < source.length && source[end] !== '"') {
        end += source[end] === "\\" ? 2 : 1;
      }
      const frame = stack.at(-1);
      if (frame?.keys && frame.wantsKey) {
        let decoded: unknown;
        try {
          decoded = JSON.parse(source.slice(index, end + 1));
        } catch {
          return true;
        }
        if (typeof decoded !== "string" || frame.keys.has(decoded)) return true;
        frame.keys.add(decoded);
        frame.wantsKey = false;
      }
      index = end;
    } else if (token === "{") {
      stack.push({ keys: new Set(), wantsKey: true });
    } else if (token === "[") {
      stack.push({ keys: null, wantsKey: false });
    } else if (token === "}" || token === "]") {
      stack.pop();
    } else if (token === "," && stack.at(-1)?.keys) {
      const frame = stack.at(-1);
      if (frame) frame.wantsKey = true;
    }
  }
  return false;
}
