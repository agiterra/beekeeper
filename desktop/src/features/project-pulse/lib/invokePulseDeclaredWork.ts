/**
 * The one call behind the declared-work section of Project Pulse.
 *
 * `pulse_declared_work` is a native command: `buzz-core` verifies the signed
 * 44244 set, folds it with the canonical team fold, and derives the one word
 * per assignment. This module invokes it and decodes the response against the
 * frozen contract. It folds nothing and words nothing.
 *
 * Three failure modes stay apart, because collapsing any of them into "no
 * declared work" is the lie this section exists to prevent:
 *
 * - the transport failed (the error propagates),
 * - the payload did not match the contract (the decoder throws, naming it),
 * - a session's own records could not be read (the response says so, per
 *   session, and the caller renders "records unreadable").
 *
 * Contract: `docs/DECLARED_WORK_PULSE_IMPL.md` §3.
 */
import { invokeTauri } from "@/shared/api/tauri";

import {
  decodePulseDeclaredWork,
  PULSE_DECLARED_WORK_COMMAND,
  PULSE_DECLARED_WORK_REQUEST_SCHEMA,
  type PulseDeclaredWorkError,
  type PulseDeclaredWorkRequest,
  type PulseDeclaredWorkResponse,
  type PulseDeclaredWorkSessionInput,
} from "./pulseDeclaredWorkWire";

/** What one page of the read asks about. */
export type PulseDeclaredWorkInput = {
  /** `30621:<owner>:<dtag>` — the same coordinate the digest read uses. */
  project: string;
  /**
   * The project's channels — the floor this read was scoped to.
   *
   * Echoed for binding: a session in a channel outside the project is not
   * discoverable, and the request records what the page was allowed to see.
   */
  channelIds: readonly string[];
  /**
   * The umbrellas whose signed 44244 records were gathered, already capped at
   * the native command's eight and in the order the surface will show them.
   *
   * A session that could not be gathered is **absent** here and present in
   * {@link readErrors}: handing over an umbrella with an empty event list
   * would render records nobody could read as records that do not exist,
   * which are different facts.
   */
  sessions?: readonly PulseDeclaredWorkSessionInput[];
  /** Reads that failed before the command was called, each named by scope. */
  readErrors?: readonly PulseDeclaredWorkError[];
  /** The viewer's own pubkey, or null when this surface has no identity. */
  viewerPubkey?: string | null;
};

/** The transport seam, so a test can drive the decoder with real bytes. */
export type PulseDeclaredWorkInvoker = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

/**
 * Read one page of a project's declared work, decoded.
 *
 * Rejects rather than returns on every disagreement; the caller renders the
 * failure. `dependencies.invoke` exists for tests and for the mock bridge.
 */
export async function invokePulseDeclaredWork(
  input: PulseDeclaredWorkInput,
  dependencies: { invoke?: PulseDeclaredWorkInvoker } = {},
): Promise<PulseDeclaredWorkResponse> {
  const invoke: PulseDeclaredWorkInvoker =
    dependencies.invoke ??
    ((command, args) => invokeTauri<unknown>(command, args));
  // Every key the native command declares: its request type refuses unknown
  // fields and defaults nothing, so a missing key is a refusal by name rather
  // than a page that quietly read less than it claimed. Typed as the shared
  // `PulseDeclaredWorkRequest` so a key that drifts from the contract fails
  // here, at compile time, rather than at the native boundary.
  const request: PulseDeclaredWorkRequest = {
    schema: PULSE_DECLARED_WORK_REQUEST_SCHEMA,
    project: input.project,
    channelIds: [...input.channelIds],
    nowUnix: Math.floor(Date.now() / 1000),
    viewerPubkey: input.viewerPubkey ?? null,
    sessions: [...(input.sessions ?? [])],
    readErrors: [...(input.readErrors ?? [])],
  };
  const raw = await invoke(PULSE_DECLARED_WORK_COMMAND, { request });
  return decodePulseDeclaredWork(raw);
}
