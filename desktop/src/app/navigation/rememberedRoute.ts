/**
 * Going back to a remembered location, safely.
 *
 * A remembered href is a snapshot of somewhere that existed once. Between then
 * and the keypress the session may have been archived, the project left, or
 * the route removed by an update — so the path is matched against the live
 * route tree before it is committed, and anything that no longer resolves
 * falls back rather than stranding the chord on a dead page.
 */

import type { AnyRouter } from "@tanstack/react-router";

export function navigateToRememberedRoute(
  router: AnyRouter,
  href: string | null,
  fallback: () => void,
): void {
  if (!href) {
    fallback();
    return;
  }

  const pathname = href.split(/[?#]/)[0];
  try {
    if (!pathname || !router.getMatchedRoutes(pathname).foundRoute) {
      fallback();
      return;
    }
  } catch {
    fallback();
    return;
  }

  void router.navigate({ href }).catch(() => {
    fallback();
  });
}
