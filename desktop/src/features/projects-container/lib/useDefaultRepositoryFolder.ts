import * as React from "react";

import { useCommunities } from "@/features/communities/useCommunities";

import { defaultReposRoot } from "./projectAgentsInit";

/** What the default-folder read settled on, and where it came from. */
export type DefaultRepositoryFolder = {
  /** The absolute parent folder, or `null` until the read settles or off-host. */
  path: string | null;
  /** `community` = the active community's Repositories folder; `host` = `default_repos_root`. */
  source: "community" | "host" | null;
  /** `true` once the read has answered, one way or the other. */
  settled: boolean;
};

/**
 * The folder new project clones go under: the active community's
 * `reposDir` when one is set (Edit community → Repositories folder, or
 * Settings → Sessions → Default repository folder — one value, two doors),
 * otherwise the host's default repos root. Off-host, or before the host
 * answers, `path` is `null` and callers name the fallback rather than a
 * guessed path.
 */
export function useDefaultRepositoryFolder(): DefaultRepositoryFolder {
  const { activeCommunity } = useCommunities();
  const communityFolder = activeCommunity?.reposDir?.trim() || null;
  const [hostRoot, setHostRoot] = React.useState<{
    path: string | null;
    settled: boolean;
  }>({ path: null, settled: false });

  React.useEffect(() => {
    let cancelled = false;
    void defaultReposRoot()
      .then((root) => {
        if (!cancelled) setHostRoot({ path: root, settled: true });
      })
      .catch(() => {
        if (!cancelled) setHostRoot({ path: null, settled: true });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return React.useMemo(() => {
    if (communityFolder) {
      return { path: communityFolder, source: "community", settled: true };
    }
    return {
      path: hostRoot.path,
      source: hostRoot.path ? "host" : null,
      settled: hostRoot.settled,
    };
  }, [communityFolder, hostRoot]);
}
