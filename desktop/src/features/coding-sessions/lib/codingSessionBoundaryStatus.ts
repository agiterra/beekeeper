/**
 * Transcript copy for the host's project execution boundary disclosure.
 *
 * Each generation's transcript carries one status item from
 * `execution_scope::boundary_status_item` (`crates/beekeeper-session-provider`):
 * `execution_boundary_enforced` with the backend that enforced it as
 * `reason`, or `execution_boundary_not_enforced` with a stable reason slug.
 * The host emits "enforced" only after the boundary started around the whole
 * process tree, so this copy states what the host observed; it never infers
 * protection from anything else on the item.
 *
 * Only a slug-shaped `reason` is shown: a known one as prose, an unknown one
 * as its bare slug, anything else (a path, a sentence, a value) not at all.
 *
 * One reason is a sentence of its own: `full-access` (ledger 303) is the
 * person's grant of full access to this computer, so the session runs with no
 * boundary at all — by choice, not for want of a backend. It reads as that
 * choice and never as protection, whichever status carries it: an
 * `execution_boundary_enforced` naming it contradicts itself, and a row that
 * might be read as protected is the one outcome this copy must never produce.
 */

/** The row title for both boundary statuses. */
export const CODING_SESSION_BOUNDARY_TITLE = "Project boundary";

/** What each boundary status means for this session. */
export const CODING_SESSION_BOUNDARY_STATUSES: ReadonlyMap<string, string> =
  new Map([
    [
      "execution_boundary_enforced",
      "Enforced — this session and every process it starts run inside this project's boundary; other projects' files are outside it",
    ],
    [
      "execution_boundary_not_enforced",
      "Not enforced — this session is not isolated from other projects' files",
    ],
  ]);

/** Reader-facing names for the backends and reason slugs the host emits. */
export const CODING_SESSION_BOUNDARY_REASONS: ReadonlyMap<string, string> =
  new Map([
    ["macos-seatbelt", "macOS Seatbelt"],
    ["no-backend-for-platform", "this platform has no boundary backend"],
  ]);

/** The reason slug for a session the person granted full access. */
export const CODING_SESSION_FULL_ACCESS_REASON = "full-access";

/**
 * The row text for a full-access generation. It does not say "by you": the
 * grant is made on the provider's computer, and a teammate reading the same
 * transcript elsewhere did not make it.
 */
export const CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT =
  "Sandbox off — this session was granted full access to the computer it runs on. It and every process it starts can reach anything that account can, including other projects' files";

const REASON_SLUG = /^[a-z0-9][a-z0-9._-]{0,63}$/;

/**
 * The row text for a boundary status, or `undefined` when `status` is not
 * one, so the caller falls through to its other status renderers.
 */
export function codingSessionBoundaryText(
  status: string,
  reason: unknown,
): string | undefined {
  const text = CODING_SESSION_BOUNDARY_STATUSES.get(status);
  if (text === undefined) {
    return undefined;
  }
  if (reason === CODING_SESSION_FULL_ACCESS_REASON) {
    return CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT;
  }
  if (typeof reason !== "string" || reason.length === 0) {
    return `${text} (no reason given)`;
  }
  if (!REASON_SLUG.test(reason)) {
    return `${text} (unrecognized reason)`;
  }
  return `${text} (${CODING_SESSION_BOUNDARY_REASONS.get(reason) ?? reason})`;
}

/** The row title for the provider's session isolation statuses. */
export const CODING_SESSION_ISOLATION_TITLE = "Session isolation";

/**
 * What the provider withheld from this session beyond the project boundary
 * (`crates/beekeeper-session-provider/src/session_isolation.rs`
 * `isolation_status_items`). Published only when the provider's setting is
 * on, and only beside an enforced boundary, so each row states a fact the
 * host applied; the `reason` is a fixed slug and adds nothing to the copy.
 */
export const CODING_SESSION_ISOLATION_STATUSES: ReadonlyMap<string, string> =
  new Map([
    [
      "operator_git_withheld",
      "Git credentials from this computer were withheld from this session",
    ],
    [
      "network_egress_proxy_only",
      "This session can reach the network only through the provider's egress proxy",
    ],
  ]);

/**
 * The title and text for a boundary or isolation status, or `undefined`
 * when `status` is neither, so the caller falls through to its other status
 * renderers.
 */
export function codingSessionBoundaryRow(
  status: string,
  reason: unknown,
): { title: string; text: string } | undefined {
  const isolation = CODING_SESSION_ISOLATION_STATUSES.get(status);
  if (isolation !== undefined) {
    return { title: CODING_SESSION_ISOLATION_TITLE, text: isolation };
  }
  const text = codingSessionBoundaryText(status, reason);
  return text === undefined
    ? undefined
    : { title: CODING_SESSION_BOUNDARY_TITLE, text };
}

/**
 * What the session's sandbox is, as one word for the composer chip (SV-17).
 *
 * - `sandboxed` — the host reported the boundary enforced.
 * - `full-access` — the person granted full access; there is no boundary.
 * - `not-sandboxed` — the host reported the boundary not enforced for any
 *   other reason (no backend on this platform, say).
 * - `unreported` — no boundary row in this generation's transcript, or one
 *   this build cannot read. Never rendered as protection.
 */
export type CodingSessionSandboxState =
  | "sandboxed"
  | "full-access"
  | "not-sandboxed"
  | "unreported";

/**
 * One boundary disclosure: the row the provider published at an execution
 * start, with the isolation rows it published beside it.
 */
export type CodingSessionSandboxPeriod = {
  id: string;
  /** ISO timestamp of the boundary row, or empty when it carried none. */
  timestamp: string;
  state: CodingSessionSandboxState;
  boundaryText: string;
  isolation: readonly string[];
};

/** The latest generation's boundary disclosure, read from its transcript. */
export type CodingSessionSandboxReport = {
  state: CodingSessionSandboxState;
  /** The boundary row's full text, or null when none was published. */
  boundaryText: string | null;
  /** The isolation rows published beside that boundary, in order. */
  isolation: readonly string[];
  /**
   * Every earlier boundary disclosure in the same transcript, newest first.
   * Boundary rows no longer sit in the reading order, so this list is where
   * an earlier full-access or unenforced period stays visible after the
   * agent restarts sandboxed. Absent is the same as empty.
   */
  earlier?: readonly CodingSessionSandboxPeriod[];
};

/** The structural slice of a transcript item this module reads. */
type BoundaryTranscriptItem = {
  id?: string;
  type: string;
  title?: string;
  text?: string;
  timestamp?: string;
};

/**
 * Classify a boundary row's text. The text is always one this module wrote
 * (`codingSessionBoundaryText`), so it is matched against the same constants;
 * anything else is `unreported`, never `sandboxed`.
 */
export function codingSessionSandboxStateFromBoundaryText(
  text: string,
): CodingSessionSandboxState {
  if (text === CODING_SESSION_FULL_ACCESS_BOUNDARY_TEXT) return "full-access";
  const enforced = CODING_SESSION_BOUNDARY_STATUSES.get(
    "execution_boundary_enforced",
  );
  const notEnforced = CODING_SESSION_BOUNDARY_STATUSES.get(
    "execution_boundary_not_enforced",
  );
  if (enforced && text.startsWith(enforced)) return "sandboxed";
  if (notEnforced && text.startsWith(notEnforced)) return "not-sandboxed";
  return "unreported";
}

/**
 * Every boundary disclosure in a transcript, oldest first, each with the
 * isolation rows published after it and before the next boundary row.
 * Linear in `items`; callers pass the session facts (the boundary and
 * continuity rows alone), not the whole transcript, where they have them.
 */
export function codingSessionSandboxPeriods(
  items: readonly BoundaryTranscriptItem[],
): CodingSessionSandboxPeriod[] {
  const periods: Array<CodingSessionSandboxPeriod & { isolation: string[] }> =
    [];
  for (const item of items) {
    if (item.type !== "lifecycle") continue;
    if (item.title === CODING_SESSION_BOUNDARY_TITLE) {
      const boundaryText = item.text ?? "";
      periods.push({
        id: item.id ?? `boundary-${periods.length}`,
        timestamp: item.timestamp ?? "",
        state: codingSessionSandboxStateFromBoundaryText(boundaryText),
        boundaryText,
        isolation: [],
      });
      continue;
    }
    if (item.title === CODING_SESSION_ISOLATION_TITLE && item.text) {
      periods.at(-1)?.isolation.push(item.text);
    }
  }
  return periods;
}

/**
 * The newest boundary disclosure in a transcript, with the isolation rows the
 * provider published after it, and every earlier disclosure. The provider
 * emits one boundary row per execution start, so the last one describes the
 * agent running now.
 */
export function codingSessionSandboxFromTranscript(
  items: readonly BoundaryTranscriptItem[],
): CodingSessionSandboxReport {
  const periods = codingSessionSandboxPeriods(items);
  const latest = periods.at(-1);
  if (!latest) {
    return { state: "unreported", boundaryText: null, isolation: [] };
  }
  return {
    state: latest.state,
    boundaryText: latest.boundaryText,
    isolation: latest.isolation,
    earlier: periods.slice(0, -1).reverse(),
  };
}

/**
 * The most serious earlier period that ran outside a boundary: full access
 * before an unenforced boundary, newest first within each. `null` when every
 * earlier period was sandboxed or unreported.
 */
export function codingSessionEarlierUnsandboxedPeriod(
  report: CodingSessionSandboxReport,
): CodingSessionSandboxPeriod | null {
  const earlier = report.earlier ?? [];
  return (
    earlier.find((period) => period.state === "full-access") ??
    earlier.find((period) => period.state === "not-sandboxed") ??
    null
  );
}

/** This computer's own full-access grant, when the host answered for it. */
export type CodingSessionLocalSandboxGrant = {
  granted: boolean;
  pending: boolean | null;
  error: string | null;
};

/** What the composer's sandbox chip says, and how loudly. */
export type CodingSessionSandboxChip = {
  label: string;
  /** `warning` whenever the agent may run, or is about to run, unsandboxed. */
  tone: "safe" | "warning" | "muted";
  /** One sentence for the top of the dropdown. */
  summary: string;
};

/**
 * Combine the transcript's report with this computer's grant (if any).
 *
 * The transcript is the agent that is running; the grant is what applies at
 * its next start. Whenever either says the sandbox is off, the chip says so —
 * the one outcome it must never produce is reading as protected while the
 * agent is not (or is about to stop being) inside a boundary.
 */
export function codingSessionSandboxChip(
  report: CodingSessionSandboxReport,
  local: CodingSessionLocalSandboxGrant | null,
): CodingSessionSandboxChip {
  if (local?.pending === true) {
    return {
      label: "Full access…",
      tone: "warning",
      summary:
        "Turning full access on — restarting the agent outside the sandbox.",
    };
  }
  if (local?.pending === false) {
    return {
      label: "Sandboxing…",
      tone: "warning",
      summary:
        "Turning full access off — restarting the agent inside the sandbox. Until it restarts it still runs with full access.",
    };
  }
  if (report.state === "full-access") {
    return {
      label: "Full access",
      tone: "warning",
      summary:
        "Sandbox off: this agent can reach anything the account on its computer can, including other projects' files.",
    };
  }
  if (local?.granted) {
    return {
      label: "Full access",
      tone: "warning",
      summary:
        report.state === "sandboxed"
          ? "Full access is granted on this computer. The running agent is still sandboxed; the grant applies the next time it starts."
          : "Full access is granted on this computer for this session.",
    };
  }
  const exposed = codingSessionEarlierUnsandboxedPeriod(report);
  const exposedClause =
    exposed?.state === "full-access"
      ? "Earlier in this session it ran with full access, able to reach other projects' files."
      : "Earlier in this session it ran without an enforced boundary, not isolated from other projects' files.";
  const exposedSuffix =
    exposed?.state === "full-access" ? "was full access" : "was not sandboxed";
  if (report.state === "sandboxed") {
    if (exposed) {
      // The agent running now is inside the boundary, but what it did
      // before is not: the warning outlives the restart.
      return {
        label: `Sandboxed · ${exposedSuffix}`,
        tone: "warning",
        summary: `This agent now runs inside this project's boundary. ${exposedClause}`,
      };
    }
    return {
      label: "Sandboxed",
      tone: "safe",
      summary: "This agent runs inside this project's boundary.",
    };
  }
  if (report.state === "not-sandboxed") {
    return {
      label: "Not sandboxed",
      tone: "warning",
      summary:
        "The host did not enforce a boundary: this session is not isolated from other projects' files.",
    };
  }
  if (exposed) {
    return {
      label: `Sandbox unreported · ${exposedSuffix}`,
      tone: "warning",
      summary: `This execution has not reported a project boundary, so nothing here says it is sandboxed. ${exposedClause}`,
    };
  }
  return {
    label: "Sandbox unreported",
    tone: "muted",
    summary:
      "This execution has not reported a project boundary, so nothing here says it is sandboxed.",
  };
}
