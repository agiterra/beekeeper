import { ShieldOff } from "lucide-react";

import {
  CODING_SESSION_FULL_ACCESS_BADGE,
  CODING_SESSION_FULL_ACCESS_BADGE_TITLE,
} from "@/features/coding-sessions/lib/codingSessionFullAccess";
import { Badge } from "@/shared/ui/badge";

import type { CodingSessionFullAccess } from "./useCodingSessionFullAccess";

/**
 * The header's "Full access" badge, beside the seat badge.
 *
 * Shown only when the host answered `granted` for this session — never from a
 * click that has not been read back. It also mounts the restart's receipt
 * watcher, which renders no DOM, so the header owns one slot for both.
 */
export function CodingSessionFullAccessBadge({
  fullAccess,
}: {
  fullAccess?: CodingSessionFullAccess | null;
}) {
  if (!fullAccess) return null;
  return (
    <>
      {fullAccess.granted ? (
        <Badge
          className="shrink-0 gap-1.5"
          data-testid="coding-session-header-full-access"
          title={CODING_SESSION_FULL_ACCESS_BADGE_TITLE}
          variant="warning"
        >
          <ShieldOff aria-hidden className="size-3" />
          {CODING_SESSION_FULL_ACCESS_BADGE}
        </Badge>
      ) : null}
      {fullAccess.watcher}
    </>
  );
}
