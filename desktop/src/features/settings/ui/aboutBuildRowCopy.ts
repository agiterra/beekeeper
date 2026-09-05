/**
 * The one-line answer to "what am I running", and its tooltip.
 *
 * `relayBuildDrift` already decides *whether* this app and its relay are the
 * same build and by what method; this module is only the short rendering of
 * that verdict beside the two commits themselves. The long sentence stays in
 * `relayBuildDriftDetail` — a row that shows the shas and a row that explains
 * the comparison are different jobs, and the explanation is what a person
 * reads once while the shas are what they read every time.
 *
 * Nothing here invents a value. Every absence renders the literal `unknown`,
 * which is what NIP-11 `software_commit` serves and what the app's own build
 * script embeds when it cannot determine a commit — a disclosed non-answer,
 * never an error and never a guess (finding 32).
 */

import {
  relayBuildDrift,
  type RelayBuildDrift,
  type RelayBuildDriftInput,
} from "@/features/settings/relayBuildDrift";

/** Hex digits shown to a person, matching `bee git check --ref`. */
export const SHORT_SHA_HEX = 8;

/** What an undeterminable commit renders as, everywhere. */
export const UNKNOWN = "unknown";

/** A commit as the row shows it: eight hex, or the disclosed non-answer. */
export function shortSha(commit: string | null | undefined): string {
  if (!commit) return UNKNOWN;
  return commit.slice(0, SHORT_SHA_HEX);
}

/**
 * The state clause, short enough to sit on the row.
 *
 * The relay case named in the P1 brief — hive answering `unknown` because
 * `/usr/local/sbin/autodeploy` on agincus predates the `BUZZ_SOURCE_SHA` fix
 * (Andy, 2ba548c9e) — gets its cause in the sentence rather than a bare
 * "unknown", because the bare form is what sends someone to ssh into the host.
 */
export function buildStateLabel(drift: RelayBuildDrift): string {
  switch (drift.state) {
    case "same":
      return "same build";
    case "ahead":
      return `app ahead by ${drift.commits} ${drift.commits === 1 ? "commit" : "commits"} (by commit count)`;
    case "behind":
      return `app behind by ${drift.commits} ${drift.commits === 1 ? "commit" : "commits"} (by commit count)`;
    case "unknown":
      switch (drift.reason) {
        case "relay-commit-unknown":
          return "relay unknown (deployer does not stamp builds)";
        case "app-commit-unknown":
          return "app build commit unknown";
        case "app-source-dirty":
          return "app built from a modified tree";
        case "app-count-unknown":
          return "app commit count unknown — no distance";
        case "relay-count-unknown":
          return "relay discloses no commit count — no distance";
        case "divergent-equal-ordinal":
          return "different builds at the same commit count";
        case "different-software":
          return "relay reports a different source repository";
      }
  }
}

/** The row's verdict for a pair of identities. */
export function aboutBuildState(input: RelayBuildDriftInput): {
  drift: RelayBuildDrift;
  label: string;
} {
  const drift = relayBuildDrift(input);
  return { drift, label: buildStateLabel(drift) };
}

/**
 * The tooltip: full commits, ordinals and — for the relay, which discloses one
 * — the build time, each independently `unknown`. A truncated sha is for
 * reading; the full one is for looking up, and one of them has to be present
 * somewhere.
 *
 * There is no app build time here because the app does not have one to give:
 * `desktop/src-tauri/build.rs` embeds a commit, a count and a dirty
 * observation, and no clock read. `bee --version` (`buzz_core::build_info`)
 * does carry one. Rendering "unknown" for a field this build never populates
 * would read as a failed measurement rather than an absent feature, so the row
 * omits it instead.
 */
export function aboutBuildTooltip(
  input: RelayBuildDriftInput,
  relayBuildTime: string | null,
): string {
  const { app, relay } = input;
  return [
    `App    ${app.commit ?? UNKNOWN}`,
    `       count ${app.commitCount ?? "null"}${
      app.sourceDirty === true ? " · built from a modified tree" : ""
    }`,
    `Relay  ${relay.commit ?? UNKNOWN}`,
    `       count ${relay.commitCount ?? "null"} · built ${relayBuildTime ?? UNKNOWN}`,
  ].join("\n");
}
