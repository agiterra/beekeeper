/**
 * Coding-session event kinds, as the browser observer reads them.
 *
 * Copied from `desktop/src/shared/constants/kinds.ts` — keep in sync. The
 * integers are the wire contract shared with `crates/buzz-core/src/kind.rs`;
 * a divergence here is a silently empty session list, not a type error.
 */

/** Operator-signed turn/interrupt command addressed to one execution. */
export const KIND_CODING_SESSION_COMMAND = 44220;
/** Operator-signed create / resume / stop. Names the provider authority. */
export const KIND_CODING_SESSION_LIFECYCLE_COMMAND = 44221;
/** Provider-advertised runtime catalog. */
export const KIND_CODING_SESSION_PROVIDER_CATALOG = 44222;
/** Ephemeral provider liveness lease — a snapshot of Redis, never history. */
export const KIND_CODING_SESSION_LEASE = 24223;
/** Provider-signed per-generation metadata (status, runtime, model, title). */
export const KIND_CODING_SESSION_METADATA = 44223;
/** Provider-signed answer to a 44221 or a 44220 turn. */
export const KIND_CODING_SESSION_LIFECYCLE_RECEIPT = 44224;
/** Provider-signed transcript envelope. */
export const KIND_CODING_SESSION_TRANSCRIPT = 44225;
/** Immutable founding record for one umbrella session. */
export const KIND_CODING_SESSION_GENESIS = 44226;
/** Founder-signed session goal. */
export const KIND_CODING_SESSION_GOAL = 44227;
/** Authority transition (operator grant/revoke). */
export const KIND_CODING_SESSION_AUTHORITY_TRANSITION = 44228;
/** Addressable session display name, keyed by `d` = sessionRef. */
export const KIND_CODING_SESSION_NAME = 44229;
/** Addressable session closure marker, keyed by `d` = sessionRef. */
export const KIND_CODING_SESSION_CLOSURE = 44230;
/** Relay-signed acceptance receipt / system message. */
export const KIND_SYSTEM_MESSAGE = 40099;
