import { Badge } from "@/shared/ui/badge";
import {
  type CodingSessionStatus,
  codingSessionStatusChipLabel,
} from "../domain/index.ts";

/**
 * Tone is a hint, never the message: the label comes from the domain so the
 * browser cannot invent a friendlier word than the provider signed.
 */
function toneClassName(status: CodingSessionStatus): string {
  if (status === "running") {
    return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  }
  if (status === "waiting_for_input") {
    return "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  }
  if (status === "failed" || status === "interrupted") {
    return "border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-300";
  }
  if (status === "disconnected" || status === "unknown") {
    return "border-black/15 bg-black/5 text-black/60 dark:border-white/15 dark:bg-white/10 dark:text-white/60";
  }
  return "border-black/15 bg-white text-black/70 dark:border-white/15 dark:bg-white/5 dark:text-white/70";
}

/** The D8 status chip. */
export function CodingSessionStatusChip({
  status,
}: {
  status: CodingSessionStatus;
}) {
  return (
    <Badge variant="outline" className={toneClassName(status)}>
      {codingSessionStatusChipLabel(status)}
    </Badge>
  );
}

/** The "closed" marker a 44230 closure puts on a session. */
export function CodingSessionClosedBadge() {
  return (
    <Badge
      variant="outline"
      className="border-black/15 bg-black/5 text-black/50 dark:border-white/15 dark:bg-white/10 dark:text-white/50"
    >
      Closed
    </Badge>
  );
}
