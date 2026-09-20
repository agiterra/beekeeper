/**
 * The commit the Run control offers when an action must name one.
 *
 * Lane 184 made a `checkout: required` step refuse a run that names no
 * commit. The operator should not have to go and find a sha by hand, so the
 * control is prefilled with the tip of the project's **code** repository as
 * the newest *relay-signed* kind:30618 ref state records it — the same
 * delivery observation the work fold judges a `git-ref` proof against.
 *
 * Three things this module deliberately does not do:
 *
 * - it never reads a local checkout: a working copy is not evidence, and the
 *   commit a run is bound to must be one the relay has observed;
 * - it never accepts a ref state signed by anybody but the relay: a
 *   pusher-signed 30618 is a claim, and `authors` is what makes the answer a
 *   delivery observation rather than an assertion;
 * - it never guesses when there is no ref state. `tip` is `null` with a
 *   `reason` naming what is missing, and the control asks for the commit with
 *   an empty field instead of pretending to a default.
 */
import { useQuery } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { relayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_REPO_STATE } from "@/shared/constants/kinds";

/** The agents repository's id suffix (spec § 4.11); never the code repo. */
export const AGENTS_REPO_SUFFIX = "-beekeeper-agents";

/** The tip of one repository's delivery ref, or why there is none. */
export type ProjectCodeRefTip = {
  /** Bare repository id the answer is about, or `null` when none was found. */
  repositoryId: string | null;
  /** `refs/heads/<branch>` the tip was read from. */
  refName: string | null;
  /** Full 40-hex commit, or `null` — never a shortened or guessed value. */
  tip: string | null;
  /** Unix seconds of the ref state that proved it. */
  observedAt: number | null;
  /** Why there is no tip, in words. `null` exactly when `tip` is set. */
  reason: string | null;
};

/** A project's code repositories: every `30617:…` that is not the agents one. */
export function codeRepositoryIds(repoAddrs: readonly string[]): string[] {
  return repoAddrs
    .map((addr) => addr.slice(addr.lastIndexOf(":") + 1))
    .filter((id) => id.length > 0 && !id.endsWith(AGENTS_REPO_SUFFIX));
}

const SHA_40 = /^[0-9a-f]{40}$/;

/**
 * Read the delivery ref off one relay-signed kind:30618.
 *
 * `preferred` is the branch the caller would rather have (the `HEAD` the
 * event names, when it names one); otherwise `main`, then `master`, then the
 * single branch if there is exactly one. More than one candidate and no
 * `HEAD` is *not* resolved by picking: the reason says so.
 */
export function readDeliveryRefTip(event: RelayEvent): ProjectCodeRefTip {
  const repositoryId = event.tags.find((tag) => tag[0] === "d")?.[1] ?? null;
  const branches = new Map<string, string>();
  let head: string | null = null;
  for (const tag of event.tags) {
    const [name, value] = tag;
    if (!name || !value) continue;
    if (name.startsWith("refs/heads/")) {
      if (SHA_40.test(value.toLowerCase())) {
        branches.set(name.slice("refs/heads/".length), value.toLowerCase());
      }
    } else if (name === "HEAD") {
      head = value.replace(/^ref:\s*/, "").replace(/^refs\/heads\//, "");
    }
  }
  const base = {
    repositoryId,
    observedAt: event.created_at,
  };
  const pick = (branch: string | null): string | null =>
    branch !== null && branches.has(branch) ? branch : null;
  const only = branches.size === 1 ? [...branches.keys()][0] : null;
  const branch = pick(head) ?? pick("main") ?? pick("master") ?? only;
  if (branch === null) {
    return {
      ...base,
      refName: null,
      tip: null,
      reason:
        branches.size === 0
          ? `the relay's newest ref state for ${repositoryId ?? "this repository"} names no branch`
          : `the relay's newest ref state for ${repositoryId ?? "this repository"} names ${branches.size} branches and no HEAD, so no delivery ref is implied`,
    };
  }
  return {
    ...base,
    refName: `refs/heads/${branch}`,
    tip: branches.get(branch) ?? null,
    reason: null,
  };
}

/** React Query key for one project's code-repository delivery tip. */
export function projectCodeRefTipQueryKey(projectRef: string) {
  return ["project-code-ref-tip", projectRef] as const;
}

/**
 * Fetch the newest relay-signed ref state for the first code repository of
 * `repoAddrs` that has one.
 */
export async function loadProjectCodeRefTip(
  repoAddrs: readonly string[],
  deps: {
    relaySelf?: () => Promise<string | null>;
    fetchEvents?: (
      filter: Parameters<typeof relayClient.fetchEventsBatch>[0][number],
    ) => Promise<RelayEvent[]>;
  } = {},
): Promise<ProjectCodeRefTip> {
  const none = (reason: string): ProjectCodeRefTip => ({
    repositoryId: null,
    refName: null,
    tip: null,
    observedAt: null,
    reason,
  });
  const ids = codeRepositoryIds(repoAddrs);
  if (ids.length === 0) {
    return none("this project names no code repository");
  }
  const relaySelf = await (deps.relaySelf ?? getRelaySelf)();
  if (!relaySelf) {
    return none(
      "the relay's own key is unknown here, so no ref state can be shown as relay-signed",
    );
  }
  const fetchEvents =
    deps.fetchEvents ??
    ((filter: Parameters<typeof relayClient.fetchEventsBatch>[0][number]) =>
      relayClient.fetchEventsBatch([filter]));
  const events = await fetchEvents({
    kinds: [KIND_REPO_STATE],
    authors: [relaySelf],
    "#d": ids,
    limit: ids.length,
  });
  const relaySigned = events.filter((event) => event.pubkey === relaySelf);
  if (relaySigned.length === 0) {
    return none(
      `the relay has published no ref state for ${ids.join(", ")}, so no commit is offered`,
    );
  }
  const newest = relaySigned.reduce((best, event) =>
    event.created_at > best.created_at ? event : best,
  );
  return readDeliveryRefTip(newest);
}

/** The delivery tip of the project's code repository, for the Run control. */
export function useProjectCodeRefTip(
  projectRef: string | null,
  repoAddrs: readonly string[],
) {
  const key = repoAddrs.join(",");
  return useQuery({
    queryKey: [...projectCodeRefTipQueryKey(projectRef ?? ""), key],
    enabled: projectRef !== null,
    queryFn: () => loadProjectCodeRefTip(repoAddrs),
  });
}
