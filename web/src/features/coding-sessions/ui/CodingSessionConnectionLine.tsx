import { Radio, RotateCw, WifiOff } from "lucide-react";

import type { CodingSessionObserverView } from "./observer-contract.ts";

const LINE_CLASS =
  "flex items-center gap-1.5 text-xs text-black/50 dark:text-white/50";

/**
 * What the live subscription is actually doing.
 *
 * "Live" is only printed when the reader says so. A stale socket that still
 * looked live was the exact failure this line exists to make visible.
 */
export function CodingSessionConnectionLine({
  view,
}: {
  view: CodingSessionObserverView;
}) {
  if (view.connection === "live") {
    return (
      <p className={LINE_CLASS} data-testid="coding-session-connection">
        <Radio className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
        Live — updates arrive as they are signed
      </p>
    );
  }
  if (view.connection === "reconnecting") {
    return (
      <p className={LINE_CLASS} data-testid="coding-session-connection">
        <RotateCw className="h-3.5 w-3.5 animate-spin text-amber-600 dark:text-amber-400" />
        Reconnecting — showing the last facts read
      </p>
    );
  }
  if (view.connection === "connecting") {
    return (
      <p className={LINE_CLASS} data-testid="coding-session-connection">
        <RotateCw className="h-3.5 w-3.5 animate-spin" />
        Reading session history…
      </p>
    );
  }
  return (
    <p className={LINE_CLASS} data-testid="coding-session-connection">
      <WifiOff className="h-3.5 w-3.5" />
      Not subscribed — these facts are not updating
    </p>
  );
}
