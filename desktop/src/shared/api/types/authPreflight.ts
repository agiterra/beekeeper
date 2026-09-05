/** Authentication/login status for a CLI-based ACP runtime. */
export type AuthStatus =
  | { status: "logged_in" }
  | { status: "logged_out" }
  | { status: "config_invalid"; diagnostic: string }
  | { status: "not_applicable" }
  | { status: "unknown" };

/**
 * What one live pre-flight call proved about a runtime's credential.
 *
 * `AuthStatus` above is what the CLI's `auth status` *says*; it reads the
 * credential file and never exercises the token. This is what a real call
 * *did* (finding 71). Exactly three states, and `unknown` is a state the
 * UI shows, never a reason to fall back to the status read.
 */
export type AuthPreflightState =
  | { state: "verified_live" }
  | { state: "credential_dead"; sentence: string }
  | { state: "unknown"; reason: string };

/** A pre-flight verdict with the moment it was reached. */
export type AuthPreflightVerdict = {
  runtimeId: string;
  state: AuthPreflightState;
  /** Unix milliseconds when the call finished. */
  checkedAtMs: number;
  /** The command line that was run, disclosed to the operator. */
  command: string;
  /** What to do about it; present only for a dead credential. */
  remedy: string | null;
  /** True when served from the ten-minute cache rather than a fresh call. */
  cached: boolean;
};
