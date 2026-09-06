/**
 * The one call behind the Missions half of Project Pulse.
 *
 * `pulse_mission_rows` is a native command: `buzz-core` reads the signed facts,
 * folds them, and writes every human sentence. This module invokes it, decodes
 * the response against the frozen contract, and binds the answer to the request
 * that asked for it. It computes nothing a person reads.
 *
 * Three failure modes are kept apart on purpose, because collapsing any of them
 * into "no missions" is the lie this surface exists to prevent:
 *
 * - the transport failed (the error propagates),
 * - the payload did not match the contract (the decoder throws, naming it),
 * - the payload answered about channels this read never asked about.
 *
 * All three are errors the caller renders. None of them is an empty Pulse.
 */
import { invokeTauri } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";

import {
  decodePulseMissionRows,
  type PulseMissionRowsResponse,
} from "./pulseMissionWire";

/** The Tauri command name. Owned by `buzz-core`'s Rust implementation. */
export const PULSE_MISSION_ROWS_COMMAND = "pulse_mission_rows";

/** The wire-schema string the native command accepts. */
export const PULSE_MISSION_ROWS_REQUEST_SCHEMA =
  "buzz-pulse-mission-rows-request/v1";

/**
 * One read that failed, named rather than folded away.
 *
 * Byte-identical to the native `PulseMissionError`: the command appends its
 * own disclosures to this list, so a read failure raised here and one raised
 * in the fold render through exactly the same path.
 */
export type PulseMissionReadError = {
  scope: string;
  message: string;
};

/** One active receipt-backed role seat, as the native `PulseMissionSeatInput`. */
export type PulseMissionSeatInput = {
  actorPubkey: string;
  role: string;
};

/** One active receipt-backed operator grant, as `PulseMissionGrantInput`. */
export type PulseMissionGrantInput = {
  actorPubkey: string;
  grantEventRef: string;
  maySteer: boolean;
  /** Unix seconds at which the relay accepted the transition. */
  acceptedAt: number;
  /** Whether the accepted transition granted rather than revoked steering. */
  granted: boolean;
};

/** One relay-signed kind 30618 ref, as `PulseMissionRefStateInput`. */
export type PulseMissionRefStateInput = {
  refName: string;
  sha: string;
  pusherPubkey: string;
  asOf: number | null;
};

/**
 * One umbrella's signed inputs, as the native `PulseMissionSessionInput`.
 *
 * `deny_unknown_fields` on the Rust side: every key below must be present and
 * correctly typed or the whole call is refused by name. The event lists carry
 * **whole signed events**, unmodified — `buzz-core` verifies their signatures
 * itself, and an event this layer reshaped is an event it cannot verify.
 */
export type PulseMissionSessionInput = {
  sessionKey: string;
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
  name: string | null;
  latestObservationAt: number | null;
  activeSeats: PulseMissionSeatInput[];
  activeGrants: PulseMissionGrantInput[];
  claimedSeats: string[];
  teamEvents: RelayEvent[];
  policyEvents: RelayEvent[];
  observationEvents: RelayEvent[];
  /**
   * This channel's kind 44221 lifecycle commands and kind 44224 receipts —
   * what proves who *provides* each mission (finding 90, and the 2026-09-05
   * refuter's S4).
   *
   * Not filtered by `#d`: a command carries `csl-command`, not the umbrella's
   * `d`, so the channel's page is sent and the native adapter splits it. The
   * Rust fields are `#[serde(default)]`, so omitting them decodes — and
   * resolves no provider, which makes every gate line read
   * `(observed, unverified)`. Sending them is what lets the fold check anyone.
   */
  lifecycleCommands: RelayEvent[];
  lifecycleReceipts: RelayEvent[];
  refState: PulseMissionRefStateInput[];
  overlapFiles: string[];
  overlapSha: string | null;
  overlapAsOf: number | null;
  overlapAuthor: string | null;
};

/** What one read asks about: a project coordinate and its channel set. */
export type PulseMissionRowsRequest = {
  /** `30621:<owner>:<dtag>` — the same coordinate the digest read uses. */
  project: string;
  /**
   * The project's channels. The same floor the digest reads from: a session in
   * a channel outside the project is not discoverable, and the response's
   * `missionScope` sentence is what says so on screen.
   */
  channelIds: readonly string[];
  /**
   * How many open sessions the digest proved, before any of them were read.
   *
   * The number the *digest* proved, never the number this read managed to
   * gather: the native command subtracts one from the other and discloses the
   * difference by name, so passing the gathered count would silence exactly
   * the sentence that admits a session went unread.
   */
  openSessionCount?: number;
  /**
   * The umbrellas whose signed records were gathered, newest observation
   * first and already capped at the native command's eight.
   *
   * A session that could not be read completely is **absent** here and
   * present in {@link readErrors}: handing over an umbrella with empty event
   * lists would render records nobody could read as records that do not
   * exist, which are different facts.
   */
  sessions?: readonly PulseMissionSessionInput[];
  /** Reads that failed before the command was called, each named by scope. */
  readErrors?: readonly PulseMissionReadError[];
  /** The viewer's own pubkey, or null when this surface has no identity. */
  viewerPubkey?: string | null;
  /** Display names by lowercase-hex pubkey, for `{Who}`. */
  displayNames?: Readonly<Record<string, string>>;
};

/** The transport seam, so a test can drive the decoder with real bytes. */
export type PulseMissionRowsInvoker = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

/**
 * Refuse a response that answers about channels this request never named.
 *
 * Not a strictness nuance: a mission row carries a channel id and a set of
 * seats, and painting one from outside the requested set attributes another
 * project's work to this one under this project's heading.
 *
 * An empty requested set binds nothing, because it names nothing — the caller
 * whose channel set has not resolved gets whatever the native command scoped
 * for itself, and the digest's own `channels` error is what discloses that.
 */
function bindResponse(
  response: PulseMissionRowsResponse,
  request: PulseMissionRowsRequest,
): void {
  if (request.channelIds.length === 0) return;
  const asked = new Set(request.channelIds);
  const stray = response.missions.find(
    (mission) => !asked.has(mission.channelId),
  );
  if (stray) {
    throw new Error(
      `pulse mission rows: response names channel ${stray.channelId}, which this read did not ask about`,
    );
  }
}

/**
 * Read one project's mission rows, decoded and bound to this request.
 *
 * Rejects rather than returns on every disagreement; the caller renders the
 * failure. `dependencies.invoke` exists for tests and for the mock bridge.
 */
export async function invokePulseMissionRows(
  request: PulseMissionRowsRequest,
  dependencies: { invoke?: PulseMissionRowsInvoker } = {},
): Promise<PulseMissionRowsResponse> {
  const invoke: PulseMissionRowsInvoker =
    dependencies.invoke ??
    ((command, args) => invokeTauri<unknown>(command, args));
  // Every key the native command declares, because its request type refuses
  // unknown fields and defaults nothing a reader would notice. `sessions` and
  // `readErrors` are the caller's gathered read: what it opened, and what it
  // could not. Neither is composed here — this module folds nothing.
  const raw = await invoke(PULSE_MISSION_ROWS_COMMAND, {
    request: {
      schema: PULSE_MISSION_ROWS_REQUEST_SCHEMA,
      project: request.project,
      channelIds: [...request.channelIds],
      nowUnix: Math.floor(Date.now() / 1000),
      viewerPubkey: request.viewerPubkey ?? null,
      displayNames: { ...(request.displayNames ?? {}) },
      openSessionCount: request.openSessionCount ?? 0,
      sessions: [...(request.sessions ?? [])],
      readErrors: [...(request.readErrors ?? [])],
    },
  });
  const response = decodePulseMissionRows(raw);
  bindResponse(response, request);
  return response;
}
