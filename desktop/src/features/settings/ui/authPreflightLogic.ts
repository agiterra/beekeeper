import type {
  AcpRuntimeCatalogEntry,
  AuthPreflightVerdict,
} from "@/shared/api/types";

/**
 * Which runtimes have a live login pre-flight today. Mirrors the backend's
 * `preflight_command_for`: only Claude, whose `auth status` read has been
 * caught reporting a dead credential as logged in (finding 71).
 */
export const AUTH_PREFLIGHT_RUNTIME_IDS: readonly string[] = ["claude"];

/**
 * Whether a runtime row should ask for (and show) a live login verdict.
 *
 * Only an installed runtime whose status read says it is signed in — or
 * signed out, which is what the catalog reports once the pre-flight itself
 * found the credential dead — has anything to verify. A row that still needs
 * an install, or whose config is broken, has an earlier problem to show.
 */
export function runtimeHasAuthPreflight(
  runtime: Pick<AcpRuntimeCatalogEntry, "id" | "availability" | "authStatus">,
): boolean {
  return (
    AUTH_PREFLIGHT_RUNTIME_IDS.includes(runtime.id) &&
    runtime.availability === "available" &&
    (runtime.authStatus.status === "logged_in" ||
      runtime.authStatus.status === "logged_out")
  );
}

export type AuthPreflightTone = "ok" | "bad" | "muted";

export type AuthPreflightPresentation = {
  /** Short label for the row: "Login verified", "Login expired", … */
  label: string;
  tone: AuthPreflightTone;
  /** The CLI's own line or the reason no verdict was reached; null when none. */
  detail: string | null;
  /** What to do about it; null unless the credential is dead. */
  remedy: string | null;
  /** "checked just now" / "checked 3 min ago" / null while pending. */
  checkedLabel: string | null;
};

/** Human wording for how long ago a verdict was reached. */
export function checkedAgoLabel(checkedAtMs: number, nowMs: number): string {
  const elapsedMs = Math.max(0, nowMs - checkedAtMs);
  if (elapsedMs < 60_000) return "checked just now";
  const minutes = Math.floor(elapsedMs / 60_000);
  if (minutes < 60) {
    return `checked ${minutes} min ago`;
  }
  const hours = Math.floor(minutes / 60);
  return `checked ${hours} h ago`;
}

/**
 * Fold a query's state into the one line the runtime row shows. Every state
 * is a state: a pending check says it is checking, a failed command says the
 * check itself failed, and `unknown` says why — none of them is silence.
 */
export function authPreflightPresentation(input: {
  verdict: AuthPreflightVerdict | undefined;
  isFetching: boolean;
  error: unknown;
  nowMs: number;
}): AuthPreflightPresentation {
  const { verdict, isFetching, error, nowMs } = input;
  if (!verdict && isFetching) {
    return {
      label: "Checking login…",
      tone: "muted",
      detail: null,
      remedy: null,
      checkedLabel: null,
    };
  }
  if (!verdict) {
    return {
      label: "Login check failed",
      tone: "bad",
      detail: error instanceof Error ? error.message : "The check did not run.",
      remedy: null,
      checkedLabel: null,
    };
  }
  const checkedLabel = checkedAgoLabel(verdict.checkedAtMs, nowMs);
  switch (verdict.state.state) {
    case "verified_live":
      return {
        label: "Login verified",
        tone: "ok",
        detail: null,
        remedy: null,
        checkedLabel,
      };
    case "credential_dead":
      return {
        label: "Login expired",
        tone: "bad",
        detail: verdict.state.sentence,
        remedy: verdict.remedy,
        checkedLabel,
      };
    default:
      return {
        label: "Login unverified",
        tone: "muted",
        detail: verdict.state.reason,
        remedy: null,
        checkedLabel,
      };
  }
}
