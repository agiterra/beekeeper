import * as React from "react";

import { cn } from "@/shared/lib/cn";

import type { SeatRow } from "../lib/rolesViewModel";
import {
  SEAT_NO_AGENT,
  SEAT_NO_PROJECT,
  SEAT_NO_ROLE,
  SEAT_UNMANAGED_AGENT,
  seatStatusText,
  shaText,
} from "./rolesCopy";

type Segment = {
  kind: "agent" | "project" | "role" | "status" | "sha";
  text: string;
  className?: string;
  title?: string;
};

function agentSegment(seat: SeatRow): Segment {
  if (seat.agentName) {
    return {
      kind: "agent",
      text: seat.agentName,
      className: "text-foreground",
    };
  }
  if (seat.agentPubkey) {
    return {
      kind: "agent",
      text: SEAT_UNMANAGED_AGENT,
      className: "text-foreground",
      title: seat.agentPubkey,
    };
  }
  return { kind: "agent", text: SEAT_NO_AGENT, className: "text-foreground" };
}

/**
 * One seat as a row that opens its session: `agent · project · status (age)
 * · sha` on a role card, `agent · role · status (age)` under a project.
 *
 * Every column is a disclosed value: an agent this computer does not manage
 * is "unmanaged agent" with its pubkey in the tooltip, a seat with no
 * `packRef` says "version unknown", and the status word is the catalog's own.
 */
export function SeatRowButton({
  seat,
  onOpen,
  columns,
}: {
  seat: SeatRow;
  onOpen?: (seat: SeatRow) => void;
  columns: "role-card" | "project-block";
}) {
  const segments: Segment[] = [agentSegment(seat)];
  if (columns === "role-card") {
    segments.push({
      kind: "project",
      text: seat.projectName ?? SEAT_NO_PROJECT,
    });
  } else {
    segments.push({ kind: "role", text: seat.role ?? SEAT_NO_ROLE });
  }
  segments.push({
    kind: "status",
    text: seatStatusText(seat.status, seat.ageSeconds),
  });
  if (columns === "role-card") {
    segments.push({
      kind: "sha",
      text: shaText(seat.packSha),
      className: "font-mono",
      title: seat.packSha ?? undefined,
    });
  }
  // On a role card the row shares a narrow column with everything else the
  // card says, so it wraps onto a second line instead of shortening every
  // column to an unreadable stub ("unmanaged… · Ge… · idle (just… · sha
  // unk…"). Only the two open-ended columns — the agent and the project —
  // are capped; the status and the version always render whole. Under a
  // project block the row keeps its single truncating line.
  const wraps = columns === "role-card";
  return (
    <button
      className={cn(
        "flex w-full min-w-0 rounded px-2 py-1 text-left text-xs text-muted-foreground hover:bg-muted/40",
        wraps
          ? "flex-wrap items-baseline gap-x-1 gap-y-0.5"
          : "items-center gap-1",
      )}
      data-seat-key={seat.key}
      data-seat-status={seat.status}
      data-testid="seat-row"
      onClick={() => onOpen?.(seat)}
      title={seat.label}
      type="button"
    >
      {segments.map((segment, index) => (
        <React.Fragment key={segment.kind}>
          {index > 0 ? <span aria-hidden>·</span> : null}
          <span
            className={cn(
              wraps
                ? segment.kind === "agent" || segment.kind === "project"
                  ? "max-w-40 truncate"
                  : "whitespace-nowrap"
                : "truncate",
              segment.className,
            )}
            data-seat-column={segment.kind}
            title={segment.title}
          >
            {segment.text}
          </span>
        </React.Fragment>
      ))}
    </button>
  );
}
