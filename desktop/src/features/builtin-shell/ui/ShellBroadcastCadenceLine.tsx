import { Radio } from "lucide-react";

import { shellBroadcastCadenceLabel } from "../observe/shellBroadcastCadence";
import { useShellBroadcastCadence } from "../observe/useShellBroadcastCadence";

/**
 * The owner's honest one-liner about how fast their shared terminal streams:
 * "≤1 frame/s, ≤40/min — the relay's per-key quota", naming the wider spacing
 * while the broadcaster is backing off from a `rate-limited:` refusal.
 */
export function ShellBroadcastCadenceLine({
  sessionId,
}: {
  sessionId: string;
}) {
  const cadence = useShellBroadcastCadence(sessionId);
  return (
    <p
      className="flex items-center gap-1 truncate text-2xs text-muted-foreground"
      data-testid="shell-broadcast-cadence"
      title="Frames are charged to your relay message quota"
    >
      <Radio className="size-3 shrink-0" />
      {shellBroadcastCadenceLabel(
        cadence?.intervalMs ?? null,
        cadence?.capPerMinute,
      )}
    </p>
  );
}
