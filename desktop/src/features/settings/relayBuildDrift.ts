/**
 * Whether this app's build is behind the relay it is talking to, and — the
 * part that matters — how that was decided.
 *
 * `git rev-list --count X` is the **size of the set of commits reachable from
 * X**, not a position on a line. `count(relay) - count(app)` is "commits the
 * relay has that the app does not" *only when the app's commit is an ancestor
 * of the relay's*. On a topic branch it is the difference of two unrelated
 * quantities and understates by the app's own unpushed commits; after a
 * rebase or squash-merge it changes meaning entirely.
 *
 * The desktop cannot resolve that: it has no clone. `bee git check` can, and
 * discloses which method it used (`EnforcementCheckMethod::{Ancestry,Date}`,
 * `crates/beekeeper-cli/src/commands/git_setup.rs`). This module mirrors that
 * discipline — every verdict names its method, and the copy carries the
 * method into the sentence the user reads, because a bare "12 commits behind"
 * would be a control that lies whenever the assumption fails.
 */

/** How a verdict was reached. Never omitted from what the UI renders. */
export type RelayBuildDriftMethod =
  /**
   * The two commits are byte-identical. Stronger than `ordinal`, and checked
   * first, so a corrupt count can never contradict it.
   */
  | "commit"
  /**
   * A subtraction of two `git rev-list --count` values. Exact only when the
   * app's commit is an ancestor of the relay's — which is unverifiable here.
   */
  | "ordinal"
  /** Nothing could answer. */
  | "none";

/** Why no comparison was possible. Each maps to its own sentence. */
export type RelayBuildDriftUnknownReason =
  | "app-commit-unknown"
  | "app-count-unknown"
  | "relay-commit-unknown"
  | "relay-count-unknown"
  | "app-source-dirty"
  | "divergent-equal-ordinal"
  | "different-software";

export type RelayBuildDrift =
  | { state: "unknown"; method: "none"; reason: RelayBuildDriftUnknownReason }
  | { state: "same"; method: "commit" }
  | { state: "ahead"; method: "ordinal"; commits: number }
  | { state: "behind"; method: "ordinal"; commits: number };

export type RelayBuildDriftInput = {
  app: {
    commit: string | null;
    commitCount: number | null;
    /**
     * `true` only when the build script *observed* a modified tree. `null`
     * means "not observed dirty", which is not the same as clean — the build
     * script deliberately never embeds a clean claim.
     */
    sourceDirty: boolean | null;
  };
  relay: {
    commit: string | null;
    commitCount: number | null;
    /** NIP-11 `software` — the repository the relay names as its source. */
    software: string | null;
  };
};

/** The repository this app is built from; ordinals only compare within it. */
export const APP_SOFTWARE_URL = "https://github.com/agiterra/beekeeper";

function sameRepository(software: string | null): boolean {
  if (software === null) return true; // not disclosed — not evidence of difference
  const normalize = (value: string) =>
    value.trim().replace(/\/+$/, "").toLowerCase();
  return normalize(software) === normalize(APP_SOFTWARE_URL);
}

function isUsableCount(value: number | null): value is number {
  // `null` coerces to 0 in JS arithmetic, so an unguarded subtraction would
  // report a drift equal to whichever ordinal *was* known. The guard is what
  // stops a missing value becoming a confident number.
  return typeof value === "number" && Number.isInteger(value) && value >= 1;
}

/**
 * Compare this build against the relay's. The order of the checks *is* the
 * honesty: each one removes a case where a number would be a lie.
 */
export function relayBuildDrift(input: RelayBuildDriftInput): RelayBuildDrift {
  const { app, relay } = input;

  // 1. A modified tree means the binary is not the commit it names, so
  //    nothing downstream describes what is actually running.
  if (app.sourceDirty === true) {
    return { state: "unknown", method: "none", reason: "app-source-dirty" };
  }

  // 2. Ordinals from different repositories are unrelated graphs.
  if (!sameRepository(relay.software)) {
    return { state: "unknown", method: "none", reason: "different-software" };
  }

  // 3. Both commits are required even though only the counts get subtracted:
  //    the equality check below is the only cross-check available, and
  //    without it a divergence is indistinguishable from a distance.
  if (!app.commit) {
    return { state: "unknown", method: "none", reason: "app-commit-unknown" };
  }
  if (!relay.commit) {
    return { state: "unknown", method: "none", reason: "relay-commit-unknown" };
  }

  // 4. Identical commits are the same build whatever the counts say. Checked
  //    before them, so a corrupt count can never render a number here.
  if (app.commit === relay.commit) {
    return { state: "same", method: "commit" };
  }

  if (!isUsableCount(app.commitCount)) {
    return { state: "unknown", method: "none", reason: "app-count-unknown" };
  }
  if (!isUsableCount(relay.commitCount)) {
    return { state: "unknown", method: "none", reason: "relay-count-unknown" };
  }

  const delta = relay.commitCount - app.commitCount;

  // 5. Equal ordinals with different commits are siblings at the same depth.
  //    Reading that as "same" would be this design's most seductive lie.
  if (delta === 0) {
    return {
      state: "unknown",
      method: "none",
      reason: "divergent-equal-ordinal",
    };
  }
  if (delta < 0) {
    return { state: "ahead", method: "ordinal", commits: -delta };
  }
  return { state: "behind", method: "ordinal", commits: delta };
}

const plural = (n: number) => (n === 1 ? "commit" : "commits");

/**
 * The sidebar card's copy, or `null` when nothing should be shown.
 *
 * Only `behind` surfaces: `ahead` is a developer running a local build, and
 * every `unknown` is a case where a number would be invented. Keeping the
 * predicate here rather than in the component is the same split
 * `sidebarUpdateCardVisibility.ts` uses.
 *
 * `updateAvailable` changes the second line rather than being ignored: the
 * relay routinely runs ahead of the newest release, and a "you are behind"
 * prompt with nothing to install is a control that lies by implication.
 */
export function relayBuildDriftNotice(
  drift: RelayBuildDrift,
  updateAvailable: boolean,
): { title: string; description: string } | null {
  if (drift.state !== "behind") return null;
  const count = `${drift.commits} ${plural(drift.commits)}`;
  // "by commit count" is the method name in plain words — the same slot
  // `bee git check` fills with "(method: ancestry)". Not decoration.
  const lead = `This relay's build is ${count} ahead of yours by commit count.`;
  return {
    title: "App is behind the relay",
    description: updateAvailable
      ? lead
      : `${lead} No app update is available yet.`,
  };
}

/**
 * A full sentence for Settings, for every state. Never a bare yes/no, and
 * never silence — an unexplained absence is what sent someone looking at the
 * relay in the first place.
 */
export function relayBuildDriftDetail(drift: RelayBuildDrift): string {
  switch (drift.state) {
    case "same":
      return "This app and this relay are running the same build.";
    case "behind":
      return (
        `This relay's build is ${drift.commits} ${plural(drift.commits)} ahead of yours ` +
        "by commit count. A commit count difference is exact only when this app's " +
        "commit is an ancestor of the relay's; on a branch, or after a rebase or " +
        "squash-merge, it can understate. This app has no clone of the repository, " +
        "so it cannot check ancestry."
      );
    case "ahead":
      return (
        `This app's build is ${drift.commits} ${plural(drift.commits)} ahead of the ` +
        "relay's by commit count — usually a local development build."
      );
    case "unknown":
      switch (drift.reason) {
        case "app-source-dirty":
          return (
            "This app was built from a modified working tree, so its commit does " +
            "not describe what is running. No comparison is made."
          );
        case "different-software":
          return (
            "This relay reports a different source repository, so its commit count " +
            "is not comparable with this app's."
          );
        case "app-commit-unknown":
          return "This app could not determine its own build commit, so no comparison is possible.";
        case "relay-commit-unknown":
          return "This relay does not disclose a build commit, so no comparison is possible.";
        case "app-count-unknown":
          return "This app does not know its own commit count, so no distance can be given.";
        case "relay-count-unknown":
          return (
            "This relay does not disclose a commit count, so no distance can be " +
            "given. It may predate the field, or have been built where the count " +
            "could not be determined — those are indistinguishable from here."
          );
        case "divergent-equal-ordinal":
          return (
            "This app and this relay report the same commit count but different " +
            "commits, so they are on different lines of history and no distance " +
            "can be given."
          );
      }
  }
}
